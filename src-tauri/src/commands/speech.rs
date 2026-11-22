//! Pronunciation-check language packs and checks (`crate::speech`).
//!
//! Device-level commands: packs are shared by every profile and a check
//! reads no profile data, so none of these take a profile lease (a pack
//! download can run for minutes and must not hold a profile open). Each is
//! listed in `src/composables/profile-command-policy.json`.

use tauri::{AppHandle, Emitter, State};

use crate::speech::{self, PackProgress, SpeechCheckResult, SpeechPack};
use crate::AppState;

/// Every language pack with its download size and install state.
#[tauri::command]
pub async fn speech_packs_list(state: State<'_, AppState>) -> Result<Vec<SpeechPack>, String> {
    let service = speech::service(&state.app_data_dir);
    tauri::async_runtime::spawn_blocking(move || service.packs())
        .await
        .map_err(|e| e.to_string())
}

/// Download (if needed) and install `lang`'s pack, emitting
/// `speech-pack-progress` while bytes arrive. Returns the updated list.
#[tauri::command]
pub async fn speech_pack_install(
    app: AppHandle,
    state: State<'_, AppState>,
    lang: String,
) -> Result<Vec<SpeechPack>, String> {
    let service = speech::service(&state.app_data_dir);
    let model_id = speech::registry::language(&lang)
        .map(|pack| pack.model_id.to_string())
        .ok_or_else(|| speech::SpeechError::UnknownLanguage(lang.clone()).to_string())?;
    let client = speech::download_client().map_err(|e| e.to_string())?;
    let progress = |downloaded: u64, total: u64| {
        let _ = app.emit(
            speech::PROGRESS_EVENT,
            PackProgress {
                model_id: model_id.clone(),
                lang: lang.clone(),
                downloaded,
                total,
            },
        );
    };
    service
        .install(&lang, &client, &progress)
        .await
        .map_err(|e| e.to_string())?;
    Ok(service.packs())
}

/// Uninstall `lang`, deleting model files no installed language still
/// needs. Returns the updated list.
#[tauri::command]
pub async fn speech_pack_remove(
    state: State<'_, AppState>,
    lang: String,
) -> Result<Vec<SpeechPack>, String> {
    let service = speech::service(&state.app_data_dir);
    service.remove(&lang).await.map_err(|e| e.to_string())?;
    Ok(service.packs())
}

/// Score `text` read aloud in `pcm16_b64` (base64 16 kHz mono 16-bit
/// little-endian PCM, at most 30 s). Fails with `speech_pack_missing: <lang>`
/// when no pack for the language is installed.
#[tauri::command]
pub async fn speech_check(
    state: State<'_, AppState>,
    lang: String,
    text: String,
    pcm16_b64: String,
) -> Result<SpeechCheckResult, String> {
    let service = speech::service(&state.app_data_dir);
    tauri::async_runtime::spawn_blocking(move || {
        let pcm = speech::pcm_from_base64(&pcm16_b64)?;
        service.check(&lang, &text, &pcm)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
