fn main() {
    // tauri-build refuses to configure while an `externalBin` entry is missing
    // on disk, which would break a plain `cargo check --workspace` on a fresh
    // clone. The real sidecar is staged by scripts/stage-decoder.mjs, wired
    // into beforeDevCommand/beforeBuildCommand — so anything that actually
    // runs or bundles the app has the real binary in place before this rings.
    // For everything else (checks, clippy, rust-analyzer) an empty placeholder
    // satisfies the validation and is bundled by nobody.
    let target = std::env::var("TARGET").unwrap_or_default();
    let suffix = if target.contains("windows") { ".exe" } else { "" };
    let sidecar = std::path::Path::new("binaries")
        .join(format!("tunante-decoder-{target}{suffix}"));
    if !sidecar.exists() {
        let _ = std::fs::create_dir_all("binaries");
        let _ = std::fs::File::create(&sidecar);
    }

    tauri_build::build()
}
