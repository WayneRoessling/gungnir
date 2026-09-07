# Gungnir workspace structure

Companion reference to the workspace `ARCHITECTURE.md`, `architecture.md` (the crate to
verification-row map), and `agentic-workflow.md`. It describes the repository layout as
it exists and maps the six verification gates from `agentic-workflow.md` to the CI
workflow files that enforce them.

## Directory tree

```
gungnir-workspace/
├── Cargo.toml                      # workspace root: members, lint policy, shared dependency pins
├── rust-toolchain.toml             # pinned channel and components
├── deny.toml                       # cargo-deny license/advisory/source policy
├── about.toml                      # cargo-about: third-party notices for the release binaries
├── README.md                       # orientation, status, suggested build order
├── ARCHITECTURE.md                 # technical reference, §1–§10
├── CONTRIBUTING.md                 # the rules that apply to any change
├── CLAUDE.md                       # agent working notes
├── .gitignore
│
├── docs/
│   ├── README.md                   # index, reading order, glossary
│   ├── gungnir-capabilities.md     # business-analyst capability reference, all crates
│   ├── verification-capability-table.md
│   ├── scenario-crate-narrative.md
│   ├── architecture.md             # crate to verification-row map
│   ├── gungnir-workspace-structure.md   # this file
│   ├── agentic-coding-standards.md
│   ├── rust-ui-architecture-coding-standards.md
│   ├── agentic-workflow.md
│   ├── rust-ui-tech-stack-summary.md
│   ├── rust-3d-data-ecosystem-build-vs-adopt.md
│   ├── gungnir-api-v1.md           # API interface control document
│   ├── performance-budgets.md      # end-to-end SLO drafts
│   ├── release-governance.md       # assurance and release policy
│   ├── plans/                      # the ten plans (README.md indexes and sequences them)
│   ├── business/                   # plan 01 deliverables
│   ├── mission/                    # plan 02; capabilities/ (plan 04), gap-analysis/ (plan 05)
│   ├── architecture/               # uaf/ (plan 03), togaf/ (plan 10)
│   ├── ux/                         # plan 06
│   ├── test-tracks/                # plan 07 documents (data under testdata/tracks/)
│   ├── ai/                         # plan 08
│   ├── ml/                         # plan 09
│   └── old/rust-c2-project-docs.zip     # archived pre-consistency snapshot, not cited
│
├── testdata/tracks/                # committed sample track sets (plan 07); large sets generated
│
├── deploy/
│   ├── README.md                   # how the two binaries are deployed
│   └── node/
│       ├── Dockerfile              # gungnir-node container, Linux x86_64
│       └── config.example.json
│
├── benches/README.md               # index of the criterion groups (they live per crate)
│
├── .github/workflows/
│   ├── ci.yml                      # fmt, clippy, test, benches compile, node container build
│   ├── oracle-diff.yml             # gate 1
│   ├── miri.yml                    # gate 3
│   ├── loom.yml                    # gate 4
│   ├── fuzz-nightly.yml            # gate 5
│   ├── bench-regression.yml        # gate 6
│   ├── gpu-fusion.yml              # GPU-vs-CPU registration check; dormant (manual dispatch) until GAP-024
│   └── release.yml                 # cargo-deny, cargo-audit, SBOM, signed builds, image
│
│   # Tracking core (docs/agentic-coding-standards.md governs)
├── gungnir-core/                   # core: CV/CA/CT motion models; ident (TrackId, TrackStatus, ResourceId); numeric (assert_psd)
├── gungnir-coord/                  # coord: ECEF/ENU/NED/geodetic
├── gungnir-filters/                # filters: KF, EKF, UKF, PF, IMM, sqrt/UDU, RTS; benches/ekf_predict_update.rs
├── gungnir-association/            # association: NN/GNN, Hungarian/JV, gating, JPDA, MHT; benches/hungarian_solve_100x100.rs
├── gungnir-track/                  # track-manager: init/confirm/coast/delete
├── gungnir-rfs/                    # rfs: PHD/CPHD, GLMB/LMB; benches/phd_update_dense_swarm.rs
├── gungnir-track-fusion/           # track-fusion: CI/info-matrix fusion, bias estimation
├── gungnir-fusion-async/           # fusion-async: OOS/multi-rate, the tokio user
├── gungnir-allocation/             # allocation: Bellman/DP assignment
├── gungnir-scenario/               # scenario: five-scenario generator (test/bench only)
├── gungnir-metrics/                # metrics: MOTA/MOTP, purity/fragmentation
├── gungnir-oracle/                 # differential-test harness (gate 1)
├── gungnir-testkit/                # shared proptest strategies (gate 2); no workspace deps
├── gungnir-fuzz/                   # fuzz targets (gate 5); excluded from the default build
│   └── fuzz_targets/
│
│   # Service layer
├── gungnir-tracking-service/       # TrackingService trait, LiveTrackingService, Track -> TrackView projection
├── gungnir-intercept-service/      # InterceptService trait, DpInterceptService, PlanView
│
│   # Productization layer (docs/gungnir-capabilities.md §5)
├── gungnir-model/                  # src/{lib,provenance,quality,time,identity,events}.rs
├── gungnir-eventing/               # broadcast bus, Envelope
├── gungnir-store/                  # src/{lib,journal,retention}.rs -- JSON-lines journal
├── gungnir-config/                 # ConfigBaseline, FileConfigStore, BackendConfig, NodeConfig
├── gungnir-mission/
├── gungnir-ingest/                 # src/{lib,gateway}.rs, src/adapters/{recorded,simulated}.rs
├── gungnir-sensor-management/
├── gungnir-time/
├── gungnir-interop/                # schema catalog, Arrow schema, ASTERIX/STANAG codec boundary
├── gungnir-identity/
├── gungnir-identification/
├── gungnir-ml/                     # Model and FeatureExtractor traits, FakeModel, dataset extraction (no runtime, GAP-077)
├── gungnir-geo/                    # geofences (haversine), InMemoryGeoService
├── gungnir-analytics/              # line-of-sight, viewshed, coverage volumes, route deconfliction
├── gungnir-policy/
├── gungnir-command/
├── gungnir-assessment/
├── gungnir-decision/
├── gungnir-modelops/
├── gungnir-security/               # src/{lib,authn,authz,audit}.rs
├── gungnir-api/                    # src/lib.rs, src/v1/mod.rs
├── gungnir-observability/
├── gungnir-resilience/             # store-and-forward, checkpoints, reconciliation
├── gungnir-collab/                 # shared picture sync, authority arbitration
├── gungnir-workflow/               # role workspaces, alert lifecycle, annotations, cases
├── gungnir-replay/
├── gungnir-reporting/
│
│   # 3D data ecosystem (docs/rust-ui-architecture-coding-standards.md governs)
├── gungnir-data/                   # src/{pointcloud,scientific,geospatial,assets}/
├── gungnir-data-fusion/            # src/{cpu_reference,transform_solve}.rs, src/gpu/{buffers.rs,shaders/*.wgsl}
│
│   # Deployment (ARCHITECTURE.md §8)
├── gungnir-remote/                 # RemoteTrackingService / RemoteInterceptService over gungnir-api
├── gungnir-node/                   # src/main.rs -- the headless service-node binary
│
│   # UI and rendering
├── gungnir-render/                 # src/{lib,egui_integration}.rs -- headless wgpu compute device
├── gungnir-viewport3d/             # src/{scene,materials,interaction,tracks}.rs, src/{streaming,scientific}/
├── gungnir-ui/                     # src/theme.rs, src/panels/{track_table,intercept_panel,sensor_health,alerts}.rs
└── gungnir-app/                    # src/{main,state,update}.rs -- the desktop binary
```

