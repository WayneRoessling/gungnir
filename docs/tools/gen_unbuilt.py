# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Generate `docs/unbuilt.md`: what the code itself says it does not do yet.

    python docs/tools/gen_unbuilt.py

Every row is a place a crate's source returns a `...NotImplemented` error variant, in
the words the code uses. No document is allowed to keep its own list of unbuilt
subsystems: until 2026-09-16 `CLAUDE.md` did, and four of the five it named had been
built. The variants already name themselves, so the list is read from them.

Test modules are skipped the way `gungnir-app/tests/architecture_compliance.rs` skips
them, and pattern matches (`matches!`, `=>` arms, `let ... else`) are not returns. The
output names files and functions and no line numbers, so an edit elsewhere in a file
does not change it. Deterministic: a clean re-run rewrites the file byte for byte, which
is how CI checks it.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs" / "unbuilt.md"

VARIANT = re.compile(r"\b([A-Z]\w*)::(\w*NotImplemented)\b")
DECLARED = re.compile(r"^\s*(\w*NotImplemented)\b\s*([({,]|$)", re.M)


def without_test_modules(text):
    """`#[cfg(test)]`-gated items removed, brace-matched, as the compliance test does."""
    out, rest = [], text
    while True:
        i = rest.find("#[cfg(test)]")
        if i < 0:
            out.append(rest)
            return "".join(out)
        out.append(rest[:i])
        after = rest[i:]
        open_ = after.find("{")
        if open_ < 0:
            out.append(after)
            return "".join(out)
        depth = 0
        for j, c in enumerate(after[open_:]):
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    rest = after[open_ + j + 1 :]
                    break
        else:
            return "".join(out)


def matching(text, i):
    """Index just past the bracket that closes the one at `i`, skipping string literals."""
    pairs = {"(": ")", "{": "}", "[": "]"}
    stack, k = [], i
    while k < len(text):
        c = text[k]
        if c == '"':
            k += 1
            while k < len(text) and text[k] != '"':
                k += 2 if text[k] == "\\" else 1
        elif c in pairs:
            stack.append(pairs[c])
        elif stack and c == stack[-1]:
            stack.pop()
            if not stack:
                return k + 1
        k += 1
    return len(text)


def rust_strings(s):
    """The string literals in `s`, with Rust's escapes and line continuations applied."""
    out = []
    for m in re.finditer(r'"((?:[^"\\]|\\.)*)"', s, re.S):
        body = re.sub(r"\\\n\s*", "", m.group(1))
        body = body.replace('\\"', '"').replace("\\\\", "\\").replace("\\n", " ")
        out.append(" ".join(body.split()))
    return out


def line_is_comment(text, pos):
    start = text.rfind("\n", 0, pos) + 1
    return text[start:pos].lstrip().startswith("//")


def is_pattern(text, start, end):
    """True when the variant at text[start:end] is matched rather than returned."""
    before = text[max(0, start - 200) : start]
    if re.search(r"matches!\s*\([^;]*$", before) or re.search(r"\blet\s+(?:Err\()?$", before.rstrip()):
        return True
    after = text[end:]
    after = re.sub(r"^[\s)]*", "", after)
    return after.startswith("=>") or after.startswith("|") or re.match(r"=(?!=)", after) is not None \
        or after.startswith("if ")


def enclosing_fn(text, pos):
    """`Type::name` or `name` for the function whose body contains `pos`."""
    best = None
    for m in re.finditer(r"\bfn\s+(\w+)", text[:pos]):
        body = text.find("{", m.end())
        if 0 <= body < pos and matching(text, body) > pos:
            best = m
    if best is None:
        return "(outside a function)"
    name = best.group(1)
    owner = None
    for m in re.finditer(r"^\s*impl(?:<[^>]*>)?\s+(?:[\w:<>, ']+\s+for\s+)?([A-Z]\w*)", text[: best.start()], re.M):
        body = text.find("{", m.end())
        if 0 <= body < best.start() and matching(text, body) > best.start():
            owner = m.group(1)
    return f"{owner}::{name}" if owner else name


def resolve(expr, text):
    """A tuple payload's value: a literal, a `const`, or `self.name()`'s literal."""
    lits = rust_strings(expr)
    if lits:
        return lits[0]
    ident = expr.strip()
    m = re.search(rf"\bconst\s+{re.escape(ident)}\s*:\s*&(?:'static\s+)?str\s*=\s*\"([^\"]*)\"", text)
    if m:
        return m.group(1)
    if ident == "self.name()":
        m = re.search(r"fn\s+name\s*\(&self\)\s*->\s*&'static\s+str\s*\{\s*\"([^\"]*)\"", text)
        if m:
            return m.group(1)
    return ident


