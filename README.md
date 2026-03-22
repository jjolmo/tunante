# Tunante

*I have codevibed this to understand how replaceable I am as a programmer.*

A cross-platform music player focused on video game music formats, inspired by foobar2000.

Built with [Tauri v2](https://tauri.app/) (Rust backend) and [SvelteKit 2](https://kit.svelte.dev/) + [Svelte 5](https://svelte.dev/) (frontend).

![License](https://img.shields.io/badge/license-GPL--2.0-blue)

## Features

- **Standard audio**: MP3, FLAC, OGG Vorbis, WAV, AAC, AIFF, WMA, M4A, Opus, APE, WavPack
- **Chiptune / GME**: NSF, NSFE, SPC, GBS, VGM/VGZ, HES, KSS, AY, SAP, GYM — with auto-fade for looping tracks
- **PSF family**: GSF (GBA), PSF (PS1), PSF2 (PS2), 2SF (NDS) — plus mini variants
- **Game audio containers**: ADX, HCA, DSP, FSB, WEM, BCSTM, BFSTM, BRSTM, NUS3BANK, and 700+ formats via vgmstream
- **Library management**: Folder scanning, file watcher, full-text search, customizable columns (resize, reorder, show/hide)
- **Playlists**: Create, rename, delete, drag-and-drop tracks to add
- **Console browser**: Filter tracks by game console (NES, SNES, Genesis, Game Boy, PS1, PS2, GBA, NDS...)
- **Ratings / Favorites**: Star toggle with metadata persistence (writes back to file tags)
- **Queue system**: Enqueue tracks, middle-click to add, context-aware auto-advance
- **Shuffle & Repeat**: Shuffle, repeat all, repeat one — synced with backend queue
- **Album artwork**: Embedded art display in sidebar
- **System tray**: Minimize to tray, left-click show/hide toggle (Linux KDE/GNOME supported)
- **Metadata editor**: View and edit track metadata (title, artist, album, etc.)
- **Dark theme**: foobar2000-inspired dark color palette

## Supported Formats

| Category | Formats |
|----------|---------|
| Standard audio | MP3, FLAC, OGG, WAV, AAC, AIFF, WMA, M4A, Opus, APE, WavPack |
| GME chiptune | NSF, NSFE, SPC, GBS, VGM, VGZ, HES, KSS, AY, SAP, GYM |
| PSF family | GSF, miniGSF, PSF, miniPSF, PSF2, miniPSF2, 2SF, mini2SF |
| Game audio (vgmstream) | ADX, HCA, DSP, FSB, WEM, BNK, BCSTM, BFSTM, BRSTM, NUS3BANK, and [700+ more](https://github.com/vgmstream/vgmstream) |

## Prerequisites

### All Platforms

- **Node.js** 20+ and npm
- **Rust** stable toolchain (1.85+) — install via [rustup](https://rustup.rs/)
- **CMake** — required for building vgmstream
- **C/C++ compiler** — required for native audio libraries (gcc/g++ on Linux, Xcode on macOS, MSVC on Windows)

### Linux

```bash
# Ubuntu / Debian
sudo apt install build-essential pkg-config cmake \
  libgtk-3-dev libwebkit2gtk-4.1-dev libssl-dev libsoup-3.0-dev \
  libappindicator3-dev librsvg2-dev libasound2-dev

# Fedora
sudo dnf install gcc-c++ cmake pkg-config \
  gtk3-devel webkit2gtk4.1-devel openssl-devel libsoup3-devel \
  libappindicator-gtk3-devel librsvg2-devel alsa-lib-devel
```

### macOS

```bash
xcode-select --install
brew install cmake
```

### Windows

- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with "Desktop development with C++"
- [CMake](https://cmake.org/download/) (add to PATH)

## Quick Start

```bash
# Clone with submodules (vgmstream is a git submodule)
git clone --recurse-submodules https://github.com/jjolmo/tunante.git
cd tunante

# Install frontend dependencies
npm install

# Start development mode
npm run tauri dev
```

If you already cloned without `--recurse-submodules`:

```bash
git submodule update --init --recursive
```

## Build

```bash
# Production build
npm run tauri build
```

The built application will be in `src-tauri/target/release/bundle/`:
- **Linux**: `.deb` and `.AppImage`
- **macOS**: `.dmg`
- **Windows**: `.msi` and `.exe`

## Project Structure

```
src/                          # Frontend (SvelteKit + Svelte 5)
  lib/components/             # UI components (TrackList, Sidebar, PlayerBar...)
  lib/stores/                 # Shared state (.svelte.ts with runes)
  lib/types/                  # TypeScript type definitions
  routes/                     # SvelteKit pages

src-tauri/src/                # Backend (Rust)
  audio/                      # Audio engine, play queue, format decoders
  commands/                   # Tauri IPC commands (player, library, playlists)
  db/                         # SQLite database layer
  metadata/                   # Audio file metadata readers

src-tauri/game-music-emu-patch/  # Patched game-music-emu (C++ chiptune emulation)
src-tauri/vgmstream/             # vgmstream submodule (C, game audio decoding)
src-tauri/vgmstream-rs/          # Rust bindings for vgmstream
src-tauri/hepsf-rs/              # PSF/PSF2 playback (C, Highly Experimental + sexypsf)
src-tauri/lazygsf-rs/            # GSF playback (C, Lazy GSF + mGBA core)
src-tauri/vio2sf-rs/             # 2SF playback (C, vio2sf + DeSmuME core)
src-tauri/opus-decoder-patch/    # Pure Rust Opus decoder (patched)
```

## Other Commands

```bash
# Check Rust code
cargo check --manifest-path src-tauri/Cargo.toml

# Check Svelte/TypeScript
npm run check

# Frontend dev server only (no Tauri)
npm run dev
```

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Desktop framework | [Tauri v2](https://tauri.app/) |
| Frontend | [SvelteKit 2](https://kit.svelte.dev/) + [Svelte 5](https://svelte.dev/) |
| Styling | [Tailwind CSS v4](https://tailwindcss.com/) |
| Audio playback | [rodio](https://github.com/RustAudio/rodio) + [symphonia](https://github.com/pdeljanov/Symphonia) |
| Chiptune emulation | [game-music-emu](https://github.com/gme-rs/game-music-emu-rs) (C++) |
| Game audio decoding | [vgmstream](https://github.com/vgmstream/vgmstream) (C) |
| PSF/PS1 playback | sexypsf + Highly Experimental (C) |
| GSF/GBA playback | [Lazy GSF](https://github.com/) + mGBA core (C) |
| 2SF/NDS playback | [vio2sf](https://github.com/) + DeSmuME core (C) |
| Opus decoding | Pure Rust (patched [Rusopus](https://github.com/TadeuszWolfGang/Rusopus)) |
| Metadata | [lofty](https://github.com/Serial-ATA/lofty-rs) |
| Database | [rusqlite](https://github.com/rusqlite/rusqlite) (SQLite, bundled) |
| Concurrency | [parking_lot](https://github.com/Amanieu/parking_lot) |

## FAQ

**"I have found a bug, what should I do?"**

Create a PR, or fork it and fix it, I don't care anymore. Software development is dead, and my time is more expensive to just ask the AI to fix things when you can also contribute.

**"But it's codevibed shit, is it secure?"**

I don't know, ask Copilot, they know all the answers it seems, even a comprehensive guide about how to be a plumber.

**"I need this feature"**

Fork it or open a PR. I'll decide if I want to ship it or not. This app is meant to be customized for me with minimal elements to make it faster. Anyway in the near future you won't need a stupid GitHub to get an app. You will vibecode it on demand and dedicate the rest of your time to do creative tasks like washing your dishes or do your laundry.

## License

This project is licensed under the **GNU General Public License v2.0** — see the [LICENSE](LICENSE) file for details.

GPL v2 is required because the project statically links GPL-licensed C/C++ libraries (sexypsf, DeSmuME, MAME YM2612 emulator).
