//! The pinned catalogue of speech models and the language packs built on them.
//!
//! Every downloadable file is pinned by URL (a fixed Hugging Face commit, never
//! `main`), byte size and SHA-256. The downloader refuses anything that does
//! not match, so a changed or truncated upstream file can never be installed.
//!
//! Sources:
//! - `lmz/candle-whisper` at commit `02d8350e…`: 8-bit GGUF conversions of
//!   OpenAI's `whisper-tiny.en` and `whisper-tiny` published by the candle
//!   maintainers for candle's quantized Whisper (`quantized_model`). Its
//!   `model-tiny.safetensors` is byte-identical to `openai/whisper-tiny`'s.
//! - `openai/whisper-base.en` at commit `911407f4…`: the official weights.
//!
//! The Whisper weights are released by OpenAI under the MIT licence
//! (github.com/openai/whisper); the Hugging Face model cards tag them
//! Apache-2.0. Both are permissive; neither repository above adds terms.

/// How a model's weights are stored, which picks the candle loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightsFormat {
    /// 8-bit quantized GGUF, loaded with `whisper::quantized_model`.
    Gguf,
    /// Full-precision safetensors, loaded with `whisper::model`.
    Safetensors,
}

/// One file a model needs, pinned by content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// File name inside the model's directory.
    pub name: &'static str,
    /// Download URL, pinned to an immutable revision.
    pub url: &'static str,
    /// Exact size in bytes.
    pub size: u64,
    /// Lower-case hex SHA-256 of the file.
    pub sha256: &'static str,
}

/// A Whisper model: one shared download used by one or more language packs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Directory name under `speech-models/`, stable across releases.
    pub id: &'static str,
    /// Human label for settings ("Whisper tiny.en, 8-bit").
    pub label: &'static str,
    /// English-only models take no language or task tokens in the prompt.
    pub english_only: bool,
    pub format: WeightsFormat,
    pub config: ModelFile,
    pub tokenizer: ModelFile,
    pub weights: ModelFile,
}

impl ModelSpec {
    pub fn files(&self) -> [&ModelFile; 3] {
        [&self.config, &self.tokenizer, &self.weights]
    }

    pub fn size_bytes(&self) -> u64 {
        self.files().iter().map(|file| file.size).sum()
    }
}

/// A language the learner can download checks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageSpec {
    /// Pack id, the `lang` the commands take ("en", "hi", "en-enhanced").
    pub code: &'static str,
    /// English name of the pack.
    pub name: &'static str,
    /// The language's own name for itself.
    pub native_name: &'static str,
    pub model_id: &'static str,
    /// The language a check names. Equal to `code` except for an optional
    /// upgrade pack ("en-enhanced" checks "en").
    pub checks: &'static str,
    /// Whisper language token for multilingual models (`<|hi|>`); `None` on
    /// English-only models, which have no language token.
    pub whisper_lang: Option<&'static str>,
    /// The model often gets this language wrong (tested: Whisper tiny writes
    /// Hindi and Bengali speech in Latin script, so correct reading scores as
    /// missed). Settings says so before the learner downloads it.
    pub limited_accuracy: bool,
}

impl LanguageSpec {
    /// True for a pack that upgrades another language's checks.
    pub fn is_upgrade(&self) -> bool {
        self.code != self.checks
    }
}

#[cfg(test)]
const LMZ: &str =
    "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e";
#[cfg(test)]
const OPENAI_BASE_EN: &str =
    "https://huggingface.co/openai/whisper-base.en/resolve/911407f4214e0e1d82085af863093ec0b66f9cd6";

pub const WHISPER_TINY_EN_Q8: ModelSpec = ModelSpec {
    id: "whisper-tiny-en-q8",
    label: "Whisper tiny.en (8-bit)",
    english_only: true,
    format: WeightsFormat::Gguf,
    config: ModelFile {
        name: "config.json",
        url: "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e/config-tiny-en.json",
        size: 1937,
        sha256: "b5366317df280f79e6cc366f259f379674f81a8721d458acf2e00680ccb10b1d",
    },
    tokenizer: ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e/tokenizer-tiny-en.json",
        size: 2405679,
        sha256: "5eb60cec1e77aeeb6869a2bb5a8e01a84c3fe5d072d75369343021fe6f5310d0",
    },
    weights: ModelFile {
        name: "model.gguf",
        url: "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e/model-tiny-en-q80.gguf",
        size: 41841216,
        sha256: "07997efe0b8d3577fc45d9b307ef578a01346f4088a5da4bd92377aaa3f72986",
    },
};

