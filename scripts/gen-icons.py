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
# The tray wants a silhouette, not a picture. See TRAY below.
TRAY_SOURCE = ROOT / "assets" / "system.svg"

DESKTOP = ROOT / "apps/desktop/src-tauri/icons"
MINI = ROOT / "apps/mini/dist/icons"
FAVICON = ROOT / "apps/desktop/static/favicon.svg"
TRAY = ROOT / "apps/desktop/src-tauri/icons/tray"
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


def svg(img: Image.Image) -> bytes:
    """The same drawing as vector, for the window's tab icon.

    One `<rect>` per horizontal run of identical pixels rather than one per
    pixel: same picture, a fraction of the file, and still exact — nothing is
    approximated, so it stays sharp at any size where a PNG would not.

    `shape-rendering="crispEdges"` matters. Without it a renderer antialiases
    every rectangle edge and the seams between runs show as faint lines through
    what should be flat colour.
    """
    w, h = img.size
    px = img.load()
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" '
        f'shape-rendering="crispEdges">'
    ]
    for y in range(h):
        x = 0
        while x < w:
            r, g, b, a = px[x, y]
            run = 1
            while x + run < w and px[x + run, y] == (r, g, b, a):
                run += 1
            if a:
                fill = f"#{r:02x}{g:02x}{b:02x}"
                op = "" if a == 255 else f' fill-opacity="{a / 255:.3f}"'
                out.append(
                    f'<rect x="{x}" y="{y}" width="{run}" height="1" fill="{fill}"{op}/>'
                )
            x += run
    out.append("</svg>\n")
    return "".join(out).encode()


def _glyph(source: Path) -> tuple[str, str, str]:
    """The drawing, its viewBox, and the paint rules that must travel with it.

    `fill-rule` is the one that bites. Inkscape puts it on the wrapping <g>,
    not on the paths, and a frame drawn as an outer rectangle plus an inner one
    only has a hole in it under `evenodd`. Extract the paths and leave that
    behind and the icon becomes a solid black square — which is exactly what
    the first version of this produced.
    """
    import re
    raw = source.read_text()
    body = "".join(m.group(0) for m in re.finditer(r"<path\b[^>]*/>", raw))
    if not body:
        sys.exit(f"{source}: found no self-closing <path>; cannot build a glyph")
    body = re.sub(r'\s(?:fill|style)="[^"]*"', "", body)
    box = re.search(r'viewBox="([^"]+)"', raw)
    rules = " ".join(
        f'{name}="{m.group(1)}"'
        for name in ("fill-rule", "clip-rule")
        for m in [re.search(rf'{name}="([^"]+)"', raw)]
        if m
    )
    return body, (box.group(1) if box else "0 0 100 100"), rules


def symbolic_svg(source: Path) -> bytes:
    """A monochrome glyph in the form both toolkits recolour.

    Neither desktop invents this: each looks for something specific, and the
    two requirements do not overlap, so a file has to satisfy both.

    GTK wraps the document and injects `rect,circle,path { fill: <colour> }`
    before rendering (`gtk_icon_info_load_symbolic`). A CSS rule outranks a
    presentation attribute, so a plain `fill="#000000"` on the path is
    overridden and this needs nothing — but the icon must be an SVG whose name
    ends in `-symbolic`, or GTK never takes that path at all.

    Plasma recolours through its own stylesheet, keyed on the class
    `ColorScheme-Text` with `fill="currentColor"` underneath it. Without that
    class Breeze leaves the icon alone, and a black glyph on Plasma's dark
    panel is an invisible one.

    So: the class for Plasma, `currentColor` for the cascade, and the geometry
    untouched for GTK. Anything that understands neither still gets a valid
    black-on-transparent SVG.
    """
    body, view, rules = _glyph(source)
    doc = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="%s" width="16" height="16">\n'
        '  <defs><style id="current-color-scheme" type="text/css">\n'
        "    .ColorScheme-Text { color: #000000; }\n"
        "  </style></defs>\n"
        '  <g class="ColorScheme-Text" fill="currentColor" %s>%s</g>\n'
        "</svg>\n"
    ) % (view, rules, body)
    return doc.encode()


