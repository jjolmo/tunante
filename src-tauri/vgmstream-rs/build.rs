use std::env;
use std::path::PathBuf;

fn main() {
    // macOS ARM requires deployment target >= 11.0; set it for all macOS
    // so cmake and the cc crate pass the correct -mmacosx-version-min flag.
    if let Ok(target) = env::var("TARGET") {
        if target.contains("apple") {
            env::set_var("MACOSX_DEPLOYMENT_TARGET", "11.0");
        }
    }

    let vgmstream_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .join("vgmstream");

    let dst = cmake::Config::new(&vgmstream_dir)
        .define("BUILD_STATIC", "ON")
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
        .define("USE_CELT", "OFF")
        .build_target("libvgmstream")
        .build();

    // The static library is built inside the build directory
    println!(
        "cargo:rustc-link-search=native={}/build/src",
        dst.display()
    );
    println!("cargo:rustc-link-lib=static=vgmstream");

    // Link math library on unix
    #[cfg(unix)]
    println!("cargo:rustc-link-lib=m");

    // C++ standard library (vgmstream is C but uses some C++ in codecs)
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-lib=stdc++");

    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=c++");

    // Tell cargo to rerun if vgmstream source changes
    println!("cargo:rerun-if-changed={}", vgmstream_dir.display());
}
