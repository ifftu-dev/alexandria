//! Whisper inference for pronunciation checks (candle, CPU, pure Rust).
//!
//! One check encodes the recording once, then runs the decoder twice:
//! greedily, to report what the model heard, and teacher-forced on the
//! target sentence, to read the probability of every target token. See
//! `scoring` for how those probabilities become word results.

use std::path::Path;

use candle_core::{DType, Device, IndexOp, Tensor, D};
use candle_transformers::models::whisper::{self, audio, model, quantized_model, Config};

use super::mel;
use super::registry::{ModelSpec, WeightsFormat};
use super::scoring::{self, WordScore};
use super::tokenizer::Tokenizer;

/// Above this no-speech probability the recording is treated as silent and
/// every word as missed. Whisper's own threshold; silence and background
/// noise measured 0.78–0.95, speech 0.003–0.32.
pub const NO_SPEECH_AT: f32 = whisper::NO_SPEECH_THRESHOLD as f32;

/// Marks earlier transcript text fed to the decoder as context.
const PREV_TOKEN: &str = "<|startofprev|>";

/// Context text that steers the decoder's writing system for a language.
fn previous_text(whisper_lang: &str) -> Option<&'static str> {
    match whisper_lang {
        "zh" => Some(" 以下是普通话的句子。"),
        _ => None,
    }
}

/// Silence put before a recording: 0.4 s at 16 kHz.
const LEAD_IN_SAMPLES: usize = 6_400;

enum Weights {
    Quantized(quantized_model::Whisper),
    Full(model::Whisper),
}

impl Weights {
    fn encode(&mut self, mel: &Tensor) -> candle_core::Result<Tensor> {
        match self {
            Self::Quantized(m) => m.encoder.forward(mel, true),
            Self::Full(m) => m.encoder.forward(mel, true),
        }
    }

    fn decode(
        &mut self,
        tokens: &Tensor,
        audio: &Tensor,
        flush: bool,
    ) -> candle_core::Result<Tensor> {
        match self {
            Self::Quantized(m) => m.decoder.forward(tokens, audio, flush),
            Self::Full(m) => m.decoder.forward(tokens, audio, flush),
        }
    }

    fn logits(&self, hidden: &Tensor) -> candle_core::Result<Tensor> {
        match self {
            Self::Quantized(m) => m.decoder.final_linear(hidden),
            Self::Full(m) => m.decoder.final_linear(hidden),
        }
    }
}

/// What one check produced, before it is shaped for the command.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    pub heard: String,
    pub overall: f32,
    pub words: Vec<WordScore>,
    /// Whisper's no-speech probability for the recording.
    pub no_speech: f32,
}

/// A loaded Whisper model with its tokenizer and mel bank.
pub struct Engine {
    weights: Weights,
    config: Config,
    tokenizer: Tokenizer,
    mel_filters: Vec<f32>,
    english_only: bool,
    device: Device,
    sot: u32,
    eot: u32,
    transcribe: u32,
    no_timestamps: u32,
    /// `<|nocaptions|>` (or `<|nospeech|>` in newer vocabularies).
    no_speech: Option<u32>,
    /// Added to logits during greedy decoding: −∞ on suppressed tokens and
    /// every special token except end-of-text.
    greedy_mask: Tensor,
    /// Added to logits when scoring: −∞ on every special token, so the
    /// probability is spread over text tokens only.
    text_mask: Tensor,
}

