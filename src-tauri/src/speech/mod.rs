//! On-device pronunciation checks with downloadable language packs.
//!
//! A learner reads a target sentence aloud; the engine reports which words
//! came across clearly. Checks run fully offline on OpenAI's open-source
//! Whisper models (MIT), executed with candle on the CPU. No weights ship
//! with the app: the learner downloads a language pack from settings.
//!
//! Packs are device-wide, shared by every profile, and live under
//! `<app data>/speech-models/<model-id>/`. Several languages share the
//! multilingual model, so removing one deletes only files that no installed
//! language still needs. `installed.json` beside the model directories
//! records which languages the learner chose.
//!
//! A check estimates how clearly each word came across to a speech
//! recogniser; it does not judge accent.

pub mod engine;
pub mod mel;
pub mod registry;
pub mod scoring;
pub mod tokenizer;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use engine::{CheckOutcome, Engine};
use registry::{LanguageSpec, ModelFile, ModelSpec};
use scoring::WordScore;

/// Directory under the app data dir that holds every pack.
pub const SPEECH_DIR: &str = "speech-models";
/// Tauri event carrying [`PackProgress`] while a pack downloads.
pub const PROGRESS_EVENT: &str = "speech-pack-progress";
/// Prefix of the error a check returns when its language is not installed.
/// The Language Lab plugin keys on it to offer the download.
pub const PACK_MISSING_PREFIX: &str = "speech_pack_missing:";
/// Longest recording a check accepts: Whisper's 30-second window.
pub const MAX_SAMPLES: usize = candle_transformers::models::whisper::N_SAMPLES;

