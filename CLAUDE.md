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
src-tauri/src/          # Backend (Rust)
  audio/                # Audio engine (rodio), play queue
  commands/             # Tauri IPC commands (player, library, playlists)
  db/                   # SQLite database layer
  metadata/             # Audio file metadata reader (lofty)
```

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
- `cargo check --manifest-path src-tauri/Cargo.toml` - Check Rust code