impl Engine {
    /// Load the model stored in `dir` (files named as in `spec`).
    pub fn load(dir: &Path, spec: &ModelSpec) -> anyhow::Result<Self> {
        let device = Device::Cpu;
        let config: Config = serde_json::from_slice(&std::fs::read(dir.join(spec.config.name))?)?;
        let tokenizer = Tokenizer::from_json(&std::fs::read(dir.join(spec.tokenizer.name))?)?;
        let weights_path = dir.join(spec.weights.name);
        let weights = match spec.format {
            WeightsFormat::Gguf => {
                let vb = quantized_model::VarBuilder::from_gguf(&weights_path, &device)?;
                Weights::Quantized(quantized_model::Whisper::load(&vb, config.clone())?)
            }
            WeightsFormat::Safetensors => {
                let vb = candle_nn::VarBuilder::from_buffered_safetensors(
                    std::fs::read(&weights_path)?,
                    whisper::DTYPE,
                    &device,
                )?;
                Weights::Full(model::Whisper::load(&vb, config.clone())?)
            }
        };
        let special = |token: &str| {
            tokenizer
                .special(token)
                .ok_or_else(|| anyhow::anyhow!("tokenizer has no {token}"))
        };
        let sot = special(whisper::SOT_TOKEN)?;
        let eot = special(whisper::EOT_TOKEN)?;
        let transcribe = special(whisper::TRANSCRIBE_TOKEN)?;
        let no_timestamps = special(whisper::NO_TIMESTAMPS_TOKEN)?;
        let no_speech = whisper::NO_SPEECH_TOKENS
            .iter()
            .find_map(|token| tokenizer.special(token));

        // Special tokens start at end-of-text in every Whisper vocabulary.
        let vocab = config.vocab_size;
        let greedy: Vec<f32> = (0..vocab as u32)
            .map(|id| {
                if config.suppress_tokens.contains(&id) || (id > eot) {
                    f32::NEG_INFINITY
                } else {
                    0.0
                }
            })
            .collect();
        let text: Vec<f32> = (0..vocab as u32)
            .map(|id| if id >= eot { f32::NEG_INFINITY } else { 0.0 })
            .collect();
        let greedy_mask = Tensor::new(greedy.as_slice(), &device)?;
        let text_mask = Tensor::new(text.as_slice(), &device)?;

        Ok(Self {
            weights,
            mel_filters: mel::filters(config.num_mel_bins),
            config,
            tokenizer,
            english_only: spec.english_only,
            device,
            sot,
            eot,
            transcribe,
            no_timestamps,
            no_speech,
            greedy_mask,
            text_mask,
        })
    }

    /// The decoder prompt. English-only models take no language or task
    /// token (as OpenAI's tokenizer builds it); multilingual ones are told
    /// the language and to transcribe.
    ///
    /// Chinese also gets OpenAI's suggested previous-text context, a
    /// simplified-Chinese sentence, because otherwise Whisper tends to write
    /// traditional characters and score simplified targets as missed.
    fn prompt(&self, whisper_lang: Option<&str>) -> anyhow::Result<(Vec<u32>, usize)> {
        let mut prompt = Vec::new();
        if let (Some(context), Some(prev)) = (
            whisper_lang.and_then(previous_text),
            self.tokenizer.special(PREV_TOKEN),
        ) {
            prompt.push(prev);
            for (id, _) in self.tokenizer.encode_with_spans(context)? {
                prompt.push(id);
            }
        }
        let sot_at = prompt.len();
        prompt.push(self.sot);
        if !self.english_only {
            let lang = whisper_lang
                .ok_or_else(|| anyhow::anyhow!("multilingual model needs a language"))?;
            let token = format!("<|{lang}|>");
            let id = self
                .tokenizer
                .special(&token)
                .ok_or_else(|| anyhow::anyhow!("model has no language token {token}"))?;
            prompt.push(id);
            prompt.push(self.transcribe);
        }
        prompt.push(self.no_timestamps);
        Ok((prompt, sot_at))
    }

    fn log_mel(&self, pcm: &[f32]) -> anyhow::Result<Tensor> {
        // A short lead-in of silence: recordings that start on the first
        // syllable otherwise lose most of the first word.
        let lead_in = LEAD_IN_SAMPLES.min(whisper::N_SAMPLES.saturating_sub(pcm.len()));
        let mut padded = vec![0.0; lead_in];
        padded.extend_from_slice(pcm);
        // Whisper is trained on 30-second windows: pad with silence, as its
        // own `pad_or_trim` does, and keep exactly one window of frames.
        padded.resize(whisper::N_SAMPLES, 0.0);
        let mel = audio::pcm_to_mel(&self.config, &padded, &self.mel_filters);
        let n_mels = self.config.num_mel_bins;
        let frames = mel.len() / n_mels;
        let mel = Tensor::from_vec(mel, (1, n_mels, frames), &self.device)?;
        Ok(mel.narrow(2, 0, whisper::N_FRAMES.min(frames))?)
    }

