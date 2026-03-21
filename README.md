# Tunante

A cross-platform music player focused on video game music formats, inspired by foobar2000.

## Features

- **Standard audio playback**: MP3, FLAC, OGG, WAV, AAC, AIFF, WMA, M4A, Opus, APE, WavPack
- **Chiptune / VGM support**: NSF, SPC, GBS, VGM/VGZ, HES, KSS, AY, SAP, GYM
- **PSF family**: GSF, PSF, PSF2, USF, SSF, DSF, 2SF, NCSF (+ mini variants)
- **Game audio containers**: ADX, HCA, DSP, FSB, WEM, BNK, NUS3BANK, BCSTM, BFSTM, BRSTM, and many more via vgmstream
- **Library management**: Folder monitoring, full-text search, customizable columns
- **Playlists**: Create, rename, drag-and-drop tracks
- **Ratings / Favorites**: Star toggle with metadata persistence (writes back to file tags)
- **Queue system**: Enqueue tracks, middle-click to add
- **Album artwork**: Embedded art display in sidebar
- **Dark & Light themes**: System theme auto-detection
- **Keyboard shortcuts**: Ctrl+A select all, Delete to remove from playlist, Ctrl+P settings

## Prerequisites

- **Node.js** 20+ and npm
- **Rust** stable toolchain (1.70+)
- **Linux**: GTK 3 and WebKit2GTK development libraries
  ```bash
  sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev
  ```
- **macOS**: Xcode Command Line Tools
- **Windows**: Microsoft Visual Studio C++ Build Tools

## Development

```bash
# Install frontend dependencies
npm install

# Start development mode (frontend + backend)
npm run tauri dev
```

## Build

```bash
# Build production app
npm run tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

## Supported Formats

| Category | Formats |
|----------|---------|
| Standard audio | MP3, FLAC, OGG, WAV, AAC, AIFF, WMA, M4A, Opus, APE, WavPack |
| GME chiptune | NSF, NSFE, SPC, GBS, VGM, VGZ, HES, KSS, AY, SAP, GYM |
| PSF family | GSF, PSF, PSF2, USF, SSF, DSF, 2SF, NCSF (+ mini variants) |
| Game audio | ADX, HCA, DSP, FSB, WEM, BNK, BCSTM, BFSTM, BRSTM, and 50+ more |

## Tech Stack

- **[Tauri v2](https://tauri.app/)** - Rust-based desktop framework
- **[SvelteKit 2](https://kit.svelte.dev/)** + **[Svelte 5](https://svelte.dev/)** - Frontend framework
- **[rodio](https://github.com/RustAudio/rodio)** + **[symphonia](https://github.com/pdeljanov/Symphonia)** - Audio playback
- **[lofty](https://github.com/Serial-ATA/lofty-rs)** - Audio metadata reading/writing
- **[rusqlite](https://github.com/rusqlite/rusqlite)** - SQLite database
- **[game-music-emu](https://github.com/gme-rs/game-music-emu-rs)** - Chiptune emulation
- **[vgmstream-rs](https://github.com/vgmstream/vgmstream)** - Game audio decoding
- **[hepsf-rs](https://github.com/)** - PSF/PSF2 playback via Highly Experimental
- **[lazygsf-rs](https://github.com/)** - GSF playback via Lazy GSF
- **[vio2sf-rs](https://github.com/)** - 2SF/NDS playback via vio2sf

## License

MIT
