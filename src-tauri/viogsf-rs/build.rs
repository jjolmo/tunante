use cmake::Config;
use std::env;
use std::path::Path;

/// Point CMake at the NDK toolchain when cross-compiling to Android.
///
/// `cargo-ndk` exports ANDROID_ABI and ANDROID_PLATFORM as environment
/// variables, but `cmake-rs` decides whether it is in NDK mode by looking at
/// its own `defines` map and never reads the environment. Get this wrong and
/// NDK mode stays silently off: CMake runs its own compiler detection, picks
/// up the host clang, and the build dies minutes later with "Check for working
/// C compiler - broken" rather than with anything about Android.
///
/// Passing CMAKE_TOOLCHAIN_FILE also stops cmake-rs emitting its own
/// CMAKE_SYSTEM_NAME and CMAKE_C_COMPILER, which is what we want here — the
/// NDK toolchain file owns all of that.
///
/// vgmstream-rs/build.rs has the same function, for the same reason.
fn configure_android_ndk(cfg: &mut Config) {
    let toolchain = env::var("CARGO_NDK_CMAKE_TOOLCHAIN_PATH").expect(
        "cross-compiling to Android but CARGO_NDK_CMAKE_TOOLCHAIN_PATH is unset; \
         build through `cargo ndk` so CMake can find the NDK toolchain file",
    );
    cfg.define("CMAKE_TOOLCHAIN_FILE", toolchain);
    if let Ok(abi) = env::var("ANDROID_ABI") {
        cfg.define("ANDROID_ABI", abi);
    }
    if let Ok(platform) = env::var("CARGO_NDK_ANDROID_PLATFORM") {
        cfg.define("ANDROID_PLATFORM", platform);
    }
}

fn main() {
    let target = env::var("TARGET").unwrap_or_default();

    // This used to rewrite ../viogsf/vbam/gba/GBAcpu.h in place, to guard
    // VBA-M's x86-only __attribute__((regparm(2))) so the ARM builds would
    // compile. It worked, and it meant every build dirtied the checkout: the
    // tree was never clean, `git status` always had something in it, and the
    // build was not reproducible from a read-only source.
    //
    // ../viogsf is now vendored rather than a submodule, so the guard is
    // committed in the source where it can be read and reviewed. See
    // ../viogsf/README.upstream.md for where that source came from.
    let mut cfg = Config::new(".");

    if target.contains("android") {
        configure_android_ndk(&mut cfg);
    }

    // Platform-specific compiler flags.
    //
    // Passed as flags rather than as a CMAKE_C_FLAGS define: a define replaces
    // the cache variable outright, and under the NDK toolchain file that is
    // where --target= and --sysroot= live.
    if target.contains("windows") {
        cfg.cflag("/w").cxxflag("/w");
    } else {
        for flag in ["-ffunction-sections", "-fdata-sections", "-fPIC", "-w"] {
            cfg.cflag(flag).cxxflag(flag);
        }
    }

    let dst = cfg.build_target("viogsf").build();

    // Library search paths — cmake outputs to different directories per platform
    println!("cargo:rustc-link-search=native={}/build", dst.display());
    println!("cargo:rustc-link-search=native={}/build/Release", dst.display());
    println!("cargo:rustc-link-search=native={}/build/Debug", dst.display());
    println!("cargo:rustc-link-lib=static=viogsf");

    // C++ standard library.
    //
    // These used to be `#[cfg(target_os = ...)]`, which inside a build script
    // is the HOST, not the target. It agreed with the target right up until
    // somebody cross-compiled. Android has to be tested before "linux" because
    // its triple contains it — and bionic does ship a libstdc++.so, but it is a
    // stub holding only operator new/delete and __cxa_pure_virtual, so it links
    // and then fails on every real std:: symbol.
    if target.contains("android") {
        println!("cargo:rustc-link-lib=c++_shared");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=stdc++");
    } else if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    }
}