    /// Score `text` against `pcm` (16 kHz mono, −1.0..1.0, at most 30 s).
    pub fn check(
        &mut self,
        whisper_lang: Option<&str>,
        per_character: bool,
        text: &str,
        pcm: &[f32],
    ) -> anyhow::Result<CheckOutcome> {
        let target = scoring::target(&self.tokenizer, text, per_character)?;
        let (prompt, sot_at) = self.prompt(whisper_lang)?;
        let mel = self.log_mel(pcm)?;
        let audio = self.weights.encode(&mel)?;

        // "Heard" is for display; a recording far longer than the target
        // (or a decoder stuck repeating itself) is cut off rather than
        // decoded for the whole window.
        let heard = self.greedy(&prompt, &audio, target.ids.len() * 2 + 16)?;

        let mut tokens = prompt.clone();
        tokens.extend(&target.ids);
        let input = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
        let hidden = self.weights.decode(&input, &audio, true)?;
        let no_speech = self.no_speech_probability(&hidden, sot_at)?;
        // Position i predicts token i + 1: the target's logits start at the
        // prompt's last position.
        let hidden = hidden.narrow(1, prompt.len() - 1, target.ids.len())?;
        let logits = self.weights.logits(&hidden)?.i(0)?;
        let logits = logits.broadcast_add(&self.text_mask)?;
        let log_probs = candle_nn::ops::log_softmax(&logits, D::Minus1)?.to_vec2::<f32>()?;
        let target_log_probs: Vec<f32> = target
            .ids
            .iter()
            .zip(&log_probs)
            .map(|(&id, row)| row.get(id as usize).copied().unwrap_or(f32::NEG_INFINITY))
            .collect();

        let (mut words, mut overall) = scoring::score(&target, &target_log_probs);
        let mut heard = heard;
        if no_speech > NO_SPEECH_AT {
            // Whisper fills silence with a stock word ("you") and gives
            // matching target words high probability; nothing was said.
            heard.clear();
            overall = 0.0;
            for word in &mut words {
                word.score = 0.0;
                word.status = scoring::WordStatus::Missed;
            }
        }
        Ok(CheckOutcome {
            heard,
            overall,
            words,
            no_speech,
        })
    }

    /// The probability Whisper gives "no speech" right after
    /// start-of-transcript, as OpenAI's decoder computes it.
    fn no_speech_probability(&self, hidden: &Tensor, sot_at: usize) -> anyhow::Result<f32> {
        let Some(token) = self.no_speech else {
            return Ok(0.0);
        };
        let logits = self
            .weights
            .logits(&hidden.narrow(1, sot_at, 1)?)?
            .i((0, 0))?;
        let probs = candle_nn::ops::softmax(&logits, D::Minus1)?.to_vec1::<f32>()?;
        Ok(probs.get(token as usize).copied().unwrap_or(0.0))
    }

    fn greedy(&mut self, prompt: &[u32], audio: &Tensor, limit: usize) -> anyhow::Result<String> {
        let max_len = (self.config.max_target_positions / 2).min(prompt.len() + limit);
        let mut tokens = prompt.to_vec();
        for step in 0..max_len.saturating_sub(prompt.len()) {
            let input = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let hidden = self.weights.decode(&input, audio, step == 0)?;
            let last = hidden.dim(1)? - 1;
            let logits = self.weights.logits(&hidden.i((..1, last..))?)?.i(0)?.i(0)?;
            let logits = logits.broadcast_add(&self.greedy_mask)?;
            let next = logits
                .argmax(D::Minus1)?
                .to_dtype(DType::U32)?
                .to_scalar::<u32>()?;
            if next == self.eot {
                break;
            }
            tokens.push(next);
        }
        Ok(self
            .tokenizer
            .decode(&tokens[prompt.len()..])
            .trim()
            .to_string())
    }
}

#[cfg(test)]
mod calibration {
    //! Real-model checks against synthetic speech. They need a downloaded
    //! model and macOS `say` + `ffmpeg`, so they are ignored by default:
    //!
    //! ```text
    //! SPEECH_MODEL_DIR=/path/to/speech-models \
    //!   cargo test --lib speech::engine::calibration -- --ignored --nocapture
    //! ```
    //!
    //! `SPEECH_MODEL_DIR` holds one directory per model id, laid out as the
    //! app installs them.

    use super::*;
    use crate::speech::registry;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn model_dir(spec: &ModelSpec) -> PathBuf {
        PathBuf::from(std::env::var("SPEECH_MODEL_DIR").expect("SPEECH_MODEL_DIR")).join(spec.id)
    }

    /// `say` → 16 kHz mono s16le → f32.
    fn speak(voice: &str, sentence: &str) -> Vec<f32> {
        let dir = tempfile::tempdir().expect("tempdir");
        let aiff = dir.path().join("speech.aiff");
        let raw = dir.path().join("speech.raw");
        let said = Command::new("say")
            .args(["-v", voice, "-o"])
            .arg(&aiff)
            .arg(sentence)
            .stdin(Stdio::null())
            .status()
            .expect("say");
        assert!(said.success());
        let converted = Command::new("ffmpeg")
            .args(["-nostdin", "-loglevel", "error", "-y", "-i"])
            .arg(&aiff)
            .args(["-ar", "16000", "-ac", "1", "-f", "s16le"])
            .arg(&raw)
            .status()
            .expect("ffmpeg");
        assert!(converted.success());
        let bytes = std::fs::read(raw).expect("raw");
        crate::speech::pcm_from_le_bytes(&bytes).expect("pcm")
    }

