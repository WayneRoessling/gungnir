# Contributing to Gungnir

Read `docs/README.md` first; it indexes every design and standards document and
carries the glossary. This file is the short version of the rules that apply to any
change, human- or agent-authored.

## Licensing and sign-off

Gungnir is AGPL-3.0-or-later (`LICENSE`) with additional terms under section 7
(`LICENSE-ADDITIONAL-TERMS.md`), and is also licensed commercially by Roessling Digital
Solutions LLC. That second half only works while one party can relicense the whole
codebase, so contributions carry two requirements:

1. **Sign off every commit.** `git commit -s` adds the `Signed-off-by` line that
   certifies the Developer Certificate of Origin 1.1.
2. **Agree to `CLA.md` once**, for anything beyond a trivial change. It grants
   Roessling Digital Solutions LLC the right to relicense your contribution, including
   under proprietary terms. You keep your copyright.

A contribution that cannot be relicensed cannot be merged — including a dependency
under GPL or any other license absent from `deny.toml`'s allow-list, because it would
make the commercial edition undistributable.

## Before you write code

1. Find the capability you are touching in `docs/gungnir-capabilities.md` and, for
   the tracking core, its pass criterion in `docs/verification-capability-table.md`
   §1. The criterion shapes the implementation; do not discover it when a test fails.
2. Check which standards document governs the crate (`docs/README.md`, "Which
   standard applies to which crate"): `docs/agentic-coding-standards.md` for the
   tracking, service, and productization crates; `docs/rust-ui-architecture-coding-standards.md`
   for the UI, rendering, and 3D-data crates.
3. Check the trust tier in `docs/agentic-workflow.md`. Changes to `unsafe`,
   `gungnir-fusion-async`, numerical stability, `gungnir-policy`, `gungnir-command`,
   `gungnir-security`, the `gungnir-ingest` gateway, and `gungnir-api` write paths
   are human-owned: draft them, but a human merges them.

## Rules that are checked in review

- **Dependency direction is one-way.** Do not add a `Cargo.toml` edge that
  `ARCHITECTURE.md` does not already show. If a task seems to need one, stop and
  raise it.
- **One owning crate per public type.** Shared primitives live in the lowest crate
  (`gungnir-core`, `gungnir-model`) and are re-exported, never redefined.
- **No new dependencies without sign-off.** The stack is fixed in
  `docs/agentic-coding-standards.md` §2 and §2.9. Propose additions in the PR
  description with the problem they solve.
- **No `unwrap()`/`expect()`** outside tests, `main()`, and debug-only invariant
  checks. Fallible functions return `Result` with a crate-local error enum.
- **No execution without a decision record** (contract C-01). An engagement opens,
  a plan is published as approved, or a handoff is built only from a
  `DecisionRecord` that `is_actionable()`. `gungnir-app/tests/no_execution_without_decision.rs`
  scans the sources for those constructions and drives the desktop to prove the order;
  a new execution path has to be added to that test's list with its gate.
- **No fake wiring.** A capability that is not implemented returns an explicit
  "not implemented" error or is left as `todo!()` off every runtime path; it is
  never stitched into a loop pretending to work, and health flags never claim a
  working subsystem that does not exist.
- **Doc comments cite their source.** Every crate's `lib.rs` names the document and
  section it implements; section numbers in `ARCHITECTURE.md` §1–§10 and the
  standards documents are stable, so cite them.
- **Every new source file carries the license header.** Three lines above everything
  else, including above a `//!` module doc comment, using `#` for Python and `//` for
  Rust and WGSL. A Python file with a shebang keeps it on line 1 and puts the header
  underneath.

  ```
  // Copyright (C) 2026 Roessling Digital Solutions LLC
  // SPDX-License-Identifier: AGPL-3.0-or-later
  // Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
  ```
- **Tests alongside the change.** For the tracking core that means the
  oracle-comparison test in the same PR; for everything else, unit tests of the
  invariant the code claims.

## Running the checks locally

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets
```

```bash
cargo test --workspace
```

```bash
cargo bench --workspace --no-run
```

The CI workflows under `.github/workflows/` run the same commands plus the gated
checks (`oracle-diff`, `miri`, `loom`, `fuzz-nightly`, `bench-regression`,
`gpu-fusion`, `release`). Until those workflows are enabled on a hosted runner, say
in the PR description which of them you ran by hand.

## Pull request description

State what changed, which capability-table row or capability it serves, which
standards sections apply, and why the approach was chosen (for numerical code, cite
the reference, e.g. "Joseph form, Bar-Shalom §5.3"). For `gungnir-fusion-async`
changes, describe in plain language what interleaving could be affected. For
human-owned crates, say so explicitly rather than assuming the reviewer will notice.

Commits made with an agent's help end with the attribution line the agent supplies.
