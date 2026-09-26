#!/usr/bin/env bash
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
#
# Regenerate the generated documents and run the cross-document checks, then fail if
# anything differs from what is committed. One script, so that ci.yml and
# cross-document-recheck.yml cannot drift into checking different things.
#
#   bash .github/scripts/check_documents.sh <group>...
#
# Groups: uaf, gaps, tracks, records; `cross` is uaf gaps records, the checks that read
# documents another pull request may change; `all` is every group. Set BASE_REF to the
# base branch's commit to hold append-only data (the record, gap history) against it;
# leave it empty on main. Needs Python 3 with PyYAML, and a full clone when BASE_REF is set.
set -euo pipefail

base="${BASE_REF:-}"
failed=()

committed() {
  if ! git diff --quiet -- "$@"; then
    echo "::error::generated files differ from what is committed; run the generator and commit:"
    git diff --stat -- "$@"
    return 1
  fi
}

uaf() {
  python docs/architecture/uaf/tools/build_uaf.py
  python docs/architecture/uaf/tools/export_xmi.py
  python docs/architecture/uaf/tools/export_ea_script.py
  committed docs/architecture/uaf
}

gaps() {
  python docs/mission/gap-analysis/tools/gen_gaps.py ${base:+--base "$base"}
  committed docs/mission/gap-analysis
}

tracks() {
  python docs/test-tracks/tools/build_catalogue.py
  python docs/test-tracks/tools/gen_tracks.py
  python docs/test-tracks/tools/validate_tracks.py
  committed docs/test-tracks testdata/tracks/samples testdata/tracks/sensor-models.json
}

records() {
  python docs/record/tools/record.py
  python docs/record/tools/record.py check ${base:+--base "$base"}
  python docs/tools/gen_unbuilt.py
  python docs/tools/signatures.py
  python docs/tools/signatures.py check ${base:+--base "$base"}
  committed docs/record docs/unbuilt.md docs/signatures.md
}

groups=()
for arg in "${@:-all}"; do
  case "$arg" in
    all) groups+=(uaf gaps tracks records) ;;
    cross) groups+=(uaf gaps records) ;;
    uaf | gaps | tracks | records) groups+=("$arg") ;;
    *) echo "unknown group: $arg" >&2; exit 2 ;;
  esac
done

for group in "${groups[@]}"; do
  echo "::group::$group"
  if ! "$group"; then
    failed+=("$group")
  fi
  echo "::endgroup::"
done

if [ "${#failed[@]}" -gt 0 ]; then
  echo "::error::document checks failed: ${failed[*]}"
  exit 1
fi
echo "document checks passed: ${groups[*]}"
