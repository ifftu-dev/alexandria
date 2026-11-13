//! Voice activity on the candidate's microphone during an interview held in
//! Alexandria: when a voice starts, and whether it is the voice that has been
//! speaking on this device all along or another one.
//!
//! Pure signal processing (frame energy against an adaptive noise floor,
//! normalised autocorrelation for voicing and pitch), no model and no
//! inference, so it runs under every trust profile and on every device.
//! Audio is reduced frame by frame to an energy, a voicing strength and a
//! pitch, and dropped: nothing is transcribed, kept or sent. The only output
//! is a timestamped onset with a speaker label.
//!
//! "Another voice" means a voice whose pitch sits well apart from the first
//! voice heard on the device. A second speaker with a similar pitch is not
//! told apart, and a whisper has no pitch: the signal is advisory, a reason
//! for the panel to ask, and is never certified without the measurement
//! corpus.

use serde::Serialize;

/// Analysis rate: speech pitch needs nothing above it.
const ANALYSIS_RATE: u32 = 16_000;
/// One analysis frame.
const FRAME_MS: u32 = 30;
/// Lowest and highest pitch searched, Hz.
const PITCH_MIN_HZ: f32 = 70.0;
const PITCH_MAX_HZ: f32 = 400.0;
/// Normalised autocorrelation a frame needs to count as voiced.
const VOICING_MIN: f32 = 0.55;
/// A lag within this share of the best one is preferred when shorter, so a
/// sub-harmonic never wins over the true pitch.
const OCTAVE_TOLERANCE: f32 = 0.9;
/// A speech frame is this far above the noise floor, dB.
const SPEECH_OVER_FLOOR_DB: f32 = 10.0;
/// And never quieter than this, dBFS.
const SPEECH_MIN_DBFS: f32 = -55.0;
/// How fast the noise floor rises toward louder frames, dB per frame.
const FLOOR_RISE_DB: f32 = 0.03;
/// Voiced frames a segment needs before it is reported: a cough or a click
/// never is.
const ONSET_VOICED_FRAMES: usize = 8;
/// Quiet frames that end a segment.
const HANGOVER_FRAMES: usize = 8;
/// Voiced frames that make the first voice's pitch.
const PRIMARY_FRAMES: usize = 50;
/// Pitch distance from the first voice, in semitones, that makes another.
const OTHER_VOICE_SEMITONES: f32 = 4.5;
/// After the far end was audible, the microphone is ignored this long: its
/// sound comes back through the speakers when there is no echo cancelling.
const REMOTE_HOLD_MS: u64 = 300;

/// Whose voice an onset is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    /// The voice that has spoken on this device from the start.
    Primary,
    /// A voice pitched well apart from it.
    Other,
}

/// A voice started speaking at `at_ms` (milliseconds since the Unix epoch).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VoiceOnset {
    pub at_ms: u64,
    pub speaker: Speaker,
}

struct Segment {
    started_ms: u64,
    pitches: Vec<f32>,
    quiet: usize,
    reported: bool,
    speaker: Speaker,
}

pub struct VoiceActivityDetector {
    /// Input samples averaged into one analysis sample.
    decimation: usize,
    rate: f32,
    frame_len: usize,
    pending: Vec<f32>,
    decimate_acc: f32,
    decimate_n: usize,
    /// Analysis samples taken so far, for frame timing.
    clock_ms: Option<f64>,
    floor_db: Option<f32>,
    segment: Option<Segment>,
    primary_pitches: Vec<f32>,
    primary_hz: Option<f32>,
    remote_until_ms: u64,
}

impl VoiceActivityDetector {
    /// A detector for mono samples at `sample_rate`.
    pub fn new(sample_rate: u32) -> Self {
        let decimation = ((sample_rate as f32 / ANALYSIS_RATE as f32).round() as usize).max(1);
        let rate = sample_rate as f32 / decimation as f32;
        Self {
            decimation,
            rate,
            frame_len: (rate * FRAME_MS as f32 / 1000.0) as usize,
            pending: Vec::new(),
            decimate_acc: 0.0,
            decimate_n: 0,
            clock_ms: None,
            floor_db: None,
            segment: None,
            primary_pitches: Vec::new(),
            primary_hz: None,
            remote_until_ms: 0,
        }
    }

