# Gungnir documentation index

This folder holds the design, standards, and verification references for the
`gungnir-workspace` Cargo workspace. Start with the workspace [`README.md`](../README.md)
for orientation and [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the technical crate map;
come here for the detail behind them.

All documents were brought into agreement on 2026-09-04, when the scaffold defects were
fixed, the scope was opened, and the seven deployment and productization crates were
added. The pre-consistency snapshot of the original documents is archived in
`old/rust-c2-project-docs.zip` for reference only; nothing in the workspace cites it.

## Reading order

| # | Document | Audience | What it answers |
|---|---|---|---|
| 1 | [`../README.md`](../README.md) | Everyone | What Gungnir is, how the workspace is laid out, what is real versus scaffold, how to run it. |
| 2 | [`../ARCHITECTURE.md`](../ARCHITECTURE.md) | Engineers, agents | Layers, dependency graph, service boundary, GPU contexts, deployment topology, what was fixed and what is open. |
| 3 | [`gungnir-capabilities.md`](gungnir-capabilities.md) | Stakeholders, analysts, engineers | What every crate does in business terms, why it matters, how it is proven, and its implementation status. |
| 4 | [`verification-capability-table.md`](verification-capability-table.md) | Engineers, agents, reviewers | The pass/fail matrix: oracle, method, tolerance, and data source per tracking-core capability, plus draft rows for the other layers. |
| 5 | [`scenario-crate-narrative.md`](scenario-crate-narrative.md) | Engineers, test authors | Why five generator scenarios cover the verification matrix. |
| 6 | [`architecture.md`](architecture.md) | Engineers, agents | The one table mapping every crate to its verification-table module and row status. |
| 7 | [`gungnir-workspace-structure.md`](gungnir-workspace-structure.md) | Engineers, agents | Repository layout, CI-gate mapping, naming notes. |
| 8 | [`agentic-coding-standards.md`](agentic-coding-standards.md) | Agents, reviewers | Coding and architecture standards for the tracking core, service layer, and productization crates. The approved dependency stack. |
| 9 | [`rust-ui-architecture-coding-standards.md`](rust-ui-architecture-coding-standards.md) | Agents, reviewers | Coding and architecture standards for the UI, rendering, and 3D-data crates. |
| 10 | [`agentic-workflow.md`](agentic-workflow.md) | Agents, reviewers, adopters | Agent trust tiers, the verification gates, and the review pipeline. |
| 11 | [`rust-ui-tech-stack-summary.md`](rust-ui-tech-stack-summary.md) | Engineers | The UI and rendering stack decision record, including the OpenGL versus wgpu split. |
| 12 | [`rust-3d-data-ecosystem-build-vs-adopt.md`](rust-3d-data-ecosystem-build-vs-adopt.md) | Engineers | Which 3D-data crates are adopted, which bridges are built, and the GPU point-cloud fusion design. |
| 13 | [`gungnir-api-v1.md`](gungnir-api-v1.md) | Engineers, integrators | The service node's external contract: endpoints, versioning, event-stream semantics. |
| 14 | [`performance-budgets.md`](performance-budgets.md) | Engineers, deployment owners | Draft end-to-end SLOs per profile and how they will be measured. |
| 15 | [`release-governance.md`](release-governance.md) | Engineers, release owners | Dependency and license policy, SBOM, signing, promotion. |
| 16 | [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Everyone | The short form of the rules for any change. |
| 17 | [`../deploy/README.md`](../deploy/README.md) | Deployment owners | Building and running the desktop and the node container. |
| 18 | [`plans/README.md`](plans/README.md) | Everyone | The planning set: eleven plans for the business plan, mission analysis, UAF views, capabilities, gaps, UX, test tracks, AI agent, ML, TOGAF documentation, and design gap closure, with sequencing and effort. |

## Planned document sets

The plans in `plans/` each produce a set of documents in its own folder. Every folder
exists with a README that points at its plan; the deliverables land there as the
plans execute. `plans/README.md` keeps the execution status in detail.

| Folder | Plan | Produces | Status |
|---|---|---|---|
| `business/` | 01 | Business plan, market and competitive analysis, pricing, financial model, go-to-market | First draft 2026-09-04 |
| `mission/` | 02 | Mission analysis per domain, mission threads, vignettes, roles, measures | First draft 2026-09-04 |
| `mission/capabilities/` | 04 | Capability taxonomy, statements, traceability matrices, roadmap | First draft 2026-09-04 |
| `mission/gap-analysis/` | 05 | Gap register, coverage matrix, closure roadmap | First draft 2026-09-04 |
| `architecture/uaf/` | 03 | UAF 1.2 views and the shared element registry | First draft 2026-09-04 |
| `architecture/togaf/` | 10 | TOGAF ADM phase documents, the Architecture Definition Document, the requirements specification, and a DoDAF cross-reference | First draft 2026-09-04 |
| `ux/` | 06 | Personas, task analyses, wireframes, interaction design, design system, usability plan | First draft 2026-09-04 |
| `test-tracks/` | 07 | Vehicle catalogue, class profiles, sensor models, scenario library, data format (data under `../testdata/tracks/`) | First draft 2026-09-04 |
| `ai/` | 08 | Agent concept of operations, safety boundaries, architecture, tools, provider configuration, evaluation, security | First draft 2026-09-04 |
| `ml/` | 09 | ML use cases, architecture, data pipeline, training, evaluation, MLOps, security | First draft 2026-09-04 |
| `design/` | 11 | A design note per open design gap: owning component, types, edges, behaviour, configuration and interface delta, panel delta, verification row | First draft 2026-09-05 (22 notes, 4 consolidations), plus DN-23 and DN-24 raised the same day out of the gaps they block, both signed and implemented; `external-standards.md` (2026-09-06) locates the ASTERIX and STANAG 4676 specifications for GAP-064 and, since D-24 the same day, the AIS and ADS-B candidates for GAP-010 -- AIS pinned to ITU-R M.1371-6 under D-32 the same day, and ADS-B **deliberately left unpinned** later that day because no specification is both free to obtain and permissively licensed, its decoder gated instead against two MIT decoders over two vendored captures (§4.3, §4.5) -- and `handoff-2026-09-06-radar-feed.md` hands the radar feed work on; `tak-interoperability-research.md` (2026-09-08) is the TAK ecosystem as verified from the clients' own source, and the three D-33 extensions the owner took the same day |

## Which standard applies to which crate

| Crate group | Standards document |
|---|---|
| Tracking core (`gungnir-core` through `gungnir-metrics`, plus `gungnir-oracle`, `gungnir-testkit`, `gungnir-fuzz`) | `agentic-coding-standards.md` |
| Foundation and service layer (`gungnir-model`, `gungnir-tracking-service`, `gungnir-intercept-service`) | `agentic-coding-standards.md` |
| Productization layer (`gungnir-eventing` through `gungnir-reporting`, including `gungnir-interop`, `gungnir-analytics`, `gungnir-resilience`, `gungnir-collab`, `gungnir-workflow`) | `agentic-coding-standards.md` |
| Deployment (`gungnir-remote`, `gungnir-node`) | `agentic-coding-standards.md`; `gungnir-node` is wiring only, like `gungnir-app` |
| 3D data (`gungnir-data`, `gungnir-data-fusion`) | `rust-ui-architecture-coding-standards.md` §4–§9, plus `agentic-coding-standards.md` §3 for general Rust rules |
| UI and rendering (`gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`, `gungnir-app`) | `rust-ui-architecture-coding-standards.md` |

Where the two standards documents disagree, `agentic-coding-standards.md` §7 states
which rule wins for which crate group. The lint policy both agree on is enforced by the
workspace `Cargo.toml` (`[workspace.lints]`), which every crate opts into.

## Keeping the set consistent

- Some documents are **generated** and must not be edited by hand: the five plan 05
  gap-analysis documents come from `mission/gap-analysis/tools/gen_gaps.py`, and the
  `test-tracks/` catalogue, class-profile, sensor-model, and scenario-library pages come
  from `test-tracks/tools/build_catalogue.py`. Each generator is deterministic, so
  re-running it on an unmodified tree rewrites its outputs byte for byte; that is also
  the check that no hand edit has crept in. Change the generator and re-run.
- Oracle evidence for the `verification-capability-table.md` §1 differential tests lives
  in `../testdata/oracles/`, with the generator and the oracle version that produced each
  fixture. `../testdata/oracles/README.md` records which oracles were actually run and
  which were not; MATLAB is not installed on the development machine and no fixture there
  is a MATLAB result.
- Crate names are always `gungnir-*`. No document or doc comment may use the retired
  `fusion-*` prefix or the retired single-binary `src/app`, `src/ui`, `src/data` module
  paths except when explaining history.
- Section numbers in `ARCHITECTURE.md` §1–§10, `agentic-coding-standards.md`,
  `rust-3d-data-ecosystem-build-vs-adopt.md`, `rust-ui-architecture-coding-standards.md`,
  and `gungnir-capabilities.md` §5–§9 are cited from Rust doc comments and `Cargo.toml`
  descriptions. Add new sections at the end; do not renumber existing ones.
- A capability's pass criterion lives in `verification-capability-table.md` and is
  restated, not redefined, in `gungnir-capabilities.md` and in the owning crate's doc
  comment.
- The crate manifests are the truth for the dependency graph; `ARCHITECTURE.md` reproduces
  them. When an edge changes, update the graph and §7.1 in the same change.
- `ARCHITECTURE.md` §10 is the truth for what is known to be unimplemented or undecided;
  when something lands, move it from "Open" to "Resolved" there and update the status
  lines in `gungnir-capabilities.md`.

## Glossary

| Term | Meaning in this project |
|---|---|
| AAR | After-action review: replaying and reporting on a recorded mission session. |
| C2 | Command and control. Gungnir is a C2 desktop application plus services layer, and a node in a larger system of systems. |
| CI (fusion) | Covariance intersection, a track-to-track fusion method. Distinct from CI meaning continuous integration. |
| COA | Course of action: a candidate plan produced by `gungnir-decision`. |
| CV / CA / CT | Constant-velocity, constant-acceleration, and coordinated-turn motion models. |
| EKF / UKF / IMM / PF | Extended Kalman, unscented Kalman, interacting multiple model, and particle filters. |
| Envelope | An event as delivered and journaled: bus sequence number, mission time, payload (`gungnir-eventing`). |
| GLMB / LMB / PHD / CPHD | Random-finite-set multi-target filters implemented in `gungnir-rfs`. |
| GNN / JPDA / MHT | Global nearest neighbour, joint probabilistic data association, and multi-hypothesis tracking association strategies. |
| ICD | Interface control document; `gungnir-api-v1.md` is one. |
| ICP | Iterative closest point, the point-cloud registration algorithm in `gungnir-data-fusion`. |
| Mission time | Seconds as an `f64`: Unix time live, recorded time in replay (`gungnir-model::MissionTime`). |
| OOS | Out-of-sequence measurements, handled by `gungnir-fusion-async`. |
| OSPA / GOSPA | Optimal sub-pattern assignment distance metrics for end-to-end tracking accuracy. |
| PSD | Positive semi-definite, the property every covariance matrix must keep. |
| RFS | Random finite set. |
| RTS | Rauch-Tung-Striebel smoother. |
| SBOM | Software bill of materials, produced per release (`release-governance.md`). |
| SSE | Screen-space error, the level-of-detail criterion for streamed 3D tiles. |
| Service node | The headless binary `gungnir-node` that hosts the services layer on-prem or in the cloud. See `ARCHITECTURE.md` §8. |
| Store-and-forward | Queuing detections and envelopes on a disconnected desktop for delivery when its node returns (`gungnir-remote`, `gungnir-resilience`). |
