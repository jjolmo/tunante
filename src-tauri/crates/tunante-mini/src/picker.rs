//! Browsing directories, for the first-run folder chooser.
//!
//! Our own browser rather than the XDG portal: on a phone the portal chooser is
//! a desktop file dialog squeezed into a phone-sized hole, and this only ever
//! has to do one thing — walk directories and let some be ticked.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct Picker {
    pub cwd: PathBuf,
    pub chosen: BTreeSet<PathBuf>,
}

pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub chosen: bool,
}

impl Picker {
    /// Start where the user's music most plausibly is.
    ///
    /// `~/Musica` and `~/Music` before `$HOME`, because landing straight on the
    /// answer saves the taps that a phone makes expensive.
    pub fn new() -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let start = ["Musica", "Música", "Music"]
            .iter()
            .map(|d| home.join(d))
            .find(|p| p.is_dir())
            .unwrap_or(home);

        Self { cwd: start, chosen: BTreeSet::new() }
    }

    pub fn entries(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        let Ok(dir) = std::fs::read_dir(&self.cwd) else {
            return out;
        };

        for e in dir.flatten() {
            let path = e.path();
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            // Dotfiles are noise here: nobody keeps their music in ~/.cache.
            if name.starts_with('.') {
                continue;
            }
            let chosen = self.chosen.contains(&path);
            out.push(Entry { name, path, chosen });
        }

        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out
    }

    pub fn enter(&mut self, path: &Path) {
        if path.is_dir() {
            self.cwd = path.to_path_buf();
        }
    }

    pub fn up(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
        }
    }

    /// Tick or untick a folder.
    ///
    /// Choosing a folder drops any of its descendants that were already ticked:
    /// scanning both would walk the same files twice and insert them twice.
    pub fn toggle(&mut self, path: &Path) {
        if self.chosen.remove(path) {
            return;
        }
        self.chosen.retain(|c| !c.starts_with(path));
        if self.chosen.iter().any(|c| path.starts_with(c)) {
            // An ancestor is already chosen, so this one adds nothing.
            return;
        }
        self.chosen.insert(path.to_path_buf());
    }
}
