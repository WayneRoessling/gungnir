# Gungnir workspace: agent working notes

Gungnir is a Rust command-and-control desktop application plus services layer,
one node in a system of systems that spans disconnected desktops, on-prem service
nodes, and cloud service nodes. It began as an architectural scaffold and much of it
is now built: the tracking pipeline runs (`PIPELINE_IMPLEMENTED` is true), and the
allocator, ICP registration on CPU and GPU, and the v2 API transport over mutual TLS
are real. The tracking rows are gated in `docs/verification-capability-table.md` §1
against an external oracle where one exists; for several none does, or it disagreed,
and the table says which rather than claiming agreement. What is still unbuilt says so
through `NotImplemented` errors, `todo!()` off every runtime path, and health flags --
parts of the 3D scene and the STANAG 4676 codec among it.

**Do not take a list of unbuilt subsystems from any document, this one included.** Until
2026-09-16 this paragraph named five as unimplemented: four were built, and the fifth,
the 3D scene, partly built. `ARCHITECTURE.md` §10 and
`docs/mission/gap-analysis/gap-register.md` are maintained; a returned `NotImplemented`
is the ground truth.

## Read first

- `docs/README.md`: index, reading order, which standards doc governs which crate,
  glossary.
- `ARCHITECTURE.md`: layers, dependency graph, deployment profiles (§8), version set
  and the two GPU contexts (§9), resolved defects and open decisions (§10). Section
  numbers §1–§10 are cited from code and must not be renumbered.
- `docs/agentic-coding-standards.md` and `docs/rust-ui-architecture-coding-standards.md`:
  the rules; §7 of the former reconciles the two.
- `docs/agentic-workflow.md`: trust tiers, and the authoritative human-owned list.
  Human-owned: anything `unsafe`, `gungnir-fusion-async`, numerical stability,
  `gungnir-policy`, `gungnir-command`, `gungnir-security`, the `gungnir-ingest` gateway,
  `gungnir-api` write paths, `gungnir-remote`'s TLS identity path (`src/identity.rs`, and
  `LinkTls` with `client_config` in `lib.rs`), and **any** change to a pass criterion in
  `docs/verification-capability-table.md` -- not only a widening.
- `docs/plans/README.md`: the eleven plans for the business plan, mission analysis,
  UAF and TOGAF documentation, capabilities and gaps, UX, test tracks, AI and ML
  integration, and design-gap closure; each names the folder under `docs/` its
  deliverables go to.

## Hard rules

- Never add a `Cargo.toml` dependency edge that `ARCHITECTURE.md` does not show.
  Direction is one-way: core, then service facades, then productization, then UI.
- Never add a crate to `[workspace.dependencies]` without recording it in
  `docs/agentic-coding-standards.md` §2.9.
- Never redefine a type that `gungnir-core` or `gungnir-model` owns; re-export it.
- Never make a health flag, a "connected" state, or a test claim a subsystem works
  when it does not. Prefer an explicit error variant over a silent stub.
- Never widen a pass criterion in `docs/verification-capability-table.md` to make a
  test pass.
- No `unwrap()`/`expect()` outside tests, `main()`, and debug-only invariant checks.
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

The desktop binary is `gungnir-app` (eframe, `Renderer::Glow`); the headless node is
`gungnir-node [config.json]`. Both start with the default config and run without
sensors; both journal to `./gungnir-journal` under the working directory.

`gungnir-node account add|list|add-os-keystore|list-os-keystore` provisions the accounts
a node authenticates against. It is human-owned because it writes credential material,
and it reads the passphrase from standard input, never from an argument.

`gap-register.md`, `decisions-needed.md`, `closure-roadmap.md`, `coverage-matrix.md`,
and `technical-gap-map.md` under `docs/mission/gap-analysis/` are generated, and carry
no in-file warning saying so. Edit the entries in
`docs/mission/gap-analysis/tools/gen_gaps.py`, never the tables, and regenerate; CI
regenerates them and fails on any difference.

```bash
python docs/mission/gap-analysis/tools/gen_gaps.py
```

The node stops on Ctrl-C. On Windows it ignores signals sent from Git Bash (`kill`,
`kill -INT`), so a smoke run started from that shell has to be stopped with
`taskkill /PID <pid> /F` or PowerShell `Stop-Process -Name gungnir-node -Force`.

## When a doc and the code disagree

The code's `Cargo.toml` edges are the truth for the dependency graph; the
verification table is the truth for pass criteria; `ARCHITECTURE.md` §10 is the
truth for what is known to be broken or undecided. Fix the document that is wrong
in the same change, and never delete a doc without re-pointing the code comments
that cite it.
