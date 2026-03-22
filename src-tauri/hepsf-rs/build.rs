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
    let sexypsf_dir = base.join("sexypsf");
    let spu_dir = sexypsf_dir.join("spu");
    let zlib_dir = base.join("zlib");

    // =========================================================================
    // Build 1: sexypsf emulator core (+ zlib on non-macOS) — compiled with -O2
    //
    // Always optimize the PS1 emulator, even in debug builds.
    // Without this, the MIPS R3000 interpreter runs ~10x slower,
    // making seek (which fast-forwards the CPU) unacceptably slow.
    // =========================================================================
    let mut emu = cc::Build::new();
    emu.warnings(false)
        .opt_level(2)
        .include(&sexypsf_dir)
        .include(&spu_dir)
        .include(&zlib_dir)
        .flag_if_supported("-fvisibility=hidden")
        // Required: sexypsf headers use tentative definitions for globals
        // (e.g., `s8 *psxM;` in PsxMem.h, included by every .c file).
        // GCC >= 10 defaults to -fno-common which causes multiple definition errors.
        .flag_if_supported("-fcommon")
        // Disable glibc's fortified longjmp (__longjmp_chk) which aborts if it
        // detects "uninitialized" stack frames. Our setjmp/longjmp bridge crosses
        // through these -O2 emulator frames, which triggers a false positive.
        .flag("-U_FORTIFY_SOURCE")
        .flag("-D_FORTIFY_SOURCE=0")
        .flag_if_supported("-fno-stack-protector");

    #[cfg(windows)]
    {
        emu.define("PSS_STYLE", "2");
        // MSVC provides _stricmp/_strnicmp instead of POSIX strcasecmp/strncasecmp
        emu.define("strcasecmp", "_stricmp");
        emu.define("strncasecmp", "_strnicmp");
    }
    #[cfg(not(windows))]
    emu.define("PSS_STYLE", "1");

    if !is_macos {
        // Vendored zlib — on macOS, use system libz instead because the
        // vendored zlib 1.2.12 gzguts.h conflicts with Xcode 16+ SDK _stdio.h
        emu.files(&[
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

    // sexypsf core — PS1 R3000 CPU + HLE BIOS + hardware emulation
    emu.files(&[
        sexypsf_dir.join("Misc.c"),           // PSF loader, sexy_load/execute/getpsfinfo
        sexypsf_dir.join("PsxBios.c"),        // HLE BIOS (A0/B0/C0 function tables)
        sexypsf_dir.join("PsxCounters.c"),    // PS1 hardware timers
        sexypsf_dir.join("PsxDma.c"),         // DMA controller
        sexypsf_dir.join("PsxHLE.c"),         // High-Level Emulation hooks
        sexypsf_dir.join("PsxHw.c"),          // PS1 hardware registers
        sexypsf_dir.join("PsxInterpreter.c"), // MIPS R3000A interpreter (CPU loop)
        sexypsf_dir.join("PsxMem.c"),         // PS1 memory map
        sexypsf_dir.join("R3000A.c"),         // CPU init/reset/exception handling
        sexypsf_dir.join("Spu.c"),            // SPU IRQ glue (thin wrapper)
    ]);

    // sexypsf SPU plugin (Pete's SPU with modifications by kode54)
    // NOTE: spu.c #includes adsr.c, dma.c, registers.c, reverb.c directly,
    // so we only compile spu.c itself — NOT the included files separately.
    emu.file(spu_dir.join("spu.c"));

    emu.compile("hepsf_emu");

    // =========================================================================
    // Build 2: Our push-to-pull bridge — compiled WITHOUT optimization
    //
    // sexypsf_wrapper.c uses a background thread to bridge push/pull models.
    // We keep this file at -O0 to guarantee correct setjmp/longjmp semantics
    // in case the approach ever changes back from threads to coroutines.
    // =========================================================================
    let mut wrapper = cc::Build::new();
    wrapper
        .warnings(false)
        .opt_level(0)
        .include(&sexypsf_dir)
        .include(&spu_dir)
        .include(&zlib_dir)
        .flag_if_supported("-fvisibility=hidden")
        .flag_if_supported("-fcommon")
        // Disable fortified longjmp — safety measure.
        .flag("-U_FORTIFY_SOURCE")
        .flag("-D_FORTIFY_SOURCE=0")
        .flag_if_supported("-fno-stack-protector");

    #[cfg(windows)]
    wrapper.define("PSS_STYLE", "2");
    #[cfg(not(windows))]
    wrapper.define("PSS_STYLE", "1");

    wrapper.file(base.join("sexypsf_wrapper.c"));
    wrapper.compile("hepsf_wrapper");

    // =========================================================================
    // Build 3: Highly Experimental (HE) emulator core — for PSF2 (PS2 IOP)
    //
    // HE provides a complete IOP (R3000 + SPU/SPU2) emulator that supports
    // both PS1 (version=1) and PS2 (version=2). It uses psx_execute() as a
    // synchronous pull-based API, so no thread/ring-buffer wrapper is needed.
    //
    // The BIOS is synthesized at runtime via mkhebios — no real PS2 BIOS dump
    // is required. The mkhebios tool assembles a synthetic HLE BIOS from
    // embedded MIPS code scripts.
    // =========================================================================
    let he_dir = base.join("he");
    let psflib_dir = base.join("psflib");

    let mut he = cc::Build::new();
    he.warnings(false)
        .opt_level(2)
        .include(&he_dir)
        .include(&psflib_dir)
        .include(&zlib_dir)
        .flag_if_supported("-fvisibility=hidden")
        // EMU_COMPILE is required by all HE source files
        .define("EMU_COMPILE", None)
        // Set endianness for little-endian platforms (x86/x64/ARM)
        .define("EMU_LITTLE_ENDIAN", None)
        // Use stdint.h types
        .define("HAVE_STDINT_H", None)
        // Rename psf_load to avoid symbol collision with lazygsf-rs and vio2sf-rs
        .define("psf_load", "hepsf_psf_load")
        .define("strrpbrk", "hepsf_strrpbrk")
        .define("psf2fs_create", "hepsf_psf2fs_create")
        .define("psf2fs_delete", "hepsf_psf2fs_delete")
        .define("psf2fs_load_callback", "hepsf_psf2fs_load_callback")
        .define("psf2fs_virtual_readfile", "hepsf_psf2fs_virtual_readfile");

    // HE core: IOP emulator (R3000 CPU + SPU/SPU2 + timers + VFS)
    he.files(&[
        he_dir.join("bios.c"),
        he_dir.join("iop.c"),
        he_dir.join("ioptimer.c"),
        he_dir.join("psx.c"),
        he_dir.join("r3000.c"),
        he_dir.join("r3000asm.c"),
        he_dir.join("spu.c"),
        he_dir.join("spucore.c"),
        he_dir.join("vfs.c"),
        he_dir.join("mkhebios.c"),
    ]);

    // psflib: PSF container parser + psf2fs virtual filesystem
    he.files(&[
        psflib_dir.join("psflib.c"),
        psflib_dir.join("psf2fs.c"),
    ]);

    he.compile("hepsf_he");

    // Link math library on Unix (needed by SPU reverb calculations)
    #[cfg(unix)]
    println!("cargo:rustc-link-lib=m");

    // Link pthread on Unix (needed by thread-based bridge in wrapper)
    #[cfg(unix)]
    println!("cargo:rustc-link-lib=pthread");

    // On macOS, use system zlib instead of vendored (avoids Xcode 16+ SDK conflicts)
    if is_macos {
        println!("cargo:rustc-link-lib=z");
    }

    println!("cargo:rerun-if-changed=sexypsf/");
    println!("cargo:rerun-if-changed=sexypsf/spu/");
    println!("cargo:rerun-if-changed=sexypsf_wrapper.c");
    println!("cargo:rerun-if-changed=zlib/");
    println!("cargo:rerun-if-changed=he/");
    println!("cargo:rerun-if-changed=psflib/");
}
