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
TRANSLATIONS = ROOT / "crates/tunante-core/translations"
ANDROID = ROOT / "apps/android/app/src/main/java"
# Every Rust file that can put text on a screen or hand it to the system.
RUST_ROOTS = [SRC, ROOT / "apps/android/rust/src", ROOT / "crates"]
ALLOWLIST = ROOT / "scripts/i18n-allowlist.txt"

# A literal is Spanish if it carries a Spanish-only character or reads like a
# Spanish sentence. Song titles are data and never literals, so this only ever
# meets code. Tuned against the tree: a shorter word list lets "Nada con
# confianza suficiente" through, a longer one flags English ("Original Sin").
SPANISH = re.compile(
    r"[áéíóúñÁÉÍÓÚÑ¿¡]"
    r"|\b(?:la|el|los|las|del|para|con|una|que|pista|pistas|cola|carpeta|carpetas|"
    r"biblioteca|ninguna|nada|siempre|nunca|carátula|carátulas|sonando|ajustes|"
    r"reproducir|reproduciendo|añadir|quitar|abrir|buscando|analizando|guardar|"
    r"cancelar|listo|hecho|no se|se ha|podrá|disponible|deshacer|fichero|juego|"
    r"juegos|consola|lista|listas|escribir|sin fundido|aplicada|esta build|"
    r"contesta|rara|hay|colgó|rechazó|cancelado|junto|trae|ilegible|pudo|pudieron)\b",
    re.IGNORECASE,
)
# Navigation keys of the library tree ("consola:<id>", "juego:<name>"): they
# name rows internally and are never drawn. `rowKey` in Kotlin builds the same.
KEYS = re.compile(r"^(?:consola|juego):")
# Text that goes to stderr or a log, never to a person using the app.
LOGGING = re.compile(
    r"eprintln!|println!|panic!|unreachable!|assert(?:_eq|_ne)?!|debug_assert!|"
    r"log::(?:trace|debug|info|warn|error)|\b(?:trace|debug|info|warn|error)!\(|"
    r"tracing::|Log\.[vdiwe]\(|System\.err|\.expect\(|bail!|anyhow!"
)

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
    # `SHORTCUTS` descriptions go to the portal through `tr(desc)`: the
    # desktop's binding dialog shows them.
    "SHORTCUTS (shortcuts.rs)": ["Reproducir/Pausa", "Siguiente pista", "Pista anterior"],
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
    # Reviewed one by one on 2026-09-05: each is the same word in the language
    # whose catalog leaves it — English "Vertical", French "Fin", Italian
    # "Disco", and most of Portuguese. An identical entry NOT listed here is
    # treated as untranslated and fails the check.
    "Horizontal", "Vertical", "Fin", "Graves", "disponible",
    "1 elemento", "Artista", "Disco", "Lista", "Registro", "Tema", "alta", "media",
    "Abrir", "Agudos", "Anterior", "Aplicar", "Automática", "Biblioteca", "Buscar",
    "Buscar…", "Cancelar", "Claro", "Copiar", "Favoritos", "Filtrar…", "Idioma",
    "Limitador", "Lista · {}", "Listas", "Mostrar/Ocultar", "Parar",
    "Tecla · Anterior", "Tecla · Buscar", "Tecla · Favorito", "Tecla · Repetir",
    "Tecla · Silenciar", "Título", "Vibecoded por jjolmo.", "anterior", "buscar",
    "claro", "estéreo", "limitador", "mostrar/ocultar", "nada", "parar", "repetir",
    "silenciar", "simbólico", "vinculando…", "{}/{} · {} encontradas", "Álbum",
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
    # The Compose side wraps its literals in `tr("…")`, the same catalog
    # through JNI. Kotlin has no raw strings with a backslash-newline to fold.
    for f in sorted(ANDROID.rglob("*.kt")):
        ids |= {
            unescape(m)
            for m in re.findall(r'\btr\(\s*"((?:[^"\\]|\\.)*)"', f.read_text("utf-8"))
        }
    for group in INDIRECT.values():
        ids |= set(group)
    return {i for i in ids if i.strip()}


def allowlist() -> set[str]:
    """Literals that look Spanish and are not captions, one per line, `# why`
    after each. A bare word in a match pattern, a proper noun, a path."""
    if not ALLOWLIST.exists():
        return set()
    out: set[str] = set()
    for line in ALLOWLIST.read_text("utf-8").splitlines():
        body = line.split("  #", 1)[0].strip()
        if body and not body.startswith("#"):
            out.add(unescape(body))
    return out


