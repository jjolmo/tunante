use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_macos = target.contains("apple");

    // macOS ARM requires deployment target >= 11.0; set it for all macOS
    // so the cc crate passes the correct -mmacosx-version-min flag.
    if is_macos {
        std::env::set_var("MACOSX_DEPLOYMENT_TARGET", "11.0");
    }

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let vio2sf_dir = base.join("vio2sf");
    let desmume_dir = vio2sf_dir.join("desmume");
    let psflib_dir = base.join("psflib");
    let zlib_dir = base.join("zlib");

    // === C sources: DeSmuME + psflib (+ zlib on non-macOS) ===
    let mut c_build = cc::Build::new();
    c_build
        .warnings(false)
        // Always optimize the NDS emulator, even in debug builds.
        // Without this, the ARM interpreter runs ~10x slower,
        // making seek (which fast-forwards the CPU) unacceptably slow.
        .opt_level(3)
        .include(&vio2sf_dir)
        .include(&desmume_dir)
        .include(&psflib_dir)
        .include(&zlib_dir)
        // Symbol namespacing to avoid collisions with other libraries
        .define("BARRAY_DECORATE", "TWOSF")
        .define("RESAMPLER_DECORATE", "TWOSF")
        // Rename psf_load to avoid symbol collision with lazygsf-rs and hepsf-rs
        .define("psf_load", "twosf_psf_load")
        .define("strrpbrk", "twosf_strrpbrk");

    if !is_macos {
        // Vendored zlib (11 source files) — on macOS, use system libz instead
        // because the vendored zlib 1.2.12 gzguts.h conflicts with Xcode 16+ SDK _stdio.h
        c_build.files(&[
            zlib_dir.join("adler32.c"),
            zlib_dir.join("compress.c"),
            zlib_dir.join("crc32.c"),
            zlib_dir.join("deflate.c"),
            zlib_dir.join("infback.c"),
            zlib_dir.join("inffast.c"),
            zlib_dir.join("inflate.c"),
            zlib_dir.join("inftrees.c"),
            zlib_dir.join("trees.c"),
            zlib_dir.join("uncompr.c"),
            zlib_dir.join("zutil.c"),
        ]);
    }

    // psflib
    c_build.file(psflib_dir.join("psflib.c"));

    // DeSmuME core (16 C files)
    c_build.files(&[
        desmume_dir.join("arm_instructions.c"),
        desmume_dir.join("armcpu.c"),
        desmume_dir.join("barray.c"),
        desmume_dir.join("bios.c"),
        desmume_dir.join("cp15.c"),
        desmume_dir.join("FIFO.c"),
        desmume_dir.join("GPU.c"),
        desmume_dir.join("isqrt.c"),
        desmume_dir.join("matrix.c"),
        desmume_dir.join("mc.c"),
        desmume_dir.join("MMU.c"),
        desmume_dir.join("NDSSystem.c"),
        desmume_dir.join("resampler.c"),
        desmume_dir.join("state.c"),
        desmume_dir.join("thumb_instructions.c"),
    ]);

    c_build.compile("vio2sf_c");

    // === C++ source: SPU.cpp (the Sound Processing Unit) ===
    let mut cpp_build = cc::Build::new();
    cpp_build
        .cpp(true)
        .warnings(false)
        .opt_level(2)
        .include(&vio2sf_dir)
        .include(&desmume_dir)
        .include(&psflib_dir)
        .include(&zlib_dir)
        .define("BARRAY_DECORATE", "TWOSF")
        .define("RESAMPLER_DECORATE", "TWOSF")
        .flag_if_supported("-std=gnu++17");

    cpp_build.file(desmume_dir.join("SPU.cpp"));
    cpp_build.compile("vio2sf_cpp");

    // Link C++ standard library (needed for SPU.cpp)
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-lib=stdc++");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=c++");

    // On macOS, use system zlib instead of vendored (avoids Xcode 16+ SDK conflicts)
    if is_macos {
        println!("cargo:rustc-link-lib=z");
    }

    // Link math library on Unix
    #[cfg(unix)]
    println!("cargo:rustc-link-lib=m");

    println!("cargo:rerun-if-changed=vio2sf/");
    println!("cargo:rerun-if-changed=psflib/");
    println!("cargo:rerun-if-changed=zlib/");
}
