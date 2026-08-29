#!/usr/bin/env python3
"""Every icon in the repository, from one file.

`assets/logo.png` is the only drawing anyone edits. Everything under
`apps/*/…/icons` and `apps/android/…/res/mipmap-*` is output, regenerated from
it, and should never be touched by hand — an edit there survives exactly until
the next time this runs.

The source is pixel art, and that decides how it is scaled. Integer multiples
use nearest-neighbour, which is not a compromise but the correct filter: it
reproduces the artwork exactly rather than inventing gradients between pixels
the artist placed deliberately. Sizes that are not a multiple of the source go
up to 1024 with nearest first and only then down with Lanczos, so the smoothing
happens once, at the end, instead of blurring the art on the way up.

Run it by hand with `python3 scripts/gen-icons.py`, or let the pre-commit hook
in `scripts/hooks/` do it when `assets/logo.png` is staged. `--check` reports
what would change and writes nothing, which is what CI would want.
"""

import argparse
import hashlib
import io
import sys
from pathlib import Path

try:
    from PIL import Image, ImageDraw
except ImportError:
    sys.exit("needs Pillow: pip install --user Pillow")

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "assets" / "logo.png"

DESKTOP = ROOT / "apps/desktop/src-tauri/icons"
MINI = ROOT / "apps/mini/dist/icons"
ANDROID = ROOT / "apps/android/app/src/main/res"

# Android ships one bitmap per density. The launcher bitmap is 48dp and the
# adaptive foreground is 108dp, of which only the middle 66dp is guaranteed to
# survive whatever shape the launcher masks it into.
DENSITIES = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
SAFE_ZONE = 66 / 108


def load() -> Image.Image:
    if not SOURCE.is_file():
        sys.exit(f"no source at {SOURCE}")
    img = Image.open(SOURCE).convert("RGBA")
    if img.width != img.height:
        sys.exit(f"{SOURCE} is {img.width}x{img.height}; it has to be square")
    return img


def scale(img: Image.Image, n: int) -> Image.Image:
    """Resize to n×n, preserving pixel art wherever the maths allows."""
    if n == img.width:
        return img.copy()
    if n % img.width == 0:
        return img.resize((n, n), Image.NEAREST)
    if n < img.width:
        return img.resize((n, n), Image.LANCZOS)
    # Up to a clean multiple first, then down. Scaling straight up with Lanczos
    # would smear a 32px drawing into mush at 432.
    big = img.width * ((1024 + img.width - 1) // img.width)
    return img.resize((big, big), Image.NEAREST).resize((n, n), Image.LANCZOS)


def circle(img: Image.Image) -> Image.Image:
    """Android's `ic_launcher_round`, for launchers that ask for one."""
    mask = Image.new("L", (img.width * 4,) * 2, 0)
    ImageDraw.Draw(mask).ellipse((0, 0, mask.width - 1, mask.height - 1), fill=255)
    mask = mask.resize(img.size, Image.LANCZOS)
    out = img.copy()
    out.putalpha(Image.composite(img.getchannel("A"), Image.new("L", img.size, 0), mask))
    return out


def on_safe_zone(img: Image.Image, n: int) -> Image.Image:
    """Centre the logo in an adaptive-icon canvas, inside the safe zone.

    A foreground drawn edge to edge loses its corners to every mask that is not
    a square, which on this logo would cut the cartridge's own border off.
    """
    inner = max(1, round(n * SAFE_ZONE))
    canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    art = scale(img, inner)
    canvas.paste(art, ((n - inner) // 2, (n - inner) // 2), art)
    return canvas


def png(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def build(src: Image.Image) -> dict[Path, bytes]:
    """Every output file and its bytes. Nothing is written here."""
    out: dict[Path, bytes] = {}

    # --- desktop (Tauri) -------------------------------------------------
    for name, n in [("32x32.png", 32), ("128x128.png", 128),
                    ("128x128@2x.png", 256), ("master.png", 512)]:
        out[DESKTOP / name] = png(scale(src, n))

    # Both tray variants are the same drawing. The pair exists because the old
    # icon was a white note that disappeared on a light panel; this one carries
    # its own background and reads on either. Kept as two files so lib.rs, which
    # include_bytes! both, does not have to change — the theme switch is simply
    # a no-op now.
    for name in ["tray-icon-big.png", "tray-icon-big-fixed.png",
                 "tray-icon-big-black-fixed.png"]:
        out[DESKTOP / name] = png(scale(src, 128))
    out[DESKTOP / "tray-icon.png"] = png(scale(src, 32))

    buf = io.BytesIO()
    scale(src, 256).save(buf, format="ICO",
                         sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    out[DESKTOP / "icon.ico"] = buf.getvalue()

    buf = io.BytesIO()
    scale(src, 1024).save(buf, format="ICNS")
    out[DESKTOP / "icon.icns"] = buf.getvalue()

    # --- mini ------------------------------------------------------------
    for n in (48, 64, 128, 256, 512):
        out[MINI / f"{n}x{n}" / "tunante-mini.png"] = png(scale(src, n))

    # --- android ---------------------------------------------------------
    for density, factor in DENSITIES.items():
        d = ANDROID / f"mipmap-{density}"
        launcher = scale(src, round(48 * factor))
        out[d / "ic_launcher.png"] = png(launcher)
        out[d / "ic_launcher_round.png"] = png(circle(launcher))
        out[d / "ic_launcher_foreground.png"] = png(on_safe_zone(src, round(108 * factor)))

    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true",
                    help="report what is stale and write nothing")
    args = ap.parse_args()

    src = load()
    planned = build(src)
    stale = []
    for path, data in sorted(planned.items()):
        old = path.read_bytes() if path.is_file() else None
        # Compared by content, not by mtime: a checkout touches every file, and
        # regenerating on every commit would put noise in the history.
        if old is not None and hashlib.sha256(old).digest() == hashlib.sha256(data).digest():
            continue
        stale.append(path)
        if not args.check:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)

    rel = lambda p: p.relative_to(ROOT)
    if args.check:
        if stale:
            print(f"{len(stale)} icon(s) do not match {rel(SOURCE)}:")
            for p in stale:
                print(f"  {rel(p)}")
            print("\nrun: python3 scripts/gen-icons.py")
            return 1
        print(f"all {len(planned)} icons match {rel(SOURCE)}")
        return 0

    if stale:
        print(f"regenerated {len(stale)} of {len(planned)} icons from {rel(SOURCE)}")
        for p in stale:
            print(f"  {rel(p)}")
    else:
        print(f"all {len(planned)} icons already match {rel(SOURCE)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
