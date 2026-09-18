# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Print deny.toml's accepted advisories as `cargo audit --ignore` arguments.

`release.yml` runs two advisory scanners over the same RustSec database: `cargo deny
check`, which reads `deny.toml`, and `cargo audit`, which does not. Every advisory the
owner has accepted -- each with its reason and the path it covers -- is in `deny.toml`'s
`[advisories] ignore`, so `cargo audit` alone reported two of them as failures. Found on
2026-09-17 by the first run of `release.yml` ever made (GAP-061): with the `rustls`
finding fixed, `cargo deny` passed and `cargo audit` failed on RUSTSEC-2026-0194 and
RUSTSEC-2026-0195, the `quick-xml` 0.30 advisories accepted on 2026-09-07.

The owner chose (2026-09-17) to have the audit honour `deny.toml` rather than keep a
second list in `.cargo/audit.toml`: one list of accepted advisories, so an acceptance is
made, reasoned and removed in one place. An entry without a reason is refused, because
an acceptance nobody can explain is not one.

Usage, from release.yml:

    mapfile -t ignores < <(python3 .github/scripts/deny_advisory_ignores.py)
    cargo audit "${ignores[@]}"

Prints one argument per line (`--ignore`, then the id) on stdout, and a readable list of
what is being ignored and why on stderr, so the run log says it.
"""

import pathlib
import sys
import tomllib


def main(argv: list[str]) -> int:
    path = pathlib.Path(argv[1] if len(argv) > 1 else "deny.toml")
    with path.open("rb") as f:
        deny = tomllib.load(f)
    entries = deny.get("advisories", {}).get("ignore", [])
    args: list[str] = []
    for entry in entries:
        if isinstance(entry, str):
            print(f"{path}: ignore {entry} has no reason; give it one in deny.toml", file=sys.stderr)
            return 2
        advisory = entry.get("id")
        reason = entry.get("reason", "").strip()
        if not advisory or not reason:
            print(f"{path}: an ignore entry lacks an id or a reason: {entry}", file=sys.stderr)
            return 2
        print(f"ignoring {advisory}: {reason}", file=sys.stderr)
        args += ["--ignore", advisory]
    print(f"{len(args) // 2} advisory ignore(s) taken from {path}", file=sys.stderr)
    print("\n".join(args))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
