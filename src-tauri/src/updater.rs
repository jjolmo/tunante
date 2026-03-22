use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;
use std::sync::Arc;
use crate::AppState;

const GITHUB_REPO: &str = "jjolmo/tunante";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_notes: String,
    pub download_url: String,
    pub release_url: String,
    pub asset_name: String,
    pub asset_size: u64,
}

/// Get the current app version from Cargo.toml
fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Compare two semver strings. Returns true if `latest` is newer than `current`.
fn is_newer(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let s = s.trim_start_matches('v');
        let parts: Vec<&str> = s.split('.').collect();
        (
            parts.first().and_then(|p| p.parse().ok()).unwrap_or(0),
            parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0),
            parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0),
        )
    };
    let c = parse(current);
    let l = parse(latest);
    l > c
}

/// Determine which asset name to look for based on OS and arch.
fn target_asset_pattern() -> Vec<String> {
    let mut patterns = Vec::new();

    #[cfg(target_os = "linux")]
    {
        #[cfg(target_arch = "x86_64")]
        patterns.push("amd64".to_string());
        #[cfg(target_arch = "aarch64")]
        patterns.push("arm64".to_string());
        // Prefer AppImage
        patterns.push("AppImage".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        patterns.push(".msi".to_string());
        patterns.push(".exe".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        patterns.push(".dmg".to_string());
    }

    patterns
}

/// Find the best matching asset for this platform.
fn find_asset(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    let patterns = target_asset_pattern();

    // Find asset that matches all patterns (e.g. "amd64" AND "AppImage")
    for asset in assets {
        let name_lower = asset.name.to_lowercase();
        if patterns.iter().all(|p| name_lower.contains(&p.to_lowercase())) {
            return Some(asset);
        }
    }

    // Fallback: match any pattern
    for pattern in &patterns {
        for asset in assets {
            if asset.name.to_lowercase().contains(&pattern.to_lowercase()) {
                return Some(asset);
            }
        }
    }

    None
}

/// Check GitHub for the latest release.
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheck, String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::Client::builder()
        .user_agent("Tunante-Updater")
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to check for updates: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API error: {}", response.status()));
    }

    let release: ReleaseInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {}", e))?;

    let current = current_version();
    let latest = release.tag_name.trim_start_matches('v').to_string();
    let update_available = is_newer(&current, &release.tag_name);

    let (download_url, asset_name, asset_size) = find_asset(&release.assets)
        .map(|a| (a.browser_download_url.clone(), a.name.clone(), a.size))
        .unwrap_or_else(|| (release.html_url.clone(), String::new(), 0));

    Ok(UpdateCheck {
        current_version: current,
        latest_version: latest,
        update_available,
        release_notes: release.body,
        download_url,
        release_url: release.html_url,
        asset_name,
        asset_size,
    })
}

/// Download and apply update (Linux AppImage only).
/// On other platforms, opens the download URL in the browser.
#[tauri::command]
pub async fn download_and_apply_update(
    download_url: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        // Find current executable path
        let current_exe = std::env::current_exe()
            .map_err(|e| format!("Can't find current executable: {}", e))?;

        // Only self-replace if running as AppImage
        if current_exe
            .to_string_lossy()
            .contains("AppImage")
            || std::env::var("APPIMAGE").is_ok()
        {
            let appimage_path = std::env::var("APPIMAGE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| current_exe.clone());

            let tmp_path = appimage_path.with_extension("new");

            // Download new AppImage
            let client = reqwest::Client::builder()
                .user_agent("Tunante-Updater")
                .build()
                .map_err(|e| e.to_string())?;

            let response = client
                .get(&download_url)
                .send()
                .await
                .map_err(|e| format!("Download failed: {}", e))?;

            if !response.status().is_success() {
                return Err(format!("Download error: {}", response.status()));
            }

            let bytes = response
                .bytes()
                .await
                .map_err(|e| format!("Failed to read download: {}", e))?;

            // Write to temp file
            std::fs::write(&tmp_path, &bytes)
                .map_err(|e| format!("Failed to write update: {}", e))?;

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("Failed to set permissions: {}", e))?;
            }

            // Replace old with new (atomic rename)
            std::fs::rename(&tmp_path, &appimage_path)
                .map_err(|e| format!("Failed to replace AppImage: {}", e))?;

            return Ok("Update applied! Restart the app to use the new version.".to_string());
        }
    }

    // Non-AppImage or Windows/Mac: open download URL in browser
    let _ = open::that(&download_url);
    Ok("Download opened in browser.".to_string())
}