def error_messages(sources):
    """{(enum, variant): the #[error] text} over every crate's sources."""
    out = {}
    for _, text in sources:
        for em in re.finditer(r"\benum\s+([A-Z]\w*)\s*\{", text):
            body = text[em.end() - 1 : matching(text, em.end() - 1)]
            for vm in re.finditer(r'#\[error\("((?:[^"\\]|\\.)*)"\)\]\s*(?:#\[[^\]]*\]\s*)*([A-Z]\w*)', body):
                out[(em.group(1), vm.group(2))] = vm.group(1)
    return out


def main():
    files = sorted(p for p in ROOT.glob("gungnir-*/src/**/*.rs") if "target" not in p.parts)
    sources = [(p.relative_to(ROOT).as_posix(), without_test_modules(p.read_text(encoding="utf-8"))) for p in files]
    messages = error_messages(sources)
    rows, returned = [], set()
    for rel, text in sources:
        for m in VARIANT.finditer(text):
            if line_is_comment(text, m.start()):
                continue
            end = m.end()
            k = end
            while k < len(text) and text[k] in " \t\n":
                k += 1
            payload = ""
            if k < len(text) and text[k] in "({":
                close = matching(text, k)
                payload, end = text[k:close], close
            if is_pattern(text, m.start(), end):
                continue
            enum, variant = m.group(1), m.group(2)
            returned.add((enum, variant))
            crate, _, path = rel.partition("/")
            fields = dict(re.findall(r"(\w+)\s*:\s*(\"(?:[^\"\\]|\\.)*\")", payload, re.S))
            if fields:
                what = " ".join(rust_strings(fields.get("what", '""')))
                waiting = " ".join(rust_strings(fields.get("waiting_on", '""')))
            else:
                arg = resolve(payload[1:-1], text) if payload.startswith("(") else ""
                template = messages.get((enum, variant), variant)
                what = template.replace("{0}", arg) if "{0}" in template else (f"{template}: {arg}" if arg else template)
                waiting = ""
            rows.append((crate, path, enclosing_fn(text, m.start()), f"{enum}::{variant}", what, waiting))

    declared = []
    for rel, text in sources:
        for em in re.finditer(r"\benum\s+([A-Z]\w*)\s*\{", text):
            body = text[em.end() - 1 : matching(text, em.end() - 1)]
            for dm in DECLARED.finditer(body):
                key = (em.group(1), dm.group(1))
                if key not in returned:
                    declared.append((rel.partition("/")[0], rel.partition("/")[2], f"{key[0]}::{key[1]}",
                                     messages.get(key, "")))

    rows.sort()
    declared.sort()
    cell = lambda s: (s or "--").replace("|", "/")
    L = [
        "# What is not built",
        "",
        "Generated by [`tools/gen_unbuilt.py`](tools/gen_unbuilt.py) from the code: every place a",
        "crate's source returns a `NotImplemented` error, in the words the code uses. Edit the",
        "code and re-run the generator; never edit this file, and never keep a list like it",
        "anywhere else.",
        "",
        "```bash",
        "python docs/tools/gen_unbuilt.py",
        "```",
        "",
        "A row is a function that refuses, whether or not anything calls it today. The",
        "wording is the code's own, so a row whose message has gone stale is a defect in the",
        "code, not in this page.",
        "",
        f"{len(rows)} places in {len({r[0] for r in rows})} crates.",
        "",
        "| Crate | File | Function | Error | What is missing | Waiting on |",
        "|---|---|---|---|---|---|",
    ]
    for crate, path, fn, err, what, waiting in rows:
        L.append(f"| `{crate}` | `{path}` | `{fn}` | `{err}` | {cell(what)} | {cell(waiting)} |")
    L += [
        "",
        "## Declared and never returned",
        "",
        "A `NotImplemented` variant nothing in the non-test sources returns claims nothing, and",
        "is a candidate for removal once whatever matches on it is updated.",
        "",
    ]
    if declared:
        L += ["| Crate | File | Variant | Message |", "|---|---|---|---|"]
        for crate, path, var, msg in declared:
            L.append(f"| `{crate}` | `{path}` | `{var}` | {cell(msg)} |")
    else:
        L.append("None.")
    OUT.write_text("\n".join(L) + "\n", encoding="utf-8", newline="\n")
    print(f"unbuilt: {len(rows)} places, {len(declared)} declared and never returned")
    return 0


if __name__ == "__main__":
    sys.exit(main())
