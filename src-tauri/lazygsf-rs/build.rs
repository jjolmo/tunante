use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_macos = target.contains("apple");
    let is_windows = target.contains("windows");

    // macOS ARM requires deployment target >= 11.0; set it for all macOS
    // so the cc crate passes the correct -mmacosx-version-min flag.
    if is_macos {
        std::env::set_var("MACOSX_DEPLOYMENT_TARGET", "11.0");
    }

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mgba = base.join("lazygsf").join("mgba");
    let mgba_src = mgba.join("src");
    let lazygsf_src = base.join("lazygsf").join("src");
    let psflib_dir = base.join("psflib");
    let zlib_dir = mgba_src.join("third-party").join("zlib");

    // Build everything in a single cc::Build to avoid duplicate symbol conflicts
    // (mgba/src/util/crc32.c and zlib/crc32.c both define 'crc32')
    let mut build = cc::Build::new();
    build
        .warnings(false)
        .flag_if_supported("-std=gnu11")
        // MSVC: enable C11 for _Static_assert used by mGBA's serialize.h
        .flag_if_supported("/std:c11")
        // Always optimize the GBA emulator, even in debug builds.
        // Without this, the ARM interpreter runs ~10x slower,
        // making seek (which fast-forwards the CPU) unacceptably slow.
        .opt_level(2)
        // Include paths
        .include(mgba.join("include"))
        .include(&mgba_src)
        .include(&lazygsf_src)
        .include(&psflib_dir)
        .include(&zlib_dir) // psflib needs <zlib.h>
        // Defines (from CMakeLists.txt line 121)
        .define("DISABLE_THREADING", None)
        .define("M_CORE_GBA", None)
        .define("MINIMAL_CORE", "2")
        .define("BUILD_STATIC", None)
        // Tell mGBA's util/crc32.c to use zlib's crc32 instead of its own
        .define("HAVE_CRC32", None)
        // Rename psf_load to avoid symbol collision with hepsf-rs and vio2sf-rs
        // (all three crates bundle psflib with a global psf_load symbol)
        .define("psf_load", "gsf_psf_load")
        .define("strrpbrk", "gsf_strrpbrk");

    // Platform-specific defines
    if is_macos {
        build.define("HAVE_LOCALE", None);
        // Skip HAVE_SNPRINTF_L — Xcode 16+ Clang treats implicit function
        // declarations as errors, and the mGBA code doesn't include <xlocale.h>
        // macOS provides strlcpy as a builtin — skip mGBA's redeclaration
        build.define("HAVE_STRLCPY", None);
    } else if is_windows {
        // MSVC compatibility
        build.define("_CRT_SECURE_NO_WARNINGS", None);
        build.define("_CRT_NONSTDC_NO_DEPRECATE", None);
        // Use stdio-based VFS (vfs-file.c) instead of POSIX fd-based (vfs-fd.c)
        build.define("USE_VFS_FILE", None);
    } else {
        // Linux and other Unix
        build.define("HAVE_LOCALE", None);
    }

    if !is_macos {
        // Vendored zlib — on macOS, use system libz instead because the
        // vendored zlib gzguts.h conflicts with Xcode 16+ SDK _stdio.h
        build.files(&[
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

    // === psflib (PSF container parser) ===
    build.file(psflib_dir.join("psflib.c"));

    // === mGBA ARM CPU (6 files) ===
    let arm_dir = mgba_src.join("arm");
    build.files(&[
        arm_dir.join("arm.c"),
        arm_dir.join("decoder-arm.c"),
        arm_dir.join("decoder.c"),
        arm_dir.join("decoder-thumb.c"),
        arm_dir.join("isa-arm.c"),
        arm_dir.join("isa-thumb.c"),
    ]);

    // === mGBA Core (13 files) ===
    let core_dir = mgba_src.join("core");
    build.files(&[
        core_dir.join("bitmap-cache.c"),
        core_dir.join("cache-set.c"),
        core_dir.join("config.c"),
        core_dir.join("core.c"),
        core_dir.join("interface.c"),
        core_dir.join("log.c"),
        core_dir.join("map-cache.c"),
        core_dir.join("mem-search.c"),
        core_dir.join("serialize.c"),
        core_dir.join("sync.c"),
        core_dir.join("tile-cache.c"),
        core_dir.join("timing.c"),
    ]);

    // === mGBA Utilities (14 files) ===
    let util_dir = mgba_src.join("util");
    build.files(&[
        util_dir.join("circle-buffer.c"),
        util_dir.join("configuration.c"),
        util_dir.join("crc32.c"),
        util_dir.join("formatting.c"),
        util_dir.join("gbk-table.c"),
        util_dir.join("hash.c"),
        util_dir.join("memory.c"),
        util_dir.join("patch.c"),
        util_dir.join("patch-ips.c"),
        util_dir.join("patch-ups.c"),
        util_dir.join("string.c"),
        util_dir.join("table.c"),
        util_dir.join("vfs.c"),
        util_dir.join("vfs").join("vfs-mem.c"),
    ]);

    // VFS backend: vfs-fd.c uses POSIX APIs, vfs-file.c uses stdio (portable)
    if is_windows {
        build.file(util_dir.join("vfs").join("vfs-file.c"));
    } else {
        build.file(util_dir.join("vfs").join("vfs-fd.c"));
    }

    // === mGBA Third-party (2 files) ===
    let tp_dir = mgba_src.join("third-party");
    build.files(&[
        tp_dir.join("inih").join("ini.c"),
        tp_dir.join("blip_buf").join("blip_buf.c"),
    ]);

    // === GB audio (1 file — needed by GBA audio) ===
    build.file(mgba_src.join("gb").join("audio.c"));

    // === GBA (29 files) ===
    let gba_dir = mgba_src.join("gba");
    build.files(&[
        gba_dir.join("audio.c"),
        gba_dir.join("bios.c"),
        gba_dir.join("core.c"),
        gba_dir.join("dma.c"),
        gba_dir.join("gba.c"),
        gba_dir.join("hle-bios.c"),
        gba_dir.join("io.c"),
        gba_dir.join("memory.c"),
        gba_dir.join("overrides.c"),
        gba_dir.join("savedata.c"),
        gba_dir.join("serialize.c"),
        gba_dir.join("sio.c"),
        gba_dir.join("timer.c"),
        gba_dir.join("video.c"),
        // Cart
        gba_dir.join("cart").join("ereader.c"),
        gba_dir.join("cart").join("gpio.c"),
        gba_dir.join("cart").join("matrix.c"),
        gba_dir.join("cart").join("vfame.c"),
        // Renderers
        gba_dir.join("renderers").join("cache-set.c"),
        gba_dir.join("renderers").join("common.c"),
        gba_dir.join("renderers").join("software-bg.c"),
        gba_dir.join("renderers").join("software-mode0.c"),
        gba_dir.join("renderers").join("software-obj.c"),
        gba_dir.join("renderers").join("video-software.c"),
    ]);

    // === Stubs for removed cheats/gbp modules ===
    build.file(base.join("stubs.c"));

    // === lazygsf itself (1 file) ===
    build.file(lazygsf_src.join("lazygsf.c"));

    // Compile everything into a single static library
    build.compile("lazygsf");

    // On macOS, use system zlib instead of vendored (avoids Xcode 16+ SDK conflicts)
    if is_macos {
        println!("cargo:rustc-link-lib=z");
    }

    // Link math library on Unix
    if !is_windows {
        println!("cargo:rustc-link-lib=m");
    }

    // Rerun if sources change
    println!("cargo:rerun-if-changed=lazygsf/");
    println!("cargo:rerun-if-changed=psflib/");
}
