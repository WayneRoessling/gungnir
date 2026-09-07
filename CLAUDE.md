# Gungnir workspace: agent working notes

Gungnir is a Rust command-and-control desktop application plus services layer,
one node in a system of systems that spans disconnected desktops, on-prem service
nodes, and cloud service nodes. It is an architectural scaffold: the trait surfaces,
data model, wiring, and the productization layer are real; the tracking math,
allocator, ICP, three-d scene, and API transport are not yet implemented and say so
through `NotImplemented` errors, `todo!()` off every runtime path, and health flags.

## Read first

- `docs/README.md`: index, reading order, which standards doc governs which crate,
  glossary.
- `ARCHITECTURE.md`: layers, dependency graph, deployment profiles (§8), version set
  and the two GPU contexts (§9), open decisions (§10). Section numbers §1–§10 are
  cited from code and must not be renumbered.
- `docs/agentic-coding-standards.md` and `docs/rust-ui-architecture-coding-standards.md`:
  the rules; §7 of the former reconciles the two.
- `docs/agentic-workflow.md`: trust tiers. Human-owned crates: anything `unsafe`,
  `gungnir-fusion-async`, numerical stability, `gungnir-policy`, `gungnir-command`,
  `gungnir-security`, the `gungnir-ingest` gateway, `gungnir-api` write paths.
- `docs/plans/README.md`: the ten plans for the business plan, mission analysis,
  UAF and TOGAF documentation, capabilities and gaps, UX, test tracks, and AI and
  ML integration; each names the folder under `docs/` its deliverables go to.

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

The node stops on Ctrl-C. On Windows it ignores signals sent from Git Bash (`kill`,
`kill -INT`), so a smoke run started from that shell has to be stopped with
`taskkill /PID <pid> /F` or PowerShell `Stop-Process -Name gungnir-node -Force`.

## When a doc and the code disagree

The code's `Cargo.toml` edges are the truth for the dependency graph; the
verification table is the truth for pass criteria; `ARCHITECTURE.md` §10 is the
truth for what is known to be broken or undecided. Fix the document that is wrong
in the same change, and never delete a doc without re-pointing the code comments
that cite it.
