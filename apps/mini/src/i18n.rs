//! Translating the strings that live in Rust, not in `.slint`.
//!
//! Slint's bundled translations only reach `@tr(…)` in the markup; the track
//! table's column headers and the tray menu are built here, in Rust, so they
//! need their own path to the same `.po` files. Those files are embedded at
//! compile time (the same ones the UI bundles) and the one for the active
//! language is parsed once into a map; [`tr`] looks a source string up in it.
//!
//! The source language is Spanish, so an untranslated string — or the Spanish
//! and "system-is-Spanish" cases, where the catalog is empty — is returned
//! unchanged. Keep this in step with `build.rs`'s bundled set.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The embedded catalogs, by language code. Mirrors `translations/`.
const CATALOGS: &[(&str, &str)] = &[
    ("en", include_str!("../translations/en/LC_MESSAGES/tunante-mini.po")),
    ("fr", include_str!("../translations/fr/LC_MESSAGES/tunante-mini.po")),
    ("de", include_str!("../translations/de/LC_MESSAGES/tunante-mini.po")),
    ("it", include_str!("../translations/it/LC_MESSAGES/tunante-mini.po")),
    ("pt", include_str!("../translations/pt/LC_MESSAGES/tunante-mini.po")),
];

static ACTIVE: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Load the catalog for the resolved language once. `""`, `"es"` and any code
/// with no embedded `.po` leave an empty map, so [`tr`] returns the source.
/// Call once at startup, before any [`tr`]; later calls are ignored (the map
/// is fixed for the process — a language change from the settings takes full
/// effect on the next launch, like the note on the selector says).
pub fn init(lang: &str) {
    ACTIVE.get_or_init(|| {
        CATALOGS
            .iter()
            .find(|(code, _)| *code == lang)
            .map(|(_, po)| parse_po(po))
            .unwrap_or_default()
    });
}

/// Translate a source (Spanish) string, or return it unchanged when there is no
/// translation. Cheap after [`init`]: one hash lookup and a clone.
pub fn tr(source: &str) -> String {
    ACTIVE
        .get()
        .and_then(|m| m.get(source))
        .cloned()
        .unwrap_or_else(|| source.to_string())
}

/// A minimal gettext `.po` reader: `msgid`/`msgstr` pairs, each string possibly
/// split over continuation lines. Enough for these files; comments, the empty
/// header entry, and `msgctxt` (which the bundle is built without) are ignored.
fn parse_po(src: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut id = String::new();
    let mut st = String::new();
    // 0 = between entries, 1 = reading msgid, 2 = reading msgstr.
    let mut mode = 0u8;

    let flush = |id: &mut String, st: &mut String, map: &mut HashMap<String, String>| {
        if !id.is_empty() && !st.is_empty() {
            map.insert(id.clone(), st.clone());
        }
        id.clear();
        st.clear();
    };

    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            if mode == 2 {
                flush(&mut id, &mut st, &mut map);
                mode = 0;
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("msgid ") {
            if mode == 2 {
                flush(&mut id, &mut st, &mut map);
            }
            id = unquote(rest);
            mode = 1;
        } else if let Some(rest) = t.strip_prefix("msgstr ") {
            st = unquote(rest);
            mode = 2;
        } else if t.starts_with('"') {
            // A continuation line for whichever field is being read.
            let piece = unquote(t);
            match mode {
                1 => id.push_str(&piece),
                2 => st.push_str(&piece),
                _ => {}
            }
        }
    }
    flush(&mut id, &mut st, &mut map);
    map
}

/// Strip the surrounding quotes from a `.po` string token and unescape it.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let inner = s
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}