const STATE_FILE: &str = "installed.json";
/// Progress is reported at most once per this many bytes.
const PROGRESS_STEP: u64 = 512 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SpeechError {
    #[error("{PACK_MISSING_PREFIX} {0}")]
    PackMissing(String),
    #[error("unknown speech language: {0}")]
    UnknownLanguage(String),
    #[error("invalid recording: {0}")]
    InvalidAudio(String),
    #[error("download failed for {file}: {reason}")]
    Download { file: String, reason: String },
    #[error("speech pack storage: {0}")]
    Io(#[from] std::io::Error),
    #[error("pronunciation check failed: {0}")]
    Engine(String),
}

/// One downloadable language, as settings lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechPack {
    pub lang: String,
    pub name: String,
    pub native_name: String,
    pub model_id: String,
    pub model_label: String,
    /// Bytes still to download (0 when the model is already on disk, for
    /// example because another language shares it).
    pub download_bytes: u64,
    /// Total size of the model on disk.
    pub size_bytes: u64,
    pub installed: bool,
    /// Other languages that use the same model download.
    pub shared_with: Vec<String>,
    /// For an optional upgrade pack, the language whose checks it improves.
    pub upgrades: Option<String>,
    /// Checks in this language are often wrong with the current model.
    pub limited_accuracy: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackProgress {
    pub model_id: String,
    pub lang: String,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeechCheckResult {
    pub heard: String,
    pub overall: f32,
    pub words: Vec<WordScore>,
}

impl From<CheckOutcome> for SpeechCheckResult {
    fn from(outcome: CheckOutcome) -> Self {
        Self {
            heard: outcome.heard,
            overall: outcome.overall,
            words: outcome.words,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct InstalledState {
    #[serde(default)]
    langs: BTreeSet<String>,
}

/// Decode little-endian 16-bit mono PCM to samples in −1.0..1.0.
pub fn pcm_from_le_bytes(bytes: &[u8]) -> Result<Vec<f32>, SpeechError> {
    if bytes.is_empty() {
        return Err(SpeechError::InvalidAudio("the recording is empty".into()));
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(SpeechError::InvalidAudio(
            "16-bit PCM must have an even byte length".into(),
        ));
    }
    if bytes.len() / 2 > MAX_SAMPLES {
        return Err(SpeechError::InvalidAudio(
            "the recording is longer than 30 seconds".into(),
        ));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
        .collect())
}

/// Decode base64 16 kHz mono 16-bit PCM, refusing oversized input before
/// decoding it.
pub fn pcm_from_base64(encoded: &str) -> Result<Vec<f32>, SpeechError> {
    use base64::Engine as _;
    let encoded = encoded.trim();
    if encoded.len() > (MAX_SAMPLES * 2).div_ceil(3) * 4 {
        return Err(SpeechError::InvalidAudio(
            "the recording is longer than 30 seconds".into(),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| SpeechError::InvalidAudio(format!("not base64: {e}")))?;
    pcm_from_le_bytes(&bytes)
}

/// Models that removing `removing` from `installed` leaves unused: the
/// removed pack's model, unless another installed language still needs it.
pub fn models_freed_by_removal(installed: &BTreeSet<String>, removing: &str) -> Vec<&'static str> {
    let Some(pack) = registry::language(removing) else {
        return Vec::new();
    };
    let still_needed = installed
        .iter()
        .filter(|code| code.as_str() != removing)
        .any(|code| registry::language(code).is_some_and(|other| other.model_id == pack.model_id));
    if still_needed {
        Vec::new()
    } else {
        vec![pack.model_id]
    }
}

#[cfg(test)]
fn hex_sha256(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// SHA-256 of a file on disk, streamed.
fn file_sha256(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

type LoadedEngine = (&'static str, Arc<Mutex<Engine>>);

/// Device-wide pack storage and the loaded model.
pub struct SpeechService {
    root: PathBuf,
    /// Serialises read-modify-write of `installed.json`.
    state_lock: Mutex<()>,
    /// One lock per model, held while its files are downloaded or removed,
    /// so concurrent installs of a shared model cannot interleave.
    model_locks: Mutex<HashMap<&'static str, Arc<tokio::sync::Mutex<()>>>>,
    /// The most recently used model, kept loaded between checks.
    engine: Mutex<Option<LoadedEngine>>,
}

static SERVICE: OnceLock<SpeechService> = OnceLock::new();

/// The process-wide service rooted at `<app_data_dir>/speech-models`.
pub fn service(app_data_dir: &Path) -> &'static SpeechService {
    SERVICE.get_or_init(|| SpeechService::new(app_data_dir.join(SPEECH_DIR)))
}

impl SpeechService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            state_lock: Mutex::new(()),
            model_locks: Mutex::new(HashMap::new()),
            engine: Mutex::new(None),
        }
    }

    fn model_dir(&self, model: &ModelSpec) -> PathBuf {
        self.root.join(model.id)
    }

    fn model_lock(&self, model: &'static ModelSpec) -> Arc<tokio::sync::Mutex<()>> {
        self.model_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(model.id)
            .or_default()
            .clone()
    }

    fn read_state(&self) -> InstalledState {
        match std::fs::read(self.root.join(STATE_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("speech: ignoring unreadable {STATE_FILE}: {error}");
                InstalledState::default()
            }),
            Err(_) => InstalledState::default(),
        }
    }

    fn write_state(&self, state: &InstalledState) -> Result<(), SpeechError> {
        std::fs::create_dir_all(&self.root)?;
        let tmp = self
            .root
            .join(format!(".{STATE_FILE}.{}.tmp", uuid::Uuid::new_v4()));
        let json = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, self.root.join(STATE_FILE))?;
        Ok(())
    }

    fn update_state(
        &self,
        change: impl FnOnce(&mut BTreeSet<String>),
    ) -> Result<BTreeSet<String>, SpeechError> {
        let _guard = self.state_lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut state = self.read_state();
        change(&mut state.langs);
        self.write_state(&state)?;
        Ok(state.langs)
    }

    /// Bytes of `model` not yet on disk at the right size.
    fn missing_bytes(&self, model: &ModelSpec) -> u64 {
        let dir = self.model_dir(model);
        model
            .files()
            .iter()
            .filter(|file| file_len(&dir.join(file.name)) != Some(file.size))
            .map(|file| file.size)
            .sum()
    }

    fn pack_ready(&self, installed: &BTreeSet<String>, pack: &LanguageSpec) -> bool {
        installed.contains(pack.code)
            && registry::model(pack.model_id).is_some_and(|model| self.missing_bytes(model) == 0)
    }

    pub fn packs(&self) -> Vec<SpeechPack> {
        let installed = self.read_state().langs;
        registry::LANGUAGES
            .iter()
            .filter_map(|pack| {
                let model = registry::model(pack.model_id)?;
                Some(SpeechPack {
                    lang: pack.code.to_string(),
                    name: pack.name.to_string(),
                    native_name: pack.native_name.to_string(),
                    model_id: model.id.to_string(),
                    model_label: model.label.to_string(),
                    download_bytes: self.missing_bytes(model),
                    size_bytes: model.size_bytes(),
                    installed: self.pack_ready(&installed, pack),
                    shared_with: registry::LANGUAGES
                        .iter()
                        .filter(|other| other.model_id == pack.model_id && other.code != pack.code)
                        .map(|other| other.code.to_string())
                        .collect(),
                    upgrades: pack.is_upgrade().then(|| pack.checks.to_string()),
                    limited_accuracy: pack.limited_accuracy,
                })
            })
            .collect()
    }

    /// Download whatever `lang`'s model is missing, verify it, and mark the
    /// language installed. `progress(downloaded, total)` is called as bytes
    /// arrive.
    pub async fn install(
        &self,
        lang: &str,
        client: &reqwest::Client,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<(), SpeechError> {
        let pack =
            registry::language(lang).ok_or_else(|| SpeechError::UnknownLanguage(lang.into()))?;
        let model = registry::model(pack.model_id)
            .ok_or_else(|| SpeechError::UnknownLanguage(lang.into()))?;
        let lock = self.model_lock(model);
        let _guard = lock.lock().await;

        let dir = self.model_dir(model);
        tokio::fs::create_dir_all(&dir).await?;
        // Under the model lock, any partial download is left over from an
        // interrupted run.
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_name().to_string_lossy().ends_with(".part") {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
        let mut missing: Vec<&ModelFile> = Vec::new();
        for file in model.files() {
            let path = dir.join(file.name);
            let expected = file.sha256;
            let size = file.size;
            let valid = tokio::task::spawn_blocking(move || {
                file_len(&path) == Some(size)
                    && file_sha256(&path).is_ok_and(|digest| digest == expected)
            })
            .await
            .unwrap_or(false);
            if !valid {
                missing.push(file);
            }
        }

        let total: u64 = missing.iter().map(|file| file.size).sum();
        let mut done = 0u64;
        progress(0, total);
        for file in missing {
            let before = done;
            download_file(client, file, &dir, &mut |bytes| {
                progress(before + bytes, total)
            })
            .await?;
            done += file.size;
        }
        progress(total, total);

        self.update_state(|langs| {
            langs.insert(pack.code.to_string());
        })?;
        Ok(())
    }

    /// Mark `lang` uninstalled and delete model files no installed language
    /// still needs.
    pub async fn remove(&self, lang: &str) -> Result<(), SpeechError> {
        let pack =
            registry::language(lang).ok_or_else(|| SpeechError::UnknownLanguage(lang.into()))?;
        let model = registry::model(pack.model_id)
            .ok_or_else(|| SpeechError::UnknownLanguage(lang.into()))?;
        let lock = self.model_lock(model);
        let _guard = lock.lock().await;

        let mut freed = Vec::new();
        self.update_state(|langs| {
            freed = models_freed_by_removal(langs, pack.code);
            langs.remove(pack.code);
        })?;
        for model_id in freed {
            let Some(model) = registry::model(model_id) else {
                continue;
            };
            self.evict(model_id);
            match tokio::fs::remove_dir_all(self.model_dir(model)).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn evict(&self, model_id: &str) {
        let mut slot = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|(id, _)| *id == model_id) {
            *slot = None;
        }
    }

    fn engine_for(&self, model: &'static ModelSpec) -> Result<Arc<Mutex<Engine>>, SpeechError> {
        let mut slot = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((id, engine)) = slot.as_ref() {
            if *id == model.id {
                return Ok(engine.clone());
            }
        }
        // Drop the previous model before loading the next one.
        *slot = None;
        let engine = Engine::load(&self.model_dir(model), model)
            .map_err(|e| SpeechError::Engine(format!("could not load {}: {e}", model.id)))?;
        let engine = Arc::new(Mutex::new(engine));
        *slot = Some((model.id, engine.clone()));
        Ok(engine)
    }

    /// Score `text` read aloud in `pcm`. Blocking: run it off the async
    /// runtime.
    pub fn check(
        &self,
        lang: &str,
        text: &str,
        pcm: &[f32],
    ) -> Result<SpeechCheckResult, SpeechError> {
        if registry::language(lang).is_none() {
            return Err(SpeechError::UnknownLanguage(lang.into()));
        }
        if pcm.is_empty() {
            return Err(SpeechError::InvalidAudio("the recording is empty".into()));
        }
        if pcm.len() > MAX_SAMPLES {
            return Err(SpeechError::InvalidAudio(
                "the recording is longer than 30 seconds".into(),
            ));
        }
        let installed = self.read_state().langs;
        let pack = registry::packs_for_check(lang)
            .into_iter()
            .find(|pack| self.pack_ready(&installed, pack))
            .ok_or_else(|| SpeechError::PackMissing(lang.into()))?;
        let model =
            registry::model(pack.model_id).ok_or_else(|| SpeechError::PackMissing(lang.into()))?;
        let engine = self.engine_for(model)?;
        let mut engine = engine.lock().unwrap_or_else(|p| p.into_inner());
        let per_character = matches!(pack.checks, "zh" | "ja");
        engine
            .check(pack.whisper_lang, per_character, text, pcm)
            .map(SpeechCheckResult::from)
            .map_err(|e| SpeechError::Engine(e.to_string()))
    }
}

/// HTTP client for pack downloads: no overall deadline (packs are large),
/// but a stalled connection fails.
pub fn download_client() -> Result<reqwest::Client, SpeechError> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .https_only(true)
        .build()
        .map_err(|e| SpeechError::Download {
            file: "client".into(),
            reason: e.to_string(),
        })
}

/// Stream `file` into `dir`, verifying size and SHA-256 before it takes its
/// final name. Fails closed: on any mismatch nothing is left behind.
async fn download_file(
    client: &reqwest::Client,
    file: &ModelFile,
    dir: &Path,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), SpeechError> {
    let tmp = dir.join(format!(".{}.{}.part", file.name, uuid::Uuid::new_v4()));
    let result = fetch_to(client, file, &tmp, progress).await;
    match result {
        Ok(()) => {
            tokio::fs::rename(&tmp, dir.join(file.name)).await?;
            Ok(())
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(error)
        }
    }
}

async fn fetch_to(
    client: &reqwest::Client,
    file: &ModelFile,
    tmp: &Path,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<(), SpeechError> {
    use tokio::io::AsyncWriteExt;
    let fail = |reason: String| SpeechError::Download {
        file: file.name.to_string(),
        reason,
    };
    let mut response = client
        .get(file.url)
        .send()
        .await
        .map_err(|e| fail(describe(&e)))?;
    if !response.status().is_success() {
        return Err(fail(format!("HTTP {}", response.status())));
    }
    if let Some(length) = response.content_length() {
        if length != file.size {
            return Err(fail(format!(
                "expected {} bytes, server offers {length}",
                file.size
            )));
        }
    }
    let mut out = tokio::fs::File::create(tmp).await?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut reported = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|e| fail(describe(&e)))? {
        received += chunk.len() as u64;
        if received > file.size {
            return Err(fail(format!("more than the expected {} bytes", file.size)));
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await?;
        if received - reported >= PROGRESS_STEP {
            reported = received;
            progress(received);
        }
    }
    out.flush().await?;
    out.sync_all().await?;
    drop(out);
    verify(file, received, &hex::encode(hasher.finalize())).map_err(fail)?;
    progress(received);
    Ok(())
}

/// A reqwest error with its causes ("error sending request: … dns error …").
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Size and digest check for a finished download.
fn verify(file: &ModelFile, received: u64, sha256: &str) -> Result<(), String> {
    if received != file.size {
        return Err(format!("expected {} bytes, received {received}", file.size));
    }
    if sha256 != file.sha256 {
        return Err(format!("SHA-256 mismatch (got {sha256})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(codes: &[&str]) -> BTreeSet<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn decodes_little_endian_pcm() {
        let pcm = pcm_from_le_bytes(&[0x00, 0x00, 0xff, 0x7f, 0x00, 0x80, 0x00, 0x40]).unwrap();
        assert_eq!(pcm, [0.0, 32767.0 / 32768.0, -1.0, 0.5]);
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode([0x00, 0x40]);
        assert_eq!(pcm_from_base64(&encoded).unwrap(), [0.5]);
    }

    #[test]
    fn rejects_empty_odd_long_or_malformed_recordings() {
        assert!(matches!(
            pcm_from_le_bytes(&[]),
            Err(SpeechError::InvalidAudio(_))
        ));
        assert!(matches!(
            pcm_from_le_bytes(&[1, 2, 3]),
            Err(SpeechError::InvalidAudio(_))
        ));
        let too_long = vec![0u8; (MAX_SAMPLES + 1) * 2];
        assert!(matches!(
            pcm_from_le_bytes(&too_long),
            Err(SpeechError::InvalidAudio(_))
        ));
        assert!(pcm_from_le_bytes(&vec![0u8; MAX_SAMPLES * 2]).is_ok());
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&too_long);
        assert!(matches!(
            pcm_from_base64(&encoded),
            Err(SpeechError::InvalidAudio(_))
        ));
        assert!(matches!(
            pcm_from_base64("not base64!"),
            Err(SpeechError::InvalidAudio(_))
        ));
        assert!(matches!(
            pcm_from_base64(""),
            Err(SpeechError::InvalidAudio(_))
        ));
    }

    #[test]
    fn removing_a_shared_language_keeps_the_model_others_need() {
        assert!(models_freed_by_removal(&set(&["hi", "es"]), "hi").is_empty());
        assert_eq!(
            models_freed_by_removal(&set(&["hi"]), "hi"),
            ["whisper-tiny-q8"]
        );
        assert_eq!(
            models_freed_by_removal(&set(&["en", "hi"]), "en"),
            ["whisper-tiny-en-q8"]
        );
        // The upgrade pack has its own model; removing it leaves tiny.en.
        assert_eq!(
            models_freed_by_removal(&set(&["en", "en-enhanced"]), "en-enhanced"),
            ["whisper-base-en"]
        );
        assert!(models_freed_by_removal(&set(&[]), "xx").is_empty());
    }

    #[test]
    fn verify_fails_closed_on_size_or_digest_mismatch() {
        let body = b"hello speech";
        let digest = hex_sha256(body);
        let file = ModelFile {
            name: "f.bin",
            url: "https://example.invalid/f.bin",
            size: body.len() as u64,
            sha256: Box::leak(digest.clone().into_boxed_str()),
        };
        assert!(verify(&file, body.len() as u64, &digest).is_ok());
        assert!(verify(&file, body.len() as u64 - 1, &digest).is_err());
        assert!(verify(&file, body.len() as u64, &hex_sha256(b"other")).is_err());
    }

    #[tokio::test]
    async fn failed_download_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let file = ModelFile {
            name: "model.gguf",
            // Refused before any request: the client is HTTPS-only.
            url: "http://127.0.0.1:9/model.gguf",
            size: 10,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        };
        let client = download_client().unwrap();
        let result = download_file(&client, &file, dir.path(), &mut |_| {}).await;
        assert!(matches!(result, Err(SpeechError::Download { .. })));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn place(service: &SpeechService, model: &ModelSpec) {
        let dir = service.model_dir(model);
        std::fs::create_dir_all(&dir).unwrap();
        for file in model.files() {
            let handle = std::fs::File::create(dir.join(file.name)).unwrap();
            handle.set_len(file.size).unwrap();
        }
    }

    #[tokio::test]
    async fn packs_report_shared_downloads_and_removal_keeps_shared_files() {
        let root = tempfile::tempdir().unwrap();
        let service = SpeechService::new(root.path().join(SPEECH_DIR));
        let packs = service.packs();
        let hi = packs.iter().find(|p| p.lang == "hi").unwrap();
        assert!(!hi.installed);
        assert_eq!(hi.download_bytes, registry::WHISPER_TINY_Q8.size_bytes());
        assert!(hi.shared_with.contains(&"es".to_string()));
        assert_eq!(hi.shared_with.len(), 11);
        let enhanced = packs.iter().find(|p| p.lang == "en-enhanced").unwrap();
        assert_eq!(enhanced.upgrades.as_deref(), Some("en"));
        assert!(enhanced.shared_with.is_empty());

        // Simulate two installed languages on the shared model.
        place(&service, &registry::WHISPER_TINY_Q8);
        service
            .update_state(|langs| {
                langs.insert("hi".into());
                langs.insert("es".into());
            })
            .unwrap();
        let packs = service.packs();
        let es = packs.iter().find(|p| p.lang == "es").unwrap();
        assert!(es.installed);
        let fr = packs.iter().find(|p| p.lang == "fr").unwrap();
        assert!(!fr.installed);
        assert_eq!(fr.download_bytes, 0, "shared model is already on disk");

        let model_dir = service.model_dir(&registry::WHISPER_TINY_Q8);
        service.remove("hi").await.unwrap();
        assert!(model_dir.exists(), "es still needs the model");
        service.remove("es").await.unwrap();
        assert!(!model_dir.exists());
        assert!(service.read_state().langs.is_empty());
    }

    /// Downloads the English pack (42 MB) from its pinned URLs. Multi-thread
    /// runtime, as Tauri's: on the single-thread test runtime the connection
    /// never completes.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "downloads from Hugging Face"]
    async fn installs_and_removes_a_pack_from_the_network() {
        let root = tempfile::tempdir().unwrap();
        let service = SpeechService::new(root.path().join(SPEECH_DIR));
        let client = download_client().unwrap();
        let seen = Mutex::new(Vec::new());
        let progress = |done: u64, total: u64| seen.lock().unwrap().push((done, total));
        service.install("en", &client, &progress).await.unwrap();
        let seen = seen.into_inner().unwrap();
        let total = registry::WHISPER_TINY_EN_Q8.size_bytes();
        assert_eq!(seen.first(), Some(&(0, total)));
        assert_eq!(seen.last(), Some(&(total, total)));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
        let en = service
            .packs()
            .into_iter()
            .find(|p| p.lang == "en")
            .unwrap();
        assert!(en.installed);
        assert_eq!(en.download_bytes, 0);
        // A second install verifies the files and downloads nothing.
        service
            .install("en", &client, &|_, total| assert_eq!(total, 0))
            .await
            .unwrap();
        let error = service.check("en", "Hello there.", &[0.0; 16_000]).unwrap();
        assert_eq!(error.overall, 0.0, "silence");
        service.remove("en").await.unwrap();
        assert!(!service.model_dir(&registry::WHISPER_TINY_EN_Q8).exists());
    }

    #[test]
    fn check_reports_a_missing_pack_with_the_plugin_prefix() {
        let root = tempfile::tempdir().unwrap();
        let service = SpeechService::new(root.path().to_path_buf());
        let error = service.check("hi", "नमस्ते", &[0.0; 1600]).unwrap_err();
        assert!(
            error.to_string().starts_with(PACK_MISSING_PREFIX),
            "{error}"
        );
        assert_eq!(error.to_string(), "speech_pack_missing: hi");
        let error = service.check("xx", "hello", &[0.0; 1600]).unwrap_err();
        assert!(matches!(error, SpeechError::UnknownLanguage(_)));
        let error = service.check("en", "hello", &[]).unwrap_err();
        assert!(matches!(error, SpeechError::InvalidAudio(_)));
    }
}