    /// Feed mono samples that ended at `now_ms`. `remote_audible` is whether
    /// the far end is playing on this device right now.
    pub fn push(&mut self, samples: &[f32], now_ms: u64, remote_audible: bool) -> Vec<VoiceOnset> {
        if remote_audible {
            self.remote_until_ms = now_ms + REMOTE_HOLD_MS;
        }
        let input_rate = self.rate * self.decimation as f32;
        let chunk_ms = samples.len() as f64 * 1000.0 / f64::from(input_rate);
        // Frame times follow the sample count, re-anchored when the stream
        // stalls or drifts by more than a frame.
        let start = now_ms as f64 - chunk_ms;
        let clock = self.clock_ms.get_or_insert(start);
        if (*clock + self.pending.len() as f64 * 1000.0 / f64::from(self.rate) - start).abs()
            > f64::from(FRAME_MS)
        {
            *clock = start - self.pending.len() as f64 * 1000.0 / f64::from(self.rate);
        }

        let mut onsets = Vec::new();
        for &sample in samples {
            self.decimate_acc += sample;
            self.decimate_n += 1;
            if self.decimate_n < self.decimation {
                continue;
            }
            self.pending
                .push(self.decimate_acc / self.decimation as f32);
            self.decimate_acc = 0.0;
            self.decimate_n = 0;
            if self.pending.len() == self.frame_len {
                let frame = std::mem::take(&mut self.pending);
                let at = self.clock_ms.unwrap_or(start);
                self.clock_ms = Some(at + f64::from(FRAME_MS));
                if let Some(onset) = self.frame(&frame, at.max(0.0) as u64) {
                    onsets.push(onset);
                }
            }
        }
        onsets
    }

    fn frame(&mut self, frame: &[f32], at_ms: u64) -> Option<VoiceOnset> {
        let mean = frame.iter().sum::<f32>() / frame.len() as f32;
        let centred: Vec<f32> = frame.iter().map(|s| s - mean).collect();
        let rms = (centred.iter().map(|s| s * s).sum::<f32>() / centred.len() as f32).sqrt();
        let db = 20.0 * rms.max(1e-9).log10();
        let floor = *self.floor_db.get_or_insert(db);

        let gated = at_ms < self.remote_until_ms;
        let loud = db > SPEECH_MIN_DBFS && db > floor + SPEECH_OVER_FLOOR_DB;
        let pitch = if loud && !gated {
            self.pitch(&centred)
        } else {
            None
        };

        if pitch.is_none() {
            // The floor falls at once to a quieter frame and creeps up
            // toward a louder one, so steady noise becomes the floor.
            self.floor_db = Some(if db < floor {
                db
            } else {
                floor + FLOOR_RISE_DB
            });
        }
        if gated {
            // The far end is speaking: whatever is open is its echo.
            self.segment = None;
            return None;
        }

        match (pitch, self.segment.as_mut()) {
            (Some(hz), Some(segment)) => {
                segment.quiet = 0;
                segment.pitches.push(hz);
            }
            (Some(hz), None) => {
                self.segment = Some(Segment {
                    started_ms: at_ms,
                    pitches: vec![hz],
                    quiet: 0,
                    reported: false,
                    speaker: Speaker::Primary,
                });
            }
            (None, Some(segment)) => {
                segment.quiet += 1;
                if segment.quiet >= HANGOVER_FRAMES {
                    self.segment = None;
                }
                return None;
            }
            (None, None) => return None,
        }

        let primary_hz = self.primary_hz;
        let segment = self.segment.as_mut()?;
        if !segment.reported && segment.pitches.len() >= ONSET_VOICED_FRAMES {
            segment.reported = true;
            segment.speaker = match primary_hz {
                Some(primary) => {
                    let semitones = 12.0 * (median(&segment.pitches) / primary).log2();
                    if semitones.abs() > OTHER_VOICE_SEMITONES {
                        Speaker::Other
                    } else {
                        Speaker::Primary
                    }
                }
                None => Speaker::Primary,
            };
            let onset = VoiceOnset {
                at_ms: segment.started_ms,
                speaker: segment.speaker,
            };
            if self.primary_hz.is_none() {
                self.primary_pitches.extend_from_slice(&segment.pitches);
            }
            self.learn_primary();
            return Some(onset);
        }
        if segment.reported && segment.speaker == Speaker::Primary && self.primary_hz.is_none() {
            if let Some(&hz) = segment.pitches.last() {
                self.primary_pitches.push(hz);
            }
            self.learn_primary();
        }
        None
    }

