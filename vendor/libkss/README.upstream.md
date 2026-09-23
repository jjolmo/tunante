# libkss — vendored, not a submodule

Upstream: <https://github.com/digital-sound-antiques/libkss>
Taken at: `99c1aaf3e2ec8226cd8ab05b6da1578d72218001` ("Updated emu2413 and
emu8950 to reduce table size.", 2026-09-07).

Its own submodules are vendored at the commits upstream pinned:

| module   | commit         | what it emulates                |
|----------|----------------|---------------------------------|
| emu2149  | `1ca770bf7bd4` | AY-3-8910 / YM2149 (the PSG)    |
| emu2212  | `ee35dbe099f7` | SCC — Konami's wavetable chip   |
| emu2413  | `4c2e35046328` | YM2413 (MSX-MUSIC / OPLL)       |
| emu76489 | `36a784abd7d6` | SN76489                         |
| emu8950  | `fb129b18432c` | Y8950 (MSX-AUDIO)               |
| kmz80    | `86f9e79db455` | the Z80                         |

## Why it is here at all

Game Music Emu decodes every other chiptune format this player handles, KSS
included on paper — `gme_kss_type` exists and the backend compiles. It does not
*work*: on a real Konami rip the Z80 runs the init routine, writes twice to the
PSG and stops. `Kss_Emu::run_clocks` only calls the music routine when the CPU
has returned to its idle address, and these drivers never return, so the
routine is never called again and the output is silence from the first sample.

GME emulates barely any MSX: `Kss_Emu::start_track_` fills low RAM with `RET`
and hand-writes six bytes of BIOS — WRTPSG and RDPSG — and that is the machine.
libkss emulates the actual computer, which is what these drivers expect. Same
two files, measured: GME gives peak 0 and rms 0 on every subsong; libkss gives
sustained rms 500–1100, different per song.

## What is not here, and why

`modules/drivers` (the `kss-drivers` submodule) is **deliberately absent**.
libkss's own `LICENSE.md` opens by saying those blobs do not comply with its
licence, and this repository is not going to redistribute them.

They are only needed to convert MGS, BGM, OPX, MPK and MBM files into KSS —
formats this player does not claim. The five converters that reach for them
(`src/kss/{mgs,bgm,opx,mpk,mbm}2kss.c`) are therefore not compiled either, and
`libkss-rs/c/stubs.c` supplies the handful of symbols `kss.c` links against:
every detector answers "not my format", every converter declines. A `.kss` file
never takes those paths.

## What we changed

Nothing in the source. The build lives in `../libkss-rs/build.rs`, which names
the files to compile rather than globbing — that is what keeps the converters
and the two example `main`s (`kmz80/makeft.c`, `emu2413/sample2413.c`) out.