pub const WHISPER_TINY_Q8: ModelSpec = ModelSpec {
    id: "whisper-tiny-q8",
    label: "Whisper tiny multilingual (8-bit)",
    english_only: false,
    format: WeightsFormat::Gguf,
    config: ModelFile {
        name: "config.json",
        url: "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e/config-tiny.json",
        size: 1983,
        sha256: "ffdccec4f3211f4c63310f2b7098f309fe70f3952cedc5e4d11e43f5b2379b98",
    },
    tokenizer: ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e/tokenizer-tiny.json",
        size: 2480452,
        sha256: "dfc530298b6fbed1a97c6472c575b026453706e2a204c7f7038f2c9d208b0759",
    },
    weights: ModelFile {
        name: "model.gguf",
        url: "https://huggingface.co/lmz/candle-whisper/resolve/02d8350e5402f18725eadb6101b4963d181b0b5e/model-tiny-q80.gguf",
        size: 41841632,
        sha256: "edcc907db61aef092f1244dc1e53c55056b472be343e9bbb4dc12ebd4740392f",
    },
};

pub const WHISPER_BASE_EN: ModelSpec = ModelSpec {
    id: "whisper-base-en",
    label: "Whisper base.en",
    english_only: true,
    format: WeightsFormat::Safetensors,
    config: ModelFile {
        name: "config.json",
        url: "https://huggingface.co/openai/whisper-base.en/resolve/911407f4214e0e1d82085af863093ec0b66f9cd6/config.json",
        size: 1937,
        sha256: "160c1df40a60d4ef5a6014d536fd845c33aabfd1f318458ac7f87cfe91ebbe76",
    },
    tokenizer: ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/openai/whisper-base.en/resolve/911407f4214e0e1d82085af863093ec0b66f9cd6/tokenizer.json",
        size: 2405679,
        sha256: "5eb60cec1e77aeeb6869a2bb5a8e01a84c3fe5d072d75369343021fe6f5310d0",
    },
    weights: ModelFile {
        name: "model.safetensors",
        url: "https://huggingface.co/openai/whisper-base.en/resolve/911407f4214e0e1d82085af863093ec0b66f9cd6/model.safetensors",
        size: 290401888,
        sha256: "d4dd5542fd6a1d35639e21384238f3bfe6c557c849d392b5905d33ee29e71db5",
    },
};

pub const MODELS: &[ModelSpec] = &[WHISPER_TINY_EN_Q8, WHISPER_TINY_Q8, WHISPER_BASE_EN];

const fn multilingual(
    code: &'static str,
    name: &'static str,
    native_name: &'static str,
    limited_accuracy: bool,
) -> LanguageSpec {
    LanguageSpec {
        code,
        name,
        native_name,
        model_id: WHISPER_TINY_Q8.id,
        checks: code,
        whisper_lang: Some(code),
        limited_accuracy,
    }
}

/// Multilingual-model languages and whether their checks are limited.
const GOOD: bool = false;
const LIMITED: bool = true;

/// Every pack, in display order. The upgrade pack follows the pack it
/// upgrades.
pub const LANGUAGES: &[LanguageSpec] = &[
    LanguageSpec {
        code: "en",
        name: "English",
        native_name: "English",
        model_id: WHISPER_TINY_EN_Q8.id,
        checks: "en",
        whisper_lang: None,
        limited_accuracy: false,
    },
    LanguageSpec {
        code: "en-enhanced",
        name: "Enhanced English",
        native_name: "English",
        model_id: WHISPER_BASE_EN.id,
        checks: "en",
        whisper_lang: None,
        limited_accuracy: false,
    },
    multilingual("hi", "Hindi", "हिन्दी", LIMITED),
    multilingual("es", "Spanish", "Español", GOOD),
    multilingual("fr", "French", "Français", GOOD),
    multilingual("de", "German", "Deutsch", GOOD),
    multilingual("bn", "Bengali", "বাংলা", LIMITED),
    multilingual("mr", "Marathi", "मराठी", LIMITED),
    multilingual("te", "Telugu", "తెలుగు", LIMITED),
    multilingual("ur", "Urdu", "اردو", LIMITED),
    multilingual("zh", "Chinese", "中文", GOOD),
    multilingual("ja", "Japanese", "日本語", GOOD),
    multilingual("pt", "Portuguese", "Português", GOOD),
    multilingual("ar", "Arabic", "العربية", GOOD),
];