    fn learn_primary(&mut self) {
        if self.primary_hz.is_none() && self.primary_pitches.len() >= PRIMARY_FRAMES {
            self.primary_hz = Some(median(&self.primary_pitches));
            self.primary_pitches = Vec::new();
        }
    }

    /// The frame's pitch in Hz when it is voiced.
    fn pitch(&self, x: &[f32]) -> Option<f32> {
        let min_lag = (self.rate / PITCH_MAX_HZ).floor() as usize;
        let max_lag = ((self.rate / PITCH_MIN_HZ).ceil() as usize).min(x.len() / 2);
        let scores: Vec<(usize, f32)> = (min_lag..=max_lag)
            .map(|lag| {
                let (a, b) = (&x[..x.len() - lag], &x[lag..]);
                let cross: f32 = a.iter().zip(b).map(|(p, q)| p * q).sum();
                let energy =
                    a.iter().map(|p| p * p).sum::<f32>() * b.iter().map(|q| q * q).sum::<f32>();
                (
                    lag,
                    if energy > 0.0 {
                        cross / energy.sqrt()
                    } else {
                        0.0
                    },
                )
            })
            .collect();
        let best = scores.iter().map(|(_, s)| *s).fold(f32::MIN, f32::max);
        if best < VOICING_MIN {
            return None;
        }
        // The shortest lag at a local peak close to the best one.
        let lag = scores
            .windows(3)
            .find(|w| w[1].1 >= OCTAVE_TOLERANCE * best && w[1].1 >= w[0].1 && w[1].1 >= w[2].1)
            .map(|w| w[1].0)?;
        Some(self.rate / lag as f32)
    }
}

fn median(values: &[f32]) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

/// Whether voice checks may listen for this integrity session: an interview's
/// check that is still running and whose microphone consent is recorded. The
/// call's own voice checks start only after this passes.
pub fn may_listen(conn: &rusqlite::Connection, session_id: &str) -> Result<(), String> {
    use rusqlite::OptionalExtension;
    let row: Option<(String, Option<String>, bool)> = conn
        .query_row(
            "SELECT purpose, ended_at, consent_microphone FROM integrity_sessions WHERE id = ?1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        None => Err("no such integrity check".into()),
        Some((purpose, _, _)) if purpose != "interview" => {
            Err("voice checks run only in an interview's integrity check".into())
        }
        Some((_, Some(_), _)) => Err("that integrity check has ended".into()),
        Some((_, None, false)) => Err("voice checks were not allowed for this check".into()),
        Some((_, None, true)) => Ok(()),
    }
}

#[cfg(test)]
mod consent_tests {
    use super::may_listen;
    use crate::db::Database;

