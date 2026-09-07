#!/usr/bin/env bash
# Render every PlantUML (.puml) and Mermaid (.mmd) source under docs/architecture/uaf
# to SVG under docs/architecture/uaf/rendered/, mirroring the folder structure.
#
# PlantUML: uses `plantuml` on PATH if present, else the plantuml/plantuml container
# through Docker. Mermaid: uses `mmdc` on PATH if present, else
# `npx @mermaid-js/mermaid-cli`. Exit status is non-zero if any diagram fails.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/rendered"
status=0

render_puml() {
  local src="$1" rel dir
  rel="${src#"$here/"}"
  dir="$out/$(dirname "$rel")"
  mkdir -p "$dir"
  if command -v plantuml >/dev/null 2>&1; then
    plantuml -tsvg -o "$dir" "$src" || status=1
  elif command -v docker >/dev/null 2>&1; then
    docker run --rm -v "$here:/work" -w /work plantuml/plantuml -tsvg -o "/work/rendered/$(dirname "$rel")" "$rel" || status=1
  else
    echo "no plantuml or docker available for $rel" >&2; status=1
  fi
}

render_mmd() {
  local src="$1" rel dir
  rel="${src#"$here/"}"
  dir="$out/$(dirname "$rel")"
  mkdir -p "$dir"
  local target="$dir/$(basename "${rel%.mmd}").svg"
  if command -v mmdc >/dev/null 2>&1; then
    mmdc -i "$src" -o "$target" || status=1
  elif command -v npx >/dev/null 2>&1; then
    npx -y @mermaid-js/mermaid-cli -i "$src" -o "$target" || status=1
  else
    echo "no mmdc or npx available for $rel" >&2; status=1
  fi
}

while IFS= read -r -d '' f; do render_puml "$f"; done < <(find "$here" -name '*.puml' -not -path "$out/*" -print0)
while IFS= read -r -d '' f; do render_mmd "$f"; done < <(find "$here" -name '*.mmd' -not -path "$out/*" -print0)
exit $status
