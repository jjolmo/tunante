use std::path::PathBuf;

fn main() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sexypsf_dir = base.join("sexypsf");
    let spu_dir = sexypsf_dir.join("spu");
    let zlib_dir = base.join("zlib");

    // =========================================================================
    // Build 1: sexypsf emulator core + zlib — compiled with -O2
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
    emu.define("PSS_STYLE", "2");
    #[cfg(not(windows))]
    emu.define("PSS_STYLE", "1");

    // Vendored zlib (needed by sexypsf for PSF decompression)
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
    // sexypsf_wrapper.c uses setjmp/longjmp to pause/resume the PS1 CPU loop.
    // -O2 breaks this: the compiler may clobber variables between setjmp and
    // longjmp, causing "longjmp causes uninitialized stack frame" crashes.
    // We keep this file at -O0 to guarantee correct setjmp/longjmp semantics.
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
        // Disable fortified longjmp — this file IS the setjmp/longjmp bridge.
        .flag("-U_FORTIFY_SOURCE")
        .flag("-D_FORTIFY_SOURCE=0")
        .flag_if_supported("-fno-stack-protector");

    #[cfg(windows)]
    wrapper.define("PSS_STYLE", "2");
    #[cfg(not(windows))]
    wrapper.define("PSS_STYLE", "1");

    wrapper.file(base.join("sexypsf_wrapper.c"));
    wrapper.compile("hepsf_wrapper");

    // Link math library on Unix (needed by SPU reverb calculations)
    #[cfg(unix)]
    println!("cargo:rustc-link-lib=m");

    // Link pthread on Unix (needed by thread-based bridge in wrapper)
    #[cfg(unix)]
    println!("cargo:rustc-link-lib=pthread");

    println!("cargo:rerun-if-changed=sexypsf/");
    println!("cargo:rerun-if-changed=sexypsf/spu/");
    println!("cargo:rerun-if-changed=sexypsf_wrapper.c");
    println!("cargo:rerun-if-changed=zlib/");
}
