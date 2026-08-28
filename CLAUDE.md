# Tunante - Development Conventions

## Architecture
- **Framework**: Tauri v2 (Rust backend) + SvelteKit 2 (Svelte 5 frontend)
- **Audio**: rodio + symphonia for standard format playback
- **Database**: SQLite via rusqlite (bundled)
- **Metadata**: lofty crate for reading audio tags
- **Styling**: Tailwind CSS v4

## Project Structure
```
src/                    # Frontend (SvelteKit)
  lib/components/       # Svelte 5 components
  lib/stores/           # Shared state (.svelte.ts with runes)
  lib/types/            # TypeScript types
  routes/               # SvelteKit pages
src-tauri/              # Cargo workspace root (and the desktop app package)
  src/
    audio/              # Playback engine (rodio) + output device selection
    commands/           # Tauri IPC commands (player, library, playlists)
    watcher/            # Folder watching (notify)
  crates/
    tunante-core/       # UI-agnostic: db/ (SQLite), queue, session, vgm_path, dsp/
    tunante-codec/      # Every decoder + metadata reader, and all the vendored C
    tunante-decoder/    # The out-of-process helper binary: a file in, PCM out
    tunante-helper/     # Client for that helper: probe, artwork, scan, PipeSource
    tunante-mini/       # Phone app for postmarketOS (Slint)
    tunante-android/    # JNI bridge: the Rust half of the Android app (cdylib)
  <vendored FFI crates> # viogsf-rs, vio2sf-rs, hepsf-rs, lazyusf2-rs,
                        # vgmstream-rs, game-music-emu-patch, opus-decoder-patch
  viogsf/, vgmstream/   # Vendored C. vgmstream is a submodule; viogsf is not —
                        # see viogsf/README.upstream.md for why.
android/                # The Android app: Gradle, Kotlin, Compose
```

`tunante-core`, `tunante-codec` and `tunante-helper` are shared by every app.
**None of them may depend on Tauri or on any GUI toolkit.** `src-tauri/src/lib.rs`
re-exports the first two (`pub use tunante_core::db;`,
`pub use tunante_codec::metadata;`) so `crate::db::…` and `crate::metadata::…`
keep resolving inside the desktop app.

Where something belongs, when in doubt: **`tunante-core` if it only needs the
database or pure logic** (that is why `session.rs` and the queue live there);
**`tunante-helper` if it spawns or talks to the decoder process** (probe,
artwork, the library scan); **the app** if it is about a screen.

The rule this repository keeps learning the hard way: the moment a second app
needs the same thing, move it down rather than copy it. `decoder.rs`,
`scan_folder`, `folder_image` and `session.rs` all started in `tunante-mini`.

## Frontend Conventions
- Svelte 5 runes: `$state`, `$derived`, `$effect`, `$props`
- Stores: class-based pattern in `.svelte.ts` files
- No SSR (adapter-static, `ssr = false`)
- Dark theme (foobar2000-inspired color palette)

## Backend Conventions
- Error handling: `thiserror` for error types, `Result<T, String>` for Tauri commands
- Concurrency: `parking_lot::Mutex` for shared state
- IPC: Tauri commands for request/response, events for streaming updates
- UUIDs for all entity IDs

## Commands
- `npm run dev` - Start SvelteKit dev server
- `npm run tauri dev` - Start full Tauri dev mode
- `npm run build` - Build frontend
- `npm run tauri build` - Build production app
- `cargo check --manifest-path src-tauri/Cargo.toml --workspace --all-targets` - Check Rust code.
  Use `--workspace --all-targets`: without them the test and example targets of
  `tunante-core`/`tunante-codec` are never compiled, and breakage there goes unseen.
  Add `--exclude tunante-android`: that crate only builds for Android, and a
  desktop host cannot link it.
- `cd android && ./build.sh` - Build the Android APK. Compiles the Rust for both
  ABIs with cargo-ndk, stages the `.so` files into `jniLibs`, then runs Gradle.
  `ABIS="arm64-v8a" ./build.sh` skips the emulator build when only a phone matters.
  Needs `ANDROID_NDK_HOME` (defaults to the r27 in `~/Android/Sdk`) and a JDK 17+.
- `cargo test --manifest-path src-tauri/Cargo.toml -p tunante-codec --release` -
  Format smoke test. Decodes a real fixture through every emulator backend and
  asserts the PCM is not silence. This is the bar for "no regression" — CI runs it
  before every build. Release mode is required; the cores are far too slow in debug.
