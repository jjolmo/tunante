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
///
/// These are the names Tauri's bundler actually produces, which are not
/// consistent between formats and are the reason this used to be wrong:
///
/// | | x86_64 | ARM |
/// |---|---|---|
/// | AppImage | `Tunante_1.2.3_amd64.AppImage` | `Tunante_1.2.3_aarch64.AppImage` |
/// | deb | `Tunante_1.2.3_amd64.deb` | `Tunante_1.2.3_arm64.deb` |
/// | dmg | — | `Tunante_1.2.3_aarch64.dmg` |
///
/// The Debian package says `arm64` and the AppImage beside it says `aarch64`.
/// Asking for `arm64` on an ARM machine therefore matched no AppImage at all,
/// and the loose fallback in `find_asset` then handed the user the **`.deb`** —
/// which `apply_update` refuses to install, so the update was offered and then
/// failed. Every asset pattern here is covered by a test against these names.
fn target_asset_pattern() -> Vec<String> {
    let mut patterns = Vec::new();

    #[cfg(target_os = "linux")]
    {
        #[cfg(target_arch = "x86_64")]
        patterns.push("amd64".to_string());
        #[cfg(target_arch = "aarch64")]
        patterns.push("aarch64".to_string());
        // Prefer AppImage
        patterns.push("AppImage".to_string());
    }

    // The MSI only. Listing `.exe` as well made the strict pass in `find_asset`
    // impossible to satisfy — no file is both — so it always fell through to
    // the loose one and picked the MSI by luck rather than by decision.
    #[cfg(target_os = "windows")]
    {
        patterns.push(".msi".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        #[cfg(target_arch = "x86_64")]
        patterns.push("x64".to_string());
        #[cfg(target_arch = "aarch64")]
        patterns.push("aarch64".to_string());
        patterns.push(".dmg".to_string());
    }

    patterns
}

#[cfg(test)]
mod asset_tests {
    use super::*;

    /// The real asset list of a release, so the patterns are checked against
    /// what the bundler emits rather than against what anyone remembers.
    fn assets() -> Vec<ReleaseAsset> {
        [
            "Tunante_0.1.277_aarch64.AppImage",
            "Tunante_0.1.277_amd64.AppImage",
            "Tunante_0.1.277_amd64.deb",
            "Tunante_0.1.277_arm64.deb",
            "Tunante_0.1.277_aarch64.dmg",
            "Tunante_0.1.277_x64.dmg",
            "Tunante_0.1.277_x64-setup.exe",
            "Tunante_0.1.277_x64_en-US.msi",
            "tunante-android-0.1.277.apk",
            "latest.json",
        ]
        .iter()
        .map(|n| ReleaseAsset {
            name: (*n).to_string(),
            browser_download_url: format!("https://example/{n}"),
            size: 1,
        })
        .collect()
    }

    /// Whatever this platform is, the strict pass has to hit — reaching the
    /// loose fallback is how an ARM machine was offered a Debian package.
    #[test]
    fn the_strict_pass_finds_something_for_this_platform() {
        let all = assets();
        let patterns = target_asset_pattern();
        let hit = all.iter().find(|a| {
            let n = a.name.to_lowercase();
            patterns.iter().all(|p| n.contains(&p.to_lowercase()))
        });
        assert!(hit.is_some(), "no asset matches all of {patterns:?}");
    }

    #[test]
    fn the_chosen_asset_is_installable_on_this_platform() {
        let all = assets();
        let picked = find_asset(&all).expect("nothing matched").name.clone();
        if cfg!(target_os = "linux") {
            assert!(picked.ends_with(".AppImage"), "picked {picked}");
            assert!(!picked.ends_with(".deb"), "a .deb cannot be self-applied");
        } else if cfg!(target_os = "windows") {
            assert!(picked.ends_with(".msi"), "picked {picked}");
        } else if cfg!(target_os = "macos") {
            assert!(picked.ends_with(".dmg"), "picked {picked}");
        }
    }

    /// The architecture has to be in the name, or a 64-bit machine happily
    /// downloads the other one's build.
    #[test]
    fn the_chosen_asset_is_for_this_architecture() {
        let picked = find_asset(&assets()).unwrap().name.to_lowercase();
        let other = if cfg!(target_arch = "x86_64") { ["aarch64", "arm64"] } else { ["amd64", "x64"] };
        for wrong in other {
            assert!(!picked.contains(wrong), "picked {picked}, which is {wrong}");
        }
    }
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
