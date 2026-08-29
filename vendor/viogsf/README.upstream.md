# viogsf — vendored, not a submodule

Upstream: <https://github.com/kode54/viogsf>
Taken at: `6c43a9926a6a85fbb736ea8f5f7f6c4f59ed3d64` ("Updated to v141_xp platform
toolkit.", 2018-01-30), which is the last commit upstream has.

## Why it is vendored

It was a submodule until 2026-08-27. That meant the one change we need could
not live anywhere sane: `viogsf-rs/build.rs` applied it by rewriting the header
inside the submodule on every build, so the checkout was permanently dirty, the
build was not reproducible, and a read-only source tree could not build at all.

Pointing the submodule at a fork would have fixed it too, but this repository
already solves exactly this problem four other times — `game-music-emu-patch`,
`opus-decoder-patch`, `tray-icon-patch`, `libappindicator-patch` are all
vendored, patched copies of upstream C. At 512 KB across 32 files this is
smaller than two of them. Same problem, same answer.

## What we changed

One line, `vbam/gba/GBAcpu.h`:

```diff
-#ifdef __GNUC__
+#if defined(__GNUC__) && (defined(__i386__) || defined(__x86_64__))
 # define INSN_REGPARM __attribute__((regparm(2)))
```

`regparm` is an x86-only calling-convention attribute. Clang rejects it on
aarch64, so without this guard nothing ARM builds — not macOS on Apple silicon,
not the phone, not Android.

Upstream has not moved since 2018, so there is no rebase to keep up with. If it
ever does, diff against the commit named above.
