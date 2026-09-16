# Gungnir workspace: agent working notes

Gungnir is a Rust command-and-control desktop application plus services layer,
one node in a system of systems that spans disconnected desktops, on-prem service
nodes, and cloud service nodes. It began as an architectural scaffold and much of it
is now built. The tracking rows are gated in `docs/verification-capability-table.md` §1
against an external oracle where one exists; for several none does, or it disagreed,
and the table says which rather than claiming agreement.

**Do not take a list of unbuilt parts from prose, this file included.**
`docs/unbuilt.md` is generated from every `NotImplemented` error the code returns, and a
returned `NotImplemented` is the ground truth.

## Where each current fact lives

Every fact that has to stay true lives in exactly one source that is generated, checked,
or tested. A document may cite that source; it does not restate the fact, because a
restated fact is a stale fact the day the source moves.

| Fact | Its one source | What keeps it honest |
|---|---|---|
| The dependency graph | The crates' `Cargo.toml` files | `gungnir-app/tests/dependency_graph.rs` against `ARCHITECTURE.md` §7.1 |
| Pass criteria | `docs/verification-capability-table.md` | Human-owned: any change to a criterion |
| What is not built | `docs/unbuilt.md`, generated from the code | `docs/tools/gen_unbuilt.py` in CI |
| Gaps and their status | `docs/mission/gap-analysis/data/gaps.yaml`: a gap's status is its last history entry | `gen_gaps.py --base` refuses edits to merged history |
| Decisions | `docs/mission/gap-analysis/data/decisions.yaml` | `gen_gaps.py` |
| What the owner has signed | `docs/signatures.yaml`, rendered to `docs/signatures.md` | `docs/tools/signatures.py check` |
| The approved crates | `docs/agentic-coding-standards.md` §2.9's table | `architecture_compliance.rs`; `pr-rules.yml` |
| Why something was done | A `docs/record/` item, the commit, the pull request | Items are never edited once merged |

## Read first

- `docs/README.md`: index, reading order, which standards doc governs which crate,
  glossary.
- `ARCHITECTURE.md`: layers, dependency graph, deployment profiles (§8), version set
  and the two GPU contexts (§9). §10 points at the record and at the sources above.
  Section numbers §1–§10 are cited from code and must not be renumbered.
- `docs/agentic-coding-standards.md` and `docs/rust-ui-architecture-coding-standards.md`:
  the rules; §7 of the former reconciles the two.
- `docs/agentic-workflow.md`: trust tiers, and the authoritative human-owned list.
  Human-owned: anything `unsafe`, `gungnir-fusion-async`, numerical stability,
  `gungnir-policy`, `gungnir-command`, `gungnir-security`, the `gungnir-ingest` gateway,
  `gungnir-api` write paths, `gungnir-remote`'s TLS identity path (`src/identity.rs`, and
  `LinkTls` with `client_config` in `lib.rs`), and **any** change to a pass criterion in
  `docs/verification-capability-table.md` -- not only a widening.
- `docs/record/README.md`: how the record works. Items 1 to 136 and 96A are what
  `ARCHITECTURE.md` §10 held, under the same numbers.
- `docs/plans/README.md`: the eleven plans and the folders their deliverables go to.

## Hard rules

- Never add a `Cargo.toml` dependency edge that `ARCHITECTURE.md` does not show.
  Direction is one-way: core, then service facades, then productization, then UI.
- Never add a crate to `[workspace.dependencies]` without a row in
  `docs/agentic-coding-standards.md` §2.9, and a pull request description carrying a
  `Decision:` line and a `Duplicate linkage:` line.
- Never redefine a type that `gungnir-core` or `gungnir-model` owns; re-export it.
- Never make a health flag, a "connected" state, or a test claim a subsystem works
  when it does not. Prefer an explicit error variant over a silent stub.
- Never widen a pass criterion in `docs/verification-capability-table.md` to make a
  test pass.
- No `unwrap()`/`expect()` outside tests, `main()`, and debug-only invariant checks.
- **Never say whether something is signed outside the ledger.** Cite `docs/signatures.md`;
  a sentence saying something still waits for the owner fails CI outside the history.
- **Never edit a merged `docs/record/` item or gap history entry.** Add a new one. A new
  record item is `docs/record/<YYYY-MM-DD>/<subject>.md`, never a number
  (`python docs/record/tools/record.py new "Subject"`).
- Commit message bodies stay under 120 words: the row, gap or decision, the standards
  section, and why. The narrative goes in a record item.
- Keep doc comments' document citations correct when you move or rename anything;
  `docs/README.md` explains how the citations are kept in sync.

## Commands

```bash
cargo check --workspace --all-targets
```

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets
```

Every generated document and cross-document check, as CI runs them (needs Python 3 with
PyYAML):

```bash
BASE_REF=origin/main bash .github/scripts/check_documents.sh all
```

The generated documents carry a line saying so. Edit their source -- the YAML under
`docs/mission/gap-analysis/data/`, `docs/signatures.yaml`, the code for `docs/unbuilt.md`
-- and regenerate; never edit the output.

The desktop binary is `gungnir-app` (eframe, `Renderer::Glow`); the headless node is
`gungnir-node [config.json]`. Both start with the default config and run without
sensors; both journal to `./gungnir-journal` under the working directory.

`gungnir-node account add|list|add-os-keystore|list-os-keystore` provisions the accounts
a node authenticates against. It is human-owned because it writes credential material,
and it reads the passphrase from standard input, never from an argument.

The node stops on Ctrl-C. On Windows it ignores signals sent from Git Bash (`kill`,
`kill -INT`), so a smoke run started from that shell has to be stopped with
`taskkill /PID <pid> /F` or PowerShell `Stop-Process -Name gungnir-node -Force`.

## When a doc and the code disagree

The source in the table above wins, and the code wins over prose. Fix the document that
is wrong in the same change, and never delete a doc without re-pointing the code
comments that cite it.
