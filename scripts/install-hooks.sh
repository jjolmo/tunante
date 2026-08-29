#!/bin/sh
# Put this repository's hooks where git will find them.
#
# .git/hooks is not versioned, so a fresh clone has none of this. Run once.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
dest="$(git -C "$root" rev-parse --git-common-dir)/hooks"
mkdir -p "$dest"

for hook in "$root"/scripts/hooks/*; do
    name=$(basename "$hook")
    ln -sf "$hook" "$dest/$name"
    echo "linked $name"
done

# core.hooksPath REPLACES .git/hooks rather than adding to it, so if one is set
# the links above are dead unless that directory hands off. Say so rather than
# leaving a hook that silently never runs.
path=$(git -C "$root" config --get core.hooksPath || true)
if [ -n "$path" ]; then
    echo
    echo "note: core.hooksPath is set to $path"
    echo "      .git/hooks is bypassed unless that directory delegates to it."
fi
