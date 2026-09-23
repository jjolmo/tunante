// Note: MSVC is not supported by the vendored C; MinGW is what the Windows
// build uses, same as the other vendored cores here.

fn main() {
    let root = "../libkss";

    // Named rather than globbed, and that is the point: globbing would sweep in
    // the five MGS/BGM/OPX/MPK/MBM converters (they need the `kss-drivers`
    // blobs this repository does not redistribute — see the vendor README) and
    // the two example programs that carry their own `main`
    // (`kmz80/makeft.c`, `emu2413/sample2413.c`).
    let files = [
        "src/kssplay.c",
        "src/kss/kss.c",
        "src/kss/kssload.c",
        "src/kss/kss2kss.c",
        "src/vm/detect.c",
        "src/vm/mmap.c",
        "src/vm/vm.c",
        "src/rconv/psg_rconv.c",
        "src/filters/dc_filter.c",
        "src/filters/filter.c",
        "src/filters/rc_filter.c",
        "modules/emu2149/emu2149.c",
        "modules/emu2212/emu2212.c",
        "modules/emu2413/emu2413.c",
        "modules/emu76489/emu76489.c",
        "modules/emu8950/emu8950.c",
        "modules/emu8950/emuadpcm.c",
        "modules/kmz80/kmdmg.c",
        "modules/kmz80/kmevent.c",
        "modules/kmz80/kmr800.c",
        "modules/kmz80/kmz80.c",
        "modules/kmz80/kmz80c.c",
        "modules/kmz80/kmz80t.c",
    ];

    let mut build = cc::Build::new();
    build.warnings(false);
    // Always optimize: this is a Z80 plus five sound chips emulated per sample,
    // and an unoptimized build of it cannot keep up with playback.
    build.opt_level(2);

    // Decade-old C that predates the diagnostic: `vm.c` hands `kmevent_setevent`
    // a function pointer whose argument types differ. GCC 14 made that an error
    // by default. Upstream's own CMake build has the same problem, and the code
    // is correct in practice — the call goes through a cast at runtime.
    build.flag_if_supported("-Wno-error=incompatible-pointer-types");
    build.flag_if_supported("-Wno-incompatible-pointer-types");

    for f in files {
        build.file(format!("{root}/{f}"));
        println!("cargo:rerun-if-changed={root}/{f}");
    }
    build.file("c/stubs.c");
    build.file("c/shim.c");
    println!("cargo:rerun-if-changed=c/stubs.c");
    println!("cargo:rerun-if-changed=c/shim.c");

    build.include(format!("{root}/src"));
    build.include(format!("{root}/src/kss"));
    // The chip emulators include each other as `emu2149/emu2149.h`, so the
    // directory that holds them is the include root, not each one of them.
    build.include(format!("{root}/modules"));

    build.compile("kss");
}
