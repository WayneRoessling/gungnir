# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""The signature ledger: what the owner has signed, and the one place that says so.

    python docs/tools/signatures.py                    # render docs/signatures.md from the YAML
    python docs/tools/signatures.py check              # the ledger, and no pending claim elsewhere
    python docs/tools/signatures.py check --base REF   # and no entry merged on REF was edited
    python docs/tools/signatures.py scan               # only list pending claims elsewhere

`docs/signatures.yaml` is the source and `docs/signatures.md` is generated from it. A
signature was restated wherever it applied -- the §10 item, the gap's text, the crate's
module documentation, the design note's header -- and a "not signed" left behind in any
one of them read as open work long after the signature landed. That happened five times
before 2026-09-16. Now a document cites the ledger instead of saying whether something is
signed, and `check` fails a sentence outside the history that says something still waits
for the owner.

`check` verifies that every path an entry names existed at the entry's commit, so it
needs the repository's full history. An entry is never edited once merged: a signature
that is withdrawn gets a new entry saying so.
"""

import datetime
import re
import subprocess
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "docs" / "signatures.yaml"
OUT = ROOT / "docs" / "signatures.md"
FIELDS = ("date", "signer", "subject", "kind", "paths", "commit", "pre_repository", "record")
KINDS = ("code", "design-note", "stack", "edge", "criterion", "record-correction", "other")
REPOSITORY_CREATED = "2026-09-07"

# Sentences that say something still waits for the owner's signature or review. They go
# stale the day the signature lands, so outside the history they are refused; the ledger
# says what is signed, and an absent entry says what is not.
PENDING = [
    r"\b(?:human-owned|written|built|drafted)(?:,)?\s+(?:and\s+)?(?:gated,?\s+)?(?:and\s+|but\s+)?(?:not|never)\s+(?:yet\s+)?signed\b",
    r"\bhuman-owned\s+and\s+unsigned\b",
    r"\b(?:awaits?|awaiting|pending|waiting\s+(?:on|for))\s+(?:the\s+)?owner(?:'s|’s)?\s+"
    r"(?:signature|sign-?off|review|confirmation|approval)\b",
    r"\b(?:not|never)\s+(?:yet\s+)?signed\s+(?:off\s+)?by\s+the\s+owner\b",
    r"\bnot\s+(?:yet\s+)?signed\s+off\b",
    r"\b(?:still|remains?)\s+unsigned\b",
    r"\ban?\s+unsigned\s+(?:draft|design|note|amendment|entry|row|change|diff|signature)\b",
    r"\b(?:it|note|design|amendment|draft|entry|row|change|diff)\s+is\s+(?:still\s+)?(?:not|un)\s*signed\b",
    r"\bawaits?\s+(?:the\s+)?owner\b",
]
PENDING_RE = re.compile("|".join(f"(?:{p})" for p in PENDING), re.I)

SCANNED_SUFFIXES = {".rs", ".toml", ".md", ".py", ".yml", ".yaml"}
# The history: what was true on the day it was written, and not edited after.
HISTORY = (
    "docs/record/",
    "docs/old/",
    "docs/GungnirOverview.md",  # a snapshot audited at one commit, as its header says
    "docs/signatures.md",
    "docs/signatures.yaml",
    "docs/mission/gap-analysis/gap-register.md",  # renders each gap's history
    "docs/mission/gap-analysis/technical-gap-map.md",  # quotes gap histories
    "docs/mission/gap-analysis/decisions-needed.md",  # decision outcomes, as taken
    "docs/mission/gap-analysis/data/decisions.yaml",
    "docs/tools/signatures.py",  # this file names the phrases
    "testdata/",
    "target/",
)
GAPS_YAML = "docs/mission/gap-analysis/data/gaps.yaml"
GAP_CURRENT_FIELDS = ("name", "impact", "evidence", "action", "ref")


def git(*args, check=True):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True,
                          encoding="utf-8", check=check)


def iso(v):
    return v.isoformat() if isinstance(v, (datetime.date, datetime.datetime)) else v


def load(text):
    raw = yaml.safe_load(text) or []
    if not isinstance(raw, list):
        raise SystemExit("docs/signatures.yaml: expected a list of entries")
    return [{k: iso(v) for k, v in e.items()} for e in raw]


def entry_problems(entries):
    problems = []
    root = git("rev-list", "--max-parents=0", "HEAD").stdout.split()
    last = ""
    for i, e in enumerate(entries):
        where = f"entry {i + 1} ({e.get('date', '?')}, {str(e.get('subject', '?'))[:40]})"
        if set(e) != set(FIELDS):
            problems.append(f"{where}: fields are exactly {', '.join(FIELDS)}")
            continue
        if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(e["date"])):
            problems.append(f"{where}: date is YYYY-MM-DD")
        elif e["date"] < last:
            problems.append(f"{where}: entries run oldest first")
        else:
            last = e["date"]
        if e["kind"] not in KINDS:
            problems.append(f"{where}: kind is one of {', '.join(KINDS)}")
        if not re.fullmatch(r"[0-9a-f]{40}", str(e["commit"])):
            problems.append(f"{where}: commit is a full 40-character SHA")
            continue
        if git("cat-file", "-e", f"{e['commit']}^{{commit}}", check=False).returncode != 0:
            problems.append(f"{where}: commit {e['commit'][:10]} is not in this repository's history")
            continue
        if e["pre_repository"]:
            if e["commit"] not in root:
                problems.append(f"{where}: a pre-repository signature names the repository's first commit")
            if e["date"] >= REPOSITORY_CREATED:
                problems.append(f"{where}: pre_repository is for signatures before {REPOSITORY_CREATED}")
        # A finding or a ruling can be signed without being a file; anything else names one.
        if not e["paths"] and e["kind"] != "other":
            problems.append(f"{where}: name at least one path")
        for path in e["paths"] or []:
            if git("cat-file", "-e", f"{e['commit']}:{path}", check=False).returncode != 0:
                problems.append(f"{where}: {path} did not exist at {e['commit'][:10]}")
    return problems


def append_only(base, entries):
    shown = git("show", f"{base}:docs/signatures.yaml", check=False)
    if shown.returncode != 0:
        return []
    old = load(shown.stdout)
    if entries[: len(old)] != old:
        return [f"an entry merged on {base} was edited, removed or reordered; add a new entry instead"]
    return []


def pending_claims():
    """(path, line, text) for every pending-signature sentence outside the history."""
    found = []
    tracked = git("ls-files").stdout.split("\n")
    for rel in tracked:
        if not rel or Path(rel).suffix not in SCANNED_SUFFIXES or rel.startswith(HISTORY):
            continue
        path = ROOT / rel
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if rel == GAPS_YAML:
            for g in yaml.safe_load(text) or []:
                for field in GAP_CURRENT_FIELDS:
                    value = str(g.get(field) or "")
                    for m in PENDING_RE.finditer(" ".join(value.split())):
                        found.append((f"{rel} {g.get('id')}.{field}", 0, m.group(0)))
            continue
        flat = re.sub(r"\s*\n\s*(?:///?!?|#)?\s*", " ", text)
        offsets = []
        for m in PENDING_RE.finditer(flat):
            offsets.append(m)
        if not offsets:
            continue
        for m in offsets:
            # Map back to a line: the words before the match, counted in the original text.
            words_before = len(flat[: m.start()].split())
            count, line = 0, 1
            for n, l in enumerate(text.split("\n"), start=1):
                count += len(l.split())
                if count >= words_before:
                    line = n
                    break
            found.append((rel, line, m.group(0)))
    return found


def render(entries):
    cell = lambda s: " ".join(str(s).split()).replace("|", "/")
    L = [
        "# Signatures",
        "",
        "Every change the owner has signed, and the only place that says whether something is",
        "signed. Generated by [`tools/signatures.py`](tools/signatures.py) from",
        "[`signatures.yaml`](signatures.yaml); edit the YAML, never this page.",
        "",
        "A document that needs to say a change is signed links this page instead. A sentence",
        "saying something still waits for the owner is refused outside the history",
        "(`docs/record/` and the gap and decision histories), because it goes stale the day",
        "the signature lands. Something with no entry here has not been signed.",
        "",
        "A design and the code built from it are separate entries: a signature on a design note",
        "says the design is the right one to build, and a signature on code says the code does",
        "what it says.",
        "",
        "A row's commit is the first commit on `main` whose tree carried the written record of",
        "the signature, and its paths are the files and directories it covered as they were at",
        "that commit. A signature from before the repository existed on 2026-09-07 names the",
        "first commit and is marked as such. An entry is never edited; a withdrawn signature",
        "gets a new entry.",
        "",
        f"{len(entries)} signatures.",
        "",
        "| Date | Signer | What was signed | Kind | Paths | Commit | Record |",
        "|---|---|---|---|---|---|---|",
    ]
    for e in entries:
        paths = "<br>".join(f"`{p}`" for p in e["paths"]) or "--"
        commit = f"`{e['commit'][:10]}`" + (" (before the repository)" if e["pre_repository"] else "")
        L.append(f"| {e['date']} | {cell(e['signer'])} | {cell(e['subject'])} | {e['kind']} | "
                 f"{paths} | {commit} | {cell(e['record'])} |")
    return "\n".join(L) + "\n"


def main(argv):
    cmd = argv[1] if len(argv) > 1 else "build"
    if cmd == "scan":
        claims = pending_claims()
        for rel, line, text in claims:
            print(f"{rel}:{line}: {text}")
        print(f"{len(claims)} pending-signature claims outside the history")
        return 1 if claims else 0
    entries = load(SOURCE.read_text(encoding="utf-8"))
    if cmd == "build":
        OUT.write_text(render(entries), encoding="utf-8", newline="\n")
        print(f"signatures: {len(entries)} entries")
        return 0
    if cmd == "check":
        problems = entry_problems(entries)
        if "--base" in argv:
            problems += append_only(argv[argv.index("--base") + 1], entries)
        if OUT.read_text(encoding="utf-8") != render(entries):
            problems.append("docs/signatures.md is stale or hand-edited: run python docs/tools/signatures.py")
        for rel, line, text in pending_claims():
            problems.append(f"{rel}:{line}: \"{text}\" -- cite docs/signatures.md instead of saying "
                            "what still waits for the owner")
        for p in problems:
            print(f"signatures: {p}")
        print(f"signature check: {len(problems)} problems, {len(entries)} entries")
        return 1 if problems else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
