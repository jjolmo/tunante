use cmake::Config;

fn main() {
    let mut cfg = Config::new(".");

    // Platform-specific compiler flags
    if cfg!(target_os = "windows") {
        // MSVC: /w suppresses warnings, no PIC needed
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
