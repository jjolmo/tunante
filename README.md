# Tunante

![Tunante Screenshot](screenshot.png)

*I have codevibed this to understand how replaceable I am as a programmer.*

A cross-platform music player focused on video game music formats, inspired by foobar2000.

Built with [Tauri v2](https://tauri.app/) (Rust backend) and [SvelteKit 2](https://kit.svelte.dev/) + [Svelte 5](https://svelte.dev/) (frontend).

![License](https://img.shields.io/badge/license-GPL--2.0-blue)

## Features

- **Standard audio**: MP3, FLAC, OGG Vorbis, WAV, AAC, AIFF, WMA, M4A, Opus, APE, WavPack
- **Chiptune / GME**: NSF, NSFE, SPC, GBS, VGM/VGZ, HES, KSS, AY, SAP, GYM — with auto-fade for looping tracks
- **PSF family**: PSF (PS1), PSF2 (PS2), GSF (GBA), 2SF (NDS), NCSF (NDS), USF (N64), SSF (Saturn), DSF (Dreamcast), QSF (Capcom QSound) — each with its `mini` variant, played by the original emulator cores
- **Game audio containers**: ADX, HCA, DSP, FSB, WEM, BCSTM, BFSTM, BRSTM, NUS3BANK, AT3/AT9, XMA, SCD, and 700+ formats via vgmstream

### Game music, treated as game music

- **Box art downloader**: Finds the real cover for a game and writes it next to
  the music. Matches against the [libretro-thumbnails](https://github.com/libretro-thumbnails)
  `Named_Boxarts` index — the same canonical No-Intro name list emulator frontends
  use — and falls back to iTunes, Steam, Deezer, Nintendo and Wikipedia for
  soundtracks no console archive carries.
- **Matching that refuses rather than guesses**: normalisation folds accents,
  articles and roman/arabic numerals, then six ranked match stages with an
  ambiguity guard. When the runner-up is within a hair of the winner it declines,
  because a wrong cover written into a synced folder is worse than no cover.
  `Mega Man X` stays X and does not become `Mega Man 10`.
- **Bulk download with a dry run**: Preview what would be written for a folder,
  console or playlist before anything touches disk, with progress, cancel, and an
  **undo** that removes exactly the files that run created — never one of yours.
  Existing folder art is never overwritten unless you ask.
- **Console detection**: Every track gets a console from its format and its place
  in the library. `.spc` *is* SNES wherever it sits; `.vgm` names no machine, so
  the folder wins. 32 consoles in one table shared by all three apps.
- **Game detection**: The game name comes from the ID666/tag album when it has
  one — which rescues abbreviated rips like `ct/` or `ff6/` — and from the folder
  when the tag names the soundtrack rather than the game.
- **Fix what it got wrong**: Per-track and per-folder overrides that survive a
  rescan, plus a "music we could not place" list so the unclassified folders are
  visible instead of silently mislabelled.
- **Auto-fade for looping tracks** and a configurable loop count for formats that
  never end on their own.

### Player and library

- **Library management**: Folder scanning, file watcher, full-text search, customizable columns (resize, reorder, show/hide)
- **Playlists**: Create, rename, delete, drag-and-drop tracks to add, build one from a folder
- **Console browser**: Filter tracks by game console (NES, SNES, Genesis, Game Boy, PS1, PS2, GBA, NDS...)
- **Ratings / Favorites**: Star toggle with metadata persistence (writes back to file tags)
- **Queue system**: Enqueue tracks, middle-click to add, context-aware auto-advance
- **Shuffle & Repeat**: Shuffle, repeat all, repeat one — synced with backend queue
- **DSP chain**: Equaliser, gain, limiter, stereo width, balance and mono fold-down
- **Output device selection**: Pick the sound card, not whatever the system picked
- **Global shortcuts**: Media keys and custom bindings that work while the window is hidden
- **System tray**: Minimize to tray, left-click show/hide toggle (Linux KDE/GNOME supported)
- **Metadata editor**: View and edit track metadata (title, artist, album, etc.)
- **Themes**: foobar2000-inspired dark palette, a light one, or follow the system
- **Built-in updater**: Checks releases and applies signed updates in place

## Supported Formats

| Category | Formats |
|----------|---------|
| Standard audio | MP3, FLAC, OGG, WAV, AAC, AIFF, WMA, M4A, Opus, APE, WavPack |
| GME chiptune | NSF, NSFE, SPC, GBS, VGM, VGZ, HES, KSS, AY, SAP, GYM |
| PSF family | PSF, PSF2, GSF, 2SF, NCSF, USF, SSF, DSF, QSF — and every `mini` variant |
| Game audio (vgmstream) | ADX, HCA, DSP, FSB, WEM, BNK, BCSTM, BFSTM, BRSTM, NUS3BANK, AT3, AT9, XMA, SCD, GENH, TXTH/TXTP, and [700+ more](https://github.com/vgmstream/vgmstream) |

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

# Install frontend dependencies — npm lives in apps/desktop/
cd apps/desktop
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
# Production build — npm lives in apps/desktop/
cd apps/desktop && npm run tauri build
```

The built application will be in `target/release/bundle/` at the repository root:
- **Linux**: `.deb` and `.AppImage`
- **macOS**: `.dmg`
- **Windows**: `.msi` and `.exe`

## Installation

### Linux
Download the `.AppImage` from [Releases](https://github.com/jjolmo/tunante/releases), make it executable (`chmod +x`), and run it. You can create a `.desktop` entry from Settings → General inside the app.

### Windows
Download the `.msi` or `.exe` installer from [Releases](https://github.com/jjolmo/tunante/releases) and run it.

### macOS (unsigned app)
The app is not signed with an Apple Developer certificate. macOS Sequoia+ will show **"Tunante is damaged and can't be opened"**. This is not true — it's just unsigned. To fix it:

1. Download the `.dmg` from [Releases](https://github.com/jjolmo/tunante/releases)
2. Open the `.dmg` and drag **Tunante** to your Applications folder
3. Open **Terminal** and run:
   ```bash
   xattr -cr /Applications/Tunante.app
   ```
4. Now open Tunante normally — the quarantine flag is removed and macOS won't block it again

## The other two Tunantes

Tunante is three applications, not one with two ports. They share the parts that
have nothing to do with a screen — `tunante-core` (database, play queue, console
and game classification, DSP), `tunante-codec` (every decoder and metadata
reader) and `tunante-art` (cover matching and download) — and **none of those may
depend on Tauri or on any GUI toolkit**. Above that line each app is written for
the machine it runs on, because a phone is not a small desktop.

Both phone apps decode **out of process**: the emulator cores run in a separate
`tunante-decoder` binary that takes a file and returns PCM over a pipe. On a
phone that matters twice over — a malformed rip that kills a decoder cannot take
the player down with it, and the memory an emulator core allocates goes away with
the process instead of staying resident.

Because covers are written into the game's own folder, art downloaded on the
desktop simply shows up on both phones once the folder syncs. Neither app has to
download anything for it to be there — though both can.

### tunante-mini — the small one

A second full player in [Slint](https://slint.dev/). **Not phone-only**: nothing
in its source knows what a distribution or a form factor is, `backend-winit`
covers both Wayland and X11, and the same binary runs on a phone and on a
desktop. It began as the postmarketOS build and stayed useful everywhere.

No web view and no JavaScript engine — it draws straight to the GPU through
GLES2, which is what makes it usable on hardware that predates the phone in your
pocket, and what makes it start in a fraction of the desktop app's time on
hardware that does not. Its interface is built for a thumb: large targets, no
hover, no right click.

Several pieces that now live in the shared crates started life here
(`decoder.rs`, `scan_folder`, `folder_image`, `session.rs`); the moment the
Android app needed the same thing, they moved down rather than being copied.

```bash
cargo run -p tunante-mini              # from the repository root
cargo build -p tunante-mini --release  # target/release/tunante-mini
```

CI builds it four ways on every change and attaches all four to
[Releases](https://github.com/jjolmo/tunante/releases):

| Build | For |
|-------|-----|
| `tunante-mini-x86_64-linux-gnu.tar.gz` | An ordinary desktop or laptop |
| `tunante-mini-aarch64-linux-gnu.tar.gz` | ARM boards, ARM laptops, an ARM desktop |
| `tunante-mini-x86_64-windows.zip` | Windows |
| `tunante-mini-*.apk` (Alpine, musl, aarch64) | postmarketOS and any Alpine phone |

MPRIS and the sleep inhibitor are freedesktop specifications, so on Windows the
lock-screen controls are compiled out and everything else is the same code.

Each tarball carries both `tunante-mini` and `tunante-decoder`, and they must
stay side by side: the player looks for the decoder as a sibling of itself, and
splitting them breaks playback with a message that does not explain why.

It needs fontconfig, ALSA and a working GL driver at runtime — all dlopened, so
a machine that lacks one says so rather than failing to start.

### tunante-android

A native Android app: Kotlin and Jetpack Compose for the interface, with the
whole Rust half compiled into a `cdylib` and reached over JNI. Playback,
database, decoding and classification are the same code the desktop runs; only
the screen is different.

```bash
cd apps/android && ./build.sh     # both ABIs
ABIS="arm64-v8a" ./build.sh       # phone only, skips the emulator build
```

Needs `ANDROID_NDK_HOME` and a JDK 17+. The APK is at
`apps/android/app/build/outputs/apk/`, and signed builds are attached to
[Releases](https://github.com/jjolmo/tunante/releases).

**Sideload only, and it will stay that way.** The app asks for
`MANAGE_EXTERNAL_STORAGE`, which Google Play forbids to media players. That is
not laziness: under the permission Play does allow, `MediaProvider` decides file
by file from a MIME map that has never heard of `.psf`, `.nsf`, `.spc`, `.gsf`
or `.2sf`. Those files are not indexed, so they cannot be opened — `readdir()`
does not even list them. A normal music player never notices. This one does not
work at all.

## Project Structure

Three apps under `apps/`, what they share under `crates/`, third-party C under
`vendor/`. The Cargo workspace root is the repository root.

```
apps/desktop/                 # Tauri v2 + SvelteKit — the desktop app
  src/                        #   Frontend: components, stores (runes), types, routes
  package.json                #   npm runs from here
  src-tauri/src/              #   Backend (Rust)
    audio/                    #     Audio engine, play queue, output devices
    commands/                 #     Tauri IPC commands (player, library, playlists)
    watcher/                  #     Folder watching
apps/mini/                    # Phone app for postmarketOS (Slint)
apps/android/                 # Android app (Gradle, Kotlin, Compose)
  rust/                       #   Its JNI half (cdylib)

crates/tunante-core/          # SQLite layer, play queue, session, DSP
crates/tunante-codec/         # Decoders and metadata readers
crates/tunante-decoder/       # Out-of-process decoder binary
crates/tunante-helper/        # Client for it: probe, artwork, library scan
crates/tunante-art/           # Cover art: matching, download, storage

vendor/game-music-emu-patch/  # Patched game-music-emu (C++ chiptune emulation)
vendor/vgmstream/             # vgmstream submodule (C, game audio decoding)
vendor/vgmstream-rs/          # Rust bindings for vgmstream
vendor/hepsf-rs/              # PSF/PSF2 playback (C, Highly Experimental + sexypsf)
vendor/vio2sf-rs/             # 2SF playback (C, vio2sf + DeSmuME core)
vendor/viogsf-rs/, viogsf/    # GSF playback (C, VBA-M)
vendor/lazyusf2-rs/           # USF playback (C, N64 core)
vendor/opus-decoder-patch/    # Pure Rust Opus decoder (patched)
```

`assets/logo.png` is the only drawing in the repository. All 30 icons — Tauri's
bundle and tray, tunante-mini's hicolor set, Android's launcher and adaptive
mipmaps — are generated from it by `scripts/gen-icons.py`, checked in CI, and
never edited by hand.

## Other Commands

```bash
# Check Rust code (from the repository root)
cargo check --workspace --all-targets --exclude tunante-android

# Check Svelte/TypeScript
cd apps/desktop && npm run check

# Frontend dev server only (no Tauri)
cd apps/desktop && npm run dev
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

**"Why you built it?"**

First of all: I didn't do shit, I just prompted it. My creative coding happens on gamedev which is the only place I'm happy to code like we did in 2022. I built this because Foobar2000 with videogame plugins does not work on Linux and Mac. And I was tired of trying to emulate it with Wine. I tried [Cog](https://github.com/losnoco/Cog) also for Mac but it was crashing like hell with my library and doesn't cover all my consoles. Then [Fooyin](https://github.com/fooyin/fooyin) only works on Linux so I really needed something that worked on Mac because I use them in my paid fulltime work. And also it doesn't include all the videogame libraries and is not able to handle all my collection, again because it crashes. So here we are.

**"I have found a bug, what should I do?"**

Create a PR, or fork it and fix it, I don't care anymore. Software development is dead, and my time is more expensive to just ask the AI to fix things when you can also contribute.

**"But it's codevibed shit, is it secure?"**

I don't know, ask Copilot, they know all the answers it seems, even a comprehensive guide about how to be a plumber.

**"I need this feature"**

Fork it or open a PR. I'll decide if I want to ship it or not. This app is meant to be customized for me with minimal elements to make it faster. Anyway in the near future you won't need a stupid GitHub to get an app. You will vibecode it on demand and dedicate the rest of your time to do creative tasks like washing your dishes or do your laundry.

**"Format doesn't work?"**

Send me an example. I'll just tell the AI to do it so I can go to the fucking fuck anywhere and keep learning how to unclog a WC.

**"How much it took to build first 'stable' version?"**

2 days, while I was watching youtube videos to decide if I should sell all my stock before meltdown happens.

**"How many lines were made by human hand?"**

0 (Zero). So you can start reflecting now what kind of future we're moving towards ☀️

**"Why do I need that crap command to run it on macOS?"**

I don't know... ask Apple why we need to pay a fucking $1,00 yearly subscription to release an app without adding paranoid notes saying my app might destroy your family and delete pizza from Earth. The same goes for Windows. I don't have Windows to check if it works. So if it does, send a message, and I'll update the readme. But I guess you will need to authorise opening an "insecure app". I'm not going to give a penny to those fucking extortionists.

**"Do I need this app?"**

Only if you want a multiplatform app that reads almost all video game formats. Otherwise please search for a more serious player that was made by human hearts instead of this idiotic amalgam of data.

## License

This project is licensed under the **GNU General Public License v2.0** — see the [LICENSE](LICENSE) file for details.

GPL v2 is required because the project statically links GPL-licensed C/C++ libraries (sexypsf, DeSmuME, MAME YM2612 emulator).
