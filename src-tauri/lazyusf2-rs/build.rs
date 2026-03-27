use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_macos = target.contains("apple");
    let is_windows = target.contains("windows");
    let is_x86_64 = target.contains("x86_64");

    if is_macos {
        std::env::set_var("MACOSX_DEPLOYMENT_TARGET", "11.0");
    }

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let usf_root = base.join("lazyusf2");
    let psflib_dir = base.join("psflib");

    let mut build = cc::Build::new();
    build
        .warnings(false)
        .flag_if_supported("-std=gnu11")
        .flag_if_supported("/std:c11")
        // Always optimize — the N64 interpreter is very slow without optimization
        .opt_level(2)
        .flag_if_supported("-fPIC")
        // Include paths
        .include(&usf_root)
        .include(&psflib_dir)
        // Rename psf_load to avoid symbol collision with other *sf-rs crates
        .define("psf_load", "usf_psf_load")
        .define("strrpbrk", "usf_strrpbrk");

    // Platform-specific defines
    if is_windows {
        build.define("_CRT_SECURE_NO_WARNINGS", None);
        build.define("_CRT_NONSTDC_NO_DEPRECATE", None);
    }

    // Use cached interpreter on ALL platforms (no dynarec).
    // The dynarec JIT runs native code that never checks the abort_flag,
    // making stuck emulators impossible to kill. The cached interpreter
    // checks abort_flag in ADD_TO_PC() after every instruction.
    // This is ~2x slower than dynarec but allows clean thread termination.
    if is_x86_64 {
        // Still define ARCH_MIN_SSE2 for SIMD optimizations in other code
        build.define("ARCH_MIN_SSE2", None);
        // Do NOT define DYNAREC — forces cached interpreter
        // Include empty_dynarec.c as stub
        build.file(usf_root.join("r4300").join("empty_dynarec.c"));
        // No dynarec files — using cached interpreter
    } else {
        // ARM64 and others: use empty dynarec (cached interpreter)
        if target.contains("aarch64") {
            build.define("ARCH_MIN_ARM_NEON", None);
        }
        build.file(usf_root.join("r4300").join("empty_dynarec.c"));
    }

    // === psflib ===
    build.file(psflib_dir.join("psflib.c"));

    // === lazyusf2 core sources ===
    let r4300 = usf_root.join("r4300");
    build.files(&[
        // AI (Audio Interface)
        usf_root.join("ai").join("ai_controller.c"),
        // API
        usf_root.join("api").join("callbacks.c"),
        // Debugger
        usf_root.join("debugger").join("dbg_decoder.c"),
        usf_root.join("debugger").join("dbg_print.c"),
        // Main
        usf_root.join("main").join("main.c"),
        usf_root.join("main").join("rom.c"),
        usf_root.join("main").join("savestates.c"),
        usf_root.join("main").join("util.c"),
        // Memory
        usf_root.join("memory").join("memory.c"),
        // PI (Peripheral Interface)
        usf_root.join("pi").join("cart_rom.c"),
        usf_root.join("pi").join("pi_controller.c"),
        // R4300 CPU
        r4300.join("cached_interp.c"),
        r4300.join("cp0.c"),
        r4300.join("cp1.c"),
        r4300.join("exception.c"),
        r4300.join("instr_counters.c"),
        r4300.join("interupt.c"),
        r4300.join("mi_controller.c"),
        r4300.join("pure_interp.c"),
        r4300.join("r4300.c"),
        r4300.join("r4300_core.c"),
        r4300.join("recomp.c"),
        r4300.join("reset.c"),
        r4300.join("tlb.c"),
        // RDP
        usf_root.join("rdp").join("rdp_core.c"),
        // RI (RAM Interface)
        usf_root.join("ri").join("rdram.c"),
        usf_root.join("ri").join("rdram_detection_hack.c"),
        usf_root.join("ri").join("ri_controller.c"),
        // RSP core + LLE (needed even in HLE mode — rsp_core references LLE functions)
        usf_root.join("rsp").join("rsp_core.c"),
        usf_root.join("rsp_lle").join("rsp.c"),
        // RSP HLE (High-Level Emulation — faster audio processing)
        usf_root.join("rsp_hle").join("alist.c"),
        usf_root.join("rsp_hle").join("alist_audio.c"),
        usf_root.join("rsp_hle").join("alist_naudio.c"),
        usf_root.join("rsp_hle").join("alist_nead.c"),
        usf_root.join("rsp_hle").join("audio.c"),
        usf_root.join("rsp_hle").join("cicx105.c"),
        usf_root.join("rsp_hle").join("hle.c"),
        usf_root.join("rsp_hle").join("hvqm.c"),
        usf_root.join("rsp_hle").join("jpeg.c"),
        usf_root.join("rsp_hle").join("memory.c"),
        usf_root.join("rsp_hle").join("mp3.c"),
        usf_root.join("rsp_hle").join("musyx.c"),
        usf_root.join("rsp_hle").join("plugin.c"),
        usf_root.join("rsp_hle").join("re2.c"),
        // SI (Serial Interface)
        usf_root.join("si").join("cic.c"),
        usf_root.join("si").join("game_controller.c"),
        usf_root.join("si").join("n64_cic_nus_6105.c"),
        usf_root.join("si").join("pif.c"),
        usf_root.join("si").join("si_controller.c"),
        // USF format handler
        usf_root.join("usf").join("barray.c"),
        usf_root.join("usf").join("resampler.c"),
        usf_root.join("usf").join("usf.c"),
        // VI (Video Interface — stub)
        usf_root.join("vi").join("vi_controller.c"),
    ]);

    build.compile("lazyusf2");

    // Link system zlib (psflib and lazyusf2 need inflate/uncompress/adler32)
    println!("cargo:rustc-link-lib=z");

    // Link math library on Unix
    if !is_windows {
        println!("cargo:rustc-link-lib=m");
    }

    println!("cargo:rerun-if-changed=lazyusf2/");
    println!("cargo:rerun-if-changed=psflib/");
}