def bare_literals(used: set[str]) -> list[tuple[Path, int, str]]:
    """Every Spanish-looking string literal that no `tr()` will ever see.

    This is the check that was missing: the catalogs can be complete and the
    app still show "Buscando carátulas…" in English, because that line was
    built with `format!` and handed straight to the UI. A literal passes if
    `tr()` reaches it (it is a used msgid), if the statement around it is
    logging, or if the allowlist names it with a reason.
    """
    ok = used | allowlist()
    found: list[tuple[Path, int, str]] = []

    def statement(text: str, pos: int) -> str:
        # The enclosing statement, roughly: back to the previous `;` or `{`
        # and forward to the next. Enough to see the macro or the tr( call.
        a = max(text.rfind(";", 0, pos), text.rfind("{", 0, pos), pos - 400)
        b = text.find("\n", pos)
        return text[max(a, 0):pos + (b - pos if b != -1 else 0)]

    def is_comment(text: str, pos: int) -> bool:
        line_start = text.rfind("\n", 0, pos) + 1
        return text[line_start:pos].lstrip().startswith(("//", "#", "*", "/*"))

    for root in RUST_ROOTS:
        for f in sorted(root.rglob("*.rs")):
            if "/target/" in str(f) or "/tests/" in str(f) or f.name.endswith("_test.rs"):
                continue
            text = f.read_text("utf-8")
            for lit, pos in strings_in_rust(text):
                if len(lit) < 3 or not SPANISH.search(lit) or is_comment(text, pos):
                    continue
                if unescape(lit) in ok or KEYS.match(lit):
                    continue
                stmt = statement(text, pos)
                if LOGGING.search(stmt) or "#[cfg(test)]" in text[max(0, pos - 4000):pos] and "mod tests" in text[max(0, pos - 4000):pos]:
                    continue
                found.append((f, text.count("\n", 0, pos) + 1, lit))
    for f in sorted(ANDROID.rglob("*.kt")) + sorted(ANDROID.rglob("*.java")):
        text = f.read_text("utf-8")
        for m in re.finditer(r'"((?:[^"\\\n]|\\.)*)"', text):
            lit = m.group(1)
            if len(lit) < 3 or not SPANISH.search(lit) or is_comment(text, m.start()):
                continue
            if unescape(lit) in ok or KEYS.match(lit):
                continue
            if LOGGING.search(statement(text, m.start())):
                continue
            found.append((f, text.count("\n", 0, m.start()) + 1, lit))
    for f in sorted(UI.glob("*.slint")):
        text = f.read_text("utf-8")
        for m in re.finditer(r'"((?:[^"\\\n]|\\.)*)"', text):
            lit = m.group(1)
            if len(lit) < 3 or not SPANISH.search(lit) or is_comment(text, m.start()):
                continue
            if unescape(lit) in ok or text[max(0, m.start() - 6):m.start()].rstrip().endswith("@tr("):
                continue
            found.append((f, text.count("\n", 0, m.start()) + 1, lit))
    # The launcher entry is read by the desktop, in the desktop's language: a
    # Spanish base value needs a `Key[lang]=` line for every catalog language.
    langs = sorted(p.name for p in TRANSLATIONS.iterdir() if p.is_dir())
    for f in sorted((ROOT / "apps/tunante/dist").glob("*.desktop")):
        text = f.read_text("utf-8")
        for n, line in enumerate(text.splitlines(), 1):
            k, _, v = line.partition("=")
            if k in ("Comment", "GenericName", "Keywords") and SPANISH.search(v):
                if any(f"{k}[{lang}]=" not in text for lang in langs):
                    found.append((f, n, line))
    return found


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
        print("no catalogs under crates/tunante-core/translations", file=sys.stderr)
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

        # The English catalog is the one most people will see: a msgstr that
        # still carries a Spanish-only character was never translated.
        spanish_en = sorted(
            m for m in used
            if lang == "en" and entries.get(m) and re.search(r"[ñ¿¡]|[áéíóú](?![a-z]*\b(?:é|à))", entries[m])
            and m not in IDENTICAL_OK
        ) if lang == "en" else []

        if missing or empty or bad_ph or identical or spanish_en:
            problems += 1
            print(f"  {lang}: {len(missing)} missing, {len(empty)} empty, "
                  f"{len(bad_ph)} placeholder mismatches, {len(identical)} identical to source "
                  f"and not reviewed, {len(spanish_en)} Spanish in English")
            for m in identical:
                print(f"      identical   {m!r}  (translate it, or list it in IDENTICAL_OK with why)")
            for m in spanish_en:
                print(f"      spanish-en  {m!r} -> {entries[m]!r}")
            for m in missing:
                print(f"      missing     {m!r}")
            for m in empty:
                print(f"      empty       {m!r}")
            for m in bad_ph:
                print(f"      placeholder {m!r} -> {entries[m]!r}")
        else:
            print(f"  {lang}: complete")

    bare = bare_literals(used)
    if bare:
        problems += 1
        print(f"\n{len(bare)} Spanish literals that never reach tr():")
        for f, n, lit in bare:
            print(f"  {f.relative_to(ROOT)}:{n}: {lit[:90]!r}")
        print("  Route each through tr()/@tr, or name it in scripts/i18n-allowlist.txt with a reason.")

    if problems:
        print("\nA missing or empty entry falls back to Spanish at runtime.",
              file=sys.stderr)
        return 1
    print("\nEvery catalog covers every msgid the code looks up, and no caption skips tr().")
    return 0


if __name__ == "__main__":
    sys.exit(main())
