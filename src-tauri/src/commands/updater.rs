//! Cross-platform update-availability check.
//!
//! The desktop auto-updater (`tauri-plugin-updater`) is desktop-only — its
//! iOS/Android support level is `none`, and app stores forbid self-updating
//! binaries. So on mobile we can't *install* an update, but we can still tell
//! the user one exists: this command fetches the same signed `latest.json`
//! manifest the desktop updater polls and returns its version, so the UI can
//! compare it to the running version and surface a "new version available"
//! notice with a link. Runs through Rust (reqwest) because the webview CSP
//! blocks direct `fetch()` to GitHub.

use serde::Serialize;

/// The published updater manifest URL (mirrors `tauri.conf.json` → updater
/// endpoints). GitHub resolves `releases/latest/download/...` to the current
/// non-prerelease "latest" release.
const MANIFEST_URL: &str =
    "https://github.com/ifftu-dev/alexandria/releases/latest/download/latest.json";

/// The human-facing releases page a mobile user is sent to (there is no
/// in-app install path on mobile).
const RELEASES_URL: &str = "https://github.com/ifftu-dev/alexandria/releases/latest";

#[derive(Debug, Serialize)]
pub struct UpdateManifestInfo {
    /// Version string from the manifest (e.g. `"0.4.5-alpha"`).
    pub version: String,
    /// Release notes, if the manifest carries any.
    pub notes: String,
    /// Where to send the user to download it manually (mobile has no
    /// self-install path).
    pub releases_url: String,
}

/// Fetch the published updater manifest and return its version. Returns `Ok(None)`
/// when the manifest can't be reached or parsed — a best-effort check must never
/// surface an error to the user (they simply see no update notice).
#[tauri::command]
pub async fn fetch_update_manifest() -> Result<Option<UpdateManifestInfo>, String> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("update check: client build failed: {e}");
            return Ok(None);
        }
    };

    let resp = match client.get(MANIFEST_URL).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            log::debug!("update check: manifest status {}", r.status());
            return Ok(None);
        }
        Err(e) => {
            log::debug!("update check: fetch failed: {e}");
            return Ok(None);
        }
    };

    let value: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            log::debug!("update check: manifest not JSON: {e}");
            return Ok(None);
        }
    };

    let Some(version) = value.get("version").and_then(|v| v.as_str()) else {
        return Ok(None);
    };

    Ok(Some(UpdateManifestInfo {
        version: version.to_string(),
        notes: value
            .get("notes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        releases_url: RELEASES_URL.to_string(),
    }))
}
