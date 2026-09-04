//! Telling the scheduler this thread is worth clocking up.
//!
//! The UI was drawing at 68 fps on a 120 Hz panel, and the reason was neither
//! Slint nor the GPU. Measured on the phone (Poco X3, SM7150, Plasma Mobile):
//!
//! | | fps, full-speed redraw |
//! |---|---|
//! | as shipped | 68-69 |
//! | `cpufreq` governor forced to `performance` | 114-116 |
//! | `uclamp.min = 512` on this process's threads | 111-113 |
//! | `uclamp.min = 920` | 117 |
//!
//! The same binary, the same scene, the same GPU. What changed was the CPU
//! clock. `es2gears` reaches 118 fps on this phone untouched, so the panel and
//! the compositor were never the limit.
//!
//! The cause is `schedutil` doing exactly what it is designed to do. A UI is a
//! bursty load: a few milliseconds of work, then idle until the next frame. The
//! governor's utilisation signal averages that burst down, so it clocks the core
//! low — sampled while rendering, the big core wandered between 652 MHz and
//! 2304 MHz — and every frame that lands during a dip arrives late. The app is
//! punished for being efficient: a program that kept the core busy would be
//! clocked up and would feel smoother while doing more work.
//!
//! `uclamp` is the kernel's answer to precisely this. `sched_util_min` tells the
//! scheduler "whenever this task is runnable, treat it as at least this loaded",
//! so the frequency rises for this thread and nothing else, and only while it
//! actually has work. It is what Android applies to its UI threads.
//!
//! 512 rather than 1024: half the clamp recovers essentially all the frames
//! (111-113 against 117) and asks for a good deal less silicon on a device
//! running off a battery.
//!
//! No privileges are needed. Raising `uclamp.min` without `CAP_SYS_NICE` is
//! allowed up to `kernel.sched_util_clamp_min`, which is 1024 by default — the
//! request only fails on a system whose administrator has lowered that ceiling,
//! and then it fails quietly and the app runs as it did before.

/// Half of the 0..1024 scale the kernel uses for utilisation clamps.
#[cfg(target_os = "linux")]
const UI_THREAD_CLAMP: u32 = 512;

/// `SCHED_FLAG_KEEP_POLICY | SCHED_FLAG_KEEP_PARAMS | SCHED_FLAG_UTIL_CLAMP_MIN`
///
/// The two `KEEP` flags matter: without them this call would also rewrite the
/// scheduling policy and priority from the zeroed fields below, quietly moving
/// the thread to `SCHED_OTHER` at nice 0.
#[cfg(target_os = "linux")]
const FLAGS: u64 = 0x08 | 0x10 | 0x20;

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Default)]
struct SchedAttr {
    size: u32,
    sched_policy: u32,
    sched_flags: u64,
    sched_nice: i32,
    sched_priority: u32,
    sched_runtime: u64,
    sched_deadline: u64,
    sched_period: u64,
    sched_util_min: u32,
    sched_util_max: u32,
}

/// Ask for a utilisation floor on the calling thread.
///
/// Call it from the thread that renders — on this app that is the thread that
/// runs the Slint event loop, which is the one `main` is already on. Clamps are
/// per-thread, so the decoder helper and the audio thread are untouched.
///
/// Returns whether the kernel took it, for logging. Failure is not an error:
/// the app simply runs at whatever clock the governor picks, which is what it
/// did before this existed.
#[cfg(target_os = "linux")]
pub fn ask_for_ui_clock() -> bool {
    let attr = SchedAttr {
        size: std::mem::size_of::<SchedAttr>() as u32,
        sched_flags: FLAGS,
        sched_util_min: UI_THREAD_CLAMP,
        // Ignored under SCHED_FLAG_UTIL_CLAMP_MIN alone, but the kernel still
        // validates it against the current maximum, so it has to be the real
        // ceiling and not zero.
        sched_util_max: 1024,
        ..Default::default()
    };

    // SAFETY: `attr` outlives the call, `size` describes it truthfully, and pid
    // 0 means "the calling thread". The kernel only reads.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_sched_setattr,
            0,                              // pid 0 = this thread
            &attr as *const SchedAttr,
            0u32,                           // flags, none defined
        )
    };
    rc == 0
}

/// Elsewhere there is no `sched_setattr`, so there is nothing to ask for.
///
/// Reports false rather than true: the caller logs this, and claiming the
/// kernel granted a clamp that was never requested would make that log lie.
#[cfg(not(target_os = "linux"))]
pub fn ask_for_ui_clock() -> bool {
    false
}
