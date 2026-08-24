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
    tunante-core/       # UI-agnostic: db/ (SQLite), queue, vgm_path, dsp/
    tunante-codec/      # Every decoder + metadata reader, and all the vendored C
  <vendored FFI crates> # lazygsf-rs, viogsf-rs, vio2sf-rs, hepsf-rs, lazyusf2-rs,
                        # vgmstream-rs, game-music-emu-patch, opus-decoder-patch
```

`tunante-core` and `tunante-codec` are shared with `tunante-mini`, the mobile
build. **Neither may depend on Tauri or on any GUI toolkit.** `src-tauri/src/lib.rs`
re-exports them (`pub use tunante_core::db;`, `pub use tunante_codec::metadata;`)
so `crate::db::…` and `crate::metadata::…` keep resolving inside the desktop app.

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
- `cargo test --manifest-path src-tauri/Cargo.toml -p tunante-codec --release` -
  Format smoke test. Decodes a real fixture through every emulator backend and
  asserts the PCM is not silence. This is the bar for "no regression" — CI runs it
  before every build. Release mode is required; the cores are far too slow in debug.
