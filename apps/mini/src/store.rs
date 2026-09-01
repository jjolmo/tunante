//! Which database this app opens — the unification step of fase 4 of
//! docs/plan-desktop-slint.md.
//!
//! The desktop app's database is the senior one: it holds the big library,
//! the ratings and the classification corrections. When it exists, this app
//! opens it — the "mini mode" of the final app is the same program over the
//! same data — and, once, imports this shell's own `mini.*` settings from the
//! old database so nothing set on the phone-shaped shell is lost. The keys
//! both apps share on purpose (`audio_output_device`, `dsp_config`) are NOT
//! imported: for those the desktop's values win, which is what "senior"
//! means.
//!
//! Without a desktop database — the phone, a fresh machine — everything
//! stays exactly as it always was. Both databases are WAL, so the desktop
//! app staying open while this one joins is the ordinary case, not a hazard.

use std::path::{Path, PathBuf};
use tunante_core::db::Database;

fn data_home() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".local/share")
        })
}

/// The database this run should open.
pub fn resolve() -> Result<PathBuf, std::io::Error> {
    resolve_in(&data_home())
}

fn resolve_in(base: &Path) -> Result<PathBuf, std::io::Error> {
    // Where the Tauri desktop keeps it: app_data_dir for the identifier
    // com.tunante.app. Hardcoded rather than asked, because the whole point
    // is opening that file without being that app.
    let desktop = base.join("com.tunante.app").join("tunante.db");
    let mini_dir = base.join("tunante-mini");
    let mini = mini_dir.join("tunante-mini.db");

    if !desktop.exists() {
        std::fs::create_dir_all(&mini_dir)?;
        return Ok(mini);
    }

    // Import once, and only the namespaced keys. Guarded by a marker in the
    // destination, so a phone-shell setting changed later in the old database
    // can never silently overwrite what the user has since chosen here.
    if mini.exists() {
        if let (Ok(dst), Ok(src)) = (Database::new(&desktop), Database::new(&mini)) {
            let done = dst
                .get_setting("mini.imported")
                .ok()
                .flatten()
                .is_some();
            if !done {
                let moved = src
                    .get_all_settings()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| s.key.starts_with("mini."))
                    .map(|s| dst.set_setting(&s.key, &s.value).is_ok())
                    .filter(|ok| *ok)
                    .count();
                let _ = dst.set_setting("mini.imported", "1");
                eprintln!("base de datos del desktop adoptada; {moved} ajustes mini.* importados");
            }
        }
    }

    Ok(desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tunante-store-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_db(path: &Path) -> Database {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        Database::new(path).unwrap()
    }

    #[test]
    fn without_a_desktop_database_nothing_changes() {
        let base = base("solo");
        let got = resolve_in(&base).unwrap();
        assert_eq!(got, base.join("tunante-mini/tunante-mini.db"));
        assert!(got.parent().unwrap().is_dir(), "the mini dir must be created");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_desktop_database_is_adopted_and_mini_keys_imported_once() {
        let base = base("adopt");
        let desktop_path = base.join("com.tunante.app/tunante.db");
        let mini_path = base.join("tunante-mini/tunante-mini.db");

        let desktop = make_db(&desktop_path);
        // The senior database already has opinions on the shared keys.
        desktop.set_setting("audio_output_device", "Altavoces").unwrap();

        let mini = make_db(&mini_path);
        mini.set_setting("mini.volume", "0.5").unwrap();
        mini.set_setting("mini.ui_mode", "2").unwrap();
        // Shared key in the old database: must NOT travel.
        mini.set_setting("audio_output_device", "Cascos").unwrap();
        drop(mini);
        drop(desktop);

        let got = resolve_in(&base).unwrap();
        assert_eq!(got, desktop_path);

        let dst = Database::new(&desktop_path).unwrap();
        assert_eq!(dst.get_setting("mini.volume").unwrap().as_deref(), Some("0.5"));
        assert_eq!(dst.get_setting("mini.ui_mode").unwrap().as_deref(), Some("2"));
        assert_eq!(
            dst.get_setting("audio_output_device").unwrap().as_deref(),
            Some("Altavoces"),
            "shared keys stay the desktop's"
        );

        // Once means once: a later change in the old database stays there.
        let mini = Database::new(&mini_path).unwrap();
        mini.set_setting("mini.volume", "0.9").unwrap();
        drop(mini);
        drop(dst);
        let got = resolve_in(&base).unwrap();
        assert_eq!(got, desktop_path);
        let dst = Database::new(&desktop_path).unwrap();
        assert_eq!(
            dst.get_setting("mini.volume").unwrap().as_deref(),
            Some("0.5"),
            "the import must not repeat"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_desktop_database_without_an_old_mini_one_just_opens() {
        let base = base("fresh");
        let desktop_path = base.join("com.tunante.app/tunante.db");
        make_db(&desktop_path);

        let got = resolve_in(&base).unwrap();
        assert_eq!(got, desktop_path);
        let _ = std::fs::remove_dir_all(&base);
    }
}