def mono(source: Path, n: int, white: bool) -> bytes:
    """A flat render of the glyph, for the platforms that cannot recolour.

    Windows has no template mechanism and Linux panels do not touch a pixmap,
    so somebody has to pick the colour, and it has to be picked twice.
    """
    import subprocess
    colour = "#ffffff" if white else "#000000"
    body, view, rules = _glyph(source)
    flat = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" '
        f'width="{n}" height="{n}" fill="{colour}" {rules}>{body}</svg>'
    )
    # ImageMagick 7 installs `magick`; the `imagemagick` package on Ubuntu is
    # still 6, whose binary is `convert`. Try both before giving up.
    last = ""
    for exe in ("magick", "convert"):
        try:
            out = subprocess.run(
                [exe, "-background", "none", "svg:-", "-resize", f"{n}x{n}", "png:-"],
                input=flat.encode(), capture_output=True,
            )
        except FileNotFoundError:
            continue
        if out.returncode == 0:
            return out.stdout
        last = out.stderr.decode()[:400]
    sys.exit(f"no usable ImageMagick (magick/convert). Last error: {last}")


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

    # The tab icon of the desktop app's own window.
    out[FAVICON] = svg(src)

    # --- tray -------------------------------------------------------------
    #
    # Three renderings of one glyph, because the three desktops solve the
    # light/dark problem in three incompatible ways:
    #
    #   macOS   marks the image as a template and inverts it itself, including
    #           the highlighted state while the menu is open, which an app
    #           swapping files by hand cannot reproduce.
    #   Windows has no such mechanism, and the notification area follows a
    #           *different* setting from the app theme, so both colours ship.
    #   Linux   recolours only when handed an icon *name* ending in -symbolic
    #           that resolves to an SVG; a pixmap is drawn as given.
    if TRAY_SOURCE.is_file():
        out[TRAY / "tunante-symbolic.svg"] = symbolic_svg(TRAY_SOURCE)
        # A stamp of the source, because the three PNGs below cannot be
        # compared byte for byte. They come out of ImageMagick, and two
        # machines with different versions of it render the same SVG to
        # different bytes — so `--check` on the pixels fails on a difference
        # that means nothing. What it has to catch is a glyph edited without
        # regenerating, and that is exactly what this records.
        out[TRAY / "source.sha256"] = (
            hashlib.sha256(TRAY_SOURCE.read_bytes()).hexdigest() + "\n"
        ).encode()
        # macOS reads the alpha channel and ignores the colour, so black is
        # convention rather than requirement. 44px is 22pt at 2x.
        out[TRAY / "template.png"] = mono(TRAY_SOURCE, 44, white=False)
        out[TRAY / "mono-black.png"] = mono(TRAY_SOURCE, 32, white=False)
        out[TRAY / "mono-white.png"] = mono(TRAY_SOURCE, 32, white=True)

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
    ap.add_argument("--list", action="store_true",
                    help="print every path this manages, one per line, and exit")
    args = ap.parse_args()

    src = load()
    planned = build(src)

    # So the pre-commit hook can stage exactly what was written without keeping
    # its own copy of the list. The first version had one, and the moment the
    # favicon was added here the hook silently stopped staging it — the commit
    # went out with a regenerated file left behind.
    if args.list:
        for path in sorted(planned):
            print(path.relative_to(ROOT))
        return 0

    # Produced by an external rasteriser, so their bytes are not reproducible
    # across machines. `source.sha256`, generated beside them, is what proves
    # they are current.
    unstable = {TRAY / "template.png", TRAY / "mono-black.png", TRAY / "mono-white.png"}

    stale = []
    for path, data in sorted(planned.items()):
        if args.check and path in unstable:
            if not path.is_file():
                stale.append(path)
            continue
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
