fn main() {
    // Bundled translations: every `.po` under `translations/<lang>/LC_MESSAGES/`
    // is compiled straight into the binary, so the app stays a single file on
    // every target (musl and Android included, where a runtime gettext would be
    // a problem). The source strings are Spanish; a language with no `.po` — or
    // a locale we don't ship — falls back to them. Contributors add a language
    // by dropping in `translations/<lang>/LC_MESSAGES/tunante-mini.po`.
    //
    // No default translation context: `@tr("…")` keys are plain msgids with no
    // per-component `msgctxt`, which is what the `.po` files here expect.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile ui/app.slint");
}
