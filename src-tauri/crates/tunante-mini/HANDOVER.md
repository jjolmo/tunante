# tunante-mini — handover

Tunante for the phone: native Slint, no webview, no Tauri. Runs on a Poco X3 NFC
(`surya`, SM7150) under postmarketOS edge with Plasma Mobile, systemd, musl,
aarch64.

This document is for whoever picks this up next. The parts worth reading twice
are **Traps** — every one of them cost hours and none is visible by reading the
code it bit.

---

## The shape of it

Three processes, and the split is not an aesthetic choice:

```
tunante-mini      the Slint UI. Owns the audio output and the queue.
tunante-decoder   spawned per track and per scanned file. Turns a file into PCM.
tunante-core      db, queue, vgm_path. No FFI, no UI. Shared with the desktop.
tunante-codec     every decoder and all the vendored C. Shared with the desktop.
```

The decoder is a separate **process** because the emulator cores are C with
global state:

- Measured RSS per backend: mp3 6 MB, nsf 6, psf 10, psf2 11, **gsf 46, usf 48,
  2sf 49**. In-process, the UI would keep the high-water mark of ~49 MB forever,
  even once you went back to playing mp3s. Out of process the kernel takes it
  all back when the child dies.
- The desktop engine sleeps 50 ms on every track change to tear down C globals
  before building the next. A fresh process is born clean; that wait is gone.
- Metadata readers with emulators inside can hang, and a timeout cannot
  interrupt a loop running in C — it can only abandon the thread. Killing a
  process does work.

Verified on the device: the UI sits at 32–47 MB PSS whether a 3.4 MB SPC or a
175 MB NDS core is decoding beside it.

`tunante-core` and `tunante-codec` must never depend on Tauri or on a GUI
toolkit — the desktop app re-exports them from `src-tauri/src/lib.rs`.

## The rule that shapes the UI

**The library is never materialised.** Only visible rows exist. Amberol, the
reference Rust player for Linux phones, has an open bug at 3 GB RSS with 1 207
songs; Elisa uses 90 MB for the same folder. Tunante's own desktop hit this too
— see the comment in `src/lib/stores/files.svelte.ts`.

Every list is a `ListView`, which virtualises. The grid virtualises too, by
cutting cells into lines in Rust and letting the ListView iterate lines — a
Slint `struct` can hold an array, which is what makes that possible.

---

## State

Everything below has been run on the phone, not just compiled.

| | |
|---|---|
| Audio | Speaker works. Mono is the hardware: the I2S bus has no right channel. No 3.5 mm jack — the `wcd9375` codec has no driver. In practice this is a Bluetooth-headphones player. |
| Formats | All of them, on musl/aarch64. `all_supported_formats_decode` passes in CI. |
| UI | Four tabs, portrait and landscape, on a real library of 2010 tracks. |
| Library views | Tree, Discos (grid), Consolas (grid). Long-press menu in all three. |
| MPRIS | Confirmed on the lock screen. Play/Pause/Next/Previous, and the writable properties (`LoopStatus`, `Shuffle`, `Volume`) round-trip. Headset buttons are BlueZ's `mpris-proxy`, not ours. |
| Session | Restores track, position, volume, shuffle, repeat. |
| Sleep timer | Works. |
| Renderer | femtovg (GPU) by default, software compiled in behind `SLINT_BACKEND=winit-software`. |
| Frame rate | 69–82 fps while scrolling, on a 120 Hz panel. Was 20–23 when this started. |
| CI | Alpine aarch64 (musl), green, ~8 min. |
| Alpine package | Built, installed with `apk add`, and launched through its desktop entry — it came up, restored the session and played. 13.7 MB, because abuild strips; a dev build is 31 MB. |

### There is no back button

Plasma Mobile's three system buttons are **task switcher · home · close**.
Verified three ways: `NavigationPanel.qml` declares five slots and none is a
back; `kcm_navigation` offers only "panel or gestures", with no per-button
setting; and tapping the ✕ while two levels deep in the Consolas view left the
service `inactive` — it closes the app, it does not go up a level.

That is why the grid views carry their own `◂` breadcrumb. A user reaching for
that ✕ expecting "back" loses the player and the music with it.

---

## Traps

### rodio: `current_span_len() -> None` means "my rate is eternal"

Every PS2 track played 8.8 % slow and a sixth of a semitone flat. It was the
only format in this library that is not 44100 — the SPU2 runs at 48000 — which
is what made it the only one that was wrong.

`UniformSourceIterator::bootstrap` does this:

```rust
let span_len = input.current_span_len().map(|x| x.min(32768));
let input = Take { iter: input, n: span_len };
let from_sample_rate = input.sample_rate();
```

With `n: None` the `Take` never runs out, so bootstrap runs once and the rate it
captured that day is applied to every track that follows. Note the `.min(32768)`:
rodio rebuilds on that period for every ordinary source, so a finite answer is
the normal path, not a workaround. Held by two tests in `decoder.rs`.

