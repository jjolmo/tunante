# Translations

Tunante's interface strings are translatable through Slint's **bundled
translations**: every `.po` file in this tree is compiled straight into the
binary at build time, so there are no separate language files to install and the
app stays a single executable on every target.

## How the language is chosen

At startup the app picks, in order:

1. A saved override — the `language` setting in the database, if set.
2. The system locale — the primary subtag of `LC_ALL` / `LC_MESSAGES` / `LANG`
   (e.g. `fr_FR.UTF-8` → `fr`).

The **source strings are Spanish**. If the chosen language has no `.po` here (or
the locale is Spanish, or `C`/`POSIX`), the app falls back to those Spanish
strings. So Spanish needs no file; every other language is a `.po`.

## Adding or improving a language

Drop a file at:

```
translations/<lang>/LC_MESSAGES/tunante-mini.po
```

where `<lang>` is the locale code the system reports (`fr`, `de`, `pt`, `pt_BR`,
`zh`, `ja`, …). The app tries the primary subtag, so `pt` covers `pt_PT` and
`pt_BR` unless you ship a region-specific folder.

The `.po` format is standard gettext. The **msgid is the Spanish source string**
exactly as it appears in the UI; `msgstr` is your translation. A minimal file:

```po
msgid ""
msgstr ""
"Project-Id-Version: tunante-mini\n"
"Language: fr\n"
"MIME-Version: 1.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Content-Transfer-Encoding: 8bit\n"

msgid "Filtrar pistas…"
msgstr "Filtrer les pistes…"
```

You don't have to translate everything — any missing `msgid` falls back to the
Spanish source. Rebuild the app (`cargo build -p tunante-mini`) for the change
to take effect; the `.po` is bundled at compile time, not read at run time.

Strings with a placeholder keep the `{}` in the translation, e.g.
`msgid "{} carpeta(s) elegidas"` → `msgstr "{} dossier(s) sélectionné(s)"`.

## Regenerating the string list

The translatable strings are the `@tr("…")` calls in `apps/mini/ui/*.slint`. To
produce a fresh `.pot` template you can install and run Slint's extractor:

```
cargo install slint-tr-extractor
find apps/mini/ui -name '*.slint' | xargs slint-tr-extractor --no-default-translation-context -o apps/mini/translations/tunante-mini.pot
```

Then merge it into each `.po` with `msgmerge`.
