use std::env;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let is_macos = target.contains("apple");

    // macOS ARM requires deployment target >= 11.0; set it for all macOS
    // so cmake and the cc crate pass the correct -mmacosx-version-min flag.
    if is_macos {
        env::set_var("MACOSX_DEPLOYMENT_TARGET", "11.0");
    }

    let vgmstream_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .join("vgmstream");

    let mut cfg = cmake::Config::new(&vgmstream_dir);
    cfg.define("BUILD_STATIC", "ON")
        .define("BUILD_CLI", "OFF")
        .define("BUILD_FB2K", "OFF")
        .define("BUILD_V123", "OFF")
        .define("BUILD_AUDACIOUS", "OFF")
        .define("USE_FFMPEG", "OFF")
        .define("USE_MPEG", "OFF")
        .define("USE_VORBIS", "OFF")
        .define("USE_G719", "OFF")
        .define("USE_ATRAC9", "OFF")
        .define("USE_SPEEX", "OFF")
        .define("USE_CELT", "OFF");

    // On Windows, the cmake target is "libvgmstream" but the output lib name
    // and directory structure differ between generators
    let dst = cfg.build_target("libvgmstream").build();

    // The static library path varies by platform and cmake generator:
    // - Unix (make): build/src/libvgmstream.a
    // - MSVC (multi-config): build/src/Release/vgmstream.lib
    let build_src = format!("{}/build/src", dst.display());
    println!("cargo:rustc-link-search=native={}", build_src);
    println!("cargo:rustc-link-search=native={}/Release", build_src);
    println!("cargo:rustc-link-search=native={}/Debug", build_src);
    println!("cargo:rustc-link-lib=static=vgmstream");

    // Link math library on unix
    if !target.contains("windows") {
        println!("cargo:rustc-link-lib=m");
    }

    // C++ standard library (vgmstream is C but uses some C++ in codecs)
    if target.contains("linux") {
        println!("cargo:rustc-link-lib=stdc++");
    } else if is_macos {
        println!("cargo:rustc-link-lib=c++");
    }

    // Tell cargo to rerun if vgmstream source changes
    println!("cargo:rerun-if-changed={}", vgmstream_dir.display());
}