Fifty crates: forty-nine workspace members plus `gungnir-fuzz`. Every member
`Cargo.toml` carries a `description` naming its scope and the document section it
derives from, opts into the workspace lint policy with `[lints] workspace = true`,
and every `src/lib.rs` (or `src/main.rs`) opens with a doc comment cross-referencing
the same. Crates with a single source file are shown without a file list above.

## Crate to verification-row map

The map lives in `architecture.md` so it exists in exactly one place.

## Agentic verification gates and CI workflows

Direct mapping from the six-gate verification stack in `agentic-workflow.md` to the
workflow files that enforce them. They run on GitHub (D-10 as amended 2026-09-07);
`gpu-fusion.yml` alone is dormant, on manual dispatch, until GAP-024 delivers the GPU
path, its tests and a GPU-labelled self-hosted runner.

| # | Gate | Workflow | Trigger |
|---|---|---|---|
| 1 | Differential testing versus oracle | `oracle-diff.yml` | PR touching an oracle-comparable core crate, `gungnir-oracle`, or `gungnir-scenario` |
| 2 | Property-based invariants (`proptest`) | Runs as part of `cargo test` in `ci.yml`; the strategies live in `gungnir-testkit` | Every PR |
| 3 | `cargo miri` on `unsafe` | `miri.yml` | Any PR whose diff adds `unsafe` |
| 4 | `loom` exhaustive interleaving | `loom.yml` | PR touching `gungnir-fusion-async` or `gungnir-tracking-service` |
| 5 | Fuzzing (`cargo-fuzz`) | `fuzz-nightly.yml` | Scheduled nightly, not per-PR |
| 6 | Benchmark regression gate | `bench-regression.yml` | Every PR |

`gungnir-fusion-async` (concurrency) and any `unsafe` block are two of the items
`agentic-workflow.md` marks human-owned. Both get their own always-required workflow
rather than being folded into general CI, and both require explicit human sign-off in
addition to a green check. Two further workflows are not numbered gates:
`gpu-fusion.yml` validates the GPU registration path against the CPU reference on a
GPU runner (dormant since 2026-09-07: manual dispatch only, and it fails a run that
executed zero tests, because the path and its tests are GAP-024's and do not exist
yet), and `release.yml` runs the assurance track in `release-governance.md`.

## Naming notes

Recorded when the tracking-core crate names were chosen; not re-verified since.

- `gungnir` was unclaimed on crates.io at the time of the check (unlike an earlier
  candidate, `sextant`, whose bare name is taken). A facade crate named plain `gungnir`
  re-exporting the sub-crates remains possible if wanted.
- The fourteen tracking-core crate names (`gungnir-core` through `gungnir-fuzz`) were
  confirmed available on crates.io at that time. The names added later were not
  checked. **This note used to end "the workspace license is `UNLICENSED`, so
  publication is not currently planned and availability only matters if that
  changes." That changed on 2026-09-07**: the workspace is `AGPL-3.0-or-later`
  (`../LICENSE`), which crates.io accepts, so nothing in the license blocks
  publication and name availability now matters. Neither the later names nor the
  original fourteen have been re-checked since.
- The `fusion-` prefix used by early drafts for the data and UI crates is retired
  (`ARCHITECTURE.md` §7).
- A GitHub organization or repository name was not verified.