    /// Quiet deterministic white noise, like a room with nobody speaking.
    fn noise(samples: usize) -> Vec<f32> {
        let mut state: u32 = 1;
        (0..samples)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((state >> 8) as f32 / 16_777_216.0 - 0.5) * 0.02
            })
            .collect()
    }

    #[test]
    #[ignore = "needs a downloaded model, macOS say and ffmpeg"]
    fn silence_and_noise_score_every_word_missed() {
        let spec = &registry::WHISPER_TINY_EN_Q8;
        let mut engine = Engine::load(&model_dir(spec), spec).expect("load");
        for (label, pcm) in [("silence", vec![0f32; 32_000]), ("noise", noise(32_000))] {
            let outcome = check_pcm(
                &mut engine,
                spec,
                None,
                false,
                label,
                &pcm,
                "Hello, how are you?",
            );
            assert!(outcome.no_speech > NO_SPEECH_AT);
            assert_eq!(outcome.heard, "");
            assert_eq!(outcome.overall, 0.0);
            assert!(outcome
                .words
                .iter()
                .all(|w| w.status == scoring::WordStatus::Missed));
        }
    }

    fn report(spec: &ModelSpec, label: &str, target: &str, outcome: &CheckOutcome, ms: f64) {
        println!(
            "[{}] {label} / target {target:?} ({ms:.0} ms)\n  heard {:?} overall {:.3} no-speech {:.3}",
            spec.id, outcome.heard, outcome.overall, outcome.no_speech
        );
        for word in &outcome.words {
            println!("  {:<12} {:.3} {:?}", word.word, word.score, word.status);
        }
    }

    fn check_pcm(
        engine: &mut Engine,
        spec: &ModelSpec,
        lang: Option<&str>,
        per_character: bool,
        label: &str,
        pcm: &[f32],
        target: &str,
    ) -> CheckOutcome {
        let started = std::time::Instant::now();
        let outcome = engine
            .check(lang, per_character, target, pcm)
            .expect("check");
        report(
            spec,
            label,
            target,
            &outcome,
            started.elapsed().as_secs_f64() * 1000.0,
        );
        outcome
    }

    fn run(
        spec: &ModelSpec,
        lang: Option<&str>,
        voice: &str,
        spoken: &str,
        target: &str,
    ) -> CheckOutcome {
        let mut engine = Engine::load(&model_dir(spec), spec).expect("load");
        let pcm = speak(voice, spoken);
        let label = format!("{voice} said {spoken:?}");
        check_pcm(&mut engine, spec, lang, false, &label, &pcm, target)
    }

    fn assert_mismatch_flagged(spec: &ModelSpec, lang: Option<&str>, voice: &str) {
        let matched = run(spec, lang, voice, "She is a teacher.", "She is a teacher.");
        let mismatched = run(spec, lang, voice, "She is a teacher.", "She is a doctor.");
        assert!(matched.heard.to_lowercase().contains("teacher"));
        let teacher = matched.words[3].score;
        let doctor = mismatched.words[3].score;
        assert!(teacher > 0.5, "spoken word should be clear: {teacher}");
        assert!(doctor < 0.15, "replaced word should be missed: {doctor}");
        assert!(matched.overall > mismatched.overall);
        assert_eq!(mismatched.words[3].status, scoring::WordStatus::Missed);
        for word in &matched.words {
            assert_eq!(word.status, scoring::WordStatus::Clear, "{}", word.word);
        }
    }

    #[test]
    #[ignore = "needs a downloaded model, macOS say and ffmpeg"]
    fn tiny_en_flags_a_replaced_word() {
        assert_mismatch_flagged(&registry::WHISPER_TINY_EN_Q8, None, "Samantha");
    }

    #[test]
    #[ignore = "needs a downloaded model, macOS say and ffmpeg"]
    fn base_en_flags_a_replaced_word() {
        assert_mismatch_flagged(&registry::WHISPER_BASE_EN, None, "Samantha");
    }

    #[test]
    #[ignore = "needs a downloaded model, macOS say and ffmpeg"]
    fn tiny_multilingual_flags_a_replaced_word() {
        assert_mismatch_flagged(&registry::WHISPER_TINY_Q8, Some("en"), "Samantha");
    }

    /// Prints scores for a spread of cases (accents, a minimal pair, a
    /// partial reading, silence, other languages). Used to pick
    /// `scoring::CLEAR_AT` / `UNCLEAR_AT`; asserts nothing.
    #[test]
    #[ignore = "needs downloaded models, macOS say and ffmpeg"]
    fn survey() {
        type Case<'a> = (&'a str, &'a str, &'a [&'a str]);
        let english: &[Case] = &[
            (
                "Samantha",
                "She is a teacher.",
                &["She is a teacher.", "She is a doctor."],
            ),
            (
                "Aman",
                "I live in Delhi with my family.",
                &[
                    "I live in Delhi with my family.",
                    "I live in Mumbai with my family.",
                ],
            ),
            (
                "Daniel",
                "The ship is in the harbour.",
                &[
                    "The ship is in the harbour.",
                    "The sheep is in the harbour.",
                ],
            ),
            (
                "Samantha",
                "I like to read books.",
                &["I like to read books every evening."],
            ),
            (
                "Aman",
                "Can I have a glass of water, please?",
                &[
                    "Can I have a glass of water, please?",
                    "Can I have a cup of tea, please?",
                ],
            ),
        ];
        let silence = vec![0f32; 16_000 * 2];
        let noise = noise(16_000 * 2);
        for spec in [&registry::WHISPER_TINY_EN_Q8, &registry::WHISPER_BASE_EN] {
            let mut engine = Engine::load(&model_dir(spec), spec).expect("load");
            for (voice, spoken, targets) in english {
                let pcm = speak(voice, spoken);
                for target in *targets {
                    let label = format!("{voice} said {spoken:?}");
                    check_pcm(&mut engine, spec, None, false, &label, &pcm, target);
                }
            }
            check_pcm(
                &mut engine,
                spec,
                None,
                false,
                "silence",
                &silence,
                "Hello, how are you?",
            );
            check_pcm(
                &mut engine,
                spec,
                None,
                false,
                "noise",
                &noise,
                "Hello, how are you?",
            );
        }

        let spec = &registry::WHISPER_TINY_Q8;
        let mut engine = Engine::load(&model_dir(spec), spec).expect("load");
        // (language, voice, per-character, spoken, matching target, other targets)
        type OtherCase<'a> = (&'a str, &'a str, bool, &'a str, &'a str, &'a [&'a str]);
        let other: &[OtherCase] = &[
            (
                "hi",
                "Lekha",
                false,
                "मेरा नाम लेखा है।",
                "मेरा नाम लेखा है।",
                &["मेरा नाम सीता है।"],
            ),
            (
                "hi",
                "Lekha",
                false,
                "मैं हर दिन स्कूल जाती हूँ।",
                "मैं हर दिन स्कूल जाती हूँ।",
                &["मैं हर दिन बाज़ार जाती हूँ।"],
            ),
            (
                "es",
                "Paulina",
                false,
                "Me gusta leer libros.",
                "Me gusta leer libros.",
                &["Me gusta comer libros."],
            ),
            (
                "fr",
                "Thomas",
                false,
                "Je voudrais un café.",
                "Je voudrais un café.",
                &["Je voudrais un thé."],
            ),
            (
                "de",
                "Anna",
                false,
                "Ich wohne in Berlin.",
                "Ich wohne in Berlin.",
                &["Ich wohne in Hamburg."],
            ),
            (
                "zh",
                "Tingting",
                true,
                "我喜欢学习中文。",
                "我喜欢学习中文。",
                &["我喜欢学习英文。"],
            ),
            (
                "ja",
                "Kyoko",
                true,
                "私は学生です。",
                "私は学生です。",
                &["私は先生です。"],
            ),
            (
                "pt",
                "Luciana",
                false,
                "Eu gosto de ler livros.",
                "Eu gosto de ler livros.",
                &["Eu gosto de comer livros."],
            ),
            (
                "ar",
                "Majed",
                false,
                "اسمي ماجد.",
                "اسمي ماجد.",
                &["اسمي أحمد."],
            ),
            ("bn", "Piya", false, "আমার নাম পিয়া।", "আমার নাম পিয়া।", &[]),
        ];
        for (lang, voice, per_character, spoken, matched, mismatched) in other {
            let pcm = speak(voice, spoken);
            let label = format!("{voice} said {spoken:?}");
            check_pcm(
                &mut engine,
                spec,
                Some(lang),
                *per_character,
                &label,
                &pcm,
                matched,
            );
            for target in *mismatched {
                check_pcm(
                    &mut engine,
                    spec,
                    Some(lang),
                    *per_character,
                    &label,
                    &pcm,
                    target,
                );
            }
        }
    }
}