    fn db_with(purpose: &str, ended: bool, microphone: bool) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, purpose, ended_at, consent_microphone)
                 VALUES ('s', ?1, CASE WHEN ?2 THEN datetime('now') END, ?3)",
                rusqlite::params![purpose, ended, microphone],
            )
            .unwrap();
        db
    }

    #[test]
    fn voice_checks_listen_only_in_a_running_interview_check_with_microphone_consent() {
        assert!(may_listen(db_with("interview", false, true).conn(), "s").is_ok());
        for (purpose, ended, microphone) in [
            ("interview", false, false),
            ("interview", true, true),
            ("assessment", false, true),
        ] {
            let db = db_with(purpose, ended, microphone);
            assert!(
                may_listen(db.conn(), "s").is_err(),
                "{purpose} {ended} {microphone}"
            );
        }
        assert!(may_listen(db_with("interview", false, true).conn(), "missing").is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    /// 10 ms of microphone audio per push, as a capture callback delivers it.
    const CHUNK: usize = 480;

    /// A voiced sound: a glottal-like harmonic series at `pitch` Hz.
    fn voice(pitch: f32, ms: u32, t0: &mut f64) -> Vec<f32> {
        let n = (RATE * ms / 1000) as usize;
        let start = *t0;
        *t0 += f64::from(ms) / 1000.0;
        (0..n)
            .map(|i| {
                let t = start + i as f64 / f64::from(RATE);
                let wave: f64 = (1..=12)
                    .map(|k| {
                        let k = f64::from(k);
                        (2.0 * std::f64::consts::PI * f64::from(pitch) * k * t).sin() / k
                    })
                    .sum();
                wave as f32 * 0.08
            })
            .collect()
    }

    /// Room noise: quiet, deterministic, aperiodic.
    fn noise(ms: u32, seed: &mut u32) -> Vec<f32> {
        (0..(RATE * ms / 1000))
            .map(|_| {
                *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (*seed >> 8) as f32 / (1u32 << 24) as f32 * 0.004 - 0.002
            })
            .collect()
    }

    /// Feed `audio` from `start_ms` in capture-sized chunks; `remote` says
    /// whether the far end is audible during each chunk.
    fn run(
        detector: &mut VoiceActivityDetector,
        audio: &[f32],
        start_ms: u64,
        remote: impl Fn(u64) -> bool,
    ) -> Vec<VoiceOnset> {
        let mut onsets = Vec::new();
        for (i, chunk) in audio.chunks(CHUNK).enumerate() {
            let end = start_ms + ((i + 1) * CHUNK) as u64 * 1000 / u64::from(RATE);
            onsets.extend(detector.push(chunk, end, remote(end)));
        }
        onsets
    }

    struct Script {
        audio: Vec<f32>,
        seed: u32,
        t: f64,
    }

    impl Script {
        fn new() -> Self {
            Self {
                audio: Vec::new(),
                seed: 7,
                t: 0.0,
            }
        }
        fn quiet(mut self, ms: u32) -> Self {
            let n = noise(ms, &mut self.seed);
            self.audio.extend(n);
            self.t += f64::from(ms) / 1000.0;
            self
        }
        fn speak(mut self, pitch: f32, ms: u32) -> Self {
            let mut v = voice(pitch, ms, &mut self.t);
            let n = noise(ms, &mut self.seed);
            for (s, e) in v.iter_mut().zip(n) {
                *s += e;
            }
            self.audio.extend(v);
            self
        }
    }

    const T0: u64 = 1_700_000_000_000;

    #[test]
    fn room_noise_alone_is_never_a_voice() {
        let script = Script::new().quiet(3_000);
        let mut detector = VoiceActivityDetector::new(RATE);
        assert!(run(&mut detector, &script.audio, T0, |_| false).is_empty());
    }

    #[test]
    fn a_voice_after_silence_is_reported_once_from_when_it_started() {
        let script = Script::new().quiet(1_000).speak(120.0, 1_200).quiet(600);
        let mut detector = VoiceActivityDetector::new(RATE);
        let onsets = run(&mut detector, &script.audio, T0, |_| false);
        assert_eq!(onsets.len(), 1, "{onsets:?}");
        assert_eq!(onsets[0].speaker, Speaker::Primary);
        let late = onsets[0].at_ms as i64 - (T0 + 1_000) as i64;
        // Within a frame of when it began.
        assert!(
            (-30..=60).contains(&late),
            "onset {late} ms after the voice began"
        );
    }

    #[test]
    fn a_click_or_a_cough_is_too_short_to_be_a_voice() {
        let script = Script::new().quiet(1_000).speak(150.0, 120).quiet(1_000);
        let mut detector = VoiceActivityDetector::new(RATE);
        assert!(run(&mut detector, &script.audio, T0, |_| false).is_empty());
    }

    #[test]
    fn a_voice_pitched_apart_from_the_first_one_is_another_voice() {
        // The candidate talks for a while; then someone else, higher, speaks;
        // then the candidate again.
        let script = Script::new()
            .quiet(500)
            .speak(115.0, 2_500)
            .quiet(800)
            .speak(220.0, 900)
            .quiet(800)
            .speak(118.0, 900)
            .quiet(500);
        let mut detector = VoiceActivityDetector::new(RATE);
        let speakers: Vec<Speaker> = run(&mut detector, &script.audio, T0, |_| false)
            .into_iter()
            .map(|o| o.speaker)
            .collect();
        assert_eq!(
            speakers,
            [Speaker::Primary, Speaker::Other, Speaker::Primary]
        );
    }

    #[test]
    fn the_far_end_heard_through_the_speakers_is_not_a_voice_in_the_room() {
        // The interviewer's question leaks into the microphone while the
        // call plays it, and just after.
        let script = Script::new().quiet(500).speak(200.0, 1_500).quiet(1_000);
        let mut detector = VoiceActivityDetector::new(RATE);
        let playing = |at: u64| (T0 + 500..T0 + 2_000).contains(&at);
        assert!(run(&mut detector, &script.audio, T0, playing).is_empty());
    }
}