### A pipe holds 64 KB, so drain it before waiting

Cover art vanished for some tracks and not others, and the pattern was size.
The client waited for the child to exit and only then read its stdout. A 1.7 MB
folder cover is 2.2 MB of base64: the helper blocked writing, never exited, and
the wait ran to its deadline and killed a child that was doing what it was
asked. Anything under 64 KB arrived fine, which made it read as "some files have
no cover".

`probe` had it too, and there it is worse: it runs on every file of a scan, so a
file with enough subsongs to push its JSON over 64 KB would have been dropped
from the library silently. Both go through `capture()` now.

### An MPRIS property you never publish is a property that lies

`can_control(true)` tells every client that `LoopStatus`, `Shuffle` and `Volume`
are writable. Setting them over D-Bus **succeeded** — `busctl set-property`
returned 0 — and nothing happened, because only `connect_set_volume` was wired
and `publish` hard-set `LoopStatus::None` on every playback-status change. So
the lock screen's repeat and shuffle buttons did nothing, and the property
reported "None" no matter what the UI showed.

Volume was the worse half: the command *was* applied to the player and the
property was never updated, so a client that set 0.4 heard 0.4 and read back
1.0 — the two directions disagreed about the same number. Both ways are wired
now, and `publish` sends all three when they change.

There is no error to find here. A silent success on a write is what this looks
like from the outside; only reading the property back afterwards catches it.

### `get_tracks_by_folder` is recursive

It matches `path LIKE 'folder/%'`. The tree was listing every descendant under a
folder row **as well as** that folder's subfolders: opening the root produced
1839 file rows — the whole collection, flat — and each appeared again inside the
folder it really lives in. The count gave it away: 1860 rows with only the root
open, for a root whose direct children are twenty folders and no files.

### `is_folder` on a row does not mean "directory"

A file with several subsongs — an `.nsf`, a `.gsflib` — is drawn as a folder,
because to whoever is listening that is what it is. Its `path` is the file. Ask
the filesystem, not the flag: `get_tracks_by_folder` on a file path matches
`LIKE 'file.nsf/%'` and returns nothing.

### Slint

- **`component X { }` with no base has no geometry** and collapses to zero size.
  It cost an afternoon of "why is the tab bar invisible". Use
  `inherits Rectangle`.
- **Deriving a layout input from the window size closes a cycle Slint rejects**
  as a runtime-panic risk. `portrait` and `cramped` are assigned from `changed
  width` / `changed height` handlers — side effects, not bindings.
- **The default style builds `ScrollView`/`ListView` with `interactive: false`**,
  so the finger does nothing and only the scrollbar works. Every scrollable
  thing here needs `mouse-drag-pan-enabled: true`.
- **A later sibling is on top and takes the events.** A `TouchArea` declared
  after `@children` covered the queue's drag handle completely.
- **Setting an explicit `alignment` on a layout voids `horizontal-stretch` on
  its children** — they get their preferred size and are packed. `alignment:
  start` is what left the whole right-hand third of the landscape Playing tab
  empty.
- **A wrapping `Text` reports a minimum width — its longest word — and a
  `HorizontalLayout` honours it by growing the cell.** "Game Boy Advance"
  widened its grid column and pushed the third one off the screen. Grid cards
  need an explicit width.
