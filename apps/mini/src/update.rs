//! Self-update from GitHub releases (Linux, `updater` feature).
//!
//! The release artifact for this app is a tarball with the player and the
//! decoder side by side — the two travel together or the sibling lookup
//! breaks — so updating means: fetch `releases/latest`, pick the tarball for
//! this architecture, unpack it, and swap both binaries next to
//! `current_exe`. Linux lets a running executable be renamed out from under
//! itself, which is the whole trick: rename the old ones aside, rename the
//! new ones in, and the swap is atomic per file on the same filesystem.
//!
//! Off by `--no-default-features` like the tray: the Alpine package updates
//! through apk, and a package-managed binary must not overwrite itself.
//!
//! Workers report over a channel the UI timer drains, like everything else
//! in this app that leaves the main thread.

#[derive(Debug)]
pub enum UpdateMsg {
    UpToDate,
    Available { version: String, url: String },
    Installed(String),
    Error(String),
}

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(all(target_os = "linux", feature = "updater"))]
mod imp {
    use super::UpdateMsg;
    use std::path::Path;
    use std::sync::mpsc::Sender;

    const GITHUB_REPO: &str = "jjolmo/tunante";

    fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
        let mut it = v.trim().trim_start_matches('v').splitn(3, '.');
        Some((
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
        ))
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .user_agent("tunante-mini")
            .build()
            .into()
    }

    /// Ask GitHub what the newest release is and whether it beats us.
    pub fn spawn_check(tx: Sender<UpdateMsg>) {
        std::thread::spawn(move || {
            let msg = check().unwrap_or_else(UpdateMsg::Error);
            let _ = tx.send(msg);
        });
    }

    fn check() -> Result<UpdateMsg, String> {
        let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
        let body: serde_json::Value = agent()
            .get(&url)
            .call()
            .map_err(|e| format!("GitHub no contesta: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("respuesta rara de GitHub: {e}"))?;

        let tag = body["tag_name"].as_str().unwrap_or_default().to_string();
        let (Some(remote), Some(local)) =
            (parse_version(&tag), parse_version(super::CURRENT_VERSION))
        else {
            return Err(format!("versión ilegible: {tag}"));
        };
        if remote <= local {
            return Ok(UpdateMsg::UpToDate);
        }

        let wanted = format!(
            "tunante-mini-{}-linux-gnu.tar.gz",
            std::env::consts::ARCH
        );
        let asset = body["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["name"].as_str() == Some(wanted.as_str()))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(str::to_string);

        match asset {
            Some(url) => Ok(UpdateMsg::Available {
                version: tag.trim_start_matches('v').to_string(),
                url,
            }),
            // A newer release without our tarball: mini's jobs only run when
            // its code changed, so this is ordinary, not an error.
            None => Ok(UpdateMsg::UpToDate),
        }
    }

    /// Download the tarball and swap both binaries in place.
    pub fn spawn_install(tx: Sender<UpdateMsg>, version: String, url: String) {
        std::thread::spawn(move || {
            let msg = match install(&url) {
                Ok(()) => UpdateMsg::Installed(version),
                Err(e) => UpdateMsg::Error(e),
            };
            let _ = tx.send(msg);
        });
    }

    fn install(url: &str) -> Result<(), String> {
        let bytes = agent()
            .get(url)
            .call()
            .map_err(|e| format!("descarga fallida: {e}"))?
            .body_mut()
            .with_config()
            // The stripped pair is ~30 MB; a corrupted CDN answer should fail
            // loudly rather than fill the disk.
            .limit(512 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("descarga fallida: {e}"))?;

        let work = std::env::temp_dir().join(format!("tunante-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let tarball = work.join("release.tar.gz");
        std::fs::write(&tarball, &bytes).map_err(|e| e.to_string())?;

        // The system tar: this is Linux-only code, and every Linux this runs
        // on has one — a tar crate would be a dependency for one execvp.
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&work)
            .status()
            .map_err(|e| format!("no hay tar: {e}"))?;
        if !status.success() {
            return Err("el tarball no se pudo extraer".to_string());
        }

        let name = format!("tunante-mini-{}-linux-gnu", std::env::consts::ARCH);
        let unpacked = work.join(&name);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe.parent().ok_or("el ejecutable no tiene directorio")?;

        for bin in ["tunante-mini", "tunante-decoder"] {
            let fresh = unpacked.join(bin);
            if !fresh.is_file() {
                return Err(format!("el tarball no trae {bin}"));
            }
            swap_in(&fresh, &dir.join(bin))?;
        }

        let _ = std::fs::remove_dir_all(&work);
        Ok(())
    }

    /// Replace `dest` with `fresh`: copy in as a sibling, then two renames.
    /// Rename is atomic on one filesystem, and Linux is fine renaming the
    /// binary this very process is running from.
    fn swap_in(fresh: &Path, dest: &Path) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;

        let incoming = dest.with_extension("new");
        let outgoing = dest.with_extension("old");
        std::fs::copy(fresh, &incoming)
            .map_err(|e| format!("no se pudo escribir junto a {}: {e}", dest.display()))?;
        std::fs::set_permissions(&incoming, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        if dest.exists() {
            std::fs::rename(dest, &outgoing).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&incoming, dest).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&outgoing);
        Ok(())
    }
}

#[cfg(not(all(target_os = "linux", feature = "updater")))]
mod imp {
    use super::UpdateMsg;
    use std::sync::mpsc::Sender;

    pub fn spawn_check(tx: Sender<UpdateMsg>) {
        let _ = tx.send(UpdateMsg::Error(
            "esta build se actualiza con su gestor de paquetes".to_string(),
        ));
    }

    pub fn spawn_install(tx: Sender<UpdateMsg>, _version: String, _url: String) {
        let _ = tx.send(UpdateMsg::Error(
            "esta build se actualiza con su gestor de paquetes".to_string(),
        ));
    }
}

pub use imp::{spawn_check, spawn_install};
