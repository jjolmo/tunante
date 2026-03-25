use cmake::Config;
use std::path::Path;

fn main() {
    // Fix regparm attribute for non-x86 platforms (macOS ARM64, Linux ARM64).
    // VBA-M's GBAcpu.h uses __attribute__((regparm(2))) which is x86-only.
    // We patch the header before cmake builds, if needed.
    let viogsf_dir = Path::new("..").join("viogsf");
    let gbacpu_h = viogsf_dir.join("vbam/gba/GBAcpu.h");
    if gbacpu_h.exists() {
        let content = std::fs::read_to_string(&gbacpu_h).unwrap_or_default();
        if content.contains("#ifdef __GNUC__") && !content.contains("__x86_64__") {
            let patched = content.replace(
                "#ifdef __GNUC__",
                "#if defined(__GNUC__) && (defined(__i386__) || defined(__x86_64__))"
            );
            let _ = std::fs::write(&gbacpu_h, patched);
        }
    }

    let mut cfg = Config::new(".");

    // Platform-specific compiler flags
    if cfg!(target_os = "windows") {
        cfg.define("CMAKE_C_FLAGS", "/w");
        cfg.define("CMAKE_CXX_FLAGS", "/w");
    } else {
        cfg.define("CMAKE_C_FLAGS", "-ffunction-sections -fdata-sections -fPIC -w");
        cfg.define("CMAKE_CXX_FLAGS", "-ffunction-sections -fdata-sections -fPIC -w");
    }

    let dst = cfg.build_target("viogsf").build();

    // Library search paths — cmake outputs to different directories per platform
    println!("cargo:rustc-link-search=native={}/build", dst.display());
    println!("cargo:rustc-link-search=native={}/build/Release", dst.display());
    println!("cargo:rustc-link-search=native={}/build/Debug", dst.display());
    println!("cargo:rustc-link-lib=static=viogsf");

    // C++ standard library
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-lib=stdc++");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=c++");
}
