# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Rules about a pull request itself, run by `.github/workflows/pr-rules.yml`.

    python3 .github/scripts/check_pr_rules.py dependencies
    python3 .github/scripts/check_pr_rules.py commits

Both read BASE_SHA and HEAD_SHA from the environment and compare HEAD against its
merge base with BASE, so a change that reached main after the branch was cut is not
charged to the branch. `dependencies` also reads PR_BODY. Either can be run locally with
those three variables set.

- `dependencies`: a change to `[workspace.dependencies]` needs a `Decision:` line and a
  `Duplicate linkage:` line in the description (`docs/agentic-coding-standards.md` §2.9).
- `commits`: each commit message body is at most BODY_WORDS words (`CONTRIBUTING.md`,
  "Commit messages"). The narrative belongs in a `docs/record/` item.
"""

import os
import re
import subprocess
import sys

BODY_WORDS = 120
TRAILER = re.compile(r"^(Co-Authored-By|Signed-off-by|Reviewed-by|Acked-by):", re.I)


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, encoding="utf-8",
                          check=True).stdout


def merge_base():
    return git("merge-base", os.environ["BASE_SHA"], os.environ["HEAD_SHA"]).strip()


def workspace_dependencies(rev):
    """{crate: the entry's text} for `[workspace.dependencies]` at `rev`."""
    try:
        text = git("show", f"{rev}:Cargo.toml")
    except subprocess.CalledProcessError:
        return {}
    deps, inside, current = {}, False, None
    for raw in text.split("\n"):
        line = raw.split("#", 1)[0].rstrip()
        if raw.startswith("["):
            inside = raw.strip() == "[workspace.dependencies]"
            current = None
            continue
        if not inside or not line.strip():
            continue
        m = re.match(r"^([A-Za-z0-9_-]+)\s*=\s*(.*)$", line)
        if m:
            current = m.group(1)
            deps[current] = m.group(2).strip()
        elif current:
            deps[current] += " " + line.strip()
    return deps


def dependencies():
    base, head = workspace_dependencies(merge_base()), workspace_dependencies(os.environ["HEAD_SHA"])
    changed = sorted(n for n in set(base) | set(head) if base.get(n) != head.get(n))
    if not changed:
        print("no change to [workspace.dependencies]")
        return 0
    for n in changed:
        print(f"  {n}: {base.get(n, '(absent)')} -> {head.get(n, '(removed)')}")
    body = os.environ.get("PR_BODY") or ""
    missing = [label for label in ("Decision:", "Duplicate linkage:")
               if not re.search(rf"(?im)^\s*{re.escape(label)}\s*\S", body)]
    if missing:
        print(f"::error::this pull request changes [workspace.dependencies] ({', '.join(changed)}); "
              f"its description needs a line beginning {' and a line beginning '.join(missing)} "
              "(docs/agentic-coding-standards.md §2.9)")
        return 1
    print("the description states the decision and the duplicate-linkage check")
    return 0


def commits():
    shas = git("rev-list", "--no-merges", f"{merge_base()}..{os.environ['HEAD_SHA']}").split()
    problems = 0
    for sha in shas:
        message = git("log", "-1", "--format=%B", sha)
        lines = message.strip("\n").split("\n")
        subject, body = lines[0], [l for l in lines[1:] if not TRAILER.match(l.strip())]
        words = len(" ".join(body).split())
        if words > BODY_WORDS:
            problems += 1
            print(f"::error::{sha[:10]} \"{subject[:60]}\": {words} words in the body; keep it to "
                  f"{BODY_WORDS} -- the row, the standards section and the why -- and put the "
                  "narrative in a docs/record/ item (CONTRIBUTING.md, Commit messages)")
    print(f"{len(shas)} commits checked, {problems} over {BODY_WORDS} words")
    return 1 if problems else 0


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "dependencies":
        sys.exit(dependencies())
    if command == "commits":
        sys.exit(commits())
    print(__doc__)
    sys.exit(2)