pub fn language(code: &str) -> Option<&'static LanguageSpec> {
    LANGUAGES.iter().find(|language| language.code == code)
}

pub fn model(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|model| model.id == id)
}

/// The packs that can serve a check in `lang`, best first: an upgrade pack
/// ahead of the base pack. `lang` may name either.
pub fn packs_for_check(lang: &str) -> Vec<&'static LanguageSpec> {
    let Some(requested) = language(lang) else {
        return Vec::new();
    };
    let mut packs: Vec<_> = LANGUAGES
        .iter()
        .filter(|pack| pack.checks == requested.checks)
        .collect();
    packs.sort_by_key(|pack| !pack.is_upgrade());
    packs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_language_names_a_known_model() {
        for pack in LANGUAGES {
            let model = model(pack.model_id).expect("model");
            assert_eq!(
                model.english_only,
                pack.whisper_lang.is_none(),
                "{}: English-only models take no language token",
                pack.code
            );
            assert!(pack.checks == pack.code || language(pack.checks).is_some());
        }
    }

    #[test]
    fn codes_and_model_ids_are_unique() {
        let codes: HashSet<_> = LANGUAGES.iter().map(|l| l.code).collect();
        assert_eq!(codes.len(), LANGUAGES.len());
        let ids: HashSet<_> = MODELS.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), MODELS.len());
    }

    #[test]
    fn registry_urls_use_pinned_revisions() {
        for model in MODELS {
            let base = match model.id {
                "whisper-base-en" => OPENAI_BASE_EN,
                _ => LMZ,
            };
            let names: HashSet<_> = model.files().iter().map(|f| f.name).collect();
            assert_eq!(names.len(), 3, "{}: file names must differ", model.id);
            for file in model.files() {
                assert!(file.url.starts_with(base), "{}", file.url);
                assert!(!file.url.contains("/resolve/main/"), "{}", file.url);
                assert!(file.url.starts_with("https://"));
                assert_eq!(file.sha256.len(), 64);
                assert!(file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
                assert!(file.size > 0);
                assert!(!file.name.contains('/') && !file.name.starts_with('.'));
            }
            let weights_ext = match model.format {
                WeightsFormat::Gguf => ".gguf",
                WeightsFormat::Safetensors => ".safetensors",
            };
            assert!(model.weights.name.ends_with(weights_ext));
        }
    }

    #[test]
    fn multilingual_languages_share_one_model() {
        let shared: Vec<_> = LANGUAGES
            .iter()
            .filter(|l| l.model_id == WHISPER_TINY_Q8.id)
            .map(|l| l.code)
            .collect();
        assert_eq!(
            shared,
            ["hi", "es", "fr", "de", "bn", "mr", "te", "ur", "zh", "ja", "pt", "ar"]
        );
    }

    #[test]
    fn limited_languages_are_the_ones_tiny_cannot_write() {
        let limited: Vec<_> = LANGUAGES
            .iter()
            .filter(|l| l.limited_accuracy)
            .map(|l| l.code)
            .collect();
        assert_eq!(limited, ["hi", "bn", "mr", "te", "ur"]);
    }

    #[test]
    fn checks_prefer_the_upgrade_pack() {
        let en: Vec<_> = packs_for_check("en").iter().map(|p| p.code).collect();
        assert_eq!(en, ["en-enhanced", "en"]);
        let enhanced: Vec<_> = packs_for_check("en-enhanced")
            .iter()
            .map(|p| p.code)
            .collect();
        assert_eq!(enhanced, ["en-enhanced", "en"]);
        let hi: Vec<_> = packs_for_check("hi").iter().map(|p| p.code).collect();
        assert_eq!(hi, ["hi"]);
        assert!(packs_for_check("xx").is_empty());
    }
}