- **No `rotation-angle` on a `Rectangle`.** `Path` exists and is the way to draw
  anything angled (the N64's three prongs).
- **A `struct` can hold an array**, including an array of structs. That is what
  lets the grid virtualise.
- **Slint has no long-press.** It is a timer armed on pointer-down. Only `up`
  and `cancel` may disarm it — not `move`, because a finger resting on glass
  jitters. And when the list's Flickable decides a drag is a scroll it takes the
  grab and sends the TouchArea **nothing** (the `Cancel` that Slint's TouchArea
  emits only fires when the area is *disabled* while grabbed), so the timer
  survives a fling. `touch.pressed` at fire time is what catches that.
- **A tap still fires after a long press.** Holding a row opened the menu *and*
  enqueued the row on release.

### The phone

- **`schedutil` punishes an efficient UI.** A UI is bursty — a few ms of work,
  then idle — so the utilisation signal averages the burst down and the core is
  clocked low. Sampled while rendering, the big core wandered between 652 MHz
  and 2304 MHz. That is why a browser feels smooth here and this did not: a
  program that keeps the core busy gets clocked up. `boost.rs` asks the kernel
  for a `uclamp` floor on the UI thread — 68 fps becomes 112, no privileges
  needed, and it costs nothing while the thread is idle.
- **`kscreen-doctor` emits ANSI colour into a pipe**, so a naive grep drops the
  *active* mode — the one marked with `*`. It cost a wrong conclusion ("the
  panel is at 60 Hz") that was published before being checked.
- **`spectacle` returns a blank PNG with the screen off**, same exit code, always
  10 255 bytes. `kscreen-doctor --dpms on` first, and check the file size.
- **`journalctl -u <unit>` mixes every past run of that unit.** Two measurements
  in a row will silently report the first one twice. Mark the time and use
  `--since`.
- **Anything you start over ssh dies when the session closes** — and `setsid
  nohup` does not save it. logind puts the whole ssh session in a scope and
  tears it down: `session-c427.scope: Killing process 347049 (tunante-mini)
  with signal SIGTERM`. That reads exactly like the app crashing on launch, so
  check the journal before believing it did. Use `systemd-run --user
  --unit=…`, for the app as much as for a build.
- **Do not build at `-j6` on a 500 mA port.** The PMIC cuts the rail with no
  oops, no panic and nothing in pstore. It happened three times. `-j2`.
- **`perf` on a stripped binary gives a flat profile with no names**, which
  reads like "no hot spot" rather than like "no symbols". Build with
  `--config 'profile.release.strip="none"'` before profiling.

### abuild

The APKBUILD had never been executed once. It looked right and was wrong in four
places, none of them visible by reading it:

- `sha512sums="SKIP"` is ebuild syntax, from Gentoo. abuild stops with "is
  missing in checksums".
- A maintainer needs an RFC822 address, not a nickname.
- `--frozen` is `--locked` plus `--offline`. With no vendored crates and a clean
  container there is nothing to resolve, and cargo says "no matching package
  named `rodio` found", which looks nothing like what is wrong.
- **abuild exports `CMAKE_GENERATOR=Ninja`**, so the `build.rs` scripts that
  shell out to cmake ask for Ninja even though outside abuild they are happy
  with make. `samurai` provides `/usr/bin/ninja` on Alpine. This is the only one
  that goes wrong *only* under abuild, which is what made it invisible.

And two of my own, worth the same warning: **an APKBUILD is a shell script, so a
comment inside a quoted string is not a comment** — one with double quotes in it
closed `makedepends` halfway down the list and the rest ran as commands. CI now
runs `sh -n` on the recipe first, which takes a second. And `abuild-keygen -a`
says in its own output that the public key has to go into `/etc/apk/keys`;
without it the last step of `abuild -r` rejects the package abuild has just
signed, with "UNTRUSTED signature".

---

## Working on it

Cross-compiling is not worth it: Slint drags in `yeslogic-fontconfig-sys`, so a
cross build needs a whole aarch64-musl sysroot. Build natively.

**Alpine's `rust`/`cargo`, never rustup.** rustup's musl target has
`crt-static` on by default and the link against fontconfig and alsa fails.
Alpine patches `crt_static_default` to false.

On the phone, `~/tunante-spike/crates/tunante-mini` is a standalone crate with
its own `Cargo.lock` and `target/`. Sync the files you changed — **not** every
`Cargo.toml`, or you invalidate the whole graph and lose ten minutes.

```sh
# build there, detached from the ssh session
systemd-run --user --unit=build --working-directory=$D \
    --setenv=CARGO_BUILD_JOBS=2 /usr/bin/cargo build --release --offline

# measure frames: refresh_lazy is what a finger sees, refresh_full_speed is a
# stress test that flatters everything
SLINT_DEBUG_PERFORMANCE=refresh_lazy,console tunante-mini

# drive it without a finger on the glass
python3 /tmp/tactil.py tap|flick|drag|hold …
```

`tactil.py` is a virtual touchscreen over `/dev/uinput`. It lives in `/tmp` on
the phone and **does not survive a reboot**. Its `hold` exists because the
Flickable claims any vertical drag that starts within 500 ms of the press.
Screen coordinates are the device's own only in portrait; rotated right, the
mapping is `device(dx,dy) → screen(dy, 1080−dx)`.

Two instrument flags on the binary, and they are not features:
`--rows N` fills the library with generated rows to measure what a list costs,
and `--focus-search` starts on Library with the field focused — which proves the
text-input request is sent but *not* that the keyboard appears, because a
compositor only raises it when the last input came from touch.

---

## What is left

- **A long listen.** Hours of playback, to see whether anything degrades. This
  needs time, not hands. `escucha.sh` in the phone's home directory samples one
  line a minute — UI and decoder PSS, playback status, position, the ALSA
  substate and the battery — under `systemd-run --user --unit=escucha`. Read it
  with `journalctl --user -u escucha`.
- **The USB port does not keep up.** `pm8150b-charger` says `Charging` while
  `qcom_qg` says `Discharging`: plugged in, the battery still falls. A listen
  long enough to matter needs a real charger, or it ends by running out.
- **Rebuild the package with the MPRIS fix.** The installed `0.1.237` still has
  the properties that lie; the fix was verified with a native build in
  `~/tunante-spike`, not through abuild.
- `real_library_sweep` is ignored in CI on purpose — it wants a real collection
  via `TUNANTE_MUSIC_DIR`.
- The GitHub ARM runner queue can back up badly; a job sat 1 h 46 m without
  starting and was cancelled. `gh run rerun` is usually enough.
