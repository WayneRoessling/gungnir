#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
"""Regenerate the generated documents and run the cross-document checks, then fail if
anything differs from what is committed. One script, so that ci.yml and
cross-document-recheck.yml cannot drift into checking different things.

    python .github/scripts/check_documents.py [--base REF] <group>...

Groups: uaf, gaps, tracks, records; `cross` is uaf gaps records, the checks that read
documents another pull request may change; `all` is every group, and the default.
`--base` (or the BASE_REF environment variable) names the base branch's commit, against
which append-only data (the record, gap history, signatures) is held; leave it unset on
main. Needs Python 3 with PyYAML, and a full clone when a base is given. It runs from
any working directory and on any platform; it was a bash script until 2026-10-29.

Every step of a group runs, and the group fails if any step does. The bash script it
replaces called each group as `if ! "$group"`, and bash ignores `set -e` inside a
function called that way, so a failing check in a group (record.py check,
signatures.py check, validate_tracks.py, gen_gaps.py --base) failed nothing unless it
also left a diff behind. "Differs from what is committed" now also counts a generated
file git does not know about; it compares the working tree with the index, as
`git diff` does, so `git add` is what makes a local run pass.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
IN_ACTIONS = os.environ.get("GITHUB_ACTIONS") == "true"


def say(line: str) -> None:
    print(line, flush=True)


def error(line: str) -> None:
    say(f"::error::{line}" if IN_ACTIONS else f"error: {line}")


def python(script: str, *args: str) -> bool:
    say(f"$ python {script} {' '.join(args)}".rstrip())
    result = subprocess.run([sys.executable, script, *args], cwd=ROOT)
    if result.returncode != 0:
        error(f"{script} exited with {result.returncode}")
    return result.returncode == 0


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=False
    )


def committed(*paths: str) -> bool:
    changed = git("diff", "--quiet", "--", *paths).returncode != 0
    untracked = git("ls-files", "--others", "--exclude-standard", "--", *paths).stdout
    if not changed and not untracked:
        return True
    error("generated files differ from what is committed; run the generator and commit:")
    if changed:
        say(git("diff", "--stat", "--", *paths).stdout.rstrip())
    for path in untracked.splitlines():
        say(f" {path} (not in git)")
    return False


def base_args(base: str) -> list[str]:
    return ["--base", base] if base else []


def uaf(base: str) -> bool:
    steps = [
        python("docs/architecture/uaf/tools/build_uaf.py"),
        python("docs/architecture/uaf/tools/export_xmi.py"),
        python("docs/architecture/uaf/tools/export_ea_script.py"),
        committed("docs/architecture/uaf"),
    ]
    return all(steps)


def gaps(base: str) -> bool:
    steps = [
        python("docs/mission/gap-analysis/tools/gen_gaps.py", *base_args(base)),
        committed("docs/mission/gap-analysis"),
    ]
    return all(steps)


def tracks(base: str) -> bool:
    steps = [
        python("docs/test-tracks/tools/build_catalogue.py"),
        python("docs/test-tracks/tools/gen_tracks.py"),
        python("docs/test-tracks/tools/validate_tracks.py"),
        committed(
            "docs/test-tracks",
            "testdata/tracks/samples",
            "testdata/tracks/sensor-models.json",
        ),
    ]
    return all(steps)


def records(base: str) -> bool:
    steps = [
        python("docs/record/tools/record.py"),
        python("docs/record/tools/record.py", "check", *base_args(base)),
        python("docs/tools/gen_unbuilt.py"),
        python("docs/tools/signatures.py"),
        python("docs/tools/signatures.py", "check", *base_args(base)),
        committed("docs/record", "docs/unbuilt.md", "docs/signatures.md"),
    ]
    return all(steps)


GROUPS = {"uaf": uaf, "gaps": gaps, "tracks": tracks, "records": records}
ALIASES = {"all": ["uaf", "gaps", "tracks", "records"], "cross": ["uaf", "gaps", "records"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "groups",
        nargs="*",
        default=["all"],
        choices=[*GROUPS, *ALIASES],
        metavar="group",
        help="uaf, gaps, tracks, records, cross or all (the default)",
    )
    parser.add_argument(
        "--base",
        default=os.environ.get("BASE_REF", ""),
        help="the base branch's commit (default: $BASE_REF, else none)",
    )
    args = parser.parse_args()

    groups: list[str] = []
    for name in args.groups:
        for group in ALIASES.get(name, [name]):
            if group not in groups:
                groups.append(group)

    failed = []
    for group in groups:
        say(f"::group::{group}" if IN_ACTIONS else f"== {group}")
        if not GROUPS[group](args.base):
            failed.append(group)
        if IN_ACTIONS:
            say("::endgroup::")

    if failed:
        error(f"document checks failed: {' '.join(failed)}")
        return 1
    say(f"document checks passed: {' '.join(groups)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
