#!/usr/bin/env python3
"""Every user-visible string must be translatable, and every catalog complete.

This exists because "everything is translated" was claimed three times and was
three times wrong: a literal that never reaches `@tr`/`i18n::tr` cannot be
fixed by any amount of editing the `.po` files, and a msgid that reaches them
but is missing from a catalog falls back to Spanish without a word of warning.
Neither failure shows up in a build, so it has to show up here.

Run with no arguments to check; the exit code is what CI reads.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UI = ROOT / "apps/tunante/ui"
SRC = ROOT / "apps/tunante/src"
TRANSLATIONS = ROOT / "apps/tunante/translations"

# Match arms and const tables reached through `i18n::tr(match …)` or
# `i18n::tr(table_field)`: real msgids that no literal-call scan can see.
# Keep this in step with the helpers it names.
INDIRECT: dict[str, list[str]] = {
    "SHORTCUT_ACTIONS (main.rs)": [
        "Tecla · Reproducir/Pausa", "Tecla · Stop", "Tecla · Anterior",
        "Tecla · Siguiente", "Tecla · Subir volumen", "Tecla · Bajar volumen",
        "Tecla · Silenciar", "Tecla · Aleatorio", "Tecla · Repetir",
        "Tecla · Buscar", "Tecla · Favorito",
    ],
    "shortcut_combo key names (main.rs)": [
        "Espacio", "Arriba", "Abajo", "Izquierda", "Derecha",
        "Inicio", "Fin", "RePág", "AvPág",
    ],
    "mouse_action_label (main.rs)": [
        "nada", "reproducir/pausa", "parar", "anterior", "siguiente",
        "subir volumen", "bajar volumen", "silenciar", "aleatorio",
        "repetir", "buscar",
    ],
    "cover-match confidence (main.rs)": ["exacta", "alta", "media", "baja"],
    "bulk cover plan (main.rs)": [
        "ya tiene (se conserva)", "sin resultado", "se escribiría",
        "Buscando", "Descargando",
    ],
    "settings values (main.rs)": [
        "el juego", "el álbum", "disponible", "nada que deshacer",
    ],
    # `TABLE_COLUMNS` feeds `i18n::tr(d.label)`, so every label in that const
    # table is a msgid even though no literal call names it. The glyph-only
    # ones ("▶", "#", "★") are deliberately absent: they need no translation.
    "TABLE_COLUMNS (main.rs)": [
        "Título", "Artista", "Álbum", "Juego", "Consola", "Álbum / Juego",
        "Artista del álbum", "Disco", "Duración", "Códec", "Bitrate",
        "Muestreo", "Canales", "Tamaño", "Ruta",
        # The "playing" column's header swaps to this while a track runs.
        "Reproduciendo",
    ],
}

# Words that are legitimately the same in Spanish and the target language, or
# are not language at all. A catalog may leave these identical to the source.
IDENTICAL_OK = {
    "#", "▶", "★", "DSP", "Mono", "mono", "Balance", "balance", "Bitrate",
    "auto", "logo", "original", "stop", "General", "no", "sistema", "Sistema",
}


def strings_in_rust(src: str) -> list[tuple[str, int]]:
    """Every Rust string literal, with `\\`-newline continuations folded the
    way rustc folds them: the backslash, the newline and the indentation that
    follows all vanish. Without this the long multi-line messages are invisible.
    """
    out: list[tuple[str, int]] = []
    i = 0
    while i < len(src):
        if src[i] != '"':
            i += 1
            continue
        j, buf = i + 1, []
        broken = False
        while j < len(src):
            c = src[j]
            if c == "\\":
                nxt = src[j + 1]
                if nxt == "\n":
                    j += 2
                    while j < len(src) and src[j] in " \t":
                        j += 1
                    continue
                buf.append(c + nxt)
                j += 2
                continue
            if c == '"':
                break
            if c == "\n":
                broken = True
                break
            buf.append(c)
            j += 1
        if not broken and j < len(src):
            out.append(("".join(buf), i))
        i = j + 1
    return out


def unescape(s: str) -> str:
    return s.encode().decode("unicode_escape") if "\\" in s else s


def used_msgids() -> set[str]:
    ids: set[str] = set()
    for f in sorted(UI.glob("*.slint")):
        ids |= {
            unescape(m)
            for m in re.findall(r'@tr\(\s*"((?:[^"\\]|\\.)*)"', f.read_text("utf-8"))
        }
    for f in sorted(SRC.glob("*.rs")):
        text = f.read_text("utf-8")
        for lit, pos in strings_in_rust(text):
            before = text[max(0, pos - 300):pos]
            if not re.search(r"i18n::tr\(\s*(?:match\s+[\w.]+\s*\{[^{}]*)?$", before):
                continue
            # Inside `i18n::tr(match x { "k" => "v", … })` the arm *patterns* sit
            # in the same window as the arm *values*. Only the values are text;
            # a pattern is followed by `=>`, possibly through the closing paren
            # of a `Some("k")`.
            after = text[pos + len(lit) + 2:pos + len(lit) + 12]
            if re.match(r"\s*\)?\s*(?:\||=>)", after):
                continue
            ids.add(unescape(lit))
    for group in INDIRECT.values():
        ids |= set(group)
    return {i for i in ids if i.strip()}


def catalog(path: Path) -> dict[str, str]:
    """msgid -> msgstr, folding gettext's multi-line continuation blocks."""
    entries: dict[str, str] = {}
    key: str | None = None
    field: str | None = None
    parts: list[str] = []

    def flush() -> None:
        nonlocal key, field, parts
        if field == "msgid":
            key = "".join(parts)
        elif field == "msgstr" and key is not None:
            entries[key] = "".join(parts)
        parts = []

    for line in path.read_text("utf-8").splitlines():
        line = line.strip()
        if line.startswith("msgid "):
            flush()
            field, parts = "msgid", [unescape(line[6:].strip().strip('"'))]
        elif line.startswith("msgstr "):
            flush()
            field, parts = "msgstr", [unescape(line[7:].strip().strip('"'))]
        elif line.startswith('"') and field:
            parts.append(unescape(line.strip().strip('"')))
        elif not line:
            flush()
            field = None
    flush()
    entries.pop("", None)
    return entries


