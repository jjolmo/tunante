use std::path::Path;

fn main() {
    // slint-build reads the `.po` files under translations/ and bundles them
    // into the binary, but Cargo does not know they are build inputs — so a
    // change to a translation alone would not rebuild the bundle. Tell it to
    // re-run whenever any of them (or the tree) changes.
    emit_rerun("translations");

    // Bundled translations: every `.po` under `translations/<lang>/LC_MESSAGES/`
    // is compiled straight into the binary, so the app stays a single file on
    // every target (musl and Android included), with no runtime gettext. The
    // source strings are Spanish; a language with no `.po` — or a locale we
    // don't ship — falls back to them. Contributors add a language by dropping
    // in `translations/<lang>/LC_MESSAGES/tunante-mini.po`.
    //
    // No default translation context: `@tr("…")` keys are plain msgids with no
    // per-component `msgctxt`, which is what the `.po` files here expect.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile ui/app.slint");
}

/// Emit `cargo:rerun-if-changed` for a path and, recursively, everything under
/// it — a directory's own mtime does not move when a file inside it is edited.
fn emit_rerun<P: AsRef<Path>>(path: P) {
    let path = path.as_ref();
    println!("cargo:rerun-if-changed={}", path.display());
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            emit_rerun(entry.path());
        }
    }
}