def main() -> int:
    used = used_msgids()
    problems = 0

    langs = sorted(p.name for p in TRANSLATIONS.iterdir() if p.is_dir())
    if not langs:
        print("no catalogs under apps/tunante/translations", file=sys.stderr)
        return 1

    print(f"{len(used)} msgids used in code · catalogs: {', '.join(langs)}")

    for lang in langs:
        po = TRANSLATIONS / lang / "LC_MESSAGES/tunante.po"
        if not po.exists():
            print(f"  {lang}: MISSING {po.relative_to(ROOT)}")
            problems += 1
            continue
        entries = catalog(po)
        missing = sorted(m for m in used if m not in entries)
        empty = sorted(m for m in used if m in entries and not entries[m])
        # A placeholder dropped in translation crashes the substitution or
        # silently swallows a number, so the counts have to match.
        bad_ph = sorted(
            m for m in used
            if m in entries and entries[m] and m.count("{}") != entries[m].count("{}")
        )
        identical = sorted(
            m for m in used
            if entries.get(m) == m and m not in IDENTICAL_OK
        )

        if missing or empty or bad_ph:
            problems += 1
            print(f"  {lang}: {len(missing)} missing, {len(empty)} empty, "
                  f"{len(bad_ph)} placeholder mismatches")
            for m in missing:
                print(f"      missing     {m!r}")
            for m in empty:
                print(f"      empty       {m!r}")
            for m in bad_ph:
                print(f"      placeholder {m!r} -> {entries[m]!r}")
        else:
            note = f" ({len(identical)} identical to source)" if identical else ""
            print(f"  {lang}: complete{note}")

    if problems:
        print("\nA missing or empty entry falls back to Spanish at runtime.",
              file=sys.stderr)
        return 1
    print("\nEvery catalog covers every msgid the code looks up.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
