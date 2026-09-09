# Gungnir Architecture

Gungnir is a command-and-control (C2) desktop application and services layer. This
workspace merges three lines of design work into one deployable system:

1. The **tracking, estimation, and intercept stack** (`gungnir-core` through
   `gungnir-metrics`, with `gungnir-oracle`, `gungnir-testkit`, and `gungnir-fuzz` as
   its verification crates). Its design documents are `docs/agentic-coding-standards.md`,
   `docs/verification-capability-table.md`, `docs/scenario-crate-narrative.md`,
   `docs/gungnir-workspace-structure.md`, and `docs/architecture.md`.
2. The **UI, rendering, and 3D-data stack** (`eframe`/`egui`/`three-d`, plus `wgpu` for
   compute), described in `docs/rust-ui-tech-stack-summary.md`,
   `docs/rust-ui-architecture-coding-standards.md`, and
   `docs/rust-3d-data-ecosystem-build-vs-adopt.md`.
3. The **productization layer** (`gungnir-model` through `gungnir-reporting`, plus the
   deployment crates `gungnir-remote` and `gungnir-node`): the crates that turn a
   tracking engine with a UI into a fieldable C2 system. Their business rationale is
   `docs/gungnir-capabilities.md` §5.

The first two were designed as if they would stay independent; §1–§6 describe the seam
between them. §7 covers the productization layer. §8 describes how the same crates are
deployed as a disconnected desktop, an on-prem node, or a cloud node within a larger
system of systems. §9 records the tested version set and the two GPU contexts. §10 lists
what was fixed in the 2026-09-04 pass and what is still open.

**Scope.** The scope is deliberately open: every capability the independent review
recommended is scaffolded as a crate, and the planned repository files exist. A later
pass locks the scope; nothing here should be read as a commitment to ship every crate
in the first release.

Section numbers §1–§10 are cited from Rust doc comments and `Cargo.toml` descriptions
and must not be renumbered.

## Top-level dependency graph

Arrows point from dependent to dependency. Only normal dependencies are shown; every
tracking-core crate additionally dev-depends on `gungnir-testkit`, and `gungnir-oracle`,
`gungnir-mission`, `gungnir-ingest`, `gungnir-tracking-service` and `gungnir-app`
dev-depend on `gungnir-scenario`.
The last two were added on 2026-09-05: `gungnir-ingest` hosts the scenario round-trip
fidelity row, which needs both the replay adapter and `gungnir_model::DetectionView`, and
`gungnir-app` hosts the tick harness, which `docs/performance-budgets.md` requires to be
fed from generated scenario output. Neither is a runtime edge and neither can cycle,
because `gungnir-scenario` depends on neither.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ TRACKING CORE (pure Rust, no UI or GPU types, ever)                          │
│   gungnir-core (motion models; owns TrackId, TrackStatus, ResourceId,        │
│                 assert_psd)                     gungnir-coord (frames)       │
│   gungnir-filters ──► core, coord                                            │
│   gungnir-association ──► filters                                            │
│   gungnir-track ──► core, association                                        │
│   gungnir-rfs ──► track      gungnir-metrics ──► track, association          │
│   gungnir-track-fusion ──► track, coord                                      │
│   gungnir-fusion-async ──► track, rfs, track-fusion   (the tokio user)       │
│   gungnir-allocation ──► core                                                │
│   gungnir-scenario ──► core, coord, fusion-async   (test/bench only)         │
│   gungnir-oracle, gungnir-testkit, gungnir-fuzz: dev/test only               │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                    │ trait facades (§2), typed with gungnir-model
┌───────────────────────────────────▼──────────────────────────────────────────┐
│ FOUNDATION + SERVICE LAYER                                                   │
│   gungnir-model ──► core, coord            (canonical views, events)         │
│   gungnir-tracking-service ──► the eight core crates + model                 │
│   gungnir-intercept-service ──► core, coord, allocation, model               │
└──────────┬──────────────────────────────────────────┬────────────────────────┘
           │                                          │
┌──────────▼───────────────────────────┐   ┌──────────▼──────────────────────────┐
│ PRODUCTIZATION LAYER (§7.1)          │   │ 3D DATA (§3)                        │
│   eventing, store, config, mission   │   │   gungnir-data (file I/O, no GPU)   │
│   time, ingest, sensor-management,   │   │   gungnir-data-fusion ──► data      │
│     interop                          │   │     (wgpu compute; borrows the      │
│   identity, identification, geo,     │   │      device from gungnir-render)    │
│     analytics                        │   └──────────┬──────────────────────────┘
│   policy, command, assessment,       │              │
│     decision, modelops               │              │
│   security, api, observability,      │              │
│     resilience, collab, workflow     │              │
│   replay, reporting                  │              │
└──────────┬───────────────────────────┘              │
           │                                          │
┌──────────▼──────────────────────────────────────────▼────────────────────────┐
│ DEPLOYMENT + UI (§4, §8)                                                     │
│   gungnir-remote ──► api, eventing, model, both facades   (remote backends)  │
│   gungnir-node ──► facades + productization; no UI crate   (headless binary) │
│   gungnir-ui ──► model                                     (egui panels)     │
│   gungnir-viewport3d ──► data, data-fusion, model, ui      (three-d, GL)     │
│   gungnir-render ──► wgpu, egui                            (compute device)  │
│   gungnir-app ──► everything above it                      (desktop binary)  │
└──────────────────────────────────────────────────────────────────────────────┘
```

The exact normal-dependency edges of the tracking core, foundation, service, data,
deployment, and UI crates, from the crate manifests:

| Crate | Depends on |
|---|---|
| `gungnir-core`, `gungnir-coord`, `gungnir-testkit`, `gungnir-security`, `gungnir-render`, `gungnir-data` | No workspace crates |
| `gungnir-model` | `core`, `coord` |
| `gungnir-filters` | `core`, `coord` |
| `gungnir-association` | `filters` |
| `gungnir-track` | `core`, `association` |
| `gungnir-rfs` | `track` |
| `gungnir-metrics` | `track`, `association` |
| `gungnir-track-fusion` | `track`, `coord` |
| `gungnir-fusion-async` | `track`, `rfs`, `track-fusion`, `core` (r), `filters` (r), `association` (r) |
| `gungnir-allocation` | `core` |
| `gungnir-scenario` | `core`, `coord`, `fusion-async` |
| `gungnir-oracle` | `filters`, `association`, `rfs`, `scenario` (dev: `testkit`) |
| `gungnir-fuzz` (excluded from the default build) | `association`, `ingest`, `model` |
| `gungnir-tracking-service` | `core`, `coord`, `filters`, `association`, `track`, `rfs`, `track-fusion`, `fusion-async`, `model` |
| `gungnir-intercept-service` | `core`, `coord`, `allocation`, `model` |
| `gungnir-data-fusion` | `data` |
| `gungnir-remote` | `model`, `eventing`, `api`, `tracking-service`, `intercept-service`, `security` (t) |
| `gungnir-node` | `model`, `config`, `mission`, `eventing`, `store`, `time`, `ingest`, `sensor-management`, `tracking-service`, `intercept-service`, `api`, `analytics` (g), `security`, `observability`, `modelops` (h), `policy` (k), `geo` (l), `remote` (p), `identity` (s) |
| `gungnir-ui` | `model` |
| `gungnir-viewport3d` | `data`, `data-fusion`, `model`, `ui` (theme only) |
| `gungnir-app` | Both facades, `remote`, `data`, `data-fusion`, `render`, `viewport3d`, `ui`, `workflow`, `security`, `policy`, `command`, `geo`, `replay`, `reporting`, `analytics`, `sensor-management`, `assessment`, `modelops` (h), `decision` (i), `resilience` (m), `identification` (n), `identity` (o), `coord` (u), `model`, `config`, `mission`, `eventing`, `store`, `time`, `ingest`, `observability` |

The productization-layer edges are listed in §7.1.

`gungnir-remote` → `gungnir-security` is edge (t). It was in the manifest from 2026-09-06
but absent from this table until 2026-09-07, when the first CI run of the UAF generator
(GAP-061) produced a view showing an edge the table did not. Its reason, the two refused
alternatives, and why no test caught it are in `docs/design/dependency-edges.md` §14.

## §1 — Why the tracking core stays untouched and fully separate

`docs/agentic-coding-standards.md` §1.1 fixes a one-way dependency chain for the
tracking-core crates and states plainly: *"An agent must not add a dependency edge that
isn't already implied by this graph."* Wiring a UI directly into `gungnir-filters` or
`gungnir-fusion-async` would violate that rule for no benefit; none of those crates needs
to know a UI exists. They keep:

- Zero dependency on `egui`, `three-d`, `wgpu`, or `eframe`.
- Their verification stack unchanged: `gungnir-oracle` differential tests,
  `gungnir-testkit` property tests, the `loom` and `miri` gates, and `criterion`
  benchmarks. All of this is orthogonal to whether a UI exists downstream.
- Full `cargo test`-ability with no GPU context, a rule both standards documents landed
  on independently (`docs/agentic-coding-standards.md` §1.5,
  `docs/rust-ui-architecture-coding-standards.md` §8).

The tracking core is fourteen crates: eleven capability crates (`gungnir-core`,
`gungnir-coord`, `gungnir-filters`, `gungnir-association`, `gungnir-track`, `gungnir-rfs`,
`gungnir-track-fusion`, `gungnir-fusion-async`, `gungnir-allocation`, `gungnir-scenario`,
`gungnir-metrics`) and three verification crates (`gungnir-oracle`, `gungnir-testkit`,
`gungnir-fuzz`). Every other layer depends on them only through the two service facades
in §2, or through the primitives `gungnir-core` and `gungnir-coord` own and re-export
upward: `TrackId`, `TrackStatus`, `ResourceId`, the debug-only `assert_psd` helper, and
`Geodetic`. Those live in the lowest crates precisely so that `gungnir-model` can share
them with the core without the core ever depending on the model
(`docs/agentic-coding-standards.md` §1.2).

The tokio runtime is not owned by the tracking core. `gungnir-fusion-async` is the only
core crate that *uses* the runtime; the runtime itself is created by the host binary
(`gungnir-app` or `gungnir-node`) and handed to `LiveTrackingService::new` as a
`tokio::runtime::Handle`. See `docs/agentic-coding-standards.md` §2.2.

## §2 — The service layer: where "combined" actually happens

The appropriate seam between tracking and UI is not at the UI boundary; it is one layer
below it, because the UI should not need to know that "tracking" is eight cooperating
crates. Two facade crates absorb that, and both speak the canonical `gungnir-model`
types (§7.2):

- **`gungnir-tracking-service`** wraps the filter, association, track-lifecycle, RFS,
  track-fusion, and async multi-rate machinery behind one `TrackingService` trait: submit
  a `DetectionView`, `poll` to pull pipeline output into a snapshot, take a non-blocking
  `&[TrackView]` snapshot, ask whether the pipeline is healthy. Internally it owns the
  IMM bank, the associator, the track manager, and the out-of-sequence pipeline, and it
  projects the core's kinematic `Track` into `TrackView` (`project_track`); externally
  it exposes nothing `wgpu`- or `egui`-shaped. It is the "data source" the UI standards'
  `data → state → ui` rule expects the application state to depend on.
- **`gungnir-intercept-service`** wraps `gungnir-allocation` (Bellman/DP resource-to-track
  assignment) behind an `InterceptService` trait: given the current `TrackView`s and
  `ResourceView`s, produce a `PlanView`. It is its own crate rather than part of the
  tracking service because it answers a different question ("what should we do about
  these tracks") than tracking does ("where are these tracks"), and
  `docs/gungnir-capabilities.md` treats allocation as its own capability for the same
  reason. `gungnir-assessment` supplies the reward matrix through
  `DpInterceptService::plan_with_rewards`.

Both facades are still on the pure-computation side of the graph, but they are the
crates `gungnir-app` and `gungnir-node` link against, so neither binary carries an
eight-crate tracking dependency list of its own.

```rust
// gungnir-tracking-service/src/lib.rs
pub trait TrackingService: Send + Sync {
    fn submit_detection(&mut self, detection: DetectionView);
    fn poll(&mut self, now: MissionTime);
    fn tracks(&self) -> &[TrackView];
    fn is_healthy(&self) -> bool;
}

// gungnir-intercept-service/src/lib.rs
pub trait InterceptService: Send + Sync {
    fn plan(&mut self, now: MissionTime, tracks: &[TrackView], resources: &[ResourceView]) -> PlanView;
    fn is_healthy(&self) -> bool;
}
```

`gungnir-app::AppState` holds `Box<dyn TrackingService>` and `Box<dyn InterceptService>`,
the same trait-inversion pattern `docs/rust-3d-data-ecosystem-build-vs-adopt.md` §3.5
uses for `PointCloudFusion`: upper layers depend on the interface, never on `tokio` or
async internals. The traits are also the deployment seam: the embedded implementations
(`LiveTrackingService`, `DpInterceptService`) serve the disconnected profile, and
`gungnir-remote`'s `RemoteTrackingService`/`RemoteInterceptService` serve the connected
profiles without any UI change (§8).

Health is reported, never inferred: `LiveTrackingService::is_healthy` is false while
`gungnir_fusion_async::PIPELINE_IMPLEMENTED` is false, and `DpInterceptService::is_healthy`
turns false when the allocator returns `AllocationError::NotImplemented`. No panel can
claim a working tracker or planner before one exists.

## §3 — The data ecosystem: split by GPU-touching vs. not

`docs/rust-3d-data-ecosystem-build-vs-adopt.md` recommends splitting point-cloud
registration (GPU compute) from plain file I/O. The workspace keeps that split as two
crates:

- **`gungnir-data`** is file I/O only: `pointcloud`, `scientific`, `geospatial`, and
  `assets` modules, exactly the layout that document's §1.3 describes, plus the
  `DataStore` aggregate and a background loader thread. It has no `wgpu` dependency and
  is fully unit-testable.
- **`gungnir-data-fusion`** is the GPU point-cloud registration and fusion engine (that
  document's §3). It does not create a `wgpu::Device`; `GpuFusionEngine::new` borrows the
  device and queue that `gungnir-render::GpuContext` owns, so there is exactly one wgpu
  device in the process. It is separate from `gungnir-data` because it is the one
  data-layer crate that legitimately needs a GPU handle, and mixing GPU-dependent and
  GPU-free code in one crate would break the rule about keeping `cargo test`-only logic
  out of files that touch GPU types. Its `cpu_reference` module is both the test oracle
  for the GPU path and the runtime fallback on hardware without a suitable GPU
  (`GpuContext::new` returns `RenderError::NoAdapter` on such hosts).

The wgpu device here is a **compute-only** context. It is not the context the 3D viewport
draws with (§4 and §9). Fused point clouds that need to be displayed are read back to
CPU memory as a `PointBuffer` and uploaded to the viewport's OpenGL context; there is no
zero-copy path between the two, and the design accepts that cost because fusion output
changes far less often than the frame rate.

## §4 — UI layer: three crates, matching the original UI architecture doc almost exactly

`docs/rust-ui-architecture-coding-standards.md` §1 specifies an `app/ui/viewport3d/render`
layer split for a single binary. The workspace keeps the split but as separate crates,
so `gungnir-ui` and `gungnir-viewport3d` can each be compiled and, where GPU-free, tested
on their own:

- **`gungnir-render`** owns the single `wgpu` device and queue for the process
  (`GpuContext`, created headless with no surface). Its only consumer is
  `gungnir-data-fusion`'s compute pipeline. Its `egui_integration` module is an inactive
  placeholder for an egui-over-wgpu presentation path; presentation is handled by
  eframe's `glow` backend (below). It has no tracking-domain knowledge.
- **`gungnir-viewport3d`** owns the `three_d::Camera`, the static `Scene`, the track
  glyphs, and the two bridge modules from the 3D-data document (`streaming` for 3D Tiles
  and COPC point clouds, `scientific` for the VTK-to-mesh bridge). It renders live track
  state, which neither original design thread anticipated. three-d renders through
  OpenGL via `glow`, so this crate draws into the GL context eframe provides when the
  application runs with `eframe::Renderer::Glow`; it never touches the `wgpu` device.
  Until the three-d scene is attached to that context, `render` paints a top-down 2D
  projection of the same glyphs with egui's painter (pan and zoom included), so the
  desktop runs and shows live geometry without panicking. It depends on `gungnir-ui`
  for the shared theme palette only, so 2D and 3D colours agree.
- **`gungnir-ui`** is the 2D egui dashboard: track table, intercept panel, system
  health, alerts, plus `theme`, one file per panel, all reading `gungnir-model` views.
- **`gungnir-app`** is the `eframe::App` bootstrap, `AppState`, and the per-frame
  `update` tick. It is the one crate allowed to depend on everything. It is
  intentionally thin: wiring, not logic. It selects eframe's `glow` renderer, loads the
  config baseline named by `GUNGNIR_CONFIG`, chooses the embedded or remote backend,
  opens a live session and its journal, and runs the tick in §7.3.

The consequence of keeping three-d is that the desktop has **two GPU contexts**: an
OpenGL context for everything the operator sees, and a wgpu compute context for
point-cloud fusion. §9 records the version and platform implications.

## §5 — What "combined where appropriate" means in practice

Two places in this workspace intentionally blur the tracking/UI line, both justified:

1. **`gungnir-viewport3d::tracks`** takes `&[gungnir_model::TrackView]` directly, the
   service facades' public type, rather than going through a further translation crate.
   A 3D track symbol *is* a rendering concern, so a thin "track to glyph" function
   inside the viewport crate, depending on the canonical view and never on an internal
   tracking crate, avoids an extra crate whose only job would be re-exporting a struct.
2. **`gungnir-app::state`** is the one place `TrackingService`, `InterceptService`,
   `gungnir_data::DataStore`, the productization state, and UI state are all visible
   together, because something has to be the single source of truth for rendering
   (`docs/rust-ui-architecture-coding-standards.md` §2). It is a projection of durable
   mission state, not the system of record (§7.3, §8).

Everywhere else, the boundary from `docs/agentic-coding-standards.md` §1.1 holds: the
tracking core never imports UI types, and UI crates only see tracking output through the
two service traits and the model types they are typed with.

## §6 — Verification stack: unchanged, plus one addition

All six gates from `docs/agentic-workflow.md` apply unmodified to the tracking-core
crates; the workflow files that enforce them are under `.github/workflows/`
(`docs/gungnir-workspace-structure.md`). Two additions for the fused parts:

- `gungnir-data-fusion`'s GPU registration path gets the treatment
  `docs/rust-3d-data-ecosystem-build-vs-adopt.md` §3.6 specifies: a pure-CPU
  `cpu_reference` implementation validated with ordinary `cargo test`, and the GPU path
  validated separately on a GPU-enabled runner (`gpu-fusion.yml`, feature `gpu-tests`),
  never forced into `cargo test`. Neither the GPU path nor a test behind that feature
  exists yet (GAP-024), and the workflow is dormant until they do.
- The service-layer traits are the natural seam for integration tests that replay a
  `gungnir-scenario` scenario through the whole pipeline (detections, tracking service,
  intercept service, UI-consumable state), something none of the per-module oracle tests
  can catch because each stops at its own crate boundary.

`docs/verification-capability-table.md` §2 lists the verification each non-core layer
owes before it can be called done in the same sense as the tracking core; the rows whose
tests already exist are marked.

## §7 — The productization layer: the crates from `docs/gungnir-capabilities.md` §8

§1–§6 describe the workspace as first scaffolded. This section documents the
productization layer: the crate map from `docs/gungnir-capabilities.md` §8, the
additions an independent architecture review identified as missing between "a strong
tracking and fusion engine" and "a complete operational solution" (that document's §6
has the verdict), plus the five crates added on 2026-09-04 when the review's remaining
recommendations were scaffolded (`gungnir-interop`, `gungnir-analytics`,
`gungnir-resilience`, `gungnir-collab`, `gungnir-workflow`).

**Naming convention.** Every crate uses the `gungnir-` prefix. The six UI and data crates
first scaffolded as `fusion-data`, `fusion-data-fusion`, `fusion-render`,
`fusion-viewport3d`, `fusion-ui`, and `fusion-app` were renamed to their `gungnir-`
equivalents when the productization crates were added, so the whole workspace shares
one prefix. This was purely a naming change; nothing in §1–§6 changed because of it.

### §7.1 — Where the productization crates sit in the dependency graph

`docs/gungnir-capabilities.md` §5 groups the crates into Foundational plus five domains
(Sense/Ingest, Understand, Assess/Decide, Secure/Operate, Validate). The actual
`Cargo.toml` edges are:

```
gungnir-model ──► core, coord            (Foundational; every crate below uses it
    │                                     except geo, analytics, security. modelops
    │                                     joined them 2026-09-05, GAP-086)
    ├── gungnir-eventing ──► model
    │       ├── gungnir-store ──► model, eventing
    │       │       ├── gungnir-mission ──► model, eventing, config, store
    │       │       │                                              (dev: scenario)
    │       │       ├── gungnir-replay ──► model, eventing, store, time
    │       │       ├── gungnir-reporting ──► model, eventing, store, metrics,
    │       │       │                         identity                          (d)
    │       │       └── gungnir-resilience ──► model, eventing, store
    │       └── gungnir-collab ──► model, eventing, command, security   (dev: policy)
    ├── gungnir-config ──► model
    │       ├── gungnir-modelops ──► model, config
    │       └── gungnir-sensor-management ──► coord, model, config
    │
    ├── gungnir-time ──► model
    │       └── gungnir-ingest ──► model, time, tracking-service, interop   (i)
    ├── gungnir-interop ──► model                          (arrow)
    │
    ├── gungnir-identity ──► model
    ├── gungnir-identification ──► model
    ├── gungnir-ml ──► model, interop                        (q)
    │
    ├── gungnir-assessment ──► model
    ├── gungnir-policy ──► model, geo
    │       ├── gungnir-command ──► model, policy
    │       └── gungnir-decision ──► model, assessment, policy, analytics   (a)
    │
    ├── gungnir-observability ──► model
    │       └── gungnir-workflow ──► model, security, observability,
    │                                sensor-management (b), assessment      (e)
    └── gungnir-api ──► model, eventing, security, tracking-service,
                        intercept-service, analytics                     (f)

    gungnir-geo ──► coord, data
    gungnir-analytics ──► coord, data, geo, model, sensor-management        (c)
    gungnir-security ──► (no workspace dependencies)
```

Eight edges were added on 2026-09-05, the first three from the plan 11 design set, each
accepted by the engineering reviewer and recorded with its justification in
`docs/design/dependency-edges.md`; (h) came later with DN-24, (i) with DN-13's wiring and
(j) with the ASTERIX adapter, and none of the three was part of that review. A review
record covering all of them is in `dependency-edges.md` §7, **accepted by the owner as
engineering reviewer on 2026-09-06**. The direction and acyclicity of every
edge in the graph are checked by `gungnir-app/tests/dependency_graph.rs` on every
`cargo test`:

- **(a) `gungnir-decision` ──► `gungnir-analytics`.** The sensor-plan search evaluates
  coverage inside its own loop, once per candidate; a search that cannot evaluate its own
  candidates is not a search (`docs/design/DN-13-sensor-retasking.md`).
- **(b) `gungnir-workflow` ──► `gungnir-sensor-management`.** The tasking case shows the
  state of the tasks serving a requirement. Narrow by construction: it reads task state
  and never issues a command, so exactly one crate can talk to a sensor
  (`docs/design/DN-11-sensor-control-and-tasking.md`).
- **(f) `gungnir-api` ──► `gungnir-analytics`.** `GET /v2/coverage` publishes a
  `CoverageReport`, so the crate that defines the contract has to name the type. The same
  shape as its existing edges to the two service facades: the contract publishes
  productization types rather than defining a second set of its own, which is what stops
  the interface and the picture drifting apart (`docs/design/DN-12-coverage-and-gaps.md`
  §6, GAP-006).
- **(h) `gungnir-app` and `gungnir-node` ──► `gungnir-modelops`, and `gungnir-modelops`
  ──► `gungnir-model`** (2026-09-05, GAP-086, DN-24). Both binaries build the
  algorithm-baseline registry from the configuration and journal what the session opened
  with; before this the crate had **no dependents at all**. The third is the one DN-24 §5
  missed: identity is `gungnir_model::AlgorithmBaselineId`, and the types have to live in
  the model because `Provenance` -- which is there -- carries one once the pipeline applies
  it. `gungnir-modelops` was one of the four crates the graph above names as not using the
  model; it is not any more. Acyclic: it already depends on `gungnir-config`, which depends
  on `gungnir-model`, and nothing in that chain is a binary. The alternative to the third
  edge was keying the registry on two bare strings and assembling identity in the callers,
  which would put a second answer to "what is a baseline" inside the crate that owns them.
  Review: accepted by the owner 2026-09-06 (`dependency-edges.md` §7).
- **(j) `gungnir-ingest` ──► `gungnir-interop`** (2026-09-06, GAP-001). The ASTERIX
  radar adapter splits each datagram into data blocks and hands them to the Category 048
  and 034 codecs. §8.6 described this edge from the start ("`gungnir-ingest` adapters using
  `gungnir-interop` codecs"); no manifest carried it because no adapter existed. Acyclic:
  `gungnir-interop` depends on `gungnir-model` alone. Recorded in
  `docs/design/dependency-edges.md` §4a. Review: accepted by the owner 2026-09-06 (§7 there).
- **(i) `gungnir-app` ──► `gungnir-decision`** (2026-09-06, GAP-037, DN-13). The desktop
  runs the sensor re-tasking planner; before this no crate depended on `gungnir-decision`
  and its recommendation was reachable from nothing. Downward from the binary. Acyclic:
  `gungnir-decision` depends on model, assessment, policy and analytics, none a binary.
  Review: accepted by the owner 2026-09-06 (`dependency-edges.md` §7).
- **(k) `gungnir-node` ──► `gungnir-policy`, and (l) `gungnir-node` ──► `gungnir-geo`**
  (2026-09-06, GAP-028). The node runs the policy chain on every fresh plan and publishes
  `PlanEvaluated` with the engines that ran, so a desktop reading the node sees the same
  denial it would compute. §8's profile table anticipated (l) from the first draft: "a
  node needs `gungnir-geo` only if geofence policy is evaluated server-side", and now it
  is. Two things the edges deliberately do not bring: a queue (a decision is a person's
  act, nobody signs in to a node, and DN-23 §4 left the question open) and the authority
  engine (it asks who is asking). Downward from the binary; both crates are
  productization; acyclic. Review: accepted by the owner 2026-09-06
  (`dependency-edges.md` §7a).
- **(r) `gungnir-fusion-async` ──► `gungnir-core`, `gungnir-filters` and
  `gungnir-association`** (2026-09-06, GAP-011). The out-of-sequence pipeline predicts
  a track to a measurement's time, gates the measurement, assigns, and updates. Those
  are a motion model, a filter and an associator, and this crate names the three that
  own them rather than carrying its own. **All three are already beneath this crate**
  through `gungnir-track`, which depends on `gungnir-association`, which depends on
  `gungnir-filters`, which depends on `gungnir-core`: the graph gains no reach it did
  not have, and what changes is that the manifest now says what the code imports.
  Downward, acyclic, no new depth. The refused alternative was a filter inside
  `gungnir-fusion-async`, which would have put a second Kalman update in the workspace
  beside the signed one. Review: accepted by the owner 2026-09-06
  (`docs/design/dependency-edges.md` §12).
- **(p) `gungnir-node` ──► `gungnir-remote`** (2026-09-06, GAP-009). A peer link is a client
  link to a partner's node, and the client lives in `gungnir-remote`; the node binds one
  per peer as a machine under its own certificate. Downward from the binary; `gungnir-remote`
  depends on api, the two facades, security (t) and analytics, none a binary; acyclic.
  Review: accepted by the owner 2026-09-06 (`docs/design/dependency-edges.md` §11).
- **(t) `gungnir-remote` ──► `gungnir-security`** (2026-09-06, GAP-060, D-29). `identity.rs`
  builds a host's TLS identity from that host's own `KeyProvider`, so the private half never
  leaves custody. `gungnir-remote` carries it because it is the only crate both binaries
  depend on at runtime that already holds `rustls` and owns `LinkTls`. The refused
  alternative was `gungnir-api`, the better home on layering grounds, rejected because the
  desktop holds it as a dev-dependency only. Downward, Deployment to Productization;
  `gungnir-security` has no `gungnir-*` dependency, so no cycle is reachable. **Recorded
  late**: in a manifest from 2026-09-06, in this table from 2026-09-07
  (`docs/design/dependency-edges.md` §14).
- **(q) `gungnir-ml` ──► `gungnir-model` and `gungnir-interop`** (2026-09-06, GAP-077,
  GAP-079). The crate `docs/ml/architecture.md` §1 drew as `gungnir-model ──► gungnir-ml`
  exists: the `Model` and `FeatureExtractor` traits, a fake for the consumers' tests,
  and the dataset extraction; the second edge is the dataset schema, which is a catalogue
  entry so a dataset and the wire format share one definition (`data-pipeline.md` §2).
  No inference runtime: that sign-off stays deferred (§3), and `ModelSet::load` refuses
  with the reason rather than loading nothing quietly. Nothing depends on the crate;
  the consumers take model output through their existing traits when GAP-080 promotes
  one. Both edges downward into foundational crates; acyclic. Review: accepted by the
  owner 2026-09-06 (`docs/design/dependency-edges.md` §11).
- **(n) `gungnir-app` ──► `gungnir-identification`, and (o) `gungnir-app` ──►
  `gungnir-identity`** (2026-09-06, GAP-010, GAP-019, GAP-025). The desktop constructs
  the evidence-fusion engine because it is the first host of an evidence source (the AIS
  adapter), and the identity resolver because it is the host that holds several sessions
  of its own journal. Both crates are productization and depend on model and store alone;
  downward from the binary; acyclic. The node takes neither edge: it had no tracks to
  fuse evidence into or to correlate across sessions until GAP-011. Review: accepted by
  the owner 2026-09-06 (`docs/design/dependency-edges.md` §10). **That reason expired
  later the same day**: GAP-011 closed and the node has tracks, so the two edges are
  still absent by choice rather than by necessity, and adding either is now a decision
  somebody makes rather than one the architecture makes for them.
- **(g) `gungnir-node` ──► `gungnir-analytics`.** The node computes the coverage report it
  serves. A binary depending on the productization crate it hosts is the same edge
  `gungnir-app` already has; the alternative was for the node to serve a report somebody
  else computed, and there is nobody else. Acyclic: `gungnir-analytics` depends on coord,
  data, geo, model and sensor-management, none of which is a binary.
- **(c) `gungnir-analytics` ──► `gungnir-sensor-management`, and to `gungnir-model`.**
  `coverage_from_registry` builds coverage input from live sensor records. This was the
  edge the design set flagged as weakest, because it moves analytics from a geometry
  library toward a central one; the reviewer accepted it, and the pure
  `combined_coverage` still takes sensor volumes rather than a registry, so withdrawing
  the edge would move one function and nothing else
  (`docs/design/DN-12-coverage-and-gaps.md`).

The remaining two landed later the same day:

- **(d) `gungnir-reporting` ──► `gungnir-identity`.** An order of battle is a list of
  entities, not of tracks, and the lineage is what makes an entry defensible when an
  analyst asks why two sightings are one thing
  (`docs/design/DN-19-order-of-battle.md`).
- **(e) `gungnir-workflow` ──► `gungnir-assessment`.** The warning rule reads the
  prediction to know when an obligation triggers. It is a rule about an obligation rather
  than a detector, so it belongs in a crate with tests rather than in a binary
  (`docs/design/DN-03-warning.md`).

**All five approved edges are now in manifests and drawn here.** The graph is acyclic
across all 154 crate-to-crate edges, checked on 2026-09-05. Analytics is no longer
independent of `gungnir-model`; the other four crates keep the layer they had.

`SessionId` moved from `gungnir-store` to `gungnir-model` on 2026-09-05, with the store
re-exporting it. Six crates share it, and the review case in `gungnir-workflow`
(`docs/design/DN-20-after-action-review.md`) would otherwise have needed a sixth edge to
reach it. That is `agentic-coding-standards.md` §1.2 working as intended: the type moved
down rather than the dependency reaching across.

No productization crate creates a cycle back into the tracking core: everything here
depends on the core, on `gungnir-model`, or on the two service facades, never the
reverse, preserving the one-way rule from `docs/agentic-coding-standards.md` §1.1.

### §7.2 — Why `gungnir-model` had to come first, and the migration that followed

The canonical data model was framed as *Critical* and foundational
(`docs/gungnir-capabilities.md` §5.1) rather than one gap among many: `TrackView`,
`DetectionView`, `ResourceView`, `PlanView`, `SystemHealth`, and the `TrackingEvent`,
`InterceptEvent`, `IngestEvent`, and `CommandEvent` schema are what let
`gungnir-ingest`, `gungnir-identity`, `gungnir-store`, and `gungnir-api` agree on
"what a track is" without each inventing its own shape.

The migration the first scaffold deferred is done: both service facades now depend on
`gungnir-model` and re-export its views as their public contract. The core's kinematic
`gungnir_track::Track` is projected into `TrackView` by
`gungnir_tracking_service::project_track`, and the canonical `DetectionView` is reduced
to the core's `Detection` by `to_core_detection`. The identifier primitives the core and
the model share (`TrackId`, `TrackStatus`, `ResourceId`) moved down into `gungnir-core`
so that neither crate redefines the other's types, and `Geodetic` comes from
`gungnir-coord` for the same reason. `gungnir-ui`, `gungnir-viewport3d`,
`gungnir-policy`, `gungnir-command`, `gungnir-decision`, `gungnir-assessment`, and
`gungnir-ingest` all compile against the model types only.

### §7.3 — What's wired into `gungnir-app` today, and what isn't

`gungnir-app::AppState` holds the two service facades (embedded or remote, §8), the
`DataStore`, the `ViewportState`, and from the productization layer: the applied
`ConfigBaseline`, a live `Mission` opened at launch, an `InProcessBus` event bus, a
`FileEventJournal` subscribed to that bus, a `WallClockAuthority`, an `IngestGateway`
with an allow-list authenticator built from the configured sensors, and the reported
`SystemHealth`. The per-frame tick runs, in order: ingest (adapters through validation
and quarantine into the tracking service), `poll`, planning against the snapshot,
publication of `PlanProposed` when the plan changes, health from what the services
report, and journaling of every envelope the bus carried. The viewport is called every
frame with the current tracks and plan.

Since GAP-038 the tick also runs the approval gate: every plan the intercept service
proposes is evaluated by a `PolicyChain` of the readiness-and-geofence, control-status
and authority engines, and a plan that clears it is queued in an
`InMemoryApprovalWorkflow` for a human rather than becoming actionable. A denied plan is
counted and its reason kept, so PN-06 can say why the queue is empty. `AppState` holds
the workflow and that denial history; `src/decisions.rs` is the whole of the wiring.

Nothing in this build ever reaches the queue, and that is a chain of correct steps: the
tracking pipeline is not implemented, so there are no tracks; with no tracks the
allocator returns an empty plan; an empty plan is denied before it can be queued. The
queue is therefore permanently calm, which is the most misleading screen in the product
and is why PN-06 carries a required explanation rather than a count alone.

Since GAP-071 the desktop also reads its journal twice more: `gungnir-replay` for
PN-12's cursor and `gungnir-reporting` for PN-13's counts, and `gungnir-config`'s
`ConfigStore` for PN-14's validate and apply. None of the three adds a source of truth;
they add readers of the journal and the baseline the desktop already holds.

Still not called from the tick: `gungnir-identity` and
`gungnir-identification` (global identity and classification on the snapshot),
`gungnir-assessment` (real rewards instead of the uniform matrix), `gungnir-security`
(operator login and audit), `gungnir-workflow` (role-based layouts), `gungnir-replay`
and `gungnir-reporting` (against the journal), and `gungnir-analytics`. Each has a
working in-memory implementation and tests, so wiring is the remaining step;
`AppState`'s doc comment lists where each plugs in.

`AppState` is a local, render-friendly projection of mission state. The system of record
is the `gungnir-store` journal: on the desktop in the disconnected profile, on the
service node in the connected profiles (§8).

### §7.4 — Priorities carried over unchanged

The Critical/High/Medium/Lower priorities from `docs/gungnir-capabilities.md` §8 are
reproduced in the workspace `Cargo.toml` member-list comments so the ordering is visible
without cross-referencing a second document. The suggested implementation order, which
follows those priorities and the §7.1 graph, is in the workspace `README.md`. It is a
recommendation, not an enforced build order: unlike §1.1's dependency rule, nothing
prevents building a Medium-priority crate before a High-priority one if a deployment's
requirements call for it.

## §8 — Deployment topology: one crate set, three profiles

Gungnir is a node in a system of systems that spans cloud, on-prem, and disconnected
environments. The decision is **embedded or remote, same crates**: the services layer
is compiled into the desktop application for standalone use, and the same services are
compiled into a headless service-node binary for connected and multi-user use. The
desktop switches between the two through the §2 traits.

### §8.1 — Binaries

| Binary | Status | Hosts | Depends on |
|---|---|---|---|
| `gungnir-app` | Scaffolded and runnable | The operator interface, and in the disconnected profile the whole services layer | Everything (§4) |
| `gungnir-node` | Scaffolded and runnable | The services layer and durable mission state for one or more desktops, behind `gungnir-api` | Both service facades and the productization layer; no UI or rendering crate, no `three-d`, no `wgpu` |

`gungnir-node` targets **Linux x86_64 containers** (`deploy/node/Dockerfile`,
`deploy/README.md`); `ci.yml` builds it for that target on every change. It runs the
same tick loop as the desktop, journals to the configured data directory, logs health,
and reports at startup that the API transport is not implemented yet.

### §8.2 — Profiles

| Profile | Services layer | System of record | Users | Network |
|---|---|---|---|---|
| Disconnected desktop | Embedded in `gungnir-app`: `LiveTrackingService`, `DpInterceptService`, `FileEventJournal`, local `ConfigBaseline`, local ingest adapters | The desktop's local `gungnir-store` journal | One operator | None required. `gungnir-api` may listen on loopback for local tooling. |
| On-prem connected | One or more `gungnir-node` instances on the local network | The node | Several operators, supervisors, analysts sharing one mission | LAN. Sensors may feed the node directly or via edge ingest. |
| Cloud connected | The same `gungnir-node` binary hosted in the cloud | The node | As on-prem, plus remote clients and peer C2 systems | WAN. Higher latency budget; stricter security posture. |

The profile is selected by `BackendConfig` in the desktop's config baseline. In the
connected profiles the desktop runs `gungnir-remote`'s `RemoteTrackingService` and
`RemoteInterceptService`, which subscribe to the node's event stream, keep a local
projection, and forward detections and decisions through `gungnir-api`. The v1 contract
(`docs/gungnir-api-v1.md`) defines the snapshot, event stream, detection submission, and
plan-decision endpoints; the transport (JSON over HTTP plus a WebSocket event stream) is
decided but not yet in the workspace, so `gungnir_remote::connect` reports
`TransportNotImplemented` and the desktop falls back to embedded with an alert.

### §8.3 — Where each crate group runs

| Crate group | Disconnected desktop | Service node |
|---|---|---|
| Tracking core, service facades | Yes (embedded) | Yes |
| `gungnir-model`, `gungnir-eventing`, `gungnir-config`, `gungnir-mission` | Yes | Yes |
| `gungnir-store` | Local journal | Authoritative journal |
| `gungnir-time`, `gungnir-ingest`, `gungnir-sensor-management`, `gungnir-interop` | Yes, for locally attached sensors | Yes, for sensors feeding the node |
| `gungnir-identity`, `gungnir-identification`, `gungnir-assessment`, `gungnir-decision`, `gungnir-modelops` | Yes | Yes |
| `gungnir-policy`, `gungnir-command` | Yes, local operator approval | Yes; the node is the arbiter when several operators share a mission |
| `gungnir-collab` | Projection side: applies the node's envelopes | Authoritative side: arbitrates conflicting decisions |
| `gungnir-resilience` | Store-and-forward while disconnected; reconciliation on reconnect | Accepts forwarded envelopes; reconciles |
| `gungnir-security` | Operator login and local audit log | Authentication and authorization for every API caller; central audit log |
| `gungnir-api` | Optional loopback | Yes, the node's only external surface |
| `gungnir-observability` | Local health panel | Node health endpoint, watchdogs |
| `gungnir-workflow` | Yes (role layouts, alert lifecycle) | Alert lifecycle state, shared |
| `gungnir-replay`, `gungnir-reporting` | Against the local journal | Against the node's journal |
| `gungnir-data`, `gungnir-data-fusion`, `gungnir-geo`, `gungnir-analytics` | Yes | Not required. Point-cloud fusion is a desktop capability; a node needs `gungnir-geo` only if geofence policy is evaluated server-side. |
| `gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`, `gungnir-app` | Yes | Never |

### §8.4 — Degraded connectivity

A connected desktop that loses its node must keep operating
(`docs/gungnir-capabilities.md` §5.6):

- The desktop falls back to the embedded backends and continues journaling locally.
  Today this happens at startup when the remote endpoint is unreachable; mid-session
  failover needs the transport's heartbeat.
- Detections, operator decisions, and audit entries recorded while disconnected are
  store-and-forward: `RemoteTrackingService` queues detections (bounded by
  `OUTBOX_CAPACITY`, oldest dropped and counted), and `gungnir_resilience::StoreAndForwardQueue`
  does the same for envelopes.
- Reconciliation: `gungnir_resilience::reconcile` merges the local and node journals by
  mission time, drops exact duplicates, and *reports* conflicting decisions on the same
  plan. The rule that resolves them is `gungnir_collab::RoleRankArbiter` (higher role
  wins, earlier decision wins on a tie) and is a working default, not yet locked
  (§10).

### §8.5 — Security posture by profile

`gungnir-security` is the same crate in every profile. The disconnected desktop needs
operator authentication and a local audit log. A node additionally needs authentication
and role-based authorization for every `gungnir-api` caller, encryption in transit, and
signed configuration baselines. A cloud node additionally needs encryption at rest for
the journal and key management that does not live on the same host. The role-to-action
matrix (`gungnir_security::authz::role_permits`), the audit log, and the action names
exist. The credential mechanism is **decided**, not open: D-02 settled it on 2026-09-04 as
mutual TLS for machine identities and short-lived signed tokens for operator sessions.
Neither is built -- the tokens are GAP-057 and the TLS is GAP-060, which waits on GAP-084's
key custody (DN-22) -- so as of GAP-041 a node **serves loopback only and refuses every
write path**, because it can protect neither the channel nor the caller's identity.
(This sentence said the mechanism was undecided until 2026-09-05, four days after D-02
resolved it.)

### §8.6 — Interoperability with peer systems

Gungnir exchanges data with the rest of the system of systems in two ways: peer C2
systems, analytics tools, and enterprise services integrate through `gungnir-api`
(`docs/gungnir-api-v1.md`); and sensor and surveillance data enters through
`gungnir-ingest` adapters using `gungnir-interop` codecs, including the ASTERIX and
STANAG formats the tracking core's interop schema row already names. `gungnir-interop`
also owns the schema catalog and the Arrow form of detections; export in industry
formats is a `gungnir-reporting` concern using the same codecs.

### §8.7 — Platform assumptions

- The desktop targets Windows 11 with a discrete NVIDIA GPU, per
  `docs/rust-ui-tech-stack-summary.md`. Hosts without a suitable GPU use the CPU
  registration fallback in `gungnir-data-fusion`.
- The service node targets Linux x86_64 in a container (confirmed 2026-09-04).
  `rust-toolchain.toml` pins the toolchain both targets build with.

## §9 — Tested version set and GPU contexts

The workspace `Cargo.toml` is the single place versions are pinned; every crate inherits
from it, and `rust-toolchain.toml` pins the Rust 1.98 toolchain. The set as pinned on
2026-09-04, verified with `cargo check --workspace --all-targets`:

| Dependency | Pin | Role | Notes |
|---|---|---|---|
| `nalgebra` | 0.33 (`serde-serialize`) | All linear algebra; model views serialize their state vectors | |
| `tokio` | 1 (`rt-multi-thread`, `sync`, `macros`, `time`, `signal`, `net`) | Async runtime for `gungnir-fusion-async`; created by the host binary | `net` added 2026-09-05 for the v2 transport's listener (GAP-041) |
| `serde`, `serde_json` | 1 | Serialization; JSON config baselines and the JSON-lines journal | |
| `rand`, `rand_distr` | 0.8, 0.4 | Deterministic sampling | |
| `proptest`, `criterion` | 1, 0.5 | Property tests, benchmarks | |
| `arrow` | 53 | Columnar interop (`gungnir-interop`) | |
| `tracing`, `tracing-subscriber` | 0.1, 0.3 | Diagnostics | |
| `thiserror` | 1 | Error enums in every crate | |
| `crossbeam-channel` | 0.5 | Sync channels across the render boundary and the event bus | |
| `eframe`, `egui` | 0.29 | Window shell and 2D panels | Runs with `Renderer::Glow`; the optional `wgpu` feature is off |
| `three-d` | 0.18 | 3D viewport | Renders through OpenGL via `glow`, not through `wgpu`. Taken without its default `window` feature since 2026-09-07: the viewport draws into eframe's glow context and never opens a three-d window, and the feature carried a second `winit` (0.28) with `glutin` and `instant` (RUSTSEC-2024-0384). Its `cgmath` (RUSTSEC-2026-0196, unmaintained) is not a feature and is still a dependency of three-d 0.19, so it stays until three-d drops it or is replaced; 0.19 itself is ruled out by its move to `glow` 0.17 and `egui` 0.34, since the shared context needs eframe 0.29's `glow` 0.14 |
| `wgpu` | 22 | Compute device in `gungnir-render`, used by `gungnir-data-fusion` | The same line eframe 0.29's optional wgpu feature uses, so enabling it never yields two versions |
| `gltf`, `vtkio`, `las` | 1, 0.6, 0.9 | Asset, VTK, and LAS/LAZ I/O | `vtkio` is taken without its default `xml` and `compression` features since 2026-09-07: its `quick-xml` 0.22 carries RUSTSEC-2026-0194/0195 and its `lz4_flex` 0.7 RUSTSEC-2026-0041, and no vtkio release is on a patched line (the 0.7.0 candidates pin quick-xml 0.36; the fix is 0.41). Only legacy `.vtk` loads; `gungnir-data` refuses an XML extension by name. It still pulls `nom` 3, which rustc flags as future-incompatible. Restore the features when a release moves |
| `copc-rs`, `pasture-core`, `pasture-io` | not pinned | COPC and point-buffer I/O named in the 3D-data plan | Young crates; verify on crates.io and pin in the PR that first uses them |
| `tiff` | 0.11 | GeoTIFF DEM raster decoding in `gungnir-data`; the geo tags are read by this project (GAP-023, D-25, 2026-09-06) | Replaces the plan's `oxigdal-3d`, which was never pinned |
| `uuid` | not used | | `gungnir-model` uses a `u128` newtype instead; adopting `uuid` remains optional |

**Two GPU contexts.** The operator interface (egui panels and the three-d viewport) is
drawn through one OpenGL context owned by eframe's `glow` backend; on Windows this is the
vendor OpenGL driver, which the RTX-class GPU accelerates. Point-cloud fusion runs on a
separate, headless `wgpu` compute device owned by `gungnir-render::GpuContext`; on
Windows `wgpu` selects DirectX 12 or Vulkan. There is no buffer sharing between the two;
fusion results cross to the viewport through a CPU `PointBuffer` (§3). The earlier claim
that one shared `wgpu` device served rendering and compute is retired; it was only true
of a `wgpu`-native 3D layer, which this project did not adopt.

## §10 — Resolved defects and open decisions

### Resolved on 2026-09-04

1. `gungnir-allocation` now depends on `gungnir-core`, which owns `TrackId`,
   `TrackStatus`, and `ResourceId`; `gungnir-track` and `gungnir-model` re-export them.
2. `assert_psd` moved from `gungnir-testkit` (a dev-dependency) to
   `gungnir_core::numeric`, with a Cholesky-based check and tests; `gungnir-testkit`
   now depends on no workspace crate and provides the shared `proptest` strategies.
3. `wgpu` re-pinned to 22; eframe runs on `glow` with its `wgpu` feature off.
4. `gungnir_eventing::InProcessBus` is a real broadcast: one channel per subscriber,
   bus-wide sequence numbers, stamped `Envelope`s, pruning of dropped subscribers, and
   tests for ordering and fan-out.
5. `gungnir_model::DetectionView` exists, with source and receipt time and provenance;
   `IngestEvent` and `CommandEvent` were added to the event schema.
6. Both service facades speak `gungnir-model` types (§7.2).
7. `LiveTrackingService::new` wires real channels and spawns the ingest task; the task
   drains and logs until the pipeline exists, and health says so. Both binaries start.
8. The viewport is wired every frame with a non-panicking 2D fallback and owns the
   `three_d::Camera`.
9. `thiserror`, `crossbeam-channel`, `serde_json`, `rand_distr`, and
   `tracing-subscriber` are recorded as approved stack additions
   (`docs/agentic-coding-standards.md` §2.9).
10. `CONTRIBUTING.md`, `CLAUDE.md`, the CI workflows, `benches/`, `deny.toml`,
    `rust-toolchain.toml`, `deploy/`, and the `gungnir-node` and `gungnir-remote` crates
    exist.
11. Journal reconciliation: `gungnir-resilience` merges and reports conflicts;
    `gungnir-collab` provides the default arbitration rule.
12. Remote transport: JSON over HTTP plus a WebSocket event stream, gRPC later
    (`docs/gungnir-api-v1.md`).
13. Multi-user authority: `RoleRankArbiter` (higher role wins, earlier wins on a tie).
14. Service-node platform: Linux x86_64 containers.
15. The review's remaining recommendations are scaffolded as crates
    (`gungnir-interop`, `gungnir-analytics`, `gungnir-resilience`, `gungnir-collab`,
    `gungnir-workflow`) or documents (`docs/performance-budgets.md`,
    `docs/release-governance.md`).
16. Scope lock (D-01 in `docs/mission/gap-analysis/decisions-needed.md`): everything
    ships in one release; every scaffolded crate and all 56 capabilities through
    increment 4 are release content.
17. Credential mechanism (D-02): mutual TLS for machine identities (desktops, nodes,
    peers, sensors); short-lived signed tokens for operator sessions issued by the
    node or a local identity provider; local accounts on the disconnected desktop.
    Implementation is GAP-057.
18. Reconciliation and arbitration rules locked as the defaults (D-03): mission-time
    merge, duplicates dropped, conflicts reported; higher role wins, earlier wins a tie.
19. Performance budgets confirmed as provisional gates (D-04); fsync per profile: the
    node journal fsyncs every envelope, desktop journals are buffered with fsync on
    session save and every 5 s. The harnesses remain open (GAP-056).
20. `uuid` adopted, UUID v7, for `GlobalEntityId` (D-11; `docs/agentic-coding-standards.md`
    §2.9); the manifest change is GAP-069.
21. Anomaly detection lives as pure functions in `gungnir-analytics`; the binaries
    raise alerts through `gungnir-observability`; no new dependency edge (D-13).
22. Hosting (D-10, amended 2026-09-07): GitHub at
    `https://github.com/WayneRoessling/gungnir`, GitHub Actions, and the GitHub
    Container Registry. The eight workflows under `.github/workflows/` stay where they
    are and are not ported; `gpu-fusion.yml` still needs a self-hosted runner labelled
    `gpu`, which was a property of that workflow and not of the forge. GAP-061 begins
    with the initial commit, because this workspace is not yet under version control.
    The 2026-09-04 resolution this replaces was a self-hosted GitLab with its own
    runners and container registry.
23. Vulnerability-response objective (D-10 follow-up): critical within 7 days of
    publication, high within 30 days, lower at the next release
    (`docs/release-governance.md`).
24. Measure targets (D-16): every proposed value in `docs/mission/measures.md`, the
    measures catalogue, and the to-be-agreed verification rows was confirmed or set by
    the owner; three stay deferred to plans 06, 07, and 08 with sign-off there.
25. Docking and multi-window (D-17): adopted where appropriate; panels dock within
    each role's layout and the viewport, approval queue, and replay timeline may be
    detached to a second window (`docs/ux/information-architecture.md` §1); GAP-075.

### Resolved on 2026-09-05

26. API transport stack (D-18): `axum` with its `ws` feature for the node's HTTP surface
    and the WebSocket event stream, `tokio-tungstenite` for the desktop's WebSocket
    client, `reqwest` for its HTTP client, and `rustls`, `tokio-rustls`, and
    rustls's own `pki_types::pem` reader (which replaced `rustls-pemfile` on 2026-09-07)
    for the mutual TLS item 17 requires, with `tower-http` for request
    tracing and body limits at that boundary. Recorded in
    `docs/agentic-coding-standards.md` §2.9, which carries the reasoning: rustls rather
    than `native-tls` so no system OpenSSL is needed and one TLS implementation appears in
    the SBOM, and pins chosen so exactly one `tungstenite` and one `tower-http` link.
    Resolution was checked on the pinned toolchain. The entries are in the workspace
    manifest and no member depends on them yet; building the transport is GAP-041, and
    gRPC remains a later question rather than a decided one.

27. **Coordinate transforms** (2026-09-05). `gungnir-coord` implements all four
    transforms: the closed form for geodetic-to-ECEF and Heikkinen's closed-form
    (Ferrari) solution for the inverse, chosen over a Bowring iteration because it is
    exact at every altitude and stable on the polar axis that Scenario 5 drives
    through. Gated against pymap3d 3.2.0 with a worst disagreement of 2.1e-9 m against
    the row's 1e-6 m criterion, plus `proptest` round-trip and finiteness invariants
    over a domain that includes both poles and the antimeridian. NED is a free-function
    relabelling so the oracle-comparable trait surface stays the four transforms the row
    names an oracle for.
28. **Motion models** (2026-09-05). `gungnir-core` implements CV, CA, and CT in the
    block state ordering `gungnir_track::Track` already documents, with `Q` as the
    continuous white-noise form the spectral-density field names imply. Gated by 322
    element-wise comparisons against filterpy, Stone Soup, and scipy's `expm`/Van Loan;
    the worst disagreement against the row's named closed-form oracle is 1.4e-14, and
    the larger 2e-11 to 4e-11 figures in the fixture are the *oracles'* own numerical
    noise. Two limits are recorded rather than smoothed over: MATLAB's `initcvkf`,
    `initcakf`, and `initctekf` were **not** run, because MATLAB is not installed, and
    Stone Soup's `KnownTurnRate` divides by the turn rate, so it returns NaN at
    `omega == 0` and loses precision near it. `gungnir-core` evaluates the same terms
    through a small-angle series specifically so that it does not.
29. **The five-scenario generator** (2026-09-05). `gungnir-scenario` produces Scenarios
    1 to 5 with truth from the `gungnir-core` motion models and observations from the
    procedure in `docs/test-tracks/sensor-models.md`, in the frame and units of
    `docs/test-tracks/data-format.md`. Both `scenario` verification rows are gated: the
    statistical self-check agrees with the configured Pd and clutter rate to within
    0.90σ against a 2σ criterion over 15,552 opportunities and 2,880 scans, and the
    round-trip fidelity row replays an exported timeline through `RecordedFeedAdapter`
    with exact content and order. It is **not** a port of
    `docs/test-tracks/tools/gen_tracks.py`: the TT-01 to TT-10 YAML composition engine
    and Python's Mersenne Twister are not implemented, so a TT set still comes only
    from the Python generator. GAP-016 records which half is done.
30. **The desktop tick harness** (2026-09-05). `gungnir-app` gained a library target so
    that `update::tick` can be reached at all, which `docs/performance-budgets.md`
    requires; `main.rs` is still bootstrap only and the two modules exist once.
    `gungnir-app/benches/app_tick.rs` measures four desktop budgets from generated
    scenario output through the real ingest gateway, and
    `gungnir-app/tests/frame_budgets.rs` carries the assertions. GAP-056 records what
    the numbers currently mean; GAP-085 records the one budget that fails.

31. **Docking crate (D-19)** (2026-09-05). `egui_tiles`, pinned to 0.10 because that is
    the line depending on `egui ^0.29`, the workspace pin; 0.11 moved to `egui ^0.30` and
    would link a second `egui`. Checked with `cargo tree -d` against `eframe`/`egui` 0.29
    on the pinned toolchain: no duplicate `egui`, `eframe`, `emath`, or `epaint`. Chosen
    over `egui_dock` because a tree of tabs and splits maps onto the per-role panel lists
    `WorkspaceLayout::for_role` already produces. This closes the last open
    `docs/agentic-coding-standards.md` §2.9 question. The entry is in the workspace
    manifest and no member depends on it yet; it enters a member manifest under GAP-075.

32. **Journal durability (D-04 implemented, GAP-085)** (2026-09-05). The tick harness
    found the one desktop budget whose whole path was implemented failing at six to
    eight times its allowance, and the owner signed off the fix the same day.
    `gungnir-store` now carries an explicit `DurabilityPolicy` rather than one
    hard-coded behaviour, because D-04 (item 19) settles the question *per profile*:
    `SyncEveryEnvelope` for the service node, `Buffered { fsync_interval: 5s }` for the
    desktop. The session file is held open behind a `BufWriter` instead of being
    reopened per envelope; `gungnir-app` calls `EventJournal::sync_if_due` every tick so
    the 5 s is a wall-clock bound and not merely a check that happens to run when the
    next envelope arrives. The default is the strong profile, so a call site that does
    not choose does not silently get the weaker one.

    Two things are worth keeping in mind about the result. First, 50 envelopes went from
    5.7 ms to 157 µs in release, and the per-frame `update()` p99 improved with it
    (533 µs to 40.8 µs in debug), because the journal was that path's dominant term.
    Second, the old code called `Write::flush` on a `std::fs::File`, which does nothing:
    it is not an fsync. The node profile was therefore not meeting the durability D-04
    asked of it either, and that -- not only the speed -- is why the fix is a policy
    rather than just a buffer.

33. **The first of the tracking math** (2026-09-05). Four more §1 rows are gates.
    `gungnir-filters` has the standard-form linear Kalman filter, in Joseph form so the
    covariance stays symmetric and PSD under rounding rather than drifting indefinite,
    agreeing with `filterpy` to 9.1e-13 in state and 6.1e-14 in covariance Frobenius
    norm across 155 filter steps -- compared after *every* predict and every update, so
    a right answer reached by a wrong path fails. `gungnir-association` has the
    Jonker-Volgenant assignment solver (rectangular directly, not padded to square),
    global nearest-neighbour over it, and ellipsoidal chi-square gating computed through
    a Cholesky solve rather than an explicit inverse.

    Two things about the association rows are worth recording. The Hungarian row's
    criterion says "assignment only if not tied", and that qualifier is load-bearing:
    the fixture decides by brute force over all permutations whether each case has a
    unique optimum, and the pairing is compared only where it does. And both
    `solve_assignment` and `Associator::associate` were changed from the scaffold's
    infallible signatures to return a `Result`: the cost matrix arrives from sensor
    data, `gungnir-fuzz` exists to push malformed matrices at it, and returning "no
    associations" for a matrix full of NaN would be indistinguishable from "these tracks
    matched nothing". Nothing depended on the old signatures. The fuzz target now drives
    the real solver and asserts its postconditions rather than only that it does not
    panic.

34. **Track lifecycle** (2026-09-05). `gungnir-track` has the init/confirm/coast/delete
    state machine, agreeing with Stone Soup on all 24 confirm and delete step indices
    across 12 hit/miss sequences. Two things came out of building it.

    **The scaffold's confirmation rule was wrong.** `confirm_threshold` was documented
    as "cycles of *consecutive* hits". Stone Soup's `MultiMeasurementInitiator` confirms
    on a *cumulative* count of updates, and consecutive counting is also the worse
    rule -- at Pd 0.8 a single miss would restart establishment, so a real target takes
    far longer to confirm than it should. The behaviour follows the oracle and the
    comment was moved to match, not the other way round.

    **The first version of the fixture was wrong, and the case list is what caught it.**
    `UpdateTimeStepsDeleter.check_for_deletion` tests
    `isinstance(state, Update) and state.hypothesis`, so an update built with
    `hypothesis=None` is not counted as an update at all and the deleter falls through
    to counting distinct timestamps. Driven that way it reported a track deleted while
    it was being hit on every step. The two cases that exposed it,
    `dense_hits_never_deleted` and `hit_resets_the_miss_run`, are kept for that reason.
    The `>=` comparison the implementation uses turned out to be right, but the
    evidence for it before the fix was not.

    `TrackManager::step` now returns a `StepOutcome` rather than nothing: it carries the
    step index of each transition, which is what the row compares, and it reports ids
    the caller named that the manager does not know, or named as both a hit and a miss,
    instead of ignoring them.

35. **Tracking metrics** (2026-09-05, GAP-048). `gungnir-metrics` computes MOTA, MOTP,
    purity and fragmentation, agreeing with `py-motmetrics` to 2.2e-16 against a 1e-3
    criterion across 13 sequences, with every underlying count -- objects, matches,
    identity switches, misses, false positives -- matching exactly. The counts are
    compared as well as the ratios on purpose: two wrong counts can sum to a right
    MOTA.

    **The scaffold's signature could not express the question.** `compute_metrics` took
    two flat `&[Track]` slices, but MOTA, MOTP and fragmentation are sequence metrics
    and an identity switch is not definable on a single snapshot. It now takes a frame
    per cycle on each side plus a `MetricsConfig` carrying the gate distance, which is
    a required input to the CLEAR MOT definition and previously had nowhere to live,
    and returns a `Result` because two sequences of different lengths describe
    different runs.

    **One new dependency edge: `gungnir-metrics` to `gungnir-association`,** drawn in
    the graph above and in the edge table. The argument for it: the middle stage of
    CLEAR MOT matching *is* a minimum-cost assignment, and it is the same one the
    oracle performs -- `motmetrics` calls `scipy.optimize.linear_sum_assignment`, and
    `gungnir_association::solve_assignment` is already gated against exactly that. The
    alternatives were worse: re-implementing an assignment solver inside
    `gungnir-metrics` duplicates a type and an algorithm the workspace already owns
    (agentic-coding-standards.md §1.2), and re-exporting it through `gungnir-track`
    would widen that crate's surface to dodge an edge rather than because anything
    there needs it. The edge is downward within the tracking core -- association sits
    below metrics in the existing ordering -- so it respects the one-way rule, and the
    graph remains acyclic (checked: 135 normal edges, no cycle).

    One thing worth keeping about the oracle: `motmetrics` divides through
    `quiet_divide`, so an empty sequence has a MOTA of NaN rather than 1.0.
    `gungnir-metrics` reproduces that, because a metric reporting perfect accuracy for
    a sequence containing nothing is worse than one reporting "undefined".

36. **Human sign-offs on the tracking-core work** (2026-09-05). `docs/agentic-workflow.md`
    requires the owner's signature for anything in the low-trust tier and a verification
    gate before merge for the medium-risk tier. Four sign-offs were given on 2026-09-05,
    each against the reasoning the workflow asks for rather than against a passing test
    alone:

    - **Linear Kalman filter math and its numerical-stability guarantees**
      (`gungnir-filters`), medium-risk and low-trust. Joseph form rather than the short
      covariance update, because the short form is correct only for the optimal gain in
      exact arithmetic and drifts indefinite in floating point; re-symmetrisation each
      step; `assert_psd` after every covariance mutation; the motion model as a type
      parameter so `F` and `Q` are evaluated at the `dt` actually taken. Gated at 9.1e-13
      against filterpy across 155 steps compared after every predict *and* every update.
    - **The Jonker-Volgenant assignment solver and the fallible `Associator` surface**
      (`gungnir-association`), medium-risk. Rectangular matrices handled directly rather
      than padded, because a pad value below a real cost changes the optimum; the total
      read back from the input matrix rather than accumulated from solver internals.
      Gated at 1.9e-16 against scipy, with the pairing compared only where the optimum is
      provably unique.
    - **The `gungnir-metrics` to `gungnir-association` dependency edge**, and the
      per-frame `compute_metrics` signature that replaced the scaffold's two flat slices.
    - **Cumulative rather than consecutive hits for track confirmation**
      (`gungnir-track`). This changes runtime behaviour: tracks confirm sooner than the
      scaffold's original comment implied, and a deployment tuned against that comment
      will see faster confirmation.

    **Not covered by those four, and now signed:** `gungnir-coord`, the `gungnir-core`
    motion models, the five-scenario generator, and the CLEAR MOT implementation in
    `gungnir-metrics`. None of them sits in a tier that requires a signature --
    motion-model scaffolding and simulation harnesses are named in the high-leverage
    low-risk tier, and coordinate transforms and metrics are named in no tier at all -- so
    they stood on their gates. **Signed by the owner on 2026-09-05 anyway**, deliberately
    and separately from the four above, because the low-trust "numerical stability" clause
    does reach `gungnir-core`'s `Q` and `gungnir-coord`'s finiteness invariants, and a
    clause that reaches code nobody signed is a clause that will be argued about the first
    time a covariance goes indefinite. They are signed on their gates, not folded into the
    four: the four were reviewed line by line, these were not, and the record should not
    blur that.

37. **Outstanding sign-offs walked** (2026-09-05). The owner was walked every
    outstanding sign-off in the workspace and cleared three tracks: the tracking-core
    code (item 36), the seventeen architecture principles and seventeen contracts (the
    plan 10 bullet below), and the remaining small items. Of the last:

    - **MOP-25 confirmed** at plan 07's proposed per-class prediction-error tolerances,
      with the caveat recorded in `docs/test-tracks/validation.md` §7 that they assume
      the filter-based predictor of GAP-011; the ballistic and glide-bomb rows are not
      expected to be met by today's constant-velocity predictor, and a miss there means
      the predictor is not built rather than the target is wrong.
    - **The ONNX inference runtime deferred deliberately**, not left unaddressed:
      GAP-077 is increment 4, no model exists, and the security reviewer that
      `docs/ml/architecture.md` names for a native dependency has not been appointed.
    - **MOP-35 and MOP-37 remain deferred to evidence rather than to a decision.**
      MOP-35 needs plan 08's evaluation harness and MOP-37 needs plan 06's baseline
      usability sessions; neither exists, so neither can be set by signature.

    What is *not* a sign-off, and was corrected during the walk: the §2 rows still marked
    Draft in `docs/architecture.md`. Their criteria were all agreed under D-16 on
    2026-09-04. They are Draft because their tests do not exist, which is engineering
    work and not a signature.

    Closed on 2026-09-05 by the owner acting in his other roles: the mission
    subject-matter expert approved the plan 02 mission content and the plan 07 vehicle
    data, and the technical lead made the sizing pass over gap severity and effort
    (`docs/mission/gap-analysis/README.md` carries the two scoring notes it produced).
    Round 1 of the plan 06 usability testing was begun the same day and is **not
    complete**, so GAP-074 and MOP-37 both stay open. Still outstanding and **not
    closeable by the owner in any role**: a second reader on the business figures,
    counsel on the regulatory position, and the security review of the assistant. Those
    need people who have not been engaged, and are recorded here as blocked on that
    rather than as pending.

38. **Role workspaces** (2026-09-05, GAP-068 and GAP-055). The desktop now draws the
    panels of the signed-in role's workspace instead of one fixed side panel for
    everyone, which was the whole of GAP-055's finding.

    **GAP-068** put the three roles D-05 adopted into `gungnir_security::Role`:
    commander, planner, and intelligence analyst. Two judgements were needed and both
    were the owner's, because `gungnir-security` authorisation is low-trust. First, the
    authority matrix in `docs/mission/roles-and-stakeholders.md` §4 has **no Planner
    column**, so rather than infer authority for a role inside an engagement chain the
    planner was given view-only coarse permission; widening it means adding the §4 row
    first, and the code says so. Second, `Role::rank` drives
    `gungnir_collab::RoleRankArbiter`, so the ordering decides who wins a reconciliation
    conflict: commander sits above supervisor, planner low, and every existing relative
    order is preserved.

    **GAP-055** expanded `PanelId` from ten to the twenty of
    `docs/ux/ux-to-code-map.md` §1, each carrying its `PN-nn` identifier so a panel on
    screen traces back to its wireframe. `WorkspaceLayout` now separates **docked** from
    **on demand**, which the scaffold ran together -- without that separation an
    operator would have the decision dialog permanently on screen -- and the status strip
    and viewport are in every layout by construction rather than listed eight times. All
    eight role layouts are transcribed from `docs/ux/information-architecture.md` §1 and
    tested against it.

    Two invariants are worth naming because they are enforced rather than asserted: the
    administrator can open every panel **except** the decision dialog, which is the one
    surface that commits an engagement; and no role can open the decision dialog unless
    `role_permits` grants it `DECIDE_PLAN`, which ties the layout to the authorisation
    rather than letting them drift.

    **Nine of the twenty panels do not exist** (was sixteen; GAP-072, GAP-073,
    GAP-038 and GAP-071 built eight of them). Their slots render a placeholder that names
    the panel and the gap that will build it, because an empty frame in a workspace would
    tell an operator there are no alerts, no requirements or no coverage gaps -- the
    claim AP-02 forbids. Workspace completeness today, printed by the test that records
    it: operator 7 of 7 docked panels, commander 6 of 6, analyst 5 of 5, supervisor 7 of
    8, intelligence analyst 5 of 6, administrator 4 of 5, planner 4 of 6, sensor manager
    4 of 6.

    The operator, commander and analyst workspaces are now complete in the sense that
    every docked slot draws its own panel rather than a placeholder. That is a statement
    about the panels, not about the picture: most of what those panels display is still
    unavailable, and each section says which crate owes it.

    **Two new dependency edges**, drawn in the edge table above in this change:
    `gungnir-app` to `gungnir-workflow` and to `gungnir-security`. Both are downward
    from the binary, which the graph already describes as depending on everything above
    it; the workspace layout and the role type have to be reachable from the crate that
    draws the screen. The graph remains acyclic.

    Recorded with it: the owner chose to start Area D without completing round 1 of the
    usability testing, accepting that a later session's findings land as rework on built
    panels rather than as edits to wireframes
    (`docs/ux/usability-test-plan.md`).

39. **The status strip** (2026-09-05, GAP-072). PN-01 is drawn on every layout, as a
    top panel rather than a slot in any one workspace, showing the eight elements
    `docs/ux/information-architecture.md` §2 specifies: backend, session, mission time
    with its clock source, health, weapons control status per effector layer,
    delegations in force, alert counts, and role.

    Two elements could not be answered fully, and the types make that sayable rather
    than guessable, which on this panel matters more than on any other: it is the
    surface an operator reads to answer *is this working and what is it allowed to do*.
    `AlertSummary` distinguishes counts by lifecycle state from a flat list whose states
    are not tracked -- the desktop still carries `Vec<String>` alerts, so it reports a
    total and says the states are unknown rather than printing three zeros that would
    read as "nothing new". `ControlStatusLine::configured` separates a layer someone set
    to Hold from a layer nobody configured; both are at Hold, which is
    `ControlStatusSettings::for_layer`'s deliberate safe default, but "the commander set
    this" and "nobody has set this" are different operational facts.

    The register entry said weapons control status and delegation were "not in code".
    That was out of date: DN-08 landed `ControlStatusSettings` and
    `AuthorityRule::pre_delegated`, so both are read from the baseline in force rather
    than stubbed.

    `TimeAuthority` gained a `source()` method returning `ClockSource::Wall` or
    `Replay`, with no default implementation on purpose: an implementation that did not
    say would be displayed as a wall clock, and a replay mislabelled as live makes every
    timestamp on screen mean something other than what it says.

40. **A budget gate that was measuring the wrong thing** (2026-09-05). The journal
    budget test flaked during this change: it took a single sample of fifty file
    writes and held it to the 1 ms budget, and repeated runs ranged from 612 µs to
    1.01 ms. One sample of a noisy quantity is the wrong statistic -- the rest of the
    harness reports p99 over thousands of frames for exactly this reason. It now takes
    the median of nine runs.

    The first attempt at that fix made it worse, and the reason is worth keeping: using
    a fresh session per run made every run pay a session switch, and a switch fsyncs the
    file being left, so the measurement was charged for durability work no frame does.
    Measuring one live session, which is what "journal append cost per frame" actually
    describes, gives a median of about 400 µs against the 1 ms budget. **The threshold
    was not changed**; what changed is that the measurement is now of the budgeted
    quantity.

41. **The evidence card, the commander summary and the track table columns**
    (2026-09-05, GAP-073). PN-04 and PN-17 are built, the track table has row selection
    and the score, age and asset columns, and the theme carries the DS-03 affiliation
    colours and frame shapes that `gungnir-viewport3d` now draws around every glyph.

    Both new panels are mostly *unavailable*, and making that sayable is the substance
    of the change rather than a caveat on it. PN-17 in particular is the panel where a
    plausible zero does the most damage: a commander who reads "0 pending approvals"
    from an approval queue that was never constructed concludes nothing is waiting on
    them. So a panel section is a `Result<T, Unavailable>` or a `Section<T>` -- never a
    `Vec` that happens to be empty -- and `Present(&[])` and `Unavailable(..)` compare
    unequal by construction. The unavailable arm has to name the crate that owns the
    data and the register entry that will wire it, which is what makes the claim
    checkable: identity evidence GAP-010, cross-session lineage GAP-019, threat factors
    and the score column GAP-028, queue statistics GAP-038, accepted coverage gaps
    GAP-006, outcomes GAP-043. A test in `gungnir-app/src/workspace.rs` asserts every
    one of those crates exists in this workspace, so an unavailability cannot outlive
    the crate it blames.

    The same rule decided the three new columns. Age is `now - mission_time` and asset
    is the resource the plan in force assigns, both computable from state the desktop
    already holds; the threat score is not, so the column header reads `Score (n/a)`
    with the reason above the table rather than every row showing `0.00`. An unassigned
    track shows a dash in the asset column, which is a true statement about the plan
    rather than a missing value.

    Two smaller decisions worth keeping. Selection is held as a `TrackId` rather than a
    `TrackView`: the views are regenerated every tick, so holding the struct would pin a
    stale copy on screen while the tracker moved on, which is the failure the staleness
    marking exists to prevent; a selection whose track has since been deleted says the
    track is no longer in the picture instead of drawing a blank card. And panels return
    a `PanelAction` for `main.rs` to apply after the frame is drawn rather than writing
    to `AppState`, which keeps the one-way flow of
    `rust-ui-architecture-coding-standards.md` §2 and leaves the panel functions
    testable without an `AppState`.

    DS-03 is carried by shape as well as colour -- diamond, rounded rectangle, square,
    quatrefoil for hostile, friendly, neutral, unknown -- so an operator with a
    colour-vision deficiency still reads friend from hostile; the four colours are also
    checked in `gungnir-ui`'s tests against the viewport background for the WCAG 2.1
    4.5:1 contrast ratio. The frame is drawn separately from the lifecycle fill, because
    affiliation and track lifecycle are independent facts and one cue must not overwrite
    the other: a stale hostile is a diamond with a grey centre.

42. **The approval gate is in the tick** (2026-09-05, GAP-038). PN-06 and PN-07 are
    built, and §7.3's "not yet called from the tick" no longer covers `gungnir-policy`
    and `gungnir-command`: `gungnir-app/src/decisions.rs` evaluates every proposed plan
    against a three-engine chain and queues what clears it for a human.
    `gungnir-app/tests/approval_gate.rs` asserts at the wiring level what the two crates
    assert internally -- that ticking the desktop never produces an actionable plan,
    because nothing has been decided.

    **The interesting part is the empty queue.** No plan reaches it in this build, by a
    chain of individually correct steps: no pipeline, so no tracks; no tracks, so an
    empty plan; an empty plan, so a denial before the queue. The result is a
    permanently calm approval queue, which an operator reads as *nothing needs
    deciding* when it means *nothing upstream is running*. So `EmptyBecause` is a
    required field on the view rather than an optional note, its three variants are
    three different operational situations, and only one of them --
    `NothingPending` -- means what a calm queue looks like it means. The other two are
    drawn as warnings rather than muted text, and a test asserts the desktop reports
    the pipeline as the reason rather than the reassuring variant.

    **Accept is never the default**, which the register named as this gap's closing
    condition. It is enforced in four places rather than asserted once: nothing is
    pre-selected; accept is gated on acknowledging the degraded conditions in force,
    and the acknowledgement is discarded whenever that set changes, so a tick given for
    a journal warning does not carry over to a tracker failure; accept is drawn last,
    after reject and override, so neither the reflexive click nor the reflexive
    keyboard traversal lands on it; and no control is bound to Enter. A test searches
    the dialog's state space for a combination that opens accept without a deliberate
    act. Rejecting is gated the other way -- on *saying why* rather than on authority --
    because MOE-01 needs a considered rejection to be distinguishable from an
    abandoned one, and closing the dialog abandons without recording anything.

    **The verdict names its own chain, and the checks in it that could not have
    failed.** `GeofencePolicy` does four things: it denies an empty plan, an unknown
    resource, an unready resource, and an intercept point inside a no-go fence. The
    first three are real and run against the configured resources. The fourth is
    vacuous, for **two independent reasons**: `ConfigBaseline` has no geofence section,
    so the `GeoService` holds no fences and `is_within_no_go` searches an empty list;
    and the planner sets `intercept_point: None` on every solution (GAP-031, which says
    so in as many words), so the test is never reached even with fences configured.
    Fixing either alone leaves it vacuous, which is why `PolicyChainReport` carries them
    separately and PN-07 lists both under "Could not fail".

    This was under-reported when the gap first closed: only the missing fences were
    named, which would tell an operator that configuring fences makes the check real.
    A test now asserts both are reported, and that fixing one leaves the other standing.
    The engine is also named "readiness and geofence" rather than "geofence", because
    calling the whole engine vacuous understates the chain as badly as calling it sound
    overstates it.

    **No operator is named.** There is no operator session (GAP-057), so every
    `DecisionRecord` this dialog produces carries `operator_id: None`, taking the same
    position `gungnir_command::queue::expiry_record` takes about an expiry: a false
    attribution in an append-only record is worse than a null. PN-07 says so on screen
    every time rather than leaving the record to imply otherwise.

    **Three new dependency edges**, drawn in the edge table above in this change:
    `gungnir-app` to `gungnir-policy`, to `gungnir-command`, and to `gungnir-geo`. All
    three are downward from the binary, which the table already describes as depending
    on everything above it, and the binary constructs them without implementing any
    policy of its own -- which is what keeps contract C-01 checkable in one place. The
    graph stays acyclic: none of the three depends on anything that depends on the
    binary.

    **One change to a human-owned crate, signed by the owner on 2026-09-05**:
    `gungnir_policy::PolicyChain` gained a lifetime parameter. It boxed
    `dyn PolicyEngine + 'static` -- a promise the boxed value holds no borrowed
    references -- and all three engines the crate ships borrow one: `GeofencePolicy` the
    `GeoService`, `ControlStatusPolicy` and `AuthorityPolicy` the baseline in force and
    a classifier for the current snapshot. That borrowing is what lets a plan be judged
    against the applied configuration without copying the policy settings every frame,
    so **no engine the crate ships could be put into a chain at all**. This was a latent
    defect rather than a design being fought, and it went unnoticed because the only
    test of `PolicyChain` built one from an empty vector: a chain of nothing exercises
    the combination rule without exercising the type that carries it. Two tests now
    build chains from real borrowing engines, one of them asserting that a denial from
    the *second* engine still wins, so engine order cannot quietly decide whether a plan
    is denied at all. No verdict logic changed, and a chain of owning engines is a
    `PolicyChain<'static>`, which is what every prior use inferred.

    The alternative, rejected: leave the crate alone and call the three engines in
    sequence from `gungnir-app`, reimplementing the combination rule there. That would
    put the recommend-versus-act combination logic in the binary and make contract C-01
    checkable in two places instead of one.

43. **What GAP-038 did not close.** Time remaining per queued item is not shown, and
    the column says so and names GAP-034 rather than leaving a blank cell:
    `gungnir_command::queue` computes deadlines, escalation and expiry, and
    `ApprovalWorkflow::pending` carries none of them, so the two are not connected.
    Connecting them is GAP-034 and GAP-035, both human-owned. This matters more than a
    missing column because DN-10 makes "no expiry configured" mean the item is
    *preserved indefinitely*, so a blank countdown would read as a decision the
    deployment had taken. `TimeRemaining` keeps the two apart in the type.

    The node binary still does not run the gate, so GAP-028 is advanced rather than
    closed: `gungnir-node` proposes plans without a policy verdict or a queue.

44. **The approval queue is timed** (2026-09-05, GAP-034 and GAP-035).
    `gungnir_command::queue` had computed deadlines, ordering, expiry and escalation as
    pure functions since DN-10 and **nothing called them**: the workflow kept an untimed
    `Vec<(id, plan, verdict)>`. It now holds `PendingApproval`s timed from the
    baseline's `DecisionSettings`, `ApprovalWorkflow` gained `queue()` and
    `sweep(now, ladder)`, and the desktop tick sweeps every frame -- a window closes on
    the clock, not on new input. Expiries and escalations reach the bus as the
    `CommandEvent` variants DN-10 had already specified.

    **Wiring an unused module found two defects in it**, both of which had gone
    unnoticed for the same reason: nothing called the functions, so nothing could
    disagree with them. `is_due_for_escalation` required `escalated_from` to be unset,
    which bounds escalation at once *ever*, where DN-10 §5 says once *per rank step*;
    the bound is now the escalation clock, reset at each step and cleared at the top of
    the ladder. And `PendingApproval` had a single `escalated_from` where DN-10 §5 says
    escalation "does not leave the original role's view -- both roles see it; whoever
    decides first ends it", which a single field cannot express; it gained `offered_to`.
    The lesson is the same one the `PolicyChain` lifetime taught a few items earlier: a
    module that is fully tested and entirely uncalled is not verified, it is only
    self-consistent.

    **The governing layer is the one that closes first.** A plan may task several
    layers and `DecisionSettings` sets expiry per layer, so one has to decide the
    window. Taking the longest would let a 30 s Point window close while the item sat in
    the queue looking live, so a plan touching Point and Area has the Point deadline. A
    layer with no configured expiry never closes and therefore loses to any layer that
    does.

    **The escalation ladder is derived from `role_permits` rather than configured
    beside it**, so it cannot offer an item to a role that may not decide or skip one
    that may; a test asserts both directions and that the ladder climbs by rank.

    `gungnir-command` is human-owned under `docs/agentic-workflow.md`, and these
    changes -- the two semantic corrections above, and the `Submission`, `queue` and
    `sweep` additions to the trait -- were **signed by the owner on 2026-09-05**.

45. **Resolved 2026-09-05, by conforming to the design note rather than amending it.**
    `OperatorDecision` is now DN-10 §3's type: `Accepted`, `Overridden`,
    `Rejected { reason }`, `Expired { at }`. The record says what happened, and
    `DecisionRecord::is_expiry` answers it without reference to whether an operator
    happens to be named.

    **The finding underneath it was that the code had drifted from a signed note, and
    the first report of it was wrong about which way.** It was raised here as "the record
    model needs a decision: either `OperatorDecision` gains an `Expired` variant, which
    DN-10 deliberately did not do, or the criterion waits on GAP-057." DN-10 §3 does not
    deliberately avoid the variant -- it **specifies** it, along with
    `Rejected { reason: String }`. The sentence that was read as choosing against it is
    §3's remark that `operator_id` is `None` for an expiry, which is about attribution,
    not about the variant. So this was never a design question; it was an implementation
    that had lost two of the four variants its signed note asked for, and the fix was to
    conform.

    Two consequences of that drift were live. A rejection had nowhere to put a reason, so
    PN-07 collected one from the operator and discarded it -- and MOE-01 needs the reason
    to tell a considered rejection from an abandoned decision. And `gungnir-collab`'s
    `AuthorityArbiter` reconciled two entries for the same plan by the role rank in a
    supplied `AuthorityContext`, with no way to see that one of them was an expiry, for
    which no role can honestly be supplied; a fabricated rank would have decided the
    outcome. It now resolves in favour of whoever actually chose, before rank is
    consulted at all.

    `DecisionRecord::to_event` derives the event from the record, so an expiry publishes
    `CommandEvent::Expired` rather than `Decided { accepted: false }`; there is no path
    that writes an expiry to the history and a rejection to the bus.
    `AppState::queue_outcomes` no longer counts expiries in parallel with the history --
    `decisions::expired_count` reads the records, which is now the authority.

    DN-10 §9 records the amendment this *did* need: `Escalated` is not an
    `OperatorDecision`, because an escalated item has not ended and a record for it would
    put an entry in the append-only history for something that has not happened; plus the
    `Overridden` omission, `offered_to`, the escalation bound, and the governing-layer
    rule. **Amendment 1, the `OperatorDecision` conformance, and the arbiter's expiry
    rule were all signed by the owner on 2026-09-05.**

46. **Superseded note, kept for the reasoning: an expiry and a rejection were not
    distinguishable in the record.** DN-10 §3 distinguishes them by
    `DecisionRecord::operator_id` being `None` for an expiry -- "nobody decided, and
    recording a false operator would be worse than a null" -- which is sound the day a
    decided record carries an operator id. None does: there is no operator session
    (GAP-057), so a human rejection taken in PN-07 is also `None`, and the
    `verification-capability-table.md` criterion that an expired item leaves "an
    `Expired` record distinct from a rejection" does not hold in the record itself
    today.

    **The scope is narrower than it first appears, and this was checked rather than
    assumed.** `DecisionRecord` is not a wire or journal type: the durable form is
    `CommandEvent`, which already has distinct `Expired` and `Decided` variants, and it
    is the event stream that reaches `gungnir-store`. So the system of record *can*
    answer the question; what cannot is the in-process `records()` list, and anything
    built to read it. The queue's outcomes are likewise distinct, so
    `AppState::queue_outcomes` counts expiries from there, PN-17's figures are right,
    and `decided_this_session` subtracts them so a timing-out queue is never reported as
    a queue being worked.

    Two consumers make this worth resolving rather than leaving. `gungnir-collab`'s
    `AuthorityArbiter` reconciles two decisions on the same plan by the *role rank in the
    supplied `AuthorityContext`*, and an expiry has no role to supply: reconciling one
    against a real decision would need a fabricated context, and whatever rank was
    invented would decide the outcome. And GAP-071's reporting reads the history to
    compute MOE-01, which needs a considered rejection to be distinguishable from a
    window nobody answered.

    Resolved by item 45 above.

47. **Replay, reports and the configuration baseline** (2026-09-05, GAP-071). PN-12,
    PN-13 and PN-14 are built, which leaves nine of the twenty panels unwritten and
    completes the analyst, planner and administrator workspaces.

    Each of the three carries a limitation that is the substance rather than a caveat,
    and each is a field on the view drawn every frame rather than a note in a wireframe.
    **Replay scrubs the recorded event stream and does not rebuild the picture**: the
    viewport and the track table stay live while the cursor moves, because `AppState` is
    built from the service facades and not from the event stream, and reconstituting
    state at a past moment is GAP-045. An analyst who scrubbed to a moment, looked at the
    viewport, and read it as the picture at that moment would have drawn a conclusion
    about something they never saw. **A report's counts are not the measures catalogue**
    (GAP-047), which is easy to conflate on a screen headed "report". **Applying a
    baseline persists it and the running session keeps the one it started with**: the
    services, journal, ingest gateway and approval deadlines are all built from the
    baseline at construction, so a live swap would have to rebuild most of `AppState` to
    be true, and an administrator who applied a Weapons Free baseline and believed it was
    in force immediately would be wrong about the thing that matters most on that screen.

    **A third empty state.** PN-13 distinguishes "not generated" from "generated, and
    these are the counts" from "this session has recorded nothing". The third is not a
    failure and not a report of zeros: `FileEventJournal` creates a session's file on its
    first append, so a session that has published nothing is absent from the journal
    entirely and `read_session` returns `UnknownSession`. It is the ordinary case in this
    build, because with no pipeline the desktop publishes almost nothing, and the first
    version of the panel reported it to the analyst as "report action failed".

    **Two new dependency edges**, drawn in the edge table above: `gungnir-app` to
    `gungnir-replay` and to `gungnir-reporting`. Both are readers of the journal this
    desktop already owns.

48. **A durability hole found by writing the GAP-071 tests: nothing ever called
    `EventJournal::sync`.** D-04 (§10 item 19) has two halves -- fsync every 5 s while
    running, and fsync when the session is saved or closed. `update::tick` called
    `sync_if_due`, which is the first half. Nothing called the second, so a clean exit
    could drop up to the whole 5 s interval from a session an operator had just
    deliberately finished, which is the moment losing it is least excusable.
    `AppState::save_session` now exists and `eframe`'s `on_exit` calls it, returning the
    error rather than logging it, because a caller that ignores it is claiming a
    durability it does not have.

    Worth noting how it was found: not by a test of durability, but because a test needed
    to flush the journal to read it back and there was no way to. The absent API was the
    symptom.

49. **Docking and the second window** (2026-09-05, GAP-075). The role workspace is an
    `egui_tiles` tree the operator can rearrange rather than a fixed stack in a side
    panel, and the three panels D-17 allows -- the viewport, the approval queue and the
    replay timeline -- can be drawn in their own native windows. The detached half needed
    no crate: egui 0.29's viewports provide it, which is what D-19 recorded.

    **The persisted arrangement is not the docking crate's own type.** `egui_tiles` can
    serialize its `Tree` and using that would have been less code; it is the wrong thing
    to put in a configuration baseline, which is version-gated, validated, hand-edited
    and reviewed like any other deployment artefact. `gungnir_model::LayoutNode`
    describes an arrangement in the design's vocabulary -- panels, tabs, splits and their
    shares -- so a person can read a baseline and see the screen it describes, and
    `gungnir-config` can refuse one that names a panel that does not exist.
    `gungnir-app/src/dock.rs` converts both ways, and the direction that can silently
    lose information -- reading a rearranged tree back out -- is what the round-trip test
    guards.

    **D-17's rule that decision dialogs stay with the queue is enforced twice, from both
    ends.** `gungnir-config` refuses a baseline that detaches PN-07, so it cannot be
    configured apart; `main.rs` draws PN-07 in whichever window holds PN-06, so it cannot
    drift apart at runtime. A decision separated from the queue it came from is a
    decision taken without its context.

    **The arrangement is per role and saving it needs `config.apply`.** Rearranging
    during a session changes the screen and not the baseline: a shift's worth of dragging
    must not quietly become the deployment's configuration. That also means the current
    build has no in-app "save this layout" -- the arrangement is written by editing the
    baseline file and applying it through PN-14, which is the same path every other
    configuration change takes.

    `gungnir-ui` does **not** depend on `egui_tiles`, which §2.9 had said it would: a
    `Behavior` must render panes, and rendering a pane means reaching the app's own
    state, so the tree belongs to the binary. The table has been corrected.

    **Verified by rendering, since item 50 below.** The tree draws through a headless
    egui frame in `gungnir-app/tests/rendered_workspace.rs`: every role's workspace
    renders with a pane per panel, every panel draws something, and a configured
    arrangement with a detached queue draws that queue once. What is still not verified
    is whether any of it *looks* right, which is GAP-074's usability round.

50. **A headless render probe, so "the panel says X" is a gate** (2026-09-05).
    `gungnir-ui/src/harness.rs` runs a panel through a real `egui::Context` frame -- laid
    out, no window, no GPU, no display -- and returns the text it painted, in draw order.
    `gungnir-ui/src/panels/rendered.rs` and `gungnir-app/tests/rendered_workspace.rs`
    assert on that.

    **The gap it closes is specific and was load-bearing.** Every panel built in this
    increment carries a sentence it exists to put on screen: the approval queue's reason
    for being empty, the replay timeline's warning that it does not rebuild the picture,
    the configuration editor's statement of when an apply takes effect. Those were all
    asserted against the *view structs* -- that the field held the right string -- and
    nothing checked that the panel drew it. A panel could hold a perfect `EmptyBecause`
    and render none of it, and every test in the workspace would still have passed.

    Draw order is checkable too, which turned one comment into a gate: PN-07's accept
    control is drawn after reject and override on purpose, and
    `the_decision_dialog_draws_accept_last` now asserts the positions rather than the
    module documentation asserting the intent.

    **The probe was mutation-tested before being trusted.** Deleting the replay panel's
    governing sentence makes `the_replay_panel_draws_that_it_does_not_rebuild_the_picture`
    fail, and the failure message prints everything the panel *did* draw. A render test
    that cannot fail is worse than none, because it reads like coverage.

    `egui_tiles`' `Behavior` implementation moved out of `main.rs` into
    `gungnir-app/src/dock.rs` to make this possible, which
    `rust-ui-architecture-coding-standards.md` §1 wanted anyway: deciding how each pane
    is drawn is neither bootstrap nor wiring, and while it sat in the binary the dock
    tree could not be rendered by a test at all.

    **What this does not do.** It says what text reached the screen, not whether the
    screen is good: spacing, whether a warning is noticeable, whether an operator can
    work the queue under time pressure. That is a person's judgement and it is GAP-074.
    The claim that changed is "nobody has looked at it" -- which was true of the
    *behaviour* and is no longer -- not "this has been through usability".

51. **Display vocabulary** (2026-09-05, D-12, GAP-070). `gungnir_model::Vocabulary`
    holds the word the interface shows for every domain value, defaulting to the NATO
    and joint terms of `docs/mission/glossary.md`, with a per-deployment override table
    in the baseline's new `vocabulary` section.

    **What this actually fixed was worse than "labels are not configurable".** Four
    panels were putting `format!("{:?}", ..)` on screen for a classification, an effector
    layer or a weapons control status -- Rust variant names shown to an operator. Two of
    those differ from the doctrine term in ways that matter. The NATO standard identity
    is **Friend**, not `Friendly`. And the joint terms are **weapons free**, **weapons
    tight**, **weapons hold**, which the glossary states in as many words: a bare "Free"
    on a control-status line reads close to its own opposite to anyone who has not
    memorised the enum. `SelfDefence` reached the screen too.

    **Overriding is renaming, never remapping.** The table is keyed by a closed `Term`
    enum over the model's own types, so there is no key for a concept the system does not
    hold; a deployment that calls a hostile track something else still has a hostile
    track and every policy engine still treats it as one. Two terms may even share a word
    without becoming one value, which a test pins. `gungnir-config` refuses an unknown
    key rather than ignoring it -- an administrator who mistyped `classification.hostle`
    would otherwise see the default word with nothing to tell them why their rename did
    not take -- and refuses an empty label, because a blank where a classification should
    be is the worst possible reading of a track.

    `theme::status_label` was deleted rather than kept alongside: two sources for one
    word drift, and the vocabulary owns it now. `ClassificationFrame` gained a
    `frame_label` for the same reason at a smaller scale -- the evidence card was drawing
    "RoundedRect frame", and a shape's name is "rounded rectangle".

    Rendered assertions rather than view-struct ones, using the item 50 probe: the track
    table draws `Friend` and **not** `Friendly`, an override reaches the screen, and
    overriding one term leaves the others alone.

52. **The three-d scene is attached to eframe's OpenGL context** (2026-09-05,
    GAP-022). `gungnir-viewport3d/src/gl.rs` builds a `three_d::Context` from the
    `Arc<glow::Context>` eframe owns and draws the track glyphs as one instanced mesh;
    `main.rs` issues the `egui_glow` paint callback, which is the only piece that has to
    know about eframe.

    **The attachment works because `eframe` 0.29, `egui_glow` 0.29 and `three-d` 0.18
    all depend on `glow` 0.14**, so the context type is shared. That is the fact the
    whole thing rests on and it would break silently on a version bump, so it is stated
    as a compile-time assertion in `gungnir-app/tests/gl_attachment.rs`: a function
    taking `Arc<eframe::glow::Context>` and returning
    `Arc<gungnir_viewport3d::gl::GlContext>`. If the two ever diverge, that one line
    stops compiling instead of the viewport quietly losing its 3D path. `cargo tree -d`
    is the other half and reports a single `glow 0.14.2`. §9's version set is therefore
    also a decision about this attachment.

    **`ui.scene_3d` defaults to on** (owner's decision, 2026-09-05; see item 53 for
    what that means and what carries it).

    What *is* checked is everything that decides what gets drawn, because it was written
    as pure functions over plain data: one instance per track at its own ENU position,
    the shared lifecycle colour so the 3D view, the 2D projection and the track table
    cannot disagree about a stale track, a camera that follows the operator's chosen
    centre and zoom rather than having controls of its own, a far plane beyond the ground
    it is aimed at, and a viewport conversion that survives a collapsed rect. The GL
    shell around them is as thin as it could be made.

    Attachment returns a `Result` and a failure falls back to the 2D projection with a
    log line rather than panicking: a rendering feature must not stop the picture coming
    up. A poisoned callback lock skips the draw for the same reason.

    Terrain, imagery and the coverage layers are **not** in this: GAP-022's action names
    the camera and glyphs, and the layers follow with GAP-023's loaders and GAP-007.

53. **The unverified renderer is the default, deliberately** (2026-09-05, owner's
    decision). `ui.scene_3d` defaults to on, so the three-d scene draws the picture
    unless a deployment opts out.

    Stated plainly, because it is the kind of thing that should not be discoverable only
    by reading a config default: **the GL draw call has not been rendered anywhere anyone
    could see it**, and there is no headless probe that could -- item 50's `RenderProbe`
    reads egui's shape list, and three-d does not produce shapes. What is verified is
    everything deciding *what* is drawn (instances, camera, viewport conversion, all pure
    functions) and that attachment fails cleanly. What is not verified is that the draw
    call puts anything on the screen.

    Two things carry the weight a verified renderer would otherwise carry, and both were
    added when the default changed:

    - **The status line is painted by egui over the callback's output**, so it survives
      whatever the GL draw does. A viewport that renders nothing still reads "three-d
      scene: 12 tracks, 3 assignments". An empty viewport that says twelve tracks exist
      is a fault an operator can report; an empty viewport that says nothing is an
      operator concluding the sector is clear, and that is the failure this arrangement
      exists to prevent.
    - **A runtime toggle in the corner of the viewport**, drawn by both renderers so
      switching is never one-way. Without it, a deployment that found the scene drawing
      badly would have to edit a baseline and restart during whatever was happening at
      the time. It changes the session, not the baseline: the next launch starts from
      the configured value again.

    `ViewportState::use_3d` is also gated on a context actually being attached, so a
    session can never claim a renderer that is not there: `use_3d` is
    `scene.is_some() && config.ui.scene_3d`, and a test asserts a desktop built without
    eframe reports neither.

    GAP-074's usability round is still where somebody looks at it.

54. **Coverage on the map, and the local frame it needed** (2026-09-05, GAP-007).
    `gungnir-viewport3d/src/layers.rs` draws each sensor's coverage as a ring under the
    track glyphs, in the 2D projection and over the three-d scene. It takes plain ENU
    circles rather than `gungnir_sensor_management::CoverageRegion`, so the rendering
    crate keeps its edges and the caller converts -- the same view-struct rule the panels
    follow.

    **The finding is larger than the gap.** `DetectionView::measurement` and
    `TrackView::state` are documented as "the local ENU frame, meters", sensors,
    resources and defended assets are all geodetic, and **nothing said where that local
    frame is anchored**. Worse: nothing in the workspace called `gungnir-coord`'s
    geodetic conversions at all. The functions existed, gated against pymap3d to 1e-6 m,
    with no caller anywhere.

    It had gone unnoticed because nothing yet needed both frames at once. The tracking
    pipeline produces no tracks, so no detection is ever converted; the intercept planner
    computes no intercept point, so `draw_plan_2d` labels a resource *beside its track*
    rather than at a place -- a comment in that function says exactly why, and it was the
    frame problem being routed around rather than solved. Drawing coverage is the first
    thing that cannot avoid it.

    `gungnir_model::LocalFrame` is the definition, and `ConfigBaseline::origin` is where
    a deployment declares it. **There is deliberately no default.** Guessing an origin --
    the first sensor, the mean of the assets, the equator -- would place every geodetic
    thing somewhere plausible and wrong, and a plausible wrong ring on a coverage map is
    worse than no ring: a sensor manager trusts exactly that ring. So with no origin the
    viewport draws no coverage and says *the rings are not missing, they are
    unplaceable*, which is a different statement from "nothing is covered" and the two
    are separate variants for that reason. Validation refuses an origin in degrees, which
    is the likely mistake: 55.0 is a plausible-looking latitude and nine times round the
    planet in radians.

    **The rings are nominal, and say so.** They are each sensor's configured range, not
    what it is covering now: reading a sensor's mode needs `gungnir-sensor-management`
    constructed, which is GAP-003. A standby sensor has a configured range and covers
    nothing, so the layer carries the caveat and the map prints it. Confidence is drawn
    as opacity so a weaker claim looks like one.

    A test caught a real bug in that: `Color32` stores premultiplied channels, so reading
    a base colour that carried its own alpha and re-fading its components shifted the
    *hue* rather than the opacity -- a searching sensor's ring would have been a different
    colour, not a fainter one. The theme now keeps the base opaque with the transparency
    beside it.

55. **Coverage gaps, and what flat terrain actually does at range** (2026-09-05,
    GAP-006). The register said no function combined coverage across the registry.
    **It was stale**: `gungnir-analytics/src/coverage.rs` already implemented DN-12 §3
    and §5 in full, including the monotonicity property §8 asks for -- removing a sensor
    never shrinks the reported gap set -- and `coverage_from_registry` already pushed the
    geodetic-to-ENU conversion out to a caller-supplied closure, which is the same frame
    question item 54 hit from the other side. Third stale entry found this way, after
    GAP-052 and GAP-072's description.

    What was missing was DN-12 §6 and the reachable half of §7. The baseline gained
    `analytics.coverage_sample_spacing_m` and `coverage_min_elevation_rad`, and an
    `approaches` list -- the deployment *declaring* the axes DN-12 says the caller
    supplies, which is the DN-01 move for the defended-asset list and not the approach
    model DN-12 pointedly refuses to invent. The viewport draws gaps along them,
    uncovered distinguished from single-sensor **by pattern as well as colour**, and
    PN-01 counts uncovered segments. `GET /v2/coverage` waits on the transport
    (GAP-041); PN-11's controls and PN-16 wait on those panels.

    **The finding: `FlatTerrainLineOfSight` is not "optimistic by construction" at
    coverage ranges, which is what DN-12 §5 rule 3 says it is.** It tests
    `from.u >= 0 && to.u >= 0`, and in a local ENU frame a point at constant geodetic
    altitude falls *below* the tangent plane as it recedes -- about 240 m at 55 km. So a
    sea-level approach reads as hidden past a few kilometres. That is a crude horizon
    and roughly the right direction physically, but the note's rationale for reporting
    the flat-terrain case was that it over-reports coverage, and at these ranges it
    under-reports it for low targets. The status strip now says it is wrong in **both**
    directions, and `ApproachConfig` says to declare the altitude the axis is actually
    flown at. Found by a test asserting single-sensor coverage near a sensor and getting
    none.

    **One new dependency edge**, drawn above: `gungnir-app` to `gungnir-analytics`. The
    alternative was reimplementing the sampling and grouping in the binary, which is the
    duplication DN-12 put the function in one place to avoid.

    The coverage the desktop reports is nominal in the same sense the rings are: every
    configured sensor is credited as if it were searching, because reading modes needs
    the registry constructed (GAP-003). `coverage_from_registry` is written and waiting
    for it.

56. **Sensor management wired, and coverage stops being nominal** (2026-09-05,
    GAP-003). Both binaries construct `InMemorySensorRegistry` from the baseline, PN-10
    shows and changes what each sensor is doing, and a mode change publishes
    `SensorEvent::ModeChanged` -- a new `Event` variant, which the reporting fold's
    exhaustive match forced into `EventCounts` where it belongs.

    **The payoff is that two caveats went away and one truth arrived.** GAP-006 and
    GAP-007 both had to say their coverage was *nominal*: every configured sensor
    credited as if it were searching, because nothing read the modes. Coverage now comes
    from `SensorRegistry::coverage`, which reports only what is searching or tracking.
    The caveat is deleted rather than reworded.

    **And a fresh desktop now reports covering nothing**, because
    `SensorRecord::from_config` starts every sensor at Standby. That is not a regression
    and it is the most useful thing this change did: a registry that came up claiming
    every sensor was searching would report coverage nobody had switched on, which is
    the failure MT-07 turns on. Two tests had to be rewritten because they asserted the
    nominal behaviour, and rewriting them made them exercise the whole loop -- set a
    sensor to Search in PN-10, watch the rings and the gap report change.

    `SensorMode` moved to `gungnir-model` and is re-exported from
    `gungnir-sensor-management`. An event has to carry it and events live in the model,
    which cannot depend on a crate that depends on it; the re-export keeps every
    existing path resolving, which is the workspace's own rule for a type the model
    owns. It also joined the display vocabulary, because a mode reaching an operator as
    a variant name is the thing item 51 was about.

    **Nothing here reaches a sensor.** A mode change is recorded and published; issuing
    it to hardware is GAP-004, and PN-10 says so on screen rather than letting a row
    that changed to Search imply a radar started searching. A refused transition is
    shown too -- coming back from Offline must pass through Standby, and a row that
    simply did not move would look like a click that missed.

    **Two new dependency edges**, drawn above: `gungnir-app` and `gungnir-node` to
    `gungnir-sensor-management`. The node's is what makes its registry more than
    construction: it reports the sensor and covering counts in its health line, because
    a binary that built a registry and never read it would be the fake wiring this gap
    was open about.

57. **The outbound sensor control path, short of the wire** (2026-09-05, GAP-004).
    `SensorControl` is a second trait beside `SensorRegistry` -- reading what sensors are
    doing and commanding them are different authorities, and splitting them lets a caller
    that only reads say so. Issuing records a `SensorTask`, hands it to an attached
    adapter, and tracks the acknowledgement; the tick sweeps the acknowledgement window
    every frame and alerts on what nobody answered.

    **What makes this worth having before the adapters exist is the rule it enforces:
    a command changes nothing until a sensor acknowledges.** `SensorRecord::mode` is now
    documented as the *confirmed* mode, a requested mode lives in a separate map, and
    `acknowledge` is the only path that moves one to the other. PN-10 shows them as two
    facts in one cell -- "Standby (asked: Search)" -- and coverage follows the confirmed
    one, so a sensor nobody has confirmed is searching puts no ring on the map.

    **PN-10 gained two columns where it had one.** Commanding a sensor and recording what
    an operator knows it is doing were the same control before, and they are not the same
    act: one asks, the other asserts. The command column is disabled with the reason on
    the hover for every sensor in the default baseline, because none declares a
    `control_endpoint`; the record column still works, and while no adapter exists it is
    the only one of the two that does anything. Merging them would have left the record
    unable to say which of the two happened.

    **`Event` gained `SensorTask(SensorTaskEvent)`** with `Issued`, `Acknowledged`,
    `Failed` and `Unacknowledged`, per DN-11 §6 -- distinct from `Event::Sensor`, which
    is the *fact* rather than the asking. `Unacknowledged` is not `Failed`: nobody
    refused a timed-out command, and that is the same distinction DN-10 draws between an
    expiry and a rejection. The reporting fold counts all four apart, because the gap
    between issued and acknowledged is the figure worth reading.

    `SensorTaskId` moved to `gungnir-model` and is re-exported from
    `gungnir-sensor-management`, for the reason `SensorMode` moved in item 56: the event
    schema has to carry it and the model cannot depend on a crate that depends on it.

    **`SensorControlAdapter` is now a real seam** -- `attach_adapter`, one per registry --
    and `TaskState::Sent` means an adapter accepted the task for delivery. **Nothing
    attaches one**, so every task in both binaries stops at `Issued` and the registry
    reports `has_adapter()` false rather than leaving it to be inferred. The seam exists
    because the verification row for CAP-1.3 specifies a stub that can acknowledge,
    refuse, or ignore, and that criterion is now met by a stub that really is an adapter
    instead of by tests that called the registry directly.

    **No new dependency edge**: `gungnir-app` already reached
    `gungnir-sensor-management` (item 56), `gungnir-ui` still sees only `gungnir-model`,
    and no manifest changed. DN-11 §4's `gungnir-workflow` edge belongs to the
    requirements half, GAP-005, and was not taken here.

    **GAP-004 is not closed.** Its own action says to send the command through the
    adapter that owns the sensor, and no adapter exists -- GAP-001, which the register
    already lists as its dependency. (2026-09-06: the ASTERIX radar adapter now exists,
    `gungnir-ingest/src/adapters/asterix.rs`, receive-only; it is not yet registered by
    either host and carries no command path, so GAP-004's dependency stands.) MT-07 recovery still stops at the operator's screen;
    what changed is that the screen now says so precisely instead of implying a radar
    started searching. The `POST /v2/sensors/{sensor_id}/task` endpoint of DN-11 §6 waits
    on the transport (GAP-041).

58. **Collection requirements, and a register entry that was closed too early**
    (2026-09-05, GAP-005). MT-08 steps 2 and 3 -- an analyst's tasking request and the
    sensor manager's concurrence -- happened outside the system. PN-15 brings them in:
    a requirement list with its standing, the tasks serving each, and the state, task,
    decline and answer actions.

    **The entry was marked Closed and almost nothing was wired.** `gungnir-workflow`'s
    `tasking_case.rs` existed with its types and tests, and the edge to
    `gungnir-sensor-management` was taken and drawn -- so the entry was closed on the
    types. But PN-15 was unbuilt, nothing in either binary constructed a `TaskingCase`,
    and `SensorControl::issue` hardcoded `requirement: None`, so **no requirement could
    ever be linked to a task however hard the workflow crate tried**. This is the third
    stale entry found by trying to do the work rather than by reading the register
    (GAP-052 and GAP-006 were the others), and the pattern is the same each time: a
    closing note written from the crate that was touched rather than from the thread the
    gap describes.

    `issue` now takes the requirement as a required argument. An overload would have let
    the omission recur silently; making every caller say `None` deliberately is what
    stops it.

    **Two rules the model now enforces rather than merely documents.**
    `RequirementState::Tasked` is defined as a task existing *and* a concurrence, and
    `TaskingCase::concur` checked only the second half -- a requirement could sit in
    `Tasked` with nothing serving it, which the analyst who stated it would read as work
    in hand. It now refuses without a task, and PN-15's tasking control issues the
    command first so the concurrence is recorded only if a task really came out of it.
    A decline now requires a reason, for the reason PN-07 requires one to reject a plan.

    **`Concurrence` is a new model type, and it is the interesting one.** DN-11 §5 and
    the CAP-2.12 criterion both require a concurrence to carry an operator, and this
    deployment has no operator session (GAP-057). A bare `Option<String>` would leave an
    absent name ambiguous between "nobody is signed in" and "we failed to record who" --
    the same failure DN-10 fixed for expiries. So the two are separate variants, and
    `Concurrence::operator()` is the predicate the verification criterion is written
    against. Every concurrence this build can produce is an `UnattributedRole`, and PN-15
    says so in as many words rather than letting a role name stand in for a person.

    **The authority split is drawn from the matrix rather than invented.**
    `Role::IntelligenceAnalyst` deliberately does not hold `sensor.task`, because §4
    makes tasking a request from the analyst rather than authority they hold. So stating
    a requirement is not an authorized action at all, concurring and declining are
    `sensor.task`, and answering is the analyst's judgement gated on naming evidence. An
    analyst is told which half is theirs instead of being handed a control that would be
    refused.

    **`Event` gained `Requirement(RequirementEvent)`, which DN-11 §6 does not list.**
    Without it the lifecycle would live only in memory, and CAP-2.12's method is an MT-08
    *replay* -- a concurrence recorded and never journalled cannot be reviewed, which is
    the whole point of recording who concurred. **DN-11 amendment 1, signed by the owner
    on 2026-09-05**, records this and the six other corrections GAP-004 and GAP-005 made
    to that note. `SnapshotResponse` gained `requirements` as §6 does specify, defaulted
    so a client written against the first v2 payload still decodes.

    Requirement areas come from the defended assets the baseline declares, because
    nothing on any screen turns a click into a position yet; a deployment with none
    declared gets a stated reason rather than an empty picker. `PanelAction` stopped
    being `Copy`, because PN-15's actions carry the operator's own words.

    **No new dependency edge**: `gungnir-workflow` to `gungnir-sensor-management` was
    already taken and already drawn in §7.1 (b), and no manifest changed.

    **GAP-005 is In progress, not Closed.** The requirement list lives for the session
    and is not rebuilt at startup, so an analyst's requirements are gone tomorrow; PN-15
    says so rather than implying otherwise. CAP-2.12's row stays ungated: its method is
    an MT-08 replay, and its criterion asks for a concurrence carrying an operator, which
    no build can produce until GAP-057.

59. **The v2 transport, read paths only** (2026-09-05, GAP-041). `gungnir-api` gained
    `transport`: an `axum` router serving `GET /v2/snapshot`, `GET /v2/health` and the
    `GET /v2/events` WebSocket, and `gungnir-remote` gained `link`: a `reqwest` and
    `tokio-tungstenite` client behind the same two service traits the embedded profile
    uses. `gungnir_remote::connect` no longer returns `TransportNotImplemented`, and
    `gungnir-remote/tests/transport.rs` drives a real server on a real loopback socket
    with the real client. **The connected profile can be exercised for the first time.**

    **The write paths are routed and return 501.** Nothing can authenticate a caller:
    `ApiHandler` names an `OperatorId` on every call, and `gungnir_security::Authenticator`
    -- the trait that would produce one -- has **no implementation**. (An earlier version
    of this item said the trait did not exist. It does, in
    `gungnir-security/src/authn.rs`; what is missing is any implementation of it, which is
    GAP-057. The conclusion is unchanged: nothing can turn a credential into an identity.)
    `ApprovalRequest` carries the operator *in the request body*, so an unauthenticated
    write path would let any caller assert any identity and have a plan accepted under it.
    Routing them and refusing is deliberate: a 404 would say the endpoint is not part of
    v2, which is false.

    **Only loopback is bound.** There is no TLS, and `transport::serve` returns
    `ApiError::UnprotectedBind` for anything routable rather than carrying
    command-and-control traffic in plaintext. `NodeConfig::bind_addr` therefore defaults
    to `127.0.0.1:7410` rather than `0.0.0.0:7410`: the old default could only have been
    refused. The client refuses an `https` endpoint for the mirror-image reason, rather
    than quietly speaking plaintext to an address somebody wrote as `https`.

    **Two register facts settled the scope, and they disagreed with GAP-041's own
    closing action.** That action said to implement mutual TLS here; but GAP-060
    ("encryption in transit and at rest") owns TLS, targets I4, and **depends on
    GAP-041** -- so the dependency graph puts the transport first and TLS after. It also
    depends on GAP-084, whose DN-22 design is deliberately `seal`/`unseal` and **never
    releases key bytes**, so it cannot hand `rustls` a private key. How a TLS identity is
    drawn from that provider is a real question GAP-060 has to answer, and answering it
    here by inventing a `get_key` would have broken the custody boundary DN-22 exists to
    draw. GAP-041's action text is corrected to match the graph.

    **A heartbeat, because a dead link otherwise looks alive.** A quiet node and a node
    whose host vanished put exactly the same thing on a TCP connection -- nothing -- and
    a half-open connection can stay open a long time. The server pings an idle stream
    every 10 s and a client that hears nothing for 35 s drops the link. Without it a
    desktop would show a picture under a "connected" light from a node that stopped
    existing minutes earlier, which is the worst failure this transport could have.

    `from_seq` is honoured from a bounded in-memory window, and a client asking for
    further back than it reaches is **told to take a fresh snapshot** rather than handed
    a truncated stream, which is the contract's own rule. The window is smaller than the
    journal's retention and the code says so.

    **Crates**: `axum` (`ws`) in `gungnir-api`; `reqwest`, `tokio-tungstenite` and
    `futures-util` in `gungnir-remote`, all from D-18's signed stack and §2.9. Two
    additions to §2.9: `futures-util`, named directly for the `Sink`/`Stream` traits the
    socket implements and already in the tree beneath `tokio-tungstenite`; and the `net`
    feature on the existing `tokio` pin. `rustls`, `tokio-rustls`, `rustls-pemfile` and
    `tower-http` stay unused, waiting for GAP-060. §2.9's single-copy claim was
    re-checked against today's registry and holds: one `rustls`, one `tungstenite`, one
    `tower-http`, one `hyper`.

60. **Authentication designed, and stopped at a decision** (2026-09-05, GAP-057).
    `gungnir_security::Authenticator` is a trait with **no implementor**. The
    authorization half is real -- `Authorizer`, `role_permits`, the matrix -- and is never
    reached, because nothing turns a credential into an `OperatorId`.

    Three built things already work around it, each having had to invent a way to say
    nobody was signed in: `DecisionRecord::operator_id` is `None` on every decision
    (item 47), `Concurrence::UnattributedRole` exists **only** for this reason (item 58),
    and both v2 write paths return `501` rather than accept a request whose operator is
    asserted in its own body (item 59).

    `DN-23` designs the answer: `OperatorSession` and a `SessionState` that distinguishes
    nobody-signed-in from an expired session from an unreachable store; one mechanism per
    profile as D-02 chose; rate-limiting rather than a hard lockout, because an operator
    locked out of a console during an engagement is the worse failure; a failure that
    never says which half was wrong; and a disconnected desktop that **starts** when the
    account store is unavailable and says so, which is DN-22's encryption fallback applied
    to identity.

    **It could not be implemented.** No cryptographic crate is in the workspace at all,
    and passphrase verification and token integrity each need one. D-02 chose the
    mechanism and not the libraries -- exactly the gap D-18 filled before GAP-041 could be
    written -- so DN-23 §9 raises **D-20** and recommends `argon2`, `hmac`/`sha2` and
    `subtle`, arguing a symmetric MAC over public-key signing because the node issues and
    verifies its own tokens and a private key in the process is the conflict that stopped
    TLS in item 59.

    **The owner signed off the finding, and then D-20 itself, both on 2026-09-05.**
    `argon2` with `password-hash`, `hmac` with `sha2`, and `subtle` are in the workspace
    manifest and used by no member yet; `tonic` and `prost` were taken in the same pass as
    D-21, for the gRPC second transport D-18 had left open. **DN-23 was an unsigned
    draft and no code was written**, so what blocked GAP-057 was a sign-off on the design
    rather than a missing dependency. *(Signed and implemented later the same day; the
    finding is kept because it is why the blocker was recorded as a signature and not a
    crate.)*

    Two of DN-22's three key purposes are still unnamed and D-20 does not cover them: the
    journal at rest needs an authenticated cipher and baseline signing needs a signature
    scheme, and `hmac` authenticates without encrypting. Those belong to GAP-060 and
    GAP-084.

    The tempting alternative throughout was a session model without credential
    verification; it was not taken, because it would have filled `operator_id` and
    `Concurrence` with names nobody had checked, which is worse than the honest absence
    those types carry today.

61. **Operator sessions, and attribution that finally follows from one** (2026-09-05,
    GAP-057, DN-23 signed off the same day). `gungnir-security` gained `session.rs`:
    `OperatorSession`, a `SessionState` that distinguishes nobody-signed-in from an
    expired session from an unreachable store, an `AuthFailure` that never says which half
    of a credential was wrong, and `LocalAccountAuthority` verifying passphrases with
    `argon2` against an `AccountStore`.

    **The point of the whole chain is item 58's sentence coming true.** Three earlier gaps
    had to invent ways to say the system did not know who acted -- a `DecisionRecord` with
    no operator (item 47), `Concurrence::UnattributedRole` (item 58), and two v2 write
    paths returning `501` (item 59). None of them is removed. `AppState::attributed_operator`
    is now the single place attribution is obtained, and a decision or a concurrence names
    an operator **only when one was actually verified**; an expired session and a missing
    store both still yield nothing, and the record still says so.

    **A desktop with no account store starts.** It comes up in
    `SessionState::StoreUnavailable`, attributes nothing, and behaves as it did before.
    That is DN-22's journal-encryption fallback applied to identity: a console that refused
    to run because a keystore was missing would be a worse failure than one that runs and
    says what it cannot do.

    **Failures back off; they never lock out.** An operator locked out of a
    command-and-control console during an engagement is a worse outcome than a slow
    guessing attempt against a console already inside a defended network, so the delay
    doubles and caps, a correct passphrase clears it, and a test asserts that waiting gets
    you in -- which is the assertion that distinguishes a back-off from a lockout.

    **A test caught a real defect in the first implementation.** The store was only
    consulted when somebody tried to sign in, so a console where nobody *could* sign in
    reported `NobodySignedIn` -- an ordinary absence rather than the fault DN-23 §5 rule 5
    requires. `AccountStore::available()` is probed at construction now, and DN-23 records
    the correction.

    **Signing in decides attribution and not authority.** The role still governs what may
    be done and a session does not widen it, which a test asserts by signing in as an
    account whose role differs from the selected one.

    **The node half is not built**, in this note's own order: the token, `POST /v2/session`,
    and serving the two v2 write paths. `hmac`, `sha2` and `subtle` are signed off under
    D-20 and sit unused, where `rustls` sat between D-18 and item 59.

62. **The node authenticates its callers** (2026-09-05, GAP-057, DN-23 §6).
    `gungnir-security/src/token.rs` mints and verifies short-lived session tokens with
    HMAC-SHA256, compared in constant time. **A MAC and not a signature**: the node issues
    and verifies its own, so a public-key scheme would buy nothing and would put a private
    key in the process -- the conflict that stopped TLS in item 59. A token carries who and
    until when, and **not what may be done**: authorization stays with `role_permits` per
    request, so widening a role never means reissuing tokens and a stolen token never
    carries more authority than its operator has now.

    `POST /v2/session` is the one route reachable without a token, and **every other v2
    route now requires one**, the event stream included -- its token rides in the subscribe
    frame, because a WebSocket client cannot always set a header on the upgrade.
    `ApprovalRequest` still carries an operator in its body and that field is **not
    believed**; the caller is whoever the token says.

    **`POST /v2/detections` is served, and it queues rather than accepts.** The node
    registers a `ProtocolAdapter` that polls the queue, so a submitted detection is
    authenticated against the sensor allow-list and validated by exactly the code a
    sensor's feed goes through, and `202` says taken rather than believed. Reaching past
    `gungnir-ingest` would have been a second way in with no checks on it, and that crate
    is the trust boundary for external data.

    **`POST /v2/plans/{plan_id}/decision` still refuses, for a new and truer reason: a node
    runs no approval queue.** The desktop routes plans through the policy chain and the
    queue (item 45); a node publishes `PlanProposed` and stops. Serving it would mean
    inventing a queue in a request handler and putting the recommend-versus-act boundary in
    the transport. Whether a node should hold a queue at all is a real question and not
    this gap's to answer.

    **A node with no account store authenticates nobody and says so** -- which is the
    default deployment, and what a live run reports on every route. A node that served its
    picture to anyone who asked would be worse.

    **A consequence in the desktop**: connecting to a node is now an authenticated act, and
    nobody is signed in while `AppState` is built, so a deployment configured for a remote
    backend comes up embedded with an alert saying why rather than connecting with a
    credential it would have had to invent. Connect-after-sign-in is the remaining wiring.

63. **Encryption examined, and stopped at three blockers** (2026-09-05, GAP-060). None of
    them is this gap's to decide, and the most interesting is a real hole in a signed
    design.

    **DN-22's `KeyProvider` cannot produce a TLS identity.** It offers `seal`, `unseal`
    and `rotate` and deliberately no getter, and argues that correctly -- a provider that
    hands out key bytes has no custody boundary. But a TLS handshake needs a *signature
    over the transcript*, which neither operation can make, so `KeyPurpose::TransportIdentity`
    is a purpose no consumer can use. That is why item 59 serves loopback only, and the
    question it deferred here has an answer: **not a getter, but a `sign` operation.**
    `rustls` does not need key bytes -- `sign::SigningKey` is a trait, and a hardware
    module or a managed key service terminates TLS by implementing it. That is what
    DN-22 §5's own cloud row already assumes. The certificate chain is public and needs no
    custody; only the private half goes through the provider. Proposed as **DN-22 amendment
    1 (§9)**, signed by the owner later the same day along with D-22.

    **`seal` names no algorithm, and the sealed form has no shape.** §5 requires that
    rotation never rewrite existing data and that old material be read with the key that
    protected it, which is only possible if the ciphertext carries its `KeyId`. The note
    does not say it does. Amendment 1 (b) specifies the form, and the nonce discipline
    that makes AES-GCM safe rather than catastrophic.

    **No cipher, and no way to make a certificate in a test.** D-20 covered authentication
    and explicitly not these: `hmac` authenticates and does not encrypt. Mutual TLS cannot
    be verified at all without certificates, and the workspace must not gain a checked-in
    private key -- that is key material in the repository whatever the comment above it
    says. Raised as **D-22**, recommending `aes-gcm` and `rcgen` as a dev-dependency only.

    **And GAP-084 is unbuilt**: `KeyProvider` is a trait with no implementor and no
    consumer, so there is no custody to encrypt against yet. `rustls`, `tokio-rustls` and
    `rustls-pemfile` remain signed off under D-18 and unused, waiting for exactly this.

    Nothing was implemented. The half-deliverable available was TLS with a private key
    read straight into the process, which would have quietly answered the custody question
    this note exists to ask -- and answered it the wrong way.

64. **Encryption, in transit and at rest** (2026-09-05, GAP-060, with DN-22 amendment 1
    and D-22 signed the same day). Item 63 recorded three blockers; all three were lifted
    and the work is built.

    **The first `KeyProvider` that has ever existed.** `gungnir-security/src/provider.rs`
    holds keys, seals with AES-256-GCM, and carries the `KeyId` on the ciphertext -- which
    is what makes rotation possible without rewriting anything: a retired key still reads
    what it protected, and a reader holding only the new key still opens the old lines.
    Destruction is distinguished from retirement and says what it has made unreadable.
    `sign` exists per amendment 1 (a) and this provider refuses it, holding symmetric keys
    only; the asymmetric one a cloud deployment wants was blocked on D-22's third row, which the
    owner signed on 2026-09-05 (`p256`), so what it needs now is the code and not a decision.

    **Mutual TLS, and the loopback restriction is on plaintext rather than on the
    address.** A node with TLS configured may serve anywhere; one without it still may
    not. A client certificate is **required**, not requested, and the certificate, key and
    client authority come from the environment -- never a baseline, which may name no path
    to key material.

    **A test that passed while letting anonymous clients in.** The first version asserted
    that the TLS handshake completed. Under TLS 1.3 the client finishes before the server
    has validated its certificate, so `connect` returning `Ok` says nothing about whether
    the peer was accepted; the rejection arrives on the first read. Rewritten to make a
    request and read a response, it immediately caught that anonymous and
    wrong-authority clients were being refused correctly -- the implementation had been
    right and the test had been measuring the wrong thing.

    **Journal sealing, with the edge inverted.** DN-22 §4 says this note adds no
    dependency edge, and `gungnir-store` naming `KeyProvider` would be one -- so the store
    declares a two-method `JournalSealer` and the binary wires a provider to it. The store
    learns nothing about keys, ciphers or rotation, and §4 stays true.

    **A second defect a test caught, and this one was real.** `read_session` tolerates a
    torn final line, because a process that died mid-append never acknowledged that
    envelope. That tolerance was swallowing a *sealing* failure: a journal whose key was
    missing returned an **empty session** rather than an error, so an operator would see a
    mission that recorded nothing instead of a journal they could not read. A sealing
    error is now never treated as a torn line.

    **Switching encryption on does not orphan what is already recorded.** A plaintext line
    still reads when a sealer is configured, because AP-08 forbids rewriting an
    append-only record to migrate it. And a journal that cannot be sealed **fails the
    write** rather than falling back to plaintext, which would tell an operator encryption
    was on in a deployment where it had stopped.

    **What is not built**: the asymmetric provider for the cloud profile's TLS identity,
    and a node that actually configures a key -- nothing calls `seal_with` outside tests,
    so every deployment still journals in the clear and reports so. Baseline signing waited
    on D-22's third row, signed 2026-09-05: `p256` is in the manifest and no gap builds the
    provider yet.

65. **Key custody wired, and one word in a signed design that nothing behind it**
    (2026-09-05, GAP-084). The provider from item 64 now has a configuration, a
    deployment, and a place on screen.

    **`ConfigBaseline.security.key_provider`** names a custody model and never a secret,
    with validation that refuses a value looking like key material -- and does not repeat
    the offending value in the error, because an error message is the easiest way for a
    key to reach a log. A resource path is a *reference* and is explicitly not flagged,
    since that is exactly what the baseline should hold. The two persistent profiles are
    refused at validation as designed-and-unbuilt, so a deployment learns at start-up
    rather than discovering an unencrypted journal later.

    **`looks_like_key_material` moved to `gungnir-model`.** `gungnir-config` validates
    with it and `gungnir-security` owns custody; the edge `gungnir-config` ->
    `gungnir-security` would be a foundational crate depending on a productization one,
    which is backwards. So the shared item went to the lowest crate both reach, which is
    what §1.2 says to do and the move `SensorMode` and `SensorTaskId` already made. It is
    **not** re-exported from `gungnir-security`, because that crate has no workspace
    dependencies at all -- it is a leaf, and nothing outside its own tests used it.

    **The encryption state reaches the operator**, which is DN-22 §5's fallback applied to
    a security feature. Three states, not a boolean: encrypting, configured-but-unreachable,
    and never configured. Only the middle is a fault, and PN-01 and PN-09 word them apart.
    The state is derived from what the journal is **actually doing**, so a deployment
    cannot claim protection it is not performing. `EncryptionStatus` had been dead code
    since it was written.

    **A desktop starts either way.** A provider that cannot be built leaves the journal in
    the clear, raises an alert, and the console runs -- refusing to start for want of a
    keystore would be the worse failure. The `Ephemeral` provider is named for what it
    loses rather than where it lives, and raises its own alert: **the journal it produces
    is real ciphertext that nothing will read again after the process exits**, which is a
    silent trap otherwise.

    **What is still missing, and it is in the gap's own title: escrow.** The register's
    action names "escrow for recorded journals" and DN-22 mentions the word **only in its
    heading** -- §§1 to 8 never address it. `may_destroy` and `DestructionOverride` cover
    a key being deliberately destroyed; nothing covers a key being *lost*, or a journal
    needing to be read later by an authorised party who does not hold it. For a system
    whose journals are the record an investigation reads, that is a real hole in a signed
    design rather than a missing feature. Raised here; not invented.

    Neither persistent profile is built, so no deployment encrypts a journal it can read
    tomorrow. GAP-084 stays open on that and on escrow.

66. **Collection requirements survive a restart** (2026-09-05, GAP-005 closed).
    Item 58 left the list living in memory and dying with the process; an analyst who
    stated a requirement on Monday had nothing on Tuesday. `AppState` now recovers it from
    the journal at start-up, folding every session's `RequirementEvent`s in order.

    **A defect in item 58's own event, found by trying to use it.**
    `RequirementEvent::Stated` carried the requirement's *title* and not the requirement:
    the area, the priority and the needed-by time reached the bus nowhere, so the
    lifecycle was journalled and the thing it was about was not, and a rebuild was
    impossible. It now carries the whole `CollectionRequirement`, as
    `TrackingEvent::TrackInitiated` carries a whole `TrackView` and for the same reason.

    **`RequirementId` became per-deployment rather than per-session.** It was a serial
    starting at zero each run, which was harmless while the list died with the process and
    is a collision the moment one is recovered. The serial now continues past the highest
    identifier that came back.

    **A second defect, and a worse one: `AppState::save_session` did not save.** It called
    `journal.sync()` and never drained the event bus, so anything published after the last
    frame -- a requirement stated and then the window closed -- reached the bus, never
    reached the journal, and was gone. **An fsync of a file the envelope was not written
    to is a durable record of nothing**, and it looks like it worked. `save_session` now
    drains and then syncs, sharing `update::journal_pending` so the two paths cannot
    drift. This was reachable by every event type, not only requirements.

    **An empty list says which kind of empty it is.** "Nothing has been asked for" and
    "the record could not be read" look identical on screen and mean opposite things --
    the second says nobody knows whether collection is outstanding. PN-15 words them
    apart, and says so too when a list is *partial* because a session stopped reading
    part-way. A journal that cannot be read does not stop the desktop starting, which is
    the choice DN-22 §5 makes for a missing keystore.

    GAP-005 is closed. CAP-2.12's verification row stays ungated for the reason it always
    has: its criterion asks for a concurrence carrying an operator, and a deployment
    without an account store still cannot produce one (item 61).

67. **`GET /v2/coverage`, and an entry that said Closed while its status said Open**
    (2026-09-05, GAP-006 closed). The analytics were finished earlier the same day; what
    remained was the one thing that entry deferred, "`GET /v2/coverage` waits on the
    transport (GAP-041)" -- and GAP-041 landed in between.

    **Three things were wrong with the entry itself**, none of them in the code:
    its description began "Closed 2026-09-05" while its status field still read `Open`;
    it still carried the nominal-coverage caveat that GAP-003 removed, which item 56 said
    had been "deleted rather than reworded" for GAP-006 and GAP-007 and had in fact only
    been deleted from GAP-007's; and PN-16, the planning panel, was attributed to it by
    the UX map and `workspace.rs` although this gap is an analytics function and its
    closing action never mentions a panel. **Repointing it to GAP-026 was also wrong** --
    that is the defended-asset list, which supplies PN-16's content and builds no panel.
    **No register entry built the planning surface at all**, which GAP-055 recorded at the time and GAP-087 now is (item 77).

    **A correction to DN-12 §6.** It wrote the response as a bare `Vec<CoverageGap>`,
    and §5 of the same note puts the sampling spacing and whether terrain masking was
    applied *on the result*, so that a coarse run cannot be mistaken for a fine one.
    Serving only the gaps would discard exactly what that rule preserves, so the route
    returns the whole `CoverageReport`. **Recorded in DN-12 and signed by the owner
    2026-09-05**, which also covers the `CoverageResponse::NotComputed` addition made for
    the same reason.

    **A node that computed nothing says so.** `CoverageResponse::NotComputed` carries the
    reason -- no declared local frame origin, or no approaches -- because "no gaps were
    found" and "no coverage was computed" are opposite claims about a sector and an empty
    list reads as the clean one. The answer is computed on the tick rather than in the
    request handler, so a caller's polling rate cannot decide the node's load.

    **Two dependency edges, argued and drawn before they were taken**: `gungnir-api` to
    `gungnir-analytics` (f), because the contract publishes a `CoverageReport` and must
    name the type rather than defining a second one that would drift from the picture; and
    `gungnir-node` to `gungnir-analytics` (g), because the node computes the report it
    serves and there is nobody else to compute it. Both acyclic.

68. **PN-11, the coverage layer controls** (2026-09-05, GAP-007). The entry was
    correctly closed -- the rings and gaps have been drawn since earlier the same day --
    but PN-11 was still attributed to it and unbuilt, the same mismatch item 67 found for
    PN-16. Building it is what makes the attribution true.

    **A visibility control on a coverage map carries one specific risk**: turning a layer
    off makes the map look like a sector with nothing wrong in it. So the toggle is not
    offered on its own. The panel carries each layer's count on its own control, so an
    operator can see what turning a layer back on would show without turning it back on;
    the panel says a hidden layer is not an empty one; and **the viewport itself draws a
    line saying which layers are hidden**, because that is where somebody reading the
    sector is actually looking.

    One `draw_layers` serves the 2D projection and the three-d scene, so a layer cannot be
    hidden in one and drawn in the other -- which would make the toggle mean different
    things depending on which renderer was in front.

    The toggles are session state and not baseline: hiding a layer to read the map
    underneath is a thing an operator is doing now, and the next launch draws everything
    again. Both default to on, because a viewport that started with coverage hidden would
    look exactly like one with no coverage to draw.

    **Before-and-after comparison is not built and is not this gap's.** The information
    architecture lists it against PN-11; it means comparing two laydowns, which needs the
    options PN-16 draws -- and **no entry builds that panel**. A control comparing the
    current picture with itself would do nothing, so the panel names GAP-055, which tracks
    the unbuilt ones.

69. **The defended-asset list is scored against, and a mis-attribution I made twice**
    (2026-09-05, GAP-026).

    **The correction first, because it was mine.** Item 67 found PN-16 wrongly attributed
    to GAP-006 and repointed it to GAP-026. That was also wrong: GAP-026 is the
    *defended-asset list*, which supplies PN-16's content and builds no panel. **No
    register entry builds the planning surface at all** -- which is the actual finding, now
    recorded against GAP-055, the entry that tracks unbuilt panels. PN-11's
    laydown-comparison notice was repointed with it.

    **The gap itself was half-done in the way this register keeps being half-done.**
    `AssetConfig` was in the baseline with validation, `DefendedAsset` and `AssetListView`
    in the model, and `AssetListAssessor` in `gungnir-assessment` with its own tests --
    and **no binary constructed an assessor of any kind**. `ThreatAssessor`, `RiskScore`
    and `ClosingSpeedAssessor` were all unreachable from a running system.
    `anchor_list`'s own documentation calls it "a convenience for the binaries"; there
    were no callers.

    The desktop now anchors the list at the declared origin and scores tracks against it,
    ranked highest-risk first, which is the order a saturation triage reads them in
    (MT-01).

    **Three different empties, kept apart.** `AssetRanking::NotScored` carries a reason,
    because "no local frame origin, so assets and tracks cannot be put in one picture",
    "no defended assets, so there is nothing to rank against" and "scored, and no track
    was in range" are three states an operator must not confuse -- and the last one is the
    only one that means the sector is quiet. That is precisely what
    `AssetListAssessor::is_unconfigured` was written for and what nothing was calling.

    Ranking is empty in this build because there are no tracks, which is a fourth thing
    again and belongs to the pipeline.

70. **Scenario rehearsal is blocked, and precisely how is worth recording**
    (2026-09-05, GAP-045, not built). Its closing action asks for scenario output fed
    through the recorded adapter under the replay clock against a selected baseline.
    **`gungnir-app/benches/app_tick.rs` already composes exactly that**, and runs today.

    What is missing is the pipeline. `PIPELINE_IMPLEMENTED` is false, the tracking stage
    accepts detections and produces no tracks, and with no tracks there is no plan. A
    rehearsal built on the existing plumbing would show a planner zero tracks and no
    recommendation, and **MT-09's question is what the sector would recommend** -- so the
    feature would answer the one question it exists to ask, confidently and emptily.

    Recorded rather than half-built, so the next attempt does not re-derive it. The
    remaining substance is GAP-011, which is Area A; GAP-051, its other dependency, is
    not blocked.

71. **The session lifecycle is implemented and both binaries go through it**
    (2026-09-05, GAP-051). `MissionManager` was a trait with **no implementation**, and
    create/save/load/replay existed only as signatures. Both binaries minted a session
    identifier from the wall clock, declared the mission `Live`, and wrote nothing down:
    a journal on disk had no record saying which baseline produced it or how it ended.

    `JournalMissionManager` writes `<id>.mission.json` beside the journal. Three things
    it does that the trait alone could not:

    - **`MissionState::Interrupted`.** A session that was live and never closed is not
      `Paused`, which is somebody's decision, and it is certainly not `Live`, which is a
      claim that sensors are feeding it now. Reopening a record marked live yields
      `Interrupted`, and the desktop and node both report it -- because the hole in that
      record is not in the current one, and a review reading them together must not miss
      it. Nothing returns to `Live` except a deliberate pause.
    - **The baseline is stored with the session, not looked up.** A session replayed
      under today's baseline would be judged by policy settings and authority rules it
      never ran under, and the verdicts a review read would be ones nobody made.
    - **Identifiers are allocated past both stores.** Records alone are not enough: a
      data directory whose mission records were deleted while its journals survived would
      hand a new session an identifier a journal already holds, and its replay would
      return somebody else's events.

    **A conflict worth recording.** `create` first refused a baseline that did not
    validate, and that broke the desktop's deliberate behaviour for GAP-084: it starts
    under an unbuilt key provider, journals in the clear and says so, because a console
    that will not open protects nobody. Refusing decided something the host had already
    decided. The baseline is now checked and the objection **recorded on the mission**,
    surfaced as an alert and kept in the record for the review -- so neither the decision
    nor the objection is lost.

    Not taken: an edge to `gungnir-replay`. That crate is a *cursor* -- open, step, seek,
    rate -- which is what PN-12 scrubs with. The lifecycle needs the recorded sequence in
    order, once, which is `gungnir_store::EventJournal::read_session`.

72. **Plan validity is enforced, and an authority rule can no longer be misspelled in
    silence** (2026-09-05, GAP-052). The entry was **Closed and should not have been**:
    it was closed on the schema half of its own closing action, and the four criteria in
    its verification row went unread. Three of them were unmet.

    - **`is_promotable_at` had no caller.** The method that answers "may this baseline be
      promoted now?" shipped with DN-08, had its own tests, and nothing invoked it, so an
      expired baseline went into force without objection. `ConfigStore::apply` now takes
      `now` and refuses outside the window (`ConfigError::NotPromotable`). `now` is a
      required argument rather than a clock read inside the implementation: promotion is a
      time-dependent act, and an implementation with its own clock could disagree with the
      session's.
    - **`InterceptEvent::PlanSuperseded` was declared and never published.** A plan
      produced under a baseline outside its window is now superseded *before* policy runs,
      not after: reporting it as `Denied` would put a refusal nobody made into the record,
      and `RequiresHumanApproval` would leave an item waiting that will never be applied.
      `decisions::submit` returns `Submitted::{Evaluated, Superseded}` for exactly that
      reason -- a policy verdict could not express it.
    - **DN-08 §6 rule 3 was enforced nowhere.** `actions::ALL`, whose own doc-comment says
      it exists "for validating an authority rule at load", had zero callers. This is the
      one validation failure that is *silent*: a rule reading `plan.decid` passes every
      other check, matches no request, and looks in the file exactly like a grant.
      `gungnir-config` may not depend on `gungnir-security`, so the vocabulary is supplied
      by the caller -- the inversion DN-22 §4 used for journal sealing, and no new edge.
      It is a **required** argument on `FileConfigStore`, because a store that could be
      built without one would let a caller skip the check, and skipping it looks exactly
      like passing it. An empty vocabulary is refused rather than accepting every name.

    PN-01 gained the baseline validity element DN-08 §7 asks for. Without it, a deployment
    outside its window has a calm approval queue and a finished-looking decision dialog --
    indistinguishable from a quiet sector, which is the failure this project keeps finding.

    `docs/ux/information-architecture.md` §2 was two elements behind the code (journal
    encryption from GAP-084 was never added, and the control-status row still said "not in
    code"); both are corrected here.

    The two interface changes to `gungnir-config` -- `apply` taking `now`, and the
    caller-supplied vocabulary -- are mechanisms DN-08 did not specify, because its §6 said
    "Interface: no change". Recorded as **DN-08 amendment 1, signed by the owner
    2026-09-05**, which also covers the code that conforms to it.

73. **Model governance is blocked on three things, not one, and one real defect in it
    was fixed** (2026-09-05, GAP-053, not wired). Its closing action asks the tracking
    service to take its algorithm configuration from the promoted baseline at session
    start. Recorded rather than half-built, so the next attempt does not re-derive it.

    **`gungnir-modelops` has no dependents.** No crate in the workspace imports it, so the
    registry, the promotion state machine and rollback are unreachable from any running
    system -- and `ConfigBaseline.tracking` is validated by `gungnir-config` and read by
    nobody either.

    Three things are missing, and the dependency list names only the first:

    - **A pipeline to apply it.** `PIPELINE_IMPLEMENTED` is false, and
      `gungnir_fusion_async::ingest` -- what would take a filter selection and a gate
      threshold -- is human-owned Area A. The gap cannot reach its own impact statement.
    - **Something to choose between.** `ModelBaseline` is per mission profile, and
      `mission_profile` appears nowhere outside `gungnir-modelops` and the documents that
      describe it. The baseline schema carries one `TrackingConfig` and no profile, so a
      registry wired today would validate and promote a single candidate with no
      alternative: **a promotion ceremony that would read as governance and be none.**
      That is the same reason GAP-045 was left unbuilt.
    - **A design note.** CAP-5.7 has none. Every comparable gap closed this month was
      built against one.

    **What was fixed, because it is real today and its danger is that it is invisible.**
    `LiveTrackingService` stamped `Provenance::algorithm_version` with the
    *tracking-service crate version*, in a field `gungnir-model` documents as the version
    of the algorithm configuration that produced the track, resolved through
    `gungnir-modelops`. No baseline is consulted, so the field answered a different
    question than it asks. No track exists to carry the stamp today -- which is precisely
    why it would have survived: the day GAP-011 lands, every track it produces would carry
    a semantic version that reads as a governed configuration. It now says what is missing
    and keeps the build identifier, pinned by a test.

74. **The watch's rhythm runs** (2026-09-05, GAP-054). DN-21 said "design only; no code
    exists" and that was already wrong: `gungnir-reporting/src/rhythm.rs` held the types
    and their tests, and **nothing constructed any of them**. No schedule was read from a
    baseline, no maintenance window reached a sensor record, and no handover was ever
    assembled -- the same shape as GAP-051 and GAP-005.

    Now: the baseline carries `reporting.scheduled` and per-sensor `maintenance`, both
    validated (a product delivering to an undeclared endpoint is refused, and so are
    overlapping windows for one sensor, which would put it in two maintenance states at
    once and hide the first one's overrun); the registry runs the window state machine and
    reports transitions once each; the desktop's tick produces due products and assembles
    the handover; PN-09 tells planned downtime from failure; PN-17 carries the handover
    with its notes and its acknowledgement. The node runs the window half too -- it is the
    system of record for every desktop connected to it, so an overrun belongs in its
    journal -- and not the products, because those are a watch's paperwork and a headless
    node has no watch.

    **The scheduler is on mission time**, which is the criterion a wall-clock
    implementation would pass every other check while failing, so it has its own test: the
    same session driven twice with deliberately uneven tick steps produces the same
    products at the same times, and those times are the schedule's rather than the tick's.

    **Three states of silence, kept apart.** A sensor radiating, a sensor down inside a
    planned window, and a sensor down outside one are three different things, and only the
    last is a fault. A window that closed with the sensor still down is a fourth and is
    louder than any of them. Collapsing them into a health boolean is how a scheduled
    outage becomes an unnoticed hole -- and how a real failure gets shrugged off as "that's
    the maintenance window". **A window never suppresses a coverage gap**: the gap is real
    whether it was planned or not, and there is a test for that specifically.

    **Nothing is delivered, and it says so.** There is no delivery path (GAP-040), so a
    product with a configured endpoint publishes `ProductUndelivered` naming the endpoint
    and the reason. A deployment that believed it was reporting to higher command and was
    not is what DN-21 §5's delivery rule exists to prevent, and a silent no-op would have
    produced exactly that.

Three departures from DN-21 -- the shared types placed in `gungnir-model` rather
    than in one consumer (either placement the note offers would force an edge, and §4 says
    the design adds none), one `Event::Rhythm` variant rather than a `SensorTaskEvent` one
    (every variant of that enum carries a task id, and a maintenance window is not a task),
    and the undelivered-product event. Recorded as **DN-21 amendment 1, signed by the owner
    2026-09-05**, which also covers the code that conforms to it.

75. **The GAP-053 blocker that nothing tracked now has a design note and an entry**
    (2026-09-05, DN-24, GAP-086). Item 73 recorded three reasons model governance could not
    be wired. The first is GAP-011 and is Area A. The other two -- no mission-profile
    concept or candidate configurations in the schema, and no design note for CAP-5.7 --
    were nobody's work, which is how a blocker outlives the thing it blocks.

    DN-24 gives the mission profile a schema rather than a meaning: the word is already
    CAP-5.7's and `gungnir-capabilities.md` §5.4's, and what is missing is the declaration.
    Profiles are declared by name the way endpoints are, candidates reference them by name,
    and **exactly one candidate per profile is promoted** -- zero means the deployment
    cannot say what is running, two means it cannot say either and would report whichever
    the iteration order reached first.

    Three decisions in it worth reading before signing. `tracking` and `tracking_profiles`
    are **mutually exclusive**, because both present is two answers to what is in force; a
    baseline carrying only `tracking` is read as one implicit `default` profile, so no
    existing file breaks and `SUPPORTED_CONFIG_VERSION` stays 1. The baseline declares the
    starting position and the registry governs the session -- a runtime promotion is
    journaled and does not rewrite the file, and persisting one is the already-governed
    `config.apply`. And `actions::PROMOTE_MODEL` gets its **first check**: it has been in
    the authorization table since the roles landed, granted to the analyst, and nothing has
    ever checked it because nothing promotes.

    **What the note deliberately does not do**, and the rule it leaves for GAP-011: the
    tracking service may stamp an `AlgorithmBaselineId` into `Provenance` only once it
    actually applies one. A stamp that ran ahead of the pipeline would put a
    governed-looking version on every track in the journal and nothing downstream could
    tell -- which is the defect item 73 fixed, re-entering by the front door.

    **Not signed off:** DN-24 is a first draft with no code.

76. **A deployment can say which algorithm configuration it is running** (2026-09-05,
    GAP-086, DN-24 signed the same day). `gungnir-modelops` had **no dependents at all**;
    the registry, the promotion state machine and rollback were unreachable from any
    running system, and `ConfigBaseline.tracking` was validated by `gungnir-config` and read
    by nobody.

    The baseline now declares `mission_profiles`, `tracking_profiles` and `active_profile`,
    with the six rules of DN-24 §6. **Exactly one promoted candidate per profile** is the
    one that matters: zero means the deployment cannot say what is running, two means it
    cannot say either and would report whichever the iteration order reached. `tracking` and
    `tracking_profiles` are mutually exclusive, and a baseline carrying only `tracking` is
    read as one implicit `default` profile — so no existing file breaks and
    `SUPPORTED_CONFIG_VERSION` stays 1.

    Both binaries construct the registry **through the real state machine**: candidates are
    registered, run through the registry's own gate, and the one the file marks promoted is
    promoted. The file asserting `promoted: true` does not bypass `promote`'s requirement
    that a baseline be `Validated` first, which is the point of running the gate rather than
    trusting the flag. A refused baseline does not take the console away from an operator:
    the desktop starts, governs nothing, and says why.

    **Identity gained a name.** The registry was keyed on the profile and matched by
    *configuration*, so two candidates in one profile with the same settings were one
    baseline and a rollback could not say which it restored. `AlgorithmBaselineId` is the
    profile and the candidate's own name, and there is a test that rolls back between two
    identically-configured candidates.

    **What is journaled is what happened.** `InForceAtStart` rather than `Promoted`, because
    nobody promoted anything at startup and `by` would have been fabricated; `NoneInForce`
    rather than silence, because "nothing was in force" and "we did not record it" are
    opposite claims to a reviewer. Once per session, not once per frame.

    **The rule left for GAP-011 is unchanged and now has a test**: the tracking service may
    stamp an `AlgorithmBaselineId` into `Provenance` only once it applies one.
    `governance::would_stamp` resolves the identity and nothing stamps it, because
    `gungnir_fusion_async::ingest` ignores any configuration it is given.

    **Two edges and two corrections, all in this change.** `gungnir-app` and `gungnir-node`
    to `gungnir-modelops`, both downward from a binary. And `gungnir-modelops` to
    `gungnir-model`, which **DN-24 §5 did not list** -- the crate was one of the four the
    graph names as not using the model, and the identity types have to live there because
    `Provenance` does. The corrections to §6 (nothing checks `model.promote` in this
    increment, because §9 defers the panel that would reach it) and §8 (two event variants
    the note did not name) are recorded in DN-24 and, with the missed edge, **signed by the
    owner 2026-09-05**. The engineering reviewer has still seen none of the three edges.

77. **Role workspaces close, and the misattribution that outlived three corrections now
    fails a test** (2026-09-05, GAP-055, GAP-087 filed).

    The binding was done: twenty panels with their PN numbers, `WorkspaceLayout`
    separating docked from on-demand, eight role layouts transcribed from
    `docs/ux/information-architecture.md` §1 and tested against it, and the desktop drawing
    the signed-in role's workspace. **Sixteen of the twenty panels are built**; the entry's
    own description said the inverse ("sixteen of the twenty are designed and unbuilt") and
    its closing action said seven remained. Both were stale and the count was four.

    **PN-16 had been pointed at three entries in turn, none of which builds a panel.**
    GAP-006 is the coverage-gap analytics function; GAP-026 is the defended-asset list that
    supplies PN-16's content; then item 68 pointed it at GAP-055, the entry that merely
    *tracks* unbuilt panels, as the only honest answer available. Each looked right.
    GAP-087 is now the entry that builds it, and PN-11's laydown-comparison notice points
    there too.

    **The test that was supposed to catch this checked only that the string started with
    `GAP-`.** It now checks the identifier against the generated register, for the panel
    placeholders and for every `Unavailable` this file declares — so a well-formed
    identifier for an entry nobody ever opened fails, rather than sending an operator to
    look for work that does not exist. Verified by pointing PN-16 at `GAP-999` and watching
    it fail.

    GAP-087 is blocked on two things, and the first is the shape GAP-086 had just fixed
    elsewhere: **the baseline carries one set of sensor and resource positions**, so a
    laydown options table would have one row and nothing to compare. The second is GAP-045's
    rehearsal record, which a submit is supposed to carry. A panel built before either would
    offer a comparison with one option and a rehearsal button that cannot run.

    Not closed by any of this: **MOP-37 still has no baseline**, which is the usability
    round's numbers and belongs to GAP-074.

78. **Ten gaps from GAP-056: five built, five examined and recorded** (2026-09-05).

    **`todo!()` is gone from the workspace** (GAP-082). Every one of the sites was the body
    of a public function, so the claim that none was reachable was never true: a public
    function is not a function nothing can call. Each is now a named error, and several
    needed a signature change because the alternative default was worse than an error --
    an empty track set from a PHD filter claims nothing is out there, a zero registration
    bias claims two platforms are perfectly aligned, an identity transform from ICP claims
    two clouds already match. **The count was 22, not the 18 a `todo!()` grep found**: four
    take a message and were invisible to every count this entry has carried (32, then 22,
    then 18). `gungnir-app/tests/no_reachable_todo.rs` scans the source rather than arguing
    about call graphs, because reachability is not a stable property and the refactor that
    calls a private helper will not think to check.

    **Two service contracts stopped overstating themselves** (GAP-066).
    `submit_detection` returned `()`, so the ingest gateway counted a detection as accepted
    and published `IngestEvent::Accepted` *before knowing whether the pipeline took it* --
    an ingest rate that looked healthy while nothing reached the tracker. `plan` returned a
    bare `PlanView` while the planner's documented behaviour is to return the last good one
    on failure, so **a stale plan was indistinguishable from a fresh one**. `PlanOutcome`
    separates fresh, stale-with-a-timestamp, and no-plan-at-all; the last is deliberately
    not an empty plan, because one says nobody could compute a recommendation and the other
    says the recommendation is to do nothing. Neither binary proposes a plan it was not
    given fresh. **The security-context half was struck by the owner on 2026-09-06** (GAP-066
    closed). The argument: authentication lives at the gateway, authorization at the
    point of action, and releasability at the point of release -- three homes, none of
    them the facade -- and a context type would either give a facade an edge to a
    productization crate or be a type every implementation carries and none can check.

    **Identities are UUID v7** (GAP-069). The counter they replaced was unique in one
    process and collided the moment two nodes exchanged identities, which is the whole
    point of a *global* entity id.

    **The last two performance harnesses exist** (GAP-056), and building the connectivity
    one found a budget the implementation cannot meet: fallback after link loss is budgeted
    at 2 s, and a node that goes *silent with the socket open* is not noticed for 35 s. The
    numbers cannot be reconciled by tightening the timeout alone -- the beat is every 10 s
    -- so it needs a decision. Recorded in `docs/performance-budgets.md` and pinned by a
    test that fails when the problem is fixed. The fuzz corpus is seeded from the sample
    sets (GAP-076), with a test that every seed still decodes, because `gungnir-fuzz` is
    outside the workspace and a corpus of rejected inputs rots silently.

    **Five recorded rather than half-built**, each with a specific reason. GAP-064:
    **neither the ASTERIX nor the STANAG specification is in this repository**, and a codec
    written without one would pass its own round-trip test and fail against every real
    radar -- the most confidently wrong outcome available, and worse than the honest
    `NotImplemented` there now. (Update 2026-09-06: the ASTERIX Category 048 decoder is
    built to the pinned edition 1.32 and reads every block of a public radar capture,
    and the Category 034 service-message decoder to edition 1.29 behind its own
    `ServiceMessageCodec` boundary; Category 048 encode and STANAG 4676 remain
    `NotImplemented`.
    `docs/design/external-standards.md` §1.8.) GAP-063 waits on it. GAP-065 composes two open gaps, and
    exchanging a picture before releasability marking exists is the one ordering that must
    not happen. GAP-078 is buildable and premature: no model exists to promote, and a
    manifest with no producer is the unwired pattern. GAP-057 surfaced the sharpest of
    them: **`AppState::sign_in` exists, is tested, and has no caller outside tests** -- the
    desktop can authenticate an operator and nothing ever asks it to, because there is no
    sign-in surface and the twenty-panel catalogue has none.

79. **The heartbeat and the connectivity budget agree, and the operator can see a link
    going stale** (2026-09-06, D-23). GAP-056's connectivity test found the budget --
    fallback under 2 s from the last heartbeat -- beside a 10 s beat and an independent
    35 s timeout: a node that went *silent with the socket open* was unnoticed for 35 s,
    and that is the case the budget exists for.

    Not fixed by tightening the timeout, which at a 10 s beat would declare healthy links
    dead between beats, and not fixed by forcing the number either: a half-second beat
    with a 2 s timeout turns every 2 s stall on an intermittent link into a fallback and a
    full reconnect, on exactly the links the connected profile is for. **The budget was
    conflating visibility with declaration**, and D-23 states both. The beat is 2 s, so
    PN-01's "heard N s ago" is a meaningful freshness number -- at 10 s it read "9 s" on a
    healthy quiet link. Declaration is within four beats, three misses tolerated. And the
    timeout is **derived from the beat in code**, because two independently edited
    constants is how the conflict arose.

    Raising the budget to match the code was rejected as the widen-the-criterion move the
    workspace forbids. The pinned test now asserts the relationship rather than the
    numbers. The constant reaches the desktop through `gungnir_remote::link`'s re-export,
    not a new edge from `gungnir-app` to `gungnir-api`.

    Also corrected on the way: PN-01's remote arm carried the comment "no transport exists
    yet (GAP-041)", which stopped being true the day GAP-041 landed; the true reason that
    arm is unreachable is GAP-057's -- nothing calls `connect` because there is no sign-in
    surface.

### Open

**Rewritten 2026-09-07, in the development-status review.** The seven bullets this
section carried before that date named the tracking math, intercept geometry, the
three-d attachment, the API transport, and cross-session identity as unbuilt; by
2026-09-07 all of that was built, gated, and in five of those cases already recorded
as such by later numbered items in this very section (92 to 94, 98, 100) that this
list itself was never updated to agree with. The bullets below are what a reading of
the code on that date actually found still open; nothing here was verified by
re-reading the register alone.

- **The tracking math's last filters.** `gungnir-rfs`'s CPHD cardinality distribution
  and the GLMB/LMB labelled filters return `NotImplemented` naming themselves; the
  Gaussian-mixture PHD they would extend is built and gated (item 94). Everything
  else this bullet used to list -- the EKF, UKF, particle filter, IMM, square-root/UDU
  form, RTS smoother, JPDA, MHT, track-to-track fusion and registration, the
  allocator, and the out-of-sequence pipeline itself -- is built and gated;
  `PIPELINE_IMPLEMENTED` has been `true` since 2026-09-06 (item 92, GAP-011). (GAP-015)
- **The GPU point-cloud registration path, and the compute context that has never
  been created.** `gungnir-data-fusion`'s CPU reference ICP is built and tested
  (`src/cpu_reference.rs`, `src/transform_solve.rs`); the GPU step returns
  `NotImplemented` naming the WGSL pipeline of §3.4 it waits on, and the four
  `shaders/*.wgsl` files hold that section's stage comments and no code. Reviewed end
  to end 2026-09-08, the path is inert further back than the shaders:
  `GpuContext::new` has **no caller** -- neither `gungnir-app` nor `gungnir-viewport3d`
  references `gungnir_render` or `gungnir_data_fusion` in source, though §7.1 draws
  both manifest edges -- so no `wgpu` device exists at run time and the only GPU work
  the application does is the viewport's OpenGL drawing (§9). The `gpu-tests` feature
  is declared and empty, so `gpu-fusion.yml` would run zero tests and fails such a run
  on purpose. **The `gpu` runner is registered as of 2026-09-08** (`gungnir-rtx-5060ti`,
  on the drafting host's RTX 5060 Ti), which was GAP-061's remaining item, and the
  workflow **stays on manual dispatch permanently** (D-10 as amended the same day):
  dispatch on a self-hosted runner is local execution with a recorded log, and only a
  caller with write access can fire it, which a `pull_request` trigger on a public
  repository would undo. It stays dormant until GAP-024 writes the tests. (GAP-024, and
  GAP-098 for the input and display path that would make the result reachable)
- **Live protocol adapters beyond radar.** ASTERIX (Category 048 edition 1.32,
  Category 034 edition 1.29), SAPIENT spotter tasking and detection, and -- since
  2026-09-07 -- the SAPIENT acoustic and passive-RF node types are all built and
  gated: one adapter (`SapientDetectionAdapter`) gated by an `accepted_node_type`
  rather than three separate ones, since all three node types share SAPIENT's wire
  shape. Both hosts register the configuration for all of them
  (`ConfigBaseline.radar_feeds`, `ConfigBaseline.sapient_feeds`; no longer true is
  this bullet's older claim that neither host registers it). The STANAG 4676 codec
  still returns `NotImplemented`. **This bullet's older claim that EO/IR and
  ISR-video each have a pinned specification is corrected 2026-09-07**: motion
  imagery (STANAG 4609/MISB, the ISR-video feed) is surveyed and *deliberately not
  pinned*, since it is a video-transport concern for the viewport rather than a
  detection message for the gateway (`docs/design/external-standards.md` §8); a
  passive-RF alternative over ASTERIX Category 205 is surveyed and likewise not
  pinned, passed over because SAPIENT's node type already covers passive-RF more
  cheaply (§9). EO/IR has no survey and no pinned specification at all -- nothing
  in `external-standards.md` names it. (GAP-001, GAP-064)
- **Bearing-only detections do not reach the operator.** DN-27's tracker half is built
  and gated: a bearing is a separate type, no function anywhere accepts one and
  initiates a track, and one that gates into an existing track refines it. §7, the
  display, is unbuilt -- and the chain stops earlier than the drawing.
  `FusionPipeline::retained_bearings` and the pipeline's five bearing counters have no
  caller outside `gungnir-fusion-async`'s own tests, no view carries a retained
  bearing, `gungnir-app` holds `SapientFeedStatsSink` values it never reads, and
  `SensorHealthView` has lines for radar, AIS and peer feeds and none for a spotter,
  acoustic or passive-RF one. So an acoustic array's ordinary output -- a direction
  with no range, which DN-27 §5 rule 3 calls exactly the report an operator most needs
  -- is journaled, replayable, and invisible in the picture. Four `pipeline.rs` doc
  comments stated the drawing in the present tense; corrected 2026-09-08 and signed by
  the owner the same day, they now say the bearing is retained for a caller to draw and
  name the gap as the reason none does. (GAP-096)
- **Cross-session identity correlation on the node.** The desktop resolver is built
  and wired, correlating by kinematic and classification similarity across sessions
  (`gungnir_identity::similarity`) -- not by session track id alone, which is what
  this bullet said until this rewrite and what the module's own doc comments said
  until GAP-019 corrected them on 2026-09-06. The node has had tracks since GAP-011
  closed and still has no resolver, because §7.1 draws no edge from `gungnir-node` to
  `gungnir-identity`: a graph decision now, not a missing capability. (GAP-019 is
  closed for the desktop half; the node half is this bullet)
- **Plan 05 gap register, most recently updated 2026-09-08.**
  `docs/mission/gap-analysis/gap-register.md` carries 98 gaps against the mission
  capabilities, each with a closing action, a target increment, and an owner;
  `docs/mission/gap-analysis/technical-gap-map.md` maps every item above to the gaps
  that carry it. Engineering items the list above does not name are tracked there by
  identifier: sensing and time (GAP-002 to GAP-005, GAP-008, GAP-009, GAP-023);
  picture and identity (GAP-006, GAP-007, GAP-012, GAP-014, GAP-017, GAP-018,
  GAP-020, GAP-021, GAP-024, GAP-025); the decision loop (GAP-026 to GAP-028,
  GAP-030, GAP-032 to GAP-040, GAP-042, GAP-043); sustainment and metrics (GAP-045,
  GAP-047 to GAP-049, GAP-051 to GAP-054); security (GAP-058 to GAP-060, GAP-062);
  integration, verification, and the plans in execution (GAP-044, GAP-046, GAP-055,
  GAP-061, GAP-063, GAP-065 to GAP-067); decisions D-01 to D-15, resolved on
  2026-09-04 (items 16 to 22 above and `docs/mission/gap-analysis/decisions-needed.md`),
  added GAP-068 to GAP-070; D-16 (measure targets) was resolved the same day (item
  24). Plan 06 (`docs/ux/`) added GAP-071 to GAP-074 (the replay, reports, and
  configuration panels; the status strip; the evidence card, commander summary, and
  theme additions; the usability rounds) and raised D-17 (docking and multi-window),
  resolved the same day (item 25) and implemented under GAP-075. Plan 07
  (`docs/test-tracks/`) delivered the vehicle catalogue, class profiles, sensor
  models, scenario library, data format, reference generator, and ten validated
  sample sets under `testdata/tracks/samples/`; GAP-046 is in progress and GAP-076
  covers seeding the fuzz corpus, the benchmark inputs, and the end-to-end replay.
  Plan 09 (`docs/ml/`) added GAP-077 to GAP-080 (the `gungnir-ml` crate and the
  inference-runtime sign-off, model manifests as `gungnir-modelops` baselines, the
  dataset pipeline, and the first two models); plan 08 (`docs/ai/`) resolved D-14 and
  keeps GAP-044 as the single assistant item; plan 10 (`docs/architecture/togaf/`)
  ran the first compliance assessment against this workspace and added GAP-081 to
  GAP-083 (automating the five mechanical contract checks, proving no `todo!()` is
  reachable, and making requirement identifiers traceable). GAP-092 to GAP-095 and
  D-35 to D-38 were restored to (D-35 to D-38) or added to (GAP-092 to GAP-095) the
  generator on 2026-09-07: D-35 to D-38 had been dropped from `decisions-needed.md`
  by an unrelated commit and are recovered here from `ARCHITECTURE.md` item 89's own
  record of them; GAP-092 and GAP-093 were a second, separate loss the same commit
  caused and were likewise recovered; GAP-094 and GAP-095 are new, the second because
  the first collided with GAP-090's own renumbering (item 89, and this section's own
  entry above). GAP-096 was added 2026-09-08 from a trace of the whole
  bearing path and is the bullet above; it is the first item in I3's order, priority
  40, and nothing blocks it. GAP-097 (an unchanged plan re-proposed and
  re-queued every tick) and GAP-098 were both added the same day, by separate changes
  that each claimed the number 097 within hours of each other while neither was on
  `main` -- the collision this list already records for GAP-090, GAP-094 and GAP-095,
  and for the same reason. GAP-097 kept the number, being the owner-confirmed claim
  already cited from D-28, GAP-074 and the CAP-3.3 coverage row; GAP-098 is the
  younger one and moved. **The count above had also fallen behind**: it read 96 when
  GAP-097 landed and 97 when GAP-098 did, and is corrected to 98 here rather than by
  whoever noticed it next. GAP-098 came out of the GPU review, which also rewrote
  GAP-024's closing action as five items, moved it from I4 to I3 without touching its
  severity, and removed its GAP-023 dependency so the WGSL work is unblocked.
### Resolved on 2026-09-06

**Heading added 2026-09-07.** Everything from here to item 101 was already dated
2026-09-06 or 2026-09-07 and already described completed, signed work; none of it
was open. It had no heading of its own and sat under `### Open` above by omission,
not by finding, for the time between whenever each item landed and this correction.

- **Plan 10 architecture governance (2026-09-04; signed 2026-09-05).** Seventeen
  architecture principles and seventeen contracts in
  `docs/architecture/togaf/preliminary/architecture-principles.md` and
  `docs/architecture/togaf/phase-g-implementation-governance/architecture-contracts.md`
  were signed by the owner on 2026-09-05, in four batches, each checked against the code
  first. Five findings are recorded at signature rather than resolved before it: C-01 and
  C-04 have no automated check (GAP-039, GAP-059); AP-08 is signed knowing D-04's
  buffering bounds desktop durability at 5 s (item 32); AP-12 is signed as an intended
  rule with 22 `todo!()` calls outstanding and GAP-082 open; AP-11 carries two accepted
  exceptions, `solve_assignment` and `compute_metrics` being free functions rather than
  traits; and **AP-16's merge gate has no mechanism at all**, because the workspace is
  not under version control and the CI workflows have never run. C-07 and C-11 were run
  against the tree on the day and both passed. The first compliance assessment passed six
  checks and raised two findings, both about the checks not being automatic rather than
  about the code. A DoDAF cross-reference (`docs/architecture/togaf/framework-cross-reference.md`)
  answers plan 10's open question about external frameworks; no customer framework is
  committed.

- **Plan 11 design closure (2026-09-05).** `docs/design/` holds a design note for every
  gap where the architecture did not name a component responsible: 22 notes and 4
  consolidations. Design coverage across the 56 mission capabilities moved from 30 full to
  52; 23 gaps were retyped from mission to technical because they are now designed and
  awaiting implementation, and 3 mission gaps remain, all owned by plans 07, 08, and 09.
  Two things in that set need this document changed before any of it is built, and neither
  has been:
  - **Five new dependency edges**, listed with their justification and an acyclicity check
    in `docs/design/dependency-edges.md`: `gungnir-workflow` to `gungnir-assessment` and to
    `gungnir-sensor-management`, `gungnir-analytics` to `gungnir-sensor-management`,
    `gungnir-decision` to `gungnir-analytics`, and `gungnir-reporting` to
    `gungnir-identity`. Each is drawn in §7.1 in the change that adds it to a manifest,
    never before. Four further edges were refused, one of them impossible under the
    one-way rule: a service facade may not depend on a productization crate, so engagement
    state keys on a new `DecisionId` in `gungnir-model` instead.
  - **One breaking change to the canonical model, decided 2026-09-05**:
    `PlanView.solutions` becomes `PlanView.kind: PlanKind` so a plan can be an intercept or
    a fires task. That changes a field's type, which the contract's own compatibility rules
    say needs a new schema version and a new path version. The owner took option B:
    `gungnir_model::SCHEMA_VERSION` goes from 1 to 2, the path goes from `/v1` to `/v2`,
    and `solutions` is removed rather than kept as a deprecated mirror. Removing `/v1`
    meets the contract's own condition rather than excepting it, because no client is
    deployed against it. **Landed 2026-09-05**: `gungnir_model::SCHEMA_VERSION` is 2, the
    interface module is `gungnir-api/src/v2/`, and ten call sites across six crates moved
    to `PlanView::solutions()`. `docs/gungnir-api-v1.md` (which keeps its filename so the
    doc-comment citations stay correct) and `docs/design/model-and-schema-deltas.md` §3
    carry the decision.
  - **Signed off 2026-09-05**: all five human-owned notes, DN-08, DN-09, DN-10, DN-17,
    and DN-22, and all 23 verification rows, which are now agreed criteria in
    `docs/verification-capability-table.md` §2.
  - **Implemented 2026-09-05**: all twenty-two notes. The workspace carries 340 passing
    tests, and every safety rule the notes name has a test behind it. Three type
    placements moved during implementation, each recorded in the note that assumed
    otherwise: the asset list is anchored to the local frame by the caller (DN-01 §3a),
    the anomaly detectors take primitive snapshots rather than model types (DN-15 §3a),
    and `SessionId` moved from `gungnir-store` down to `gungnir-model` because six
    crates share it. None of the three added an unapproved edge, which is what they were
    avoiding.
  - **Reviewed 2026-09-05**: the engineering reviewer accepted all five dependency edges,
    including `gungnir-analytics` to `gungnir-sensor-management`, the one the set flagged
    as weakest; and a domain reviewer checked each note against the mission thread step it
    names. The design set is fully signed and reviewed, and every note is cleared to
    implement. Each edge is drawn in §7.1 in the change that adds it to a manifest.

80. **Ten gaps from GAP-012: nine built, one examined and recorded** (2026-09-06).
    Built: GAP-012 per-class staleness, GAP-030 withheld resources, GAP-008 clock skew,
    GAP-021 anomaly detectors, GAP-037 sensor re-tasking, GAP-017 the hazard layer,
    GAP-043 engagements, GAP-047 the measures catalogue, GAP-049 the after-action review.
    Examined: GAP-028, whose desktop half is now done and whose node half waits on DN-23's
    open question and on GAP-057. The register entries carry the detail; what this item
    records is the pattern and the findings.
  - **Every one of the nine was "implemented but unwired."** Six had a design note marked
    Implemented (DN-04, DN-06, DN-13, DN-14, DN-15, DN-20) whose types had their own
    tests and no constructor anywhere in a binary; two (GAP-012, GAP-030) had their
    configuration schema landed and nothing reading it. The register had closed nothing
    wrongly, but it had also not said that "implemented" meant "a module exists." The
    entries now do, and `docs/design/README.md` distinguishes implemented from wired.
  - **One edge added, argued and drawn**: `gungnir-app` to `gungnir-decision`, (i) in
    §7.1. `gungnir-decision` had no dependents at all. Two edges were considered and
    refused: `gungnir-api` to `gungnir-geo` for `SnapshotResponse.hazards`, and any node
    edge to the policy crates ahead of DN-23's queue question.
  - **The negative test that matters is a source scan.** DN-14 §8 requires that no hazard
    contributes to a policy verdict; `gungnir-policy` already depends on `gungnir-geo` for
    geofences, so the edge cannot be the guard. `gungnir-geo/tests/no_hazard_in_the_policy_chain.rs`
    fails the day the policy or command sources name one.
  - **Findings.** The only version a baseline carries is its schema version, so the asset
    list's and the hazard layer's "baseline version" cannot say how old a survey is; a
    per-promotion revision is not in the schema. `CommandEvent::Decided` carries neither
    the verdict nor a rationale, so MOE-05 cannot be computed from the journal. Health is
    never journaled, so MOE-06 cannot be either. A rehearsal leaves no event, so MOE-12
    counts reviews but not rehearsals. The measures catalogue refuses each of those rows
    on screen with the reason, beside the rows it computes.
  - **A reading of DN-06 recorded rather than assumed**: "the plan is superseded" aborts
    an engagement only on `PlanSuperseded`, not on every re-solve; the planner proposes
    each tick, and treating a proposal as an abort would leave nothing ever assessed.
  - **Signed later the same day**: edge (i) was accepted with (h) and (j) in the edge
    review recorded below; every Area A crate stays as it was.
  - **Resolved the same day**, on request. (1) The design README now distinguishes
    *Implemented* from *Implemented and wired* across every note, and the audit found
    seven more of the first kind (DN-02, -03, -07, -16, -17, -18, -19), each row naming
    what would wire it. (2) DN-14 amendment 1 records the source scan as the guard.
    (3) `ConfigBaseline.revision`, a per-promotion counter that `apply` refuses to leave
    unadvanced (`RevisionNotAdvanced`); the asset list and hazard layer stamp it, and
    PN-14 shows schema version and revision apart (DN-01 and DN-14 amendments 1). (4)
    `CommandEvent::Decided` carries the verdict and the rationale the record has --
    a `gungnir-command` change, human-owned, gated and **signed by the owner on
    2026-09-06**; health is
    journaled on the transition by both binaries (`HealthEvent`); a replay leaves
    `ReplayEvent`s on the live session. MOE-05, -06 and -12 compute, each with a note
    saying what the figure rests on; MOE-05 reads 0 of N for accepted decisions until
    the course of action reaches the record (GAP-032), which is the true state. (5)
    DN-06 amendment 1 fixes the "superseded" reading and the no-window rule. All three
    amendments were signed by the owner on 2026-09-06.
  - **The edge review, 2026-09-06.** Edges (h) and (i) were both unreviewed, and the
    acyclicity check `dependency-edges.md` said ran in CI did not exist: the 2026-09-05
    review rested on prose. `gungnir-app/tests/dependency_graph.rs` is now the check --
    the manifests read, a cycle, an upward edge or an unplaced crate refused -- and it is
    the first of GAP-081's five. A review record for (h), (i) and (j) is in
    `dependency-edges.md` §7 with the reviewer-agent checklist walked, **accepted by the
    owner as engineering reviewer the same day**. Item 76's sentence about the reviewer
    stands as history; every edge in the graph is now reviewed.

81. **The ASTERIX radar path landed in a parallel session, and the register caught up
    from the code** (2026-09-06). A second session, working concurrently, built
    `gungnir-interop/src/asterix/` (framing to Part I edition 3.1, Category 048 to
    edition 1.32, Category 034 to edition 1.29, all pinned in
    `docs/design/external-standards.md` §1.6-1.7 by the owner), the
    `AsterixFeedAdapter` in `gungnir-ingest` with its own service-report queue, the
    `asterix_feed` fuzz target and its seed corpus, and fixtures under `testdata/asterix/`
    whose provenance `SOURCE.md` records (Croatia Control's public sample data, GPL-2.0,
    fixtures only, never shipped). It drew edge (j) and updated the verification table,
    `docs/architecture.md`, `docs/README.md` and `gungnir-capabilities.md`. It did not
    update the register: GAP-064 still read "deliberately not built" and GAP-001 did not
    mention the adapter. Both entries were reconciled from the code the same day by this
    session, and both stay **Open**: STANAG 4676 is the other half of GAP-064, and every
    sensor class but radar is the rest of GAP-001. The statement that closed GAP-064's
    examination -- a codec without its specification passes its own round-trip and
    fails every real radar -- was answered the way it asked to be: the specifications
    were pinned and a public capture was put in the repository first. That session's
    handoff is `docs/design/handoff-2026-09-06-radar-feed.md`: state, the six facts the
    capture established, the data flow and its two invariants, the next steps in order,
    the five behaviours that look like bugs and are not, and where the records live. A
    dated update at the end of its §1 reconciles it with this tree: the gate is green, the
    config change it waited on has landed, the edge is (j) and reviewed, and the register
    caught up. Its `asterix_feed` target is in the nightly fuzz matrix
    (`.github/workflows/fuzz-nightly.yml`).

82. **Ten more gaps: eight built, two examined and recorded** (2026-09-06). Built:
    GAP-081 (all five compliance checks run on every `cargo test`), GAP-028's node half
    (the chain on the node, edges (k) and (l)), GAP-039 (C-01 proved statically and at
    runtime), GAP-026 (the asset ranking on PN-04 and PN-17), GAP-018 (per-class
    identification thresholds read by the engine), GAP-036 (the fires engine in the chain,
    PN-05 lists the checks), GAP-062's report half (the report's combined marking, inside
    the file), GAP-083 (requirements in the UAF registry, generated from the
    specification). Examined: GAP-033 (stale; DN-09 was wired the day before) and GAP-050
    (nothing to fail over from until GAP-057 establishes a link). Filed: GAP-088,
    geofences have no configuration source.
  - **Findings.** The design README's DN-05 row said the chain evaluated a fires task; it
    did not, the engine was constructed by nothing. The geofence engine on both binaries
    evaluates against an empty service and PN-07's caveat was a hard-coded `true`. The
    requirements specification's total said 58 while its tables held 64. The unwrap
    policy had one violation, in `gungnir-security`. `PlanApproved` is published by
    nothing. Every caller the transport can authenticate is an operator inside the
    deployment, so DN-17's per-party filter has no caller yet and was not built.
  - **Human-owned crates touched**: `gungnir-policy` (`PolicyVerdict::summary`) and
    `gungnir-security` (the HMAC `expect` became a `Result`), both **signed by the owner
    on 2026-09-06**. Edges (k) and (l) were accepted the same day (`dependency-edges.md`
    §7a).

83. **A third ten: seven built, three examined** (2026-09-06). Built: GAP-088 (geofences
    from the baseline, on both chains and the map), GAP-057's desktop half (**the sign-in
    surface** -- PN-20, a file account store, and the node link established by a
    sign-in), GAP-059 (an audit row on every gated act), GAP-040's record half (the
    handoff from the decision, manual or undelivered, never delivered), GAP-060's at-rest
    half on the node, GAP-002's provenance (how strongly a source was authenticated).
    Examined: GAP-031 (no assignment and no speed to solve geometry against), GAP-010
    (owner decisions on editions and fixtures first, as GAP-064 established), GAP-023
    (a stack decision first), GAP-067 (statuses refreshed; the row-by-row promotion is a
    walk with the owner).
  - **Seventeen of twenty panels are built.** PN-16 (GAP-087), PN-18 and PN-19 remain.
  - **Human-owned crates touched**: `gungnir-security` (`FileAccountStore`; two
    `actions` constants) and `gungnir-ingest` (the gateway stamps the authenticator's
    strength on provenance), both **signed by the owner on 2026-09-06**.
  - **The link exists now.** A sign-in on a desktop configured for a node connects with
    that credential and a sign-out disconnects; GAP-050's mid-session switch has a link to
    switch from, once a node can verify the credential (GAP-057's node half).

84. **The decisions walk: four answered, four landed** (2026-09-06). Walked one at a
    time with the owner, each with a recommended default, and recorded as D-24 to D-27
    in the register.
  - **D-24, AIS and ADS-B (GAP-010): pin both, verify first.** `external-standards.md`
    gained §3 (AIS: ITU-R M.1371-6 of 02/2026 or -5 of 02/2014, the gpsd AIVDM document
    as the public framing reference, and gpsd's five BSD-2-Clause AIS regression logs as
    the fixture candidate) and §4 (ADS-B: ICAO Doc 9871 2nd edition with Amendment 2,
    paid; *The 1090MHz Riddle* is CC BY-NC-SA and flagged; no permissively licensed
    capture found, a self-recorded one recommended). **Nothing is pinned and no codec is
    written**: the -5 to -6 layout delta and the fixtures' provenance are unverified,
    and the note says exactly which claims were checked and which were not.

    **Superseded 2026-09-06, and one of those findings was simply wrong.** "No
    permissively licensed capture found" was false: two exist in the test suites of open
    decoders, one BSD-3-Clause at the radio layer and one MIT at the message layer, and
    both are now vendored under `testdata/adsb/`. The codec is built and gated (item 99).
    What remains true, and is now the recorded residual risk, is that **no normative source
    is pinned at all**, because none is both free to obtain and permissively licensed --
    and the freely circulating draft of one carries its publisher's copyright and is a
    draft, so its availability is a trap rather than an option.
  - **D-25, GeoTIFF (GAP-023): `tiff`, and own the geo tags.** `tiff` 0.11 (image-rs,
    MIT, pure Rust) joins §2.9 as the raster decoder; the three GeoTIFF tags and GDAL's
    no-data tag are read by `gungnir-data` itself per OGC GeoTIFF 1.1. **Both DEM
    loaders are built**: ESRI ASCII grid with no crate, GeoTIFF with this one, both to
    a `HeightGrid` that carries its frame and its no-data value honestly, and
    `TerrainMesh::from_grid` leaves a hole where data is missing rather than a wall.
    Fixtures are generated by the repository (`testdata/dem/SOURCE.md`, every byte
    explained, no licence question), and the loader thread finally dispatches: terrain
    loads, and the three unbuilt formats return their `NotImplemented` through the
    channel rather than silence. **Not wired**: nothing in either binary requests a
    terrain yet, and `FlatTerrainLineOfSight` is still flat; that needs a terrain entry
    in the baseline and is GAP-023's next step, not this one.
  - **D-26, a closing speed (GAP-031): DN-04 amendment 1.** `intercept_speed_mps:
    Option<f64>` on `ResourceConfig` and `ResourceView`, validated finite and positive,
    `None` meaning "no geometry for this resource" and never a default. Read by nothing
    until the solver exists, which waits on an assignment (GAP-029, Area A). The
    amendment was **signed by the owner the same day**.
  - **D-27, the escrow holder (GAP-084): a named security-officer role, per
    deployment.** DN-22 amendment 2 (§11) records the role (operates nothing, recovers
    only), the least mechanism the stack already supports (P-256 ECDH, HKDF, AES-GCM,
    the public key inline in the baseline and the private half never on a node), the
    audited action, and the verification. **Signed by the owner the same day; unbuilt**:
    it waits on the asymmetric provider.
  - **D-28, MOP-37's targets (GAP-074): round 1 on the built panels.** The wireframe
    round is not run; one participant per role works the rendered panels and the
    measured medians become the target proposal. The session package is written
    (`docs/ux/usability-round-1-session.md`), and it is honest about what the binary
    can show: **ten of sixteen tasks can run**, six of those only once the desktop can
    be seeded with plans and tracks, because the allocator and the pipeline are not
    implemented and nothing else puts a plan in the queue -- **GAP-089**, filed. Six
    tasks are round 2's (unbuilt PN-16 and PN-18; the node sign-in, control-status,
    sensor-transport and gap-acceptance writes do not reach the desktop) and are
    reported as not run rather than scored. The owner names participants and dates.
  - **A finding, fixed 2026-09-06.** The default `data_dir` was `./gungnir-data`, which
    from the workspace root is the `gungnir-data` **crate's directory**; a smoke run on
    2026-09-05 left `1.mission.json` and a session journal beside its `Cargo.toml`. The
    owner chose `./gungnir-journal` for both defaults (`ConfigBaseline::data_dir` for the
    desktop and `NodeConfig::data_dir` for the node) in `gungnir-config`; the two stray
    files were removed, `.gitignore` now excludes `/gungnir-journal/` instead of the
    crate directory, and `CLAUDE.md` states the new default. A configured `data_dir`
    is unaffected.

85. **A fourth ten: eight built, one closed, one half** (2026-09-06). Built and wired:
    GAP-020 (prediction on every frame, PN-04 and the viewport draw it; the entry had been
    stale, the predictor existed unwired), GAP-042 (DN-03's rule under a ledger the tick
    evaluates; **every warning fails loudly today** because no endpoint has a transport,
    which is the design's own choice over silence), GAP-031 (the constant-velocity
    intercept solver, in the planner, producing a point the moment GAP-029 produces an
    assignment), GAP-027 (affiliation lethality as a named factor; platform class waits
    on a class no track carries), GAP-023 (a `terrain` entry, the loader thread, line of
    sight masked against it; **placement honesty**: the stack has no projection library,
    so the DEM is prepared in the local frame and a file whose tags say otherwise is
    refused by name), GAP-062 (PN-03's marking column), GAP-057's node half (the file
    account store and the signing key from the environment, human-owned, **signed by the
    owner the same day**). Closed: GAP-089 (the seeded session: scripted tracks, plans
    through the real chain, the seed's hash as the first record, REHEARSAL on the strip;
    round 1's group B is runnable). Built, not wired: GAP-019 (similarity correlation with
    a confidence and a basis on the lineage; no binary constructs a resolver). Half:
    GAP-016 (CPython's `random.Random` bit for bit against local vectors; the YAML half
    needs a §2.9 decision).
  - **A file was overwritten and recovered.** `gungnir-workflow/src/warning.rs` held
    DN-03's implementation from 2026-09-05 and the batch wrote over it; the original was
    recovered from the transcript, restored verbatim, and the ledger appended over its
    primitives rather than replacing them. Its ten tests pass unchanged.
  - **Dependency edges: none added.** Edge (e) `gungnir-workflow` to `gungnir-assessment`
    was already drawn for DN-03; `gungnir-app` gained three external crates it already
    had beneath it (`serde`, `nalgebra`, `sha2`, `crossbeam-channel`), recorded in §2.9.
  - **Stale entries found and corrected**: GAP-020 and GAP-027 both described as absent
    code that existed; GAP-023's loader thread had dispatched nothing since the crate
    was scaffolded.

86. **A fifth ten: seven built, two closed, one examined** (2026-09-06). Built and wired:
    GAP-001's radar half (a `radar_feeds` section; both hosts bind a UDP source per feed
    and register the ASTERIX adapter), GAP-064's consumer (the radar's service messages
    reach the registry through a sink and **confirm `Search` on a north marker**, which is
    DN-11's rule that only a sensor's own word moves its mode), GAP-040 (the endpoint
    transport: accepted, refused with the body, or unreachable and retried every 30 s and
    never dropped; tested on a real socket), GAP-042's tail (a warning is `Sent` when an
    endpoint accepts; PN-17 counts), GAP-023's LAS loader, GAP-050's fallback half (a
    silent node past its heartbeat timeout puts the desktop on embedded services, on the
    record; a returning node marks a reconciliation due and **the desktop does not switch
    back on its own**; PN-18 is a real panel), GAP-060's trust roots. Built and unwired:
    GAP-024's CPU ICP reference (point-to-point, because `PointBuffer` has no normals),
    GAP-009's peer adapter (quality assigned by us, age visible, stale never dropped; the
    link needs a machine identity), GAP-084's P-256 provider with escrow (human-owned,
    **signed by the owner the same day**). Closed: GAP-058 (the qualifiers had existed; the
    entry was stale, and the missing thing was the twenty-four-cell MOP-38 test, now
    written in the human-owned crate and signed by the owner the same day).
  - **Two findings for the owner.** Issuing the node's TLS identity from the provider
    needs an X.509 crate at runtime, which §2.9 admits `rcgen` for only under
    `[dev-dependencies]`: a §2.9 decision (GAP-060). DN-22 §11's `SecurityOfficer` role
    is not in DN-20's role table, and adding it changes every layout match: recorded
    under GAP-084, not done in passing.
  - **Dependency edges: none added.** `gungnir-ingest` now uses `nalgebra` at runtime
    (it was dev-only) and `gungnir-security` uses `p256` with `ecdh`, both recorded.
  - **Stale entries corrected**: GAP-058 (built, unevidenced), GAP-064's "nothing
    consumes the service-report queue".

87. **The second decisions walk: four answered, none landed yet** (2026-09-06). Walked
    one at a time with the owner, each taking the recommended default, recorded as D-29
    to D-32: **rcgen at runtime for the node only** (its `SigningKey` trait lets the
    provider sign without exporting the key; §2.9's dev-only row changes when the work
    lands), **`Role::SecurityOfficer` now** (an empty layout, one action, every match
    updated in one change), **`yaml_serde`** for the test-track composition, and **the
    agent verifies and pins AIS and ADS-B with the owner reviewing**. Round 1 of the
    usability tests is not yet run and its report stays open. The four decisions are
    the next batch's first four items; nothing was built in this walk.

88. **A sixth batch, fifteen items: the four decisions landed and eleven gaps advanced**
    (2026-09-06). D-29: the node issues its TLS identity from its key provider through
    rcgen's `SigningKey` (`gungnir-node/src/identity.rs`; rcgen a runtime dependency of
    the node alone). D-30: `Role::SecurityOfficer`. D-31: `yaml_serde`, and the four
    plan-07 YAML files load and cross-check (`gungnir_scenario::TrackLibrary`). D-32:
    M.1371-6 pinned after both editions were compared table by table, the gpsd captures
    copied with provenance and the AIS decoder gated on gpsd's decodes; Doc 9871 pinned
    for ADS-B with the capture still the owner's to record -- **which the owner then chose
    not to do**: on 2026-09-06 the open-source-consensus route was taken instead, the
    document remains the specification of record and is **not held**, and buying it later
    is an upgrade that changes only which oracle the same tests cite (item 99). Gaps: the passphrase-sealed
    keystore and the escrow record at sign-in (GAP-084); VTK and glTF loaders, the
    viridis ramp, the mesh conversion and the terrain drawn under the picture (GAP-023);
    the pass-close distance on the warning obligation (GAP-042, DN-03 amendment 1); the
    event-history route, the reconciliation over both journals on PN-18 and the switch
    back as a person's act (GAP-050; edge (m) `gungnir-app` to `gungnir-resilience`);
    session expiry on the desktop and PN-01's operator line (GAP-057); the radar feeds'
    counters on PN-09 (GAP-001). Human-owned crates were touched in `gungnir-security`,
    `gungnir-node` and the gateway's adapter: written and gated, not signed at the time.
    **Signed since**: the `gungnir-security` items (`SecurityOfficer`, the
    passphrase-sealed `PersistentKeyProvider`, `LocalAccountAuthority::with_lifetime`)
    and the `gungnir-node` TLS identity issued through the key provider (D-29), both on
    2026-09-06, recorded in item 92's sign-off. The ASTERIX adapter's `FeedStatsSink` from this
    batch was not in any of those signatures -- a counter sink on a `gungnir-ingest`
    adapter, human-owned because the gateway is the trust boundary for external data --
    and was **signed by the owner 2026-09-06** once put to them on its own. It was small
    enough to be mistaken for covered by the batch around it, which is the drift these
    entries exist to catch. GAP-079 was not taken: its dataset extraction has no settled
    crate.

89. **The theme is an installed style, not a set of constants** (2026-09-06, D-35 to
    D-38). An outside review of `gungnir-ui/src/theme.rs` was assessed against the
    design system and the dependency graph; what survived is built. `theme::
    install_egui_theme` runs once from the eframe creation closure in `gungnir-app`
    and from the headless render probe, and gives egui's own chrome the tokens the
    panels already drew with: three named surfaces (`APP_BACKGROUND`,
    `PANEL_BACKGROUND`, `VIEWPORT_BACKGROUND`), primary and secondary text, a focus
    colour that is chrome-only, zero rounding and no shadows (DS-06: a console is
    edges and surfaces), striped rows, and `animation_time` at zero because DS-06
    allows no motion but the strip's counts and the queue's reorder. The dock's tab
    bar takes the same tokens through `egui_tiles::Behavior`'s colour hooks in
    `gungnir-app/src/dock.rs`. Stroke widths, the terrain alpha, the dashboard width
    and the tab-bar height are tokens now rather than literals in the viewport and
    `main.rs`, per `rust-ui-architecture-coding-standards.md` §6.

    Four things the tests forced. `track_color` returned the stale grey for a
    `Deleted` track, which told an operator a removed track might still be there;
    `TRACK_DELETED_COLOR` had no caller. The status strip kept four private colours
    beside the theme's, so its green differed from the health dots' and its red
    reached 3.2:1; they are gone and the strip reads the theme. `ALERT_COLOR` at
    (220, 60, 60) was under the 4.5:1 rule on a panel, and nothing had checked it
    there because `docs/ux/accessibility.md` said egui derived the panel colour from
    the viewport's, which it did not: the panel surface is now a token and fifteen
    text colours are held to the rule on all three surfaces. And the assignment line
    was ten units from the friendly frame's blue; it is teal, and the five colours
    that share the map are held pairwise apart.

    Compared numerals (time remaining, scores, positions, speeds, ages, evidence
    weights) are set through `theme::numeral` in egui's monospace face at body size,
    because the proportional face has no tabular figures and DS-05 asks that a column
    of numbers line up. DS-05 gained a 16 pt size for a panel's title and nothing
    else.

    What the review proposed and was refused, with the reason on the page: moving
    `TIME_REMAINING_*` to `gungnir-policy` (this crate depends on `gungnir-model`
    alone, §4; the thresholds are display, not policy; `frame_is_dashed` already takes
    the margin as an argument); four-level `system` and `decision` colour namespaces
    (DS-02 says health is two colours and never a third, principle 4 says health is
    reported never inferred, and the model's flags are booleans); a six-size font
    scale (DS-05 allows 14 and 12, now 16 for titles); a cyan selection halo (DS-04
    keeps white, the one colour nothing else on the map uses); and splitting the file
    into a directory (cited by path from four documents; deferred). DS-02's weapons
    control row was corrected to what the strip has drawn since GAP-072: free is the
    permissive state and takes the red, hold is the safe one.

    The decisions: D-35 keep the tokens as flat constants and file the night variant
    as GAP-095 rather than thread a `Palette` value through 239 call sites now; D-36
    the selection halo stays white and the assignment line moves instead; D-37
    monospace for compared numerals; D-38 the blue-black viewport set and the
    coverage alpha at 80. The night variant and the dashed low-confidence frame remain
    the two open items in `docs/ux/ux-to-code-map.md` §4.

    These four were filed the same day as D-33 to D-36 and GAP-090; both numbers were
    taken the same day by an unrelated session's work (D-33 by the Cursor-on-Target
    scope decision, D-34 by the licensing decision, GAP-090 by the friendly-set gap
    out of DN-25), so the theme decisions and gap were renumbered 2026-09-07 to D-35
    to D-38 and GAP-094, and this paragraph and `docs/ux/design-system.md` were
    re-pointed to match. The lesson: a decision or gap number is only safe to cite
    once its row is committed on `main`, not from the moment it is chosen in a
    working tree.

    **The lesson held for exactly one round.** GAP-094 was itself taken the same day,
    by an unrelated verification-and-governance gap (the advisories gate finding,
    below), and nobody re-checked the renumbering after choosing it -- so the theme
    variant was cited under a number two different gaps now claimed. Found in the
    2026-09-07 development-status review, not by the process this section describes.
    The night variant is GAP-095; `docs/ux/design-system.md` and
    `docs/ux/ux-to-code-map.md` are re-pointed again, this paragraph is corrected in
    place rather than left to describe a fix that did not hold, and the advisories
    gap keeps GAP-094, being the one of the two actually committed to `main` under
    that number.

90. **A seventh ten: nine built, one closed** (2026-09-06). Built: GAP-016 (closed: the
    `gen_tracks.py` composition ported over `TrackLibrary` and `PythonRandom`, all ten
    committed sample sets reproduced byte for byte by `reference_parity.rs`; Python's
    `int`/`float` distinction, `repr(float)`, `json.dumps` escaping and CPython's
    `math.degrees` all had to be reproduced for the last byte), GAP-010 (the AIS receiver
    adapter in `gungnir-ingest`, `ais_feeds` in the baseline, cooperative reports
    associated to tracks on the desktop and submitted to the identification engine, so
    GAP-018's engine has its first producer; PN-04 and PN-09 draw it), GAP-063 (the
    conformance suite over the catalogue; the Arrow schema was lossy on provenance and is
    not now; `SchemaCatalog::check` and a link that refuses a schema it does not speak),
    GAP-062 (enforcement: the TLS listener reads the party from the client certificate,
    every collection response is filtered per party with the withheld count, internal
    routes refuse a machine caller), GAP-065 (`exchange` agreements in the baseline,
    composed with releasability on the node's outbound side; a party with no agreement is
    refused with the DN-18 reason), GAP-050 (per-conflict resolution through the authority
    check onto the record as `LinkEvent::ConflictResolved`; the store-and-forward outbox
    teed during the outage and flushed to `POST /v2/detections` while linked), GAP-025 and
    GAP-019 (the desktop recovers the last `reporting.retention_sessions` sessions into an
    identity resolver and PN-13 assembles the order of battle over it; PN-04 shows the
    lineage), GAP-046 (the generator half of the plan is done). Two edges taken and
    recorded: (n) `gungnir-app` to `gungnir-identification` and (o) to `gungnir-identity`.

    Human-owned, **signed by the owner 2026-09-06**: the `gungnir-api` change (the
    party-bearing listener and the operator-only refusal on both write paths) and the
    `gungnir-ingest` AIS adapter. Edges (n) and (o) were accepted by the owner as
    engineering reviewer the same day (`docs/design/dependency-edges.md` §10). Nothing in
    `gungnir-security`, `gungnir-policy` or `gungnir-command` changed.

91. **An eighth batch, fifteen items: eleven built, two closed, two examined** (2026-09-06).
    Built: GAP-060 (the desktop link over mutual TLS: `reqwest` with the baseline's roots
    and the desktop's certificate, the event stream as `tokio-tungstenite` over a
    `tokio-rustls` stream; an `https` endpoint with no roots refused) and with it GAP-041
    closed; GAP-002 (the baseline's `machine_identities`, a sensor's certificate submitting
    for its own id into a queue admitted under the one authenticator that may stamp
    `MachineIdentity`, through the gateway's per-adapter authenticator); GAP-009 and
    GAP-065 inbound (`PeerLink`, a machine link to a partner's node, bound per peer on
    both binaries; edge (p) node → remote); GAP-004 (`POST /v2/sensors/{id}/task`, the
    desktop's control adapter is its link, the node's task id mapped back, the mode
    changed by the streamed acknowledgement alone; `SensorCommand` to the model); GAP-040
    (`POST /v2/handoffs/{decision}/report`, `HandoffEvent::Reported`, applied or rejected by
    the desktop that issued the handoff); GAP-062 closed (PN-04's origin line, PN-20's
    marking row); GAP-057 (PN-20 assigns a role through the account file, which the store
    re-reads); GAP-001 (the node's feeds and peers on its health line); GAP-042 (the
    pass-close due time from the time to closest approach, DN-03 amendment 2); GAP-027
    (`assessment.lethality_by_class`, an AIS ship type as a `surface.*` class, the factor
    on PN-04); GAP-077 and GAP-079 (`gungnir-ml`: the traits, the fake, honest health, no
    runtime by decision; the dataset extraction into the catalogue's Arrow rows with a
    content hash; edge (q)); GAP-050 closed (the reconciliation row against a real node
    over the real history route, and the switch back now refuses until every conflict is
    resolved -- a rule the register had claimed before the code enforced it). Examined:
    GAP-046 and GAP-076 (a Rust-generated set replays; the rest waits on the pipeline),
    GAP-067 (the §2 gate table prepared for the owner's walk).

    Human-owned and **signed by the owner 2026-09-06**: `gungnir-api` (the sensor task
    and effector report routes, the machine path on detections), `gungnir-ingest` (the
    per-adapter authenticator and `MachineIdentityAuthenticator`), `gungnir-security`
    (`assign_role`, `save`, the store re-reading its file, the `ASSIGN_ROLE` action),
    and DN-03 amendment 2. Edges (p) and (q) were accepted by the owner as engineering
    reviewer the same day (`docs/design/dependency-edges.md` §11). **Two things this
    signature did not cover were signed separately on the same day**: DN-03 amendment 1,
    and batch 6's `gungnir-security` items (`SecurityOfficer`, the passphrase-sealed
    `PersistentKeyProvider`, `LocalAccountAuthority::with_lifetime`). DN-22 amendment 3, the
    design note behind that keystore, was put separately and **signed the same day**, as
    was the node's provider-issued TLS identity (D-29), and then the ASTERIX adapter's
    `FeedStatsSink`, which item 88 had found uncovered and which was put separately rather
    than assumed. **Nothing human-owned in batches 6 to 8 is unsigned.**

92. **Area A: the tracking pipeline and the allocator** (2026-09-06, GAP-011 and
    GAP-029). The two roots of the human-owned tracking work, built on request. Sixteen
    of the register's open entries had a dependency chain into them.

    **The pipeline** (`gungnir-fusion-async/src/pipeline.rs`) composes what was already
    signed off on 2026-09-05 rather than adding an estimator: the Joseph-form linear
    Kalman filter, the Jonker-Volgenant associator, the chi-square gate and the
    lifecycle, driven by a reorder buffer that processes detections in source-time
    order. Two rows are gated: the §1 `fusion-async` out-of-sequence row against the
    offline batch (exactly, on a criterion of 1e-4), and the §2 whole-pipeline scenario
    replay row across all five scenarios (to better than 1e-6, the tightest §1 tolerance
    the pipeline exercises). `PIPELINE_IMPLEMENTED` is true, and the register's own
    condition for flipping it -- that the whole-pipeline replay row passes -- is the
    reason it was flipped rather than the pipeline merely existing.

    **The allocator** (`gungnir-allocation/src/bellman.rs`) is the exact dynamic program
    the row names, compared against a textbook Python DP at every layer and every subset
    of six cases, agreeing exactly on a 1e-9 criterion. A problem past 16 tracks or 8
    resources is refused by name rather than answered heuristically.

    **What flipping the flag changed, and what it cost.** Four tests asserted the old
    claim and were re-pointed at the new one rather than weakened; the empty approval
    queue now names the allocator instead of the pipeline as the reason it is empty,
    which was already the code's own fallback; the node's health line reports a healthy
    tracker for the first time; and the frame-budget row measures a populated snapshot
    (13 tracks, both calls at 100 ns against a 1 ms budget) instead of the cost of
    returning an empty slice. Two behaviours surfaced that had been hidden: a rehearsal
    session now has the real planner proposing against its seeded tracks, and one target
    seen by two sensors at one instant is two tracks until track-to-track fusion lands
    (GAP-013), which a test now pins.

    **What Area A still holds.** The nonlinear estimators (EKF, UKF, particle, IMM,
    square-root/UDU, RTS), JPDA and MHT, random-finite-set filtering (GAP-015) and
    track-to-track fusion (GAP-013). `filterpy` 1.4.5 and Stone Soup 1.9.1 were
    installed under `.venv-oracles/` in this change, so each of those rows can now be
    gated against the oracle it names rather than deferred for want of one.

    Human-owned, and **signed by the owner 2026-09-06**: the pipeline
    (`gungnir-fusion-async`'s `pipeline` module and its `ingest` task, with the two
    supporting additions the same change needed -- `TrackManager::update_estimate`, so an
    estimator hands its result back rather than reaching into the record, and
    `LiveTrackingService::{with_pipeline_settings, finish}`) and the allocator
    (`gungnir-allocation`'s `bellman` module and the trait implementation over it). Edge
    (r) was accepted by the owner as engineering reviewer the same day
    (`docs/design/dependency-edges.md` §12). `gungnir-fusion-async` is named in the
    low-trust tier by concurrency correctness, and the numerical-stability clause reaches
    the filter the pipeline drives, so both were brought under that rule rather than on
    their gates alone.

    **What this signature does not reach**, because none of it exists yet: the nonlinear
    estimators, the smoother, the square-root form, JPDA and MHT, random-finite-set
    filtering and track-to-track fusion. Each is its own §1 row and will want its own.

93. **The promoted baseline reaches the filter, and three estimator rows are gated**
    (2026-09-06). Work that the pipeline and the allocator had unblocked.

    **GAP-053 closed.** `PipelineSettings::from_baseline` maps a promoted algorithm
    baseline onto the pipeline's settings; both binaries build one before they build the
    tracker and stamp the baseline's identifier **only** when the settings came from it,
    which is DN-24 §7's rule stated in both directions. A baseline naming a filter this
    build does not implement is refused by name rather than run as the default, so the
    governance record and the picture disagree visibly. The default baseline is in that
    state today: it names `imm-cv-ct`, and the IMM is not built.

    **GAP-020's predictor half.** `FilterPredictor` propagates the covariance through
    `F P Fᵀ + Q(t)` with the pipeline's own process noise, so a predicted ellipse is the
    filter's uncertainty rather than a second opinion about it. `gungnir-model`
    re-exports `ConstantVelocity` and `MotionModel` rather than the process noise being
    written twice.

    **Three §1 rows gated against `filterpy` 1.4.5**: the EKF and the UKF over a
    range/azimuth/elevation measurement, within 1e-4 after every predict and every
    update; the RTS smoother within 1e-6 at every step. The smoother's signature had to
    change, because the scaffold's took states without covariances and could not have
    computed its own gain.

    **A finding worth keeping.** The first EKF fixture was wrong and the filter was
    right. `filterpy` reshapes the measurement to the state's dimensionality, so a
    column state against a one-dimensional measurement function broadcast a (3,1)
    against a (3,) into a (3,3), and the recorded state had eighteen entries instead of
    six. It read exactly like a diverging filter. The generator now asserts the shape of
    everything it writes, so that fixture cannot be produced again.

    Human-owned, and **signed by the owner 2026-09-06**: all of it -- the governance
    wiring (`PipelineSettings::from_baseline`, `with_algorithm_baseline`, and both
    binaries' use of them), the filter predictor, and the EKF, UKF and RTS smoother.
    `gungnir-filters` is reached by the numerical-stability clause and the two new
    estimators are squarely in it, so they were brought under that rule rather than
    resting on their oracle gates alone.

    **What that signature does not reach**, because none of it exists: the particle
    filter, the IMM, the square-root/UDU form, JPDA and MHT, random-finite-set filtering
    (GAP-015) and track-to-track fusion (GAP-013). Each is its own §1 row and will want
    its own.

94. **Area A's remaining tracking mathematics is built and gated** (2026-09-06).
    Eight more §1 rows, closing GAP-011, GAP-013 and GAP-014 and advancing GAP-015.

    **GAP-011 closed.** The IMM over constant-velocity and coordinated-turn modes; the
    particle filter with three resamplers; the square-root form, one array recursion
    covering both the linear and extended halves of its row; JPDA by exact joint-event
    enumeration; MHT as a bounded hypothesis tree with N-scan commitment. **Having them is not the same as
    being able to run them**, and an earlier draft of this item said otherwise. The
    pipeline's `TrackFilter` is still a fixed linear Kalman filter over a constant-velocity
    model, and `IMPLEMENTED_FILTERS` lists what the *pipeline* can apply. Adding a name to
    it without changing what the pipeline runs would make `with_algorithm_baseline` stamp
    every track with a baseline claiming an IMM produced it while a linear filter did --
    the governance record disagreeing with the picture, which is the failure item 93
    existed to prevent. Selecting between filters per mission profile needs the pipeline to
    hold more than one filter type, which it cannot: they have different state dimensions
    and different update signatures. That is its own increment, and the constant now says
    so where somebody would make the mistake.

    **GAP-013 closed.** Covariance intersection as the default fuser, the
    information-matrix sum as the alternative for a deployment that can argue its inputs
    independent, and sensor registration that publishes the residual spread of its own
    fit. **GAP-014 closed**, and the register's wording for it was deliberately not
    followed: it asked for a `gungnir-model` event consumed in `gungnir-track-fusion`,
    which is an upward edge out of the tracking core. The event lives in the model, a
    plain-data equivalent lives in the core crate, and `gungnir-tracking-service` joins
    them. **No dependency edge was added anywhere in this batch.**

    **Three of the oracles §2 planned turned out not to exist**, and each substitution is
    recorded in the row's own Method column rather than made quietly. Stone Soup 1.9.1
    has no IMM and no MHT hypothesiser, and its multi-frame assignment will not import
    without an uninstalled solver; the square-root row's method was always "compare vs
    standard-form filter", and that is what was done. No criterion was changed.

    **A defect was found in an oracle rather than in this code.** Stone Soup 1.9.1's
    `GaussianMixtureReducer.merge_components` clamps a merged component's weight to 1.0 --
    merging 0.7 and 0.6 returns 1.0, checked directly. A PHD weight is an expected target
    count, not a probability, so the clamp makes the library under-report cardinality
    whenever merging combines past one target. The `rfs` row is gated against the textbook
    Vo--Ma recursion instead, and a second disagreement on the first scan is recorded as
    **not yet explained** rather than left implied.

    **Two findings about tests demanding the wrong thing.** The particle row's 2σ bound,
    applied to all 180 quantities and required to hold for every one, is not a strict
    reading of the row but a criterion no correct implementation can meet: 2σ predicts
    about eight excursions in 180, and the first run duly failed at 2.22σ. And an earlier
    draft of the particle filter's own test asserted that a cloud a hundred standard
    deviations from the measurement kept both its lumps. It did not, and it should not
    have, because that is a correct filter degenerating.

    **A real degeneracy in covariance intersection.** When two inputs have equal
    covariances the fused determinant is exactly flat in ω while the fused *state* sweeps
    the whole line between them, so a search left to itself returns wherever its internals
    stop; Brent and a golden-section search landed 1.2e-6 apart on that case. The tie is
    now broken by specification -- equal weighting when the criterion cannot separate --
    on both sides, from the same description rather than by transcription.

    **DN-26 (laydown options) was written and is unsigned.** It unblocks GAP-087 and
    through it GAP-020. It adds no edge, and it deliberately excludes adopting a laydown:
    moving a sensor is a physical act with an authority chain this system does not model,
    and a button that appeared to do it would be the most dangerous control on the
    display.

    Human-owned, and **signed by the owner 2026-09-06**: all of it. `gungnir-filters`,
    `gungnir-association`, `gungnir-track-fusion` and `gungnir-rfs` are reached by the
    numerical-stability clause, so they were brought under that rule rather than resting
    on their oracle gates alone.

    **What that signature does not reach.** DN-26, written this batch, is a design and was
    not in it: a signature on code says the code does what it says, and a signature on a
    note says the design is the right one. Nor does it reach the rows that do not exist --
    the CPHD's cardinality distribution, GLMB and LMB -- which each return an explicit
    refusal naming themselves.

    **What is still not built in Area A**, named so that closing GAP-011 is not read as
    more than it is: the CPHD's cardinality distribution, and the GLMB and LMB labelled
    filters (GAP-015). A PHD filter carries no identity, and that is exactly what those
    add.

95. **The warning acknowledgement, the exchange routes, and the pattern-of-life
    producer** (2026-09-06). Three things that existed as types with no code behind them.

    **GAP-042 closed.** `WarningState::Acknowledged` and `Warning::acknowledged` had both
    existed since the plan-11 set and **nothing called either**, so DN-03 §5 rule 2 -- a
    warning stands until the party acknowledges -- was written and unreachable, and every
    delivered warning went `Sent` and then `Late` for ever. There is now a route by which a
    warned party's certificate, or an operator holding a new action, discharges one.
    `MachineRole::WarnedParty` is deliberately separate from `Effector`: one certificate
    must not both report an engagement and discharge a warning.

    **GAP-065's mechanism, not its producer.** Three of `ExchangeItem`'s five variants had
    never had a producer or a gate. `GET /v2/exchange/{warnings,reports,handoffs}` now
    serves them through both gates DN-18 §4 requires, counting what it withholds. **No
    product has ever been exchanged**, because nothing calls `publish_exchange` outside
    tests: the node holds none of the three items and cannot gain an edge to
    `gungnir-reporting` or `gungnir-workflow` without breaking §7.1, and the desktop that
    holds them has no `NodeApi`. The node answers `NotHeld` with a truthful reason and a
    producer was deliberately not invented. That last mile is a decision, recorded in
    DN-18 amendment 1.

    **DN-18 §6 said "no new endpoints" and three were added.** The amendment records the
    divergence rather than leaving §6 to be read as still true.

    **GAP-025 closed.** `PatternOfLife` had accessors, a test for its denominator, and no
    producer anywhere. It has one now, and three of its judgements are about not publishing
    a claim the evidence does not support: a path travelled once is a track history and not
    a route, the activity histogram counts entities rather than recorded positions, and
    evidence from a session nobody queried is dropped so the denominator is the set
    actually covered.

    **An honesty defect fixed, and it is the one worth remembering.**
    `OrderOfBattle::unattributed_tracks` was hard-coded to zero by the desktop, so
    `attribution_coverage()` returned a perfect score for every product the system had ever
    produced -- including throughout the period its own documentation said the figure was
    expected to be non-zero. It is now measured.

    **GAP-019's blocker changed rather than closed.** The node has tracks now, so the
    resolver could be constructed there; what stops it is that §7.1 does not draw
    `gungnir-node` to `gungnir-identity`, and adding an edge to satisfy a register entry is
    not a worker's call. Edge (n)'s recorded reason -- "the node has no tracks until
    GAP-011" -- has expired and is annotated in `docs/design/dependency-edges.md`.

    **No dependency edge was added.** Two human-owned crates were touched:
    `gungnir-security` gained an action constant, and `gungnir-api`'s write paths gained
    the acknowledgement route. Both **signed by the owner 2026-09-06**.

    **DN-03 amendment 3 and DN-18 amendment 1 were not in that signature.** Amendment 1 in
    particular records a divergence -- DN-18 §6 said there would be no new endpoints and
    three were added -- and a divergence signed only through the code it produced is a
    note that has quietly stopped describing the system.

96. **A second exchange bearer, decided rather than assumed** (2026-09-06, D-33, GAP-090
    and GAP-091). GAP-009 and GAP-065 wired peer and coalition exchange, and reading them
    back turned up what they had quietly settled: both work only with a participant that can
    hold a machine identity in this deployment's trust roots and read the v2 contract. The
    needlines they carry also have participants who never can, and `ExchangeFormat`'s two
    non-canonical options are both unavailable -- STANAG 4676 has no obtainable specification
    and ASTERIX encode is refused by design -- so the outbound picture reaches deployments of
    this product and nothing else.

    **D-33 puts SD-16 (Cursor-on-Target) in scope** as release content, an addition to
    D-01's scope lock taken deliberately rather than an exception granted quietly. Two pins,
    not three: **the schema is pinned now** at version 2.0 (13 June 2003, MITRE case
    #11-3895, approved for public release), ahead of the codec rather than behind it, with
    the attributes transcribed in `docs/design/external-standards.md` §5.2; **the protobuf
    framing is deliberately not pinned**, because it carries no version and no date, its only
    identifier is a commit of a GPLv3 repository, and nothing in the first increment needs it
    -- negotiation begins in XML. The licence question a commit-pin would raise is deferred
    with the pin rather than answered by taking it.

    The design is `docs/design/DN-25-cursor-on-target.md`, and **nothing in it is built**.
    Its load-bearing rule is the one that keeps AP-09 intact: releasability is a property of
    the data and not of the channel, `Releasability::permits` gives an empty party nothing,
    and a multicast bearer establishes no party at all -- so a mesh sink may carry only what
    is marked to a party this deployment has itself declared, never `Internal` and never
    `AllPeers`, which promises an authentication multicast cannot give. GAP-090 is what the
    same absence costs elsewhere: DN-05 §5 rule 1 reads friendly positions from the tracks
    carried as friendly, so an empty friendly set means both "no friendly is there" and "no
    friendly was detected", and today those are the same value.

    **Edge (s), `gungnir-remote` to `gungnir-interop`, is accepted** (`dependency-edges.md`
    §13) and is **not** in a manifest and **not** drawn in §7.1: no code needs it yet, and an
    edge drawn here that no manifest carries would be this document claiming something
    untrue. The change that adds the sink adds both in the same commit.

    **Three pins after all (2026-09-08, D-33 e, f, g).** Reading the reference client's
    own source rather than its documents showed that "nothing in the first increment needs
    it" was false: `commoncommo` starts a mesh client at protocol version 1 and drops to
    XML only for a contact advertising nothing higher than 0, so a stock ATAK or WinTAK on
    the multicast group sends protobuf from its first datagram, and the I3 mesh feed and
    mesh sink meet the unpinned framing before the I4 stream sink ever does. The framing is
    now pinned at `TAK-Product-Center/atak-civ` tag 5.5.1.8 (the repository read on
    2026-09-06 was archived in May 2025), the message set is transcribed into
    `docs/design/external-standards.md` §5.4.2 and the codec is written from that table with
    nothing copied. The corpus gained a second half: a stream connection stays XML until the
    server advertises version 1, so the recorder now also listens on TCP as a server that
    never speaks. The evidence and the three decisions are in
    `docs/design/tak-interoperability-research.md`. **Still nothing in DN-25 is built**, and
    the one step no session can take, the recording itself, is still the step before the
    codec.

96A. **Two defects in the allocator, found by building on it and fixed** (2026-09-06);
    two more found and recorded unfixed. Both fixed ones were in code item 92 closed, and
    a closed entry is where a defect is least likely to be looked for.

    *Numbering note, added 2026-09-07.* This item and the one above it were both
    numbered 96. Fixed as "96A" rather than by renumbering everything from here on,
    because items 97, 99, and 100 below are cited by number from elsewhere in this
    section ("item 92's sign-off", "item 99"), and shifting them to close a numbering
    gap would silently break every one of those citations for a cosmetic gain.

    **Plans named the wrong effector against the wrong track.** `solve_exact` is handed an
    anonymous reward matrix, so it cannot know identifiers, and it returned the winning
    row and column *numbers* wrapped in `ResourceId` and `TrackId`. The caller put them
    into a plan and then looked the track up by identity, so wherever identifiers did not
    coincide with positions the lookup found nothing, the intercept geometry silently
    became absent as though no intercept existed, and the plan named things that might not
    exist. **Nothing could catch it**: every value involved was a valid value of its type,
    and every fixture in the workspace numbers its tracks and resources from zero or one,
    so the two coincided everywhere. `AllocationPolicy::assignment` now carries positions,
    which forces the caller to do the mapping only it can do -- against the *adequate*
    resource list, since that is the ordering the matrix rows were built from.

    **At the shipped default, the system recommended doing nothing, for ever.** The model
    has no time preference, so engaging now and engaging next step tie at every state, and
    the tie-break -- chosen for determinism, and documented as keeping the matching with
    the fewest pairs -- kept the empty one. A three-by-three problem returned the full
    diagonal at a horizon of one and nothing at two, three, five or ten, reporting the same
    optimal value each time; the example configuration ships a horizon of ten. Ties now
    prefer the policy that acts. That changes **which optimal policy is reported, never
    what the optimum is**: inside the model the two are worth the same, and outside it the
    target may leave and the window may close. The oracle comparison is unchanged.

    **Two found and not fixed, recorded on GAP-011.** The pipeline fragments -- one
    aircraft becomes three tracks, none confirmed -- while its position error stays small,
    so the estimator is right and the lifecycle and association around it are not. And
    `TrackView::mission_time` is the poll time rather than the estimate time, so every
    consumer ageing or scoring a track reads the wrong clock.

    Human-owned, and **signed by the owner 2026-09-06**: both allocator fixes.
    `gungnir-allocation` is reached by the numerical-stability clause, and the tie-break in
    particular is a specification decision rather than a repair. **The signature does not
    reach the two defects recorded and not fixed**: signing that this code does what it
    says is not signing that the picture is good, and the tracker still fragments and still
    stamps the poll time.

    **A gate defect fixed with them.** Fifty-one tests named a temporary directory from
    the test's own name alone, then deleted and recreated it, so two concurrent `cargo
    test` runs destroyed each other's journals. It surfaced as a file-not-found error from
    an unrelated line, never reproduced alone, and was indistinguishable from a real
    regression; three workers chased it in one day. Every one now carries the process id.

97. **Launch warnings, conformance over a real wire, and three claims the node made about
    itself that had stopped being true** (2026-09-06).

    **GAP-009's launch warning is built end to end.** DN-16 §5's rule -- a launch warning
    is a statement about the future with no kinematic state, and **it never creates a
    track, because a track we have not observed is a track we cannot maintain** -- is held
    *structurally*: warnings and detections ride separate queues with separate accessors,
    so nothing in the ingest path can produce a detection from a warning, and a caller that
    only knows about tracks keeps compiling and keeps being right. A check that could be
    forgotten would have been the weaker guarantee. A warning received from one peer is
    never forwarded to another, whatever the agreement says, because a partner able to read
    our inbound warnings can read our peer list off the stream. **Nothing issues one**: no
    producer exists and none was faked. DN-16 amendment 1 records the type and the choice
    to gate it under the existing `Warnings` exchange item rather than add a sixth.

    **The second half of GAP-009 closed with no code.** "The peer's tracks on the node
    fused once GAP-011 gives it a pipeline" needed nothing: the node already bound the peer
    adapter into its gateway and already held the tracking-service edge.

    **GAP-063's suite now runs across a real wire** between two nodes over mutual TLS, on
    canonical JSON bytes rather than on equality, through both the snapshot and the event
    stream, plus the rule that a node one schema version ahead is refused whole. The
    catalogue now refuses to stay silent: every entry must declare the test that checks it
    over the wire or the reason it is not checked, so a new entry fails the gate until
    somebody decides.

    **Three stale claims in the node, corrected.** Its module documentation said the
    pipeline, the allocator and the transport were scaffolds serving no network endpoints;
    all three are real. Two comments said cooperative evidence is not fused there because
    the node has no tracks; it has tracks, and the actual reason is that it has no edge to
    the crate that fuses evidence. **That third one is the dangerous shape**: right
    conclusion, wrong reason, so somebody fixing the reason would find the conclusion still
    held and not know why.

    Human-owned, and **signed by the owner 2026-09-06**: the peer adapter's launch-warning
    validation and quarantine, which sit in the `gungnir-ingest` gateway -- the trust
    boundary for external data. **DN-16 amendment 1 was not in that signature**, for the
    same reason the other notes were not.

    **A trap worth recording about the event stream.** A link reports itself connected when
    the *snapshot* is answered, which is before the WebSocket has subscribed, and a
    subscription from sequence zero means "everything from now" by the v2 contract. An
    envelope published in that window reaches nobody, correctly and silently. Any client
    that publishes and then waits must wait for the subscription rather than the
    connection.

98. **Edge (s), and cross-session identity on the node** (2026-09-06). GAP-019 closed.

    `gungnir-node` to `gungnir-identity`, **accepted by the owner as engineering reviewer
    and signed the same day**. The desktop's resolver answers "what is this track" for a
    panel to draw; this one answers it for the **account**. A node's journal is the
    authoritative record of a mission (§8.1), and a watch during which no desktop was
    attached is exactly the period whose correlation would otherwise be lost. It could not
    be built here before GAP-011 closed, because a resolver needs tracks.

    **An event had to exist first, and its absence is the more interesting finding.** The
    desktop keeps lineage in memory and journals none of it, and `TrackView` carries no
    entity identity, so a node that resolved a track had nowhere to put the answer.
    `IdentityEvent` is that place. Both outcomes are recorded -- the joins and the mints --
    because a reviewer asking why two sightings were *not* joined needs the mint as much as
    the confidence of a join that was made, and the correlation carries its basis as well
    as its confidence, since a number alone is an assertion rather than evidence.

    **Two things were deliberately not done.** The picture is unchanged: putting an entity
    identity on `TrackView` alters what every consumer of a track believes it is holding,
    and the record does not need it. And a second edge to `gungnir-reporting` was refused,
    because the node has no panel to draw a product on and an edge carrying products
    nothing displays is the unwired pattern this register keeps finding.

    Edge (n)'s recorded justification -- that the node has no tracks -- expired the same
    day. Edge (s) does not contradict it: identity correlation belongs where the journal
    is, and evidence fusion belongs where the operator working the picture is.

99. **The three recommendations, executed** (2026-09-06). Certificate issuance, the
    bearing-only sensor path, and the ADS-B decoder.

    **GAP-060: the rule protected the wrong property, and the obstacle was in a type.**
    The certificate glue moved out of `gungnir-node` into `gungnir-remote` -- the only
    crate both binaries depend on at runtime that already carries the rustls stack -- so
    the node's copy is **deleted** rather than duplicated on the desktop. §2.9 point 2 no
    longer says where `rcgen` may ship. It now says **no code path may build a certificate
    over private key material that has left a `KeyProvider`**, which is checkable, and
    which forbids the thing the old rule permitted: reading a key out of a provider.

    The real obstacle was never where the generator lived. `LinkTls` held the client
    identity as PEM -- a certificate **and its private key as text** -- which is what a
    provider exists to prevent, so the desktop half could not be built by handing it a
    better string. `reqwest::Identity::from_pem` accepts nothing else. The way through is
    `tls_backend_preconfigured`, which takes a whole `rustls::ClientConfig`: both the HTTP
    client and the event stream are now given the same configuration, built once over a
    client-certificate resolver, and **no private key appears in a `String` anywhere**. The
    desktop issues from its provider first and falls back to the environment second,
    saying out loud which it took -- and the fallback is at last what it always claimed to
    be, rather than the only path while being called a fallback.

    **DN-27 built: rule 1 is a type boundary, not a check.** `BearingDetection` is a
    separate type from `Detection` and no function takes a bearing and creates a track, so
    "a bearing may not initiate one" cannot be forgotten. That matters because the failure
    it prevents is silent: a fixed sensor cannot localise from bearings at all, and a
    filter given a sequence of them converges confidently to the wrong range. The crossing
    builds its covariance from the Fisher information, so it is elongated along the
    bisector and **refused** below fifteen degrees rather than returned very wide.
    `SCHEMA_VERSION` moved 2 to 3.

    **What DN-27 left unwired, and it is stated where a reader will meet it.**
    `gungnir-tracking-service` refuses a bearing rather than offering it to the pipeline,
    because a bearing needs the reporting sensor's position and `DetectionView` carries a
    `SensorId` and no position -- nothing in this workspace resolves one. The crossing has
    no caller, since the pairing that would feed it is DN-27 §9's open row. §7's display
    half is not built. So a spotter's bearing is accepted, recorded, and refines nothing
    yet.

    **The ADS-B decoder, by the open-source-consensus route, and it says so on the wire.**
    Two permissively licensed real captures vendored, both oracles gated at pinned
    versions, and -- the part that matters -- **the checksum and the position decoding
    gated by arithmetic** against published vectors rather than against consensus: parity
    linearity over a thousand frame pairs, every single-bit error, every burst to
    twenty-four bits, and the one undetectable twenty-five-bit burst shown to be the
    generator itself. `SchemaKind::Adsb1090Es` carries
    `normative_source_pinned: false`, **so a peer negotiating the schema is told before it
    reads a decode as conformant**.

    **Three defects found in the oracles, not in this code**: one reads seven of eight
    callsign characters, one cannot hold a negative altitude, one returns nothing when only
    the vertical rate is absent. And one in the reference decoders' shared design: the
    half-cell check every one of them carries for local position decoding is **dead code in
    all of them**, and a test here asserts the wrong answer on purpose so that anyone
    adding a guard that can actually fire will see it fail.

    **A gate defect fixed with them.** The whole-pipeline replay waited four seconds of
    wall clock for a background task and failed whenever cargo ran several test binaries at
    once -- a correctness test failing for want of processor time. The bound is now a
    minute: a real deadlock still fails and load no longer does.

    Human-owned, and **signed by the owner 2026-09-06**, each named against the clause
    that reaches it: the bearing path in `gungnir-fusion-async` (out-of-order measurement
    handling -- a bearing is applied at the cursor and one outside the reorder horizon is
    refused and counted); `BearingOnly` and `AzimuthElevation` in `gungnir-filters`, and
    `cross_bearings` with its Fisher-information covariance in `gungnir-coord` (numerical
    stability); and the SAPIENT spotter adapter in `gungnir-ingest` (the trust boundary for
    external data, named in the list).

    **One thing in that signature is not on the low-trust list, and saying so is the
    point.** The certificate and key-custody path -- `gungnir-remote::identity`, `LinkTls`,
    `client_config`, and the rewrite of §2.9 point 2 -- sits in `gungnir-remote`, which the
    list does not name. It holds that code only because it moved there today, and by the
    list's own stated reason, trust boundaries, it now qualifies. It was put to the owner
    explicitly rather than folded in, because that list's own precedent note says to bring
    the borderline ones and not to decide the edge unilaterally. **The owner answered the same day:
    it is on the list**, scoped to the path -- `gungnir-remote/src/identity.rs`, and
    `LinkTls` with `client_config` -- and not to the crate, because a list that swallowed
    the whole of `gungnir-remote` would make routine transport work need a signature and
    the signatures would stop meaning anything.

    **And the rule behind it became a gate rather than a discipline.** §2.9 point 2 is now
    checked by two tests. The interesting part is that it already held *by construction*:
    `KeyProvider` has six methods and none returns a private key, so a certificate over a
    key that has left custody cannot be built because such a key cannot be obtained. That
    is exactly why it needed pinning -- an invariant that holds because nobody has yet
    added a convenience getter is one afternoon from not holding, and its death would be
    silent, since every certificate path would stay green. Both tests were checked by
    adding an exporter and watching them fail.

    **What the signature does not reach.** DN-27 is a design and is not signed: a signature
    on code says the code does what it says. Nor does it reach the three things DN-27 built
    and did not wire -- the tracking service refusing a bearing, the crossing with no
    caller, §7's display unbuilt -- which are recorded above rather than covered. The ADS-B
    codec is `gungnir-interop` and is not human-owned at all, so it stands on its gate.
    Two decisions stay open: the API path against `SCHEMA_VERSION` 3, and whether a git
    dependency may be taken to get the decoder's unreleased fix.

100. **The interface compatibility rule, amended -- and the refusal it rests on, built
    first** (2026-09-06). The rule required a new schema version **and** a new path version
    for any type change, which made a change to one payload a migration for the whole
    interface: every route moves and every client re-points for a field on one message. It
    now requires a path move only where a client that does not know about the change could
    **silently misinterpret** a payload, and accepts a schema bump where such a client is
    cleanly refused.

    **The condition had teeth immediately, because that refusal did not exist.** Writing
    the amendment meant checking it, and the check failed: the outbound direction had one
    -- a desktop compares a node's snapshot version with its own -- and **no inbound path
    had anything**. `gungnir_model::check_schema_version` existed with **no caller anywhere
    in the workspace**. A machine posting the previous detection shape was refused only
    because serde could not read a bare array as an enum, which is an accident of that
    particular change and answers "the body did not decode", sending the reader to look for
    a malformed message rather than an old client.

    So the guard was built before the rule was relaxed. `SubmitDetectionRequest` carries a
    `schema_version`; a caller ahead, a caller behind, and **a caller that states no version
    at all** are each refused by name with both versions given. That third case is the
    point: the field defaults to zero rather than to the current version, because
    defaulting to current would make every client written before the field existed silently
    claim to be current, which is the opposite of what it is for.

    **A claim in `docs/gungnir-api-v1.md` was false and is corrected**: it said such a
    client was "refused by the `schema_version` check", and there was no such check.

    **A second temporary-directory finding, and it refines the first.** Adding the test
    failed with `InvalidCertificate(BadSignature)`, which reads like a TLS bug and was a
    directory bug: two tests in one binary passed the same name to the certificate harness,
    so they shared a scratch directory and one wiped the other's authority mid-handshake.
    **The process id fixed earlier today separates processes, not tests within one
    binary.** Every one of the seven harnesses now appends a per-call counter, so the name
    is a label rather than an identity. One of the two collisions was pre-existing and
    latent in `mutual_tls.rs`.

101. **Both open decisions closed** (2026-09-06). The interface path question was
    answered by amending the rule (item 100). The git-dependency question was answered by
    **leaving the decoder oracle pinned where it is and revisiting when the upstream fix is
    released**: no source-control dependency is taken and `deny.toml` is unchanged. Two
    things make that comfortable rather than merely tolerable -- it is a dev-dependency, so
    `cargo tree -d -e normal,build` shows none of it reaching either binary, and the pinned
    release is arguably the better oracle anyway, since the newer one reads the surface
    movement field as two bits where the message gives it seven.

    **A flake fixed on the way, and it was the trap item 97 recorded.** The transport test
    waited for the link to report connected and then published, but connected means the
    snapshot was answered, which is before the event stream has subscribed -- and a
    subscription from sequence zero means "everything from now", so an envelope published
    in that window reaches nobody, correctly and silently. It now waits for the stream to
    actually follow. And one test caught the new schema guard honestly: a caller that
    stated no version was refused, which is the guard working on a real caller rather than
    a hypothetical one.

### Resolved on 2026-09-08

102. **GAP-065's write path, its store-and-forward, and one producer** (2026-09-08,
    DN-18 §5 amendment 2). Amendment 1 (item 95) built the three
    `GET /v2/exchange/{warnings,reports,handoffs}` routes and stopped at a decision: "a
    write path by which a desktop posts marked products to its node, or a desktop-hosted
    transport." The write path is chosen and built: `POST` on the same three paths,
    taking the same `ExchangeProduct` list `publish_exchange` already accepted from tests
    alone, gated by a new `gungnir_security::actions::PUBLISH_EXCHANGE` -- deliberately
    not `RELEASE_PRODUCT`, which is raising or lowering a marking rather than
    transmitting an already-marked one -- granted to `Commander` and
    `IntelligenceAnalyst`, with a proposed `docs/mission/roles-and-stakeholders.md` §4 row
    rather than a silent widening. **Amended the same day**, when the pre-existing
    Supervisor/`RELEASE_PRODUCT` gap against §4's "Product release" row was found and
    fixed: the same "whoever may release, may publish" judgment now applies to
    `Supervisor` too, so `PUBLISH_EXCHANGE` and the §4 exchange row both gained it
    alongside `Commander` and `IntelligenceAnalyst`. The caller shape mirrors `task_sensor`'s, not
    `effector_report`'s: an operator's own session token, no machine identity, because a
    desktop posting to its own node is not an outside party answering something.
    Store-and-forward mirrors `gungnir-remote`'s existing
    `task_outbox`/`queue_task`/`flush_tasks` exactly, for the reason that pattern exists:
    `OutboundExchange` and `ExchangeProductRecord` are built from `gungnir-model` types
    alone, so `gungnir-app` queues a batch without gaining the production edge to
    `gungnir-api` that §7.1 refuses it, and `flush_exchange`, which already lives where
    the edge exists, converts the record to the wire type. One producer is wired,
    honestly scoped: `gungnir-app/src/handoffs.rs` republishes this desktop's whole
    handoff set on every new one, unfiltered by marking because `NodeApi::exchange_for`
    already applies that gate per party at serve time; `Warning` and `MissionReport` are
    not wired, because the first carries no releasability field DN-17 never gave it and
    the second has no running desktop collection to republish from, and neither gap was
    papered over to make the path look more finished than it is.

    **Dependency edges: none added.** `gungnir-remote` already depended on `gungnir-api`
    for the wire contract; `gungnir-app`'s existing dev-only dependency on `gungnir-api`
    (the end-to-end failover test) is untouched, and no production edge was added.

    **Human-owned crates touched: `gungnir-security` (the new action and its two role
    grants) and the `gungnir-api` write path, per `docs/agentic-workflow.md`. Signed by
    the owner the same day.** `gungnir-remote`'s outbox is ordinary transport work outside
    the identity path `docs/agentic-workflow.md` scopes as human-owned in that crate, and
    `gungnir-app`'s producer is wiring, not logic; neither needed a signature.

    **One finding, not acted on here.**
    `docs/mission/roles-and-stakeholders.md` §4 already lists `Supervisor` as holding
    "Product release", and `gungnir_security::authz::role_permits` does not grant
    `Supervisor` `RELEASE_PRODUCT` -- a pre-existing discrepancy this change did not
    introduce and does not resolve, since fixing it is its own authorization decision.
    Flagged for the owner separately rather than folded into this one's signature.

103. **The operating system's keystore, D-39.** DN-22 amendment 3 (item 84's design) built
    a passphrase-sealed file for the disconnected desktop's persistent custody "until a
    §2.9 decision admits an OS-keystore crate" -- the row §5 actually names. That decision
    is `keyring` 4.2.0, `v1` feature (`docs/agentic-coding-standards.md` §2.9, "OS
    keystore"), and DN-22 amendment 4 (§13) builds the provider against it:
    `gungnir-security/src/os_keystore.rs` gets a high-entropy secret from the platform's
    own credential store -- Windows Credential Manager, macOS Keychain, Linux Secret
    Service -- and feeds it through `PersistentKeyProvider::open_or_create`'s existing
    argon2/AES-256-GCM mechanism unchanged, so one file format serves either source of the
    wrapping string. `gungnir-app`'s `build_encryption` opens it at start rather than
    waiting for a sign-in, because the OS session being unlocked already is the login §5
    means. `gungnir-node` is not wired, since §5 assigns this row to the desktop alone and
    the node has no operator login to unlock at. **Checked, not assumed, and the check did
    not come back clean**: on Linux the Secret Service backend duplicates a generation of
    RustCrypto and duplicates `zbus` itself against the copy `gungnir-app`'s accessibility
    stack already carries (5.19.0 beside 4.4.0) -- neither this workspace's pin to change,
    both recorded rather than hidden. Windows and macOS carry neither duplicate. Human-owned
    code; written and gated, not signed. GAP-057's node account store and GAP-060's
    transport-identity persistence both named this same decision as their remaining item;
    the decision is taken and neither is built by this entry, which is D-39's alone.

104. **GAP-057's node account store, D-39's remaining item claimed** (2026-09-08). Item
    103 admitted `keyring` and built the desktop's half; this entry builds the other one
    it named and left. `gungnir-security/src/account_store.rs::EncryptedAccountStore`
    seals a node's `Vec<Account>` in one file under a key `os_keystore::wrapping_secret`
    supplies -- the same sealed-file shape `keystore.rs::PersistentKeyProvider` uses for
    key material, applied to an account list instead, and kept a separate type rather
    than forced through that one's constructor because the two payloads have nothing
    else in common. `os_keystore::wrapping_secret` gained a `service` parameter for
    this: `NODE_KEYSTORE_SERVICE` ("gungnir-node-accounts") keeps a node's entries out
    of the desktop's `gungnir-desktop-keystore` namespace, so the two never collide on
    one machine. `AuthenticationProvider::OsKeystoreAccounts { account }` names the
    provider in the baseline (DN-22 §6, DN-23 §5 rule 6: an account, never a secret,
    checked at validation the same way `KeyProviderConfig::OperatingSystemKeystore`'s
    already is); `gungnir-node/src/auth.rs::build_with_key` wires it in beside
    `LocalAccounts`, and `gungnir-app`'s two account-provider matches gained an honest
    "this provider is for gungnir-node" arm rather than a `todo!()`, since the desktop's
    own row stays local accounts per DN-23 §5 and nothing asked it to gain a second one.
    `gungnir-node account add-os-keystore`/`list-os-keystore` provision it, mirroring
    `add`/`list`'s file-backed shape with a data directory and a keystore account in
    place of a path.

    **Item 103's "no operator login to unlock at" is about a different question than
    this entry answers.** That objection is about the *key-provider* row: §5 assigns
    `OperatingSystemKeystore` custody to an interactively logged-in desktop operator's
    own session unlocking it, and a node has none to wait for -- its key-provider row
    stays `ManagedService`, untouched by this entry. This entry is not "whose login
    unlocks this"; it is "is a sealed file better than a plaintext one for accounts a
    node already keeps somewhere" -- a node's own process identity (a Windows service
    account's Credential Manager, a Linux keyring a systemd unit has been given access
    to) can hold an entry with no human login involved at all, and the doc comment on
    both the config variant and `account_store.rs` says as much rather than leaving the
    tension unaddressed. Where no such facility is reachable, `wrapping_secret`'s own
    error path fires and the node reports `AccountStoreUnavailable` -- honest, and the
    same fallback `LocalAccounts` already has for a file that will not open.

    **Verification.** Five unit tests in `account_store.rs` against a directly-supplied
    secret (mirroring why `keystore.rs`'s own tests bypass the OS keystore too):
    round-trip across a reopen and refusal under the wrong secret, add-without-replace
    refused and with it overwrites, `assign_role` changes the role and leaves the hash,
    an unknown operator is `None` rather than an error, and the file holds no legible
    account. `gungnir-security/tests/account_store_os_keystore.rs` and a new test in
    `gungnir-node/src/auth.rs` each carry one test against whatever backend the machine
    running them actually has, honest either way, the same as item 103's own tests --
    both passed for real against Windows Credential Manager on this development
    machine, cleaning up the entry each created. Three `gungnir-config` tests cover the
    new provider's validation: a valid account validates, an empty one is refused, and
    one that looks like a PHC string is refused the same way `LocalAccounts`' path
    already is.

    **Dependency edges: none added.** `gungnir-node` already depended on
    `gungnir-security` for `FileAccountStore`; `keyring.workspace = true` was added to
    its `[dev-dependencies]` only, for the same real-backend test cleanup
    `gungnir-app`'s identical dev-dependency already does, and carries the same comment.

    **Human-owned crate touched: `gungnir-security`, the account-store type and the
    `os_keystore` signature change, per `docs/agentic-workflow.md`. Written and gated,
    not signed.** `gungnir-config`'s new variant and its validation, and the wiring in
    `gungnir-node` and `gungnir-app`, are ordinary configuration and plumbing work
    outside the identity path that crate scopes as human-owned; neither needed a
    signature on its own account.

105. **GAP-060's outbound half brought in line with its serving half** (2026-09-08).
    `gungnir-node/src/main.rs::host_tls` -- what a node presents to a peer it connects
    to -- called nothing but the `GUNGNIR_TLS_CERT`/`GUNGNIR_TLS_KEY` environment
    fallback, while `spawn_tls_from_provider` had issued the node's *serving* identity
    from its key provider since this gap's own earlier work. `host_tls` now also calls
    `gungnir_remote::identity::issue_for_client("gungnir-node")` -- the exact function
    `gungnir-app`'s `link_tls_for` already calls for the desktop's own outbound
    identity, so this is porting an existing, tested pattern rather than writing a new
    one -- and sets `LinkTls::issued` from it; the environment-PEM fallback is left in
    place and still read, since `LinkTls`'s own rule is that the issued identity wins
    when both are set. A failed issuance is logged and falls through to `identity_pem`
    or to no client certificate, never a hard failure: a node's job is to run its
    pipeline and journal it, and a peer link it cannot authenticate is a link it does
    not make, not a reason to stop.

    **What this does not do, named rather than left ambiguous.** `issue_for_client`
    builds its own ephemeral `P256KeyProvider` internally, the same as
    `spawn_tls_from_provider` already does for serving -- so this closes the
    "provider-issued or not" gap between the two roles without closing the
    "ephemeral or persistent" one either already had. Making either survive a restart
    needs a `KeyProvider` this node keeps rather than builds fresh, which is a
    `gungnir-security` change (generalising `PersistentKeyProvider::
    open_or_create_via_os_keystore`'s hardcoded desktop service name, the same
    generalisation item 104 already made for `wrapping_secret`) and a decision about
    whether the node's serving and outbound roles should then share one persisted
    identity or hold two -- real design surface, not a two-line fix, and deliberately
    not taken here alongside GAP-057 in the same batch.

    **Verification.** A new unit test in `gungnir-node/src/main.rs` (the crate's first
    inline test module, `host_tls` being private to it) confirms an ephemeral provider
    always issues, and that the configured trust roots thread through unchanged. No
    dependency edge changed: `gungnir-remote::identity` was already public and already
    a runtime dependency of `gungnir-node`.

    **Not human-owned on its own account.** `gungnir-node` is not in
    `docs/agentic-workflow.md`'s human-owned list; the function called
    (`issue_for_client`) is `gungnir-remote` code already signed off under GAP-060's
    own earlier D-29 work, unchanged here.

106. **GAP-004's node half: a SAPIENT task adapter attached, and its own open row
    closed** (2026-09-08). The desktop side of outbound SAPIENT tasking was built and
    signed 2026-09-08 (`SapientTaskAdapter`, the `TaskAck` reader); the node had
    neither an adapter attached nor a way to read a `TaskAck` back, so a task issued
    through it stopped at `NotControllable`. `gungnir-node/src/main.rs::
    bind_sapient_feeds` now builds a `SapientTaskAdapter` for every feed whose source
    is `Tcp` and whose new `destination_id` config field is set, its sink an
    independent handle to that same feed's own connection
    (`TcpSapientSource::sink`, extracted before the source is erased to `Box<dyn
    SapientSource>` for the gateway, since there is nowhere left to reach the concrete
    type afterward); a new `SapientTaskRouter` dispatches by sensor id, because
    `InMemorySensorRegistry::attach_adapter` holds one adapter for the whole registry
    and a node's SAPIENT feeds are one connection per sensor, not one shared
    middleware. `apply_sapient_task_acks` reads every feed's `TaskAck`s each tick, the
    same shape `gungnir-app/src/sapient.rs::apply_task_ack` already has on the
    desktop, but publishes `SensorTaskEvent::Acknowledged`/`Failed` where the
    desktop's own reader does not -- this node is the system of record for every
    desktop connected to it, and the desktop's local record has no further audience.

    **The wire transport itself -- named "an open row" by this gap, by GAP-004's own
    register entry, and by `sapient_task.rs`'s own doc comment -- turned out to
    already be decided.** `TcpSapientSource`'s own documentation already states what
    it connects to: "a middleware serving the protobuf-JSON mapping over TCP." Reading
    outbound and writing outbound are the same connection, not two decisions;
    `TcpStream::try_clone` gives an independent handle to it, and `TcpTaskSink` writes
    one JSON object and a newline, the exact framing `take_messages` already reads.
    Closing the row needed no new design, no new dependency, and no new agreement --
    just noticing the inbound side had already made the choice this row asked for.

    **Configuration: two fields, validated together.** `SapientFeedConfig` gains
    `destination_id: Option<String>` (`Task.destinationId`, which sensor a task on
    this feed's connection is for); `ConfigBaseline` gains a new top-level
    `sapient_node_id: Option<String>` (`Task.nodeId`, this deployment's own SAPIENT
    identity, sibling to `sapient_feeds` since both binaries could in principle read
    it, though only the node constructs a `SapientTaskAdapter` today). A
    `destination_id` with no `sapient_node_id`, an empty one, or one over a `File`
    source (nothing live to write to) are each refused at validation instead of
    surfacing as a runtime construction failure.

    **Verification.** One test in `gungnir-node/src/main.rs`'s own inline test module
    drives the whole path against a real `TcpListener`: `bind_sapient_feeds` builds
    the adapter, `SensorControl::issue` reaches it through the router, the listener
    receives the exact wire JSON (`nodeId`, `destinationId`, the mapped command), and
    a `TaskAck` built from that same wire `taskId` (never hand-encoded, the same rule
    the desktop's own `sapient_task_ack.rs` test already follows) is acknowledged and
    published as `SensorTaskEvent::Acknowledged`. `TcpTaskSink` is verified against a
    real socket independently in `gungnir-ingest/src/adapters/sapient.rs`; three
    `gungnir-config` tests cover the new fields' validation rules.

    **Human-owned crate touched: `gungnir-ingest`, `TcpSapientSource::sink` and
    `TcpTaskSink`, the same trust boundary this gap's own earlier `TaskAck` reader
    sits on, per `docs/agentic-workflow.md`. Written and gated, not signed.** The
    config fields and the `gungnir-node` wiring are ordinary configuration and
    plumbing work outside the identity path that crate scopes as human-owned; neither
    needed a signature on its own account. No dependency edge changed:
    `gungnir_sensor_management::sapient_task` was already reachable from
    `gungnir-node`.

107. **GAP-024's point-to-plane CPU reference** (2026-09-08). `cpu_reference.rs`'s own
    documentation had been explicit that point-to-plane is a distinct linearised
    solve, not a corollary of `PointBuffer::normals` existing; `gungnir-data-fusion::
    point_to_plane` is that solve. A small rotation `R = I + [ω]×` and translation `t`
    minimising the summed squared point-to-plane distance is linear in six unknowns
    via the scalar triple product identity `n·(ω×p) = ω·(p×n)`, giving normal
    equations `A x = b` (`A` the 6×6 sum of `[p×n; n][p×n; n]ᵀ` over every
    correspondence); the solved rotation vector becomes a rotation through
    `UnitQuaternion::from_scaled_axis` -- the true exponential map, not the
    non-orthogonal `I + [ω]×` the linearisation itself used to reach a linear problem.
    `CpuIcpPointToPlane` drives it the way `CpuIcp` drives Kabsch: correspond by
    nearest point, solve, compose, judge convergence: it refuses at construction when
    the target carries no normals, rather than estimating one itself (`PointBuffer::
    normals`'s own documentation names exactly this as the thing this crate's
    point-to-plane path must not rest on) -- and stores the normals it takes out of
    the target as a plain field rather than re-deriving their presence from an
    `Option` on every `step`, since doing the latter needed an `expect` this
    workspace's own rule forbids outside tests and `main` (caught by
    `architecture_compliance.rs`'s own scan, not assumed clean).

    **A degeneracy check earns its place with a real example, not a hypothetical
    one.** Below a `1e-6` ratio of `A`'s smallest to largest eigenvalue -- the same
    shape of check `normals.rs` already uses for collinearity -- no rotation is
    determined by the data, the common case being every normal pointing the same way
    (a plane, or near enough). `cpu_reference.rs`'s and `normals.rs`'s own shared test
    surface (`z = sin(1.3x+0.7y)*0.4`) turned out to **be** that case for this solve
    specifically: `cond(A)` came out above `10^17` in the independent Python check
    below, several `f32` epsilons past useless, even though the identical surface is
    perfectly fine for `estimate_normals`'s own per-point PCA -- a local computation
    that never sums curvature information globally the way assembling `A` does. A
    second sinusoidal term at a different frequency and orientation
    (`+ cos(0.9x−1.7y)*0.25`) breaks the near-planarity and brings `cond(A)` to about
    280; this module's own tests use that surface instead, rather than the one two
    sibling files already share.

    **Verified independently in Python** (`numpy`, not committed -- the same
    disclosure `normals.rs` makes about its own check): a hand-built single
    correspondence gives the exact `a`/`c` vector the formula predicts by hand; a
    36-point correspondence set under a known small transform solves to within 0.5%
    of the transform's inverse in one linearisation, with the point-to-plane residual
    falling from a mean of 0.054 m to 1.8e-8 m after one further re-linearisation --
    the Newton-like quadratic convergence a correctly linearised least-squares
    problem should show.

    **Verification.** 22 tests in the crate, all passing (was 15): the hand-checked
    single correspondence, a repeated correspondence correctly refused as degenerate
    (proving the check is not vacuous), fewer than six correspondences refused as
    `EmptyInput`, a known small transform recovered in one solve and by the full ICP
    loop to 2 mm, a target with no normals refused at construction, empty overlap,
    and an aligned cloud. No dependency edge changed and no new external crate:
    `nalgebra`'s fixed-size `SMatrix`/`SVector` and `.symmetric_eigen()`/`.cholesky()`
    were already reachable, the same crate `normals.rs` and `transform_solve.rs`
    already exercise for their own decompositions.

    **Agent-assisted with a mandatory verification gate, not human-owned.**
    `docs/agentic-workflow.md` places `gungnir-data-fusion`'s CPU ICP reference and
    GPU registration pipeline under Medium-risk ("the GPU path is gated on agreement
    with the CPU reference"), not the Low-trust/human-owned list; this entry is that
    verification gate's other half, built ahead of the GPU path it will validate
    (GAP-061, whose own self-hosted GPU runner is not yet registered either).

108. **GAP-009's outbound launch warning: `LaunchWarningEvent::Issued` gets its producer**
    (2026-09-08, DN-16 §10). `gungnir_model::events::LaunchWarningEvent::Issued`'s own
    doc comment named it unbuilt -- "nothing in this workspace issues one... a producer
    was not invented to make the path look built." `gungnir-app/src/launch_warning.rs::
    declare` is that producer: a manual operator action, since nothing in the ingest path
    can detect "something launched" on its own, the same reason `requirements.rs::
    state_requirement` is manual for a different fact nothing else observes. It is gated
    on `gungnir_security::actions::RELEASE_PRODUCT` rather than a new action -- GAP-065's
    own reasoning applies unchanged: whoever may mark a product releasable is who may
    send one, so `Role::Commander` and `Role::IntelligenceAnalyst` both already qualify.
    `id` is this deployment's own serial, continued past whatever the journal recovered
    (`AppState::next_launch_warning_id`); `at` is read from `state.clock` at the moment
    of declaration, never taken from the caller. A declared warning publishes
    `Event::LaunchWarning(LaunchWarningEvent::Issued(..))` to the event bus and
    republishes the whole issued list to the exchange queue GAP-065 built
    (`ExchangeItem::Warnings`), the same unfiltered-at-source, gated-at-serve shape
    `handoffs.rs::issue_for` already uses.

    **Recovery is a straight append.** Unlike a requirement or a handoff, a launch
    warning has no lifecycle -- once issued it is never withdrawn, amended, or answered
    -- so `launch_warning::recover` folds every `Issued` event across journal sessions
    oldest-first with no fold-by-id, structurally rather than by a check that could be
    forgotten. `AppState` recovers the issued list and a `Recovered` outcome
    (`NothingIssued`/`FromJournal`/`Unreadable`) alongside `next_launch_warning`, mirroring
    `recover_requirements_or_alert`.

    **Kept apart from DN-03's warnings**, the same rule DN-16 §9 already states: this
    module never touches `gungnir_workflow::warning::Warning`, an obligation this
    deployment owes an asset and raised only by `WarningLedger::evaluate`.
    `LaunchWarningReport` is a claim about the world outside this deployment, and DN-18's
    entry (item 102) had wrongly named `Warning` as the type still needing a
    releasability field when `LaunchWarningReport` already had one from DN-16 §9
    (amendment 1, signed 2026-09-07) -- corrected in the gap register alongside this.

    **What this does not do.** No wireframe assigns a control for `declare`, so an
    operator has no caller for it outside tests; the producer exists and the panel does
    not, named rather than left to be discovered as a silent gap.

    **Verification.** `gungnir-app/tests/launch_warning.rs`, seven tests against a real
    `AppState` and a real journal: the `Forbidden`/`EmptyDescription` refusals record
    nothing, two declarations get distinct ids, a declared warning survives a restart
    read back through a fresh `FileEventJournal`, and a second desktop life continues the
    id serial rather than colliding with the first. The last two install a
    `ReplayClockAuthority` rather than the real wall clock, the same reason
    `anomalies.rs` does: an exact-equality comparison across a save-and-reopen round trip
    cannot tolerate two wall-clock reads at `f64` precision landing on different ticks.

    **Not human-owned.** `gungnir-app` is not on the low-trust list; `RELEASE_PRODUCT` is
    reused rather than defined, so no `gungnir-security` edge is touched, and nothing
    here reaches `gungnir-api`. No dependency edge changed.

109. **GAP-015's Gaussian-mixture CPHD filter, written and gated, not signed**
    (2026-09-08). `gungnir-rfs::CphdFilter`'s own stub doc comment named exactly this
    gap: `PhdFilter::cardinality` is a mean and nothing more, and a CPHD propagates the
    whole distribution over the target count. `predict` is the PHD's own intensity
    predict (survival scaling and births are the same step regardless of which filter
    carries the mixture) plus a cardinality half: the prior distribution binomially
    thinned by `p_S`, convolved with a Poisson birth count whose mean is the summed
    weight of this scan's births -- a named modelling choice, since the birth
    components carry no separate count distribution of their own. `update` is the
    closed-form Gaussian-mixture CPHD recursion (Vo, Vo and Cantoni, "Analytic
    Implementations of the Cardinalized Probability Hypothesis Density Filter", IEEE
    TSP 55(7), 2007): every candidate component's mean and covariance is identical to
    `PhdFilter::update`'s own Kalman-updated candidates, and what CPHD changes is the
    weight -- a cardinality-derived correction computed from the elementary symmetric
    functions of the detections' predictive likelihoods against the prior cardinality
    distribution, rather than PHD's simple normalisation by clutter plus the summed
    weights. `extract_tracks` commits to the strongest components up to the
    cardinality distribution's mode, the standard CPHD extraction, rather than
    reusing PHD's per-component threshold on a filter that now knows more than a mean.

    **The recursion was re-derived, not transcribed, and independently checked before
    any Rust was written.** Vo-Vo-Cantoni's closed form is dense enough that copying
    it from memory carries real risk of a subtle index or exponent error surviving
    into gated code; it was instead rebuilt from the basic multi-object likelihood
    (the probability of observing a given measurement set under a hypothesised target
    count, summed over every association and marginalised through the mixture's own
    conjugacy) and checked four ways before being trusted with a fixture: against a
    literal brute-force enumeration of every target-to-measurement association,
    independent of the elementary-symmetric-function bookkeeping the closed form
    uses; against the identity that an updated intensity's integral must equal the
    updated cardinality distribution's mean, which any correct posterior satisfies by
    construction; against reducing exactly to the plain GM-PHD update when the
    cardinality prior is Poisson, since the PHD filter is the CPHD filter restricted
    to that one assumption; and, as the property this filter exists for rather than
    an algebraic check, a 500-trial Monte Carlo comparison showing materially lower
    cardinality-estimate variance than PHD under frequent missed detections.

    **No library oracle exists for this row, confirmed rather than assumed.** Section
    2 named Stone Soup's GM-CPHD; Stone Soup 1.9.1 -- the same pinned version already
    driven for real for the PHD row beside this one -- has no CPHD updater at all,
    checked by import (`stonesoup.updater.pointprocess` exports only `PHDUpdater`).
    Unlike the PHD row, where the library exists and was found to disagree, there is
    nothing here to disagree with or agree with. `testdata/oracles/tools/
    gen_cphd_fixtures.py` carries the re-derivation and all four checks above,
    running at import time and refusing to write a fixture if any of them fail;
    `gungnir-rfs/tests/cphd_diff.rs` asserts the worst brute-force disagreement stays
    recorded in the fixture, the same role the PHD fixture's Stone Soup disagreement
    fields play, so a regeneration cannot quietly drop the evidence.

    **Corrected alongside this**: `docs/verification-capability-table.md`'s §1 row
    had been narrowed to "PHD filter" when CPHD was not yet built, even though the
    crate's own module doc comment had always cited it as "PHD / CPHD filter" -- the
    row name is restored to match what the code already cited, per this table's own
    rule that row names are cited from the doc comments rather than the other way
    round. `docs/gungnir-capabilities.md`'s business-facing description of this row
    also still named Stone Soup as the intended verification method for both filters;
    corrected to name what actually verified each.

    **Verification.** `gungnir-rfs/tests/cphd_diff.rs` (three tests: the closed form
    against three multi-scan, multi-target scenes to the row's 1e-3 tolerance on
    cardinality mean and intensity and exact agreement on cardinality mode; the
    cross-check evidence stays recorded; the fixture exercises real merging) plus
    seven new inline unit tests covering construction, the cardinality distribution
    summing to one across scans with clutter and missed detections, convergence and
    fade of the cardinality mode, and the Monte Carlo variance comparison against
    `PhdFilter` over the same scripted detection stream.

    **Human-owned crate, and so written and gated rather than signed.**
    `gungnir-rfs` is reached by `docs/agentic-workflow.md`'s numerical-stability
    clause the same way `gungnir-filters`, `gungnir-association` and
    `gungnir-track-fusion` already are (item 94's own record); this entry stands on
    its verification rather than a signature, pending the owner's review. No
    dependency edge changed and no new external crate: only `gungnir-rfs`'s own
    `nalgebra` and `thiserror`, already reachable, are used.

110. **GAP-097: an unchanged plan no longer floods its own approval queue**
    (2026-09-08). `DpInterceptService::plan_with_rewards` minted a fresh `PlanId` and
    `mission_time` on every successful solve regardless of whether the resource/track
    assignment had changed, and `outcome()` reported `PlanOutcome::Fresh` from
    `self.solver_ok` alone with no comparison to what it had already told a caller --
    so `update::tick`'s own "publish only when the plan changes" gate (GAP-066) never
    held once a solve succeeded, and a live desktop or node with one ready resource
    and one track flooded its own approval queue at the tick rate. A new
    `assignment_changed` compares the newly solved pairing against `self.last_plan` as
    a **set** of `(ResourceId, TrackId)` pairs, not the ordered `Vec`
    `solutions_with_geometry` returns (two solves of the same assignment need not
    enumerate it in the same order) and not the full `InterceptSolutionView` (a moving
    track's intercept point and time-to-intercept legitimately change every tick even
    when the resource stays tasked to it, and comparing them would defeat the fix by
    minting a new plan for that reason alone). `fresh_plan` is now called -- and
    `self.last_plan` replaced, geometry included -- only when the set differs; an
    unchanged assignment leaves the existing plan exactly as it was.

    **A second, related defect surfaced while fixing the first.** `update::tick`'s
    gate compared the live planner's output against `state.last_plan`, a field
    `gungnir-app/src/rehearsal.rs`'s scripted plans also write so PN-04/PN-05 draw
    whichever plan -- live or scripted -- was proposed most recently. A scripted
    submission overwriting that field made the live planner's own already-unchanged
    plan compare as new again on the very next tick, so the flood persisted in any
    rehearsal even after the first fix. `AppState` gained `last_live_plan_id: Option<
    PlanId>`, touched only by the live-planner step, compared by id rather than by
    value against a field something else also writes; `DpInterceptService::fresh_plan`
    never reuses a `PlanId` for a different assignment, so the id alone answers "have
    I already announced this one" without being disturbed by what else wrote
    `last_plan`. Seeded to `PlanId::default()` (`PlanId(0)`, which `next_plan_id`
    starting at 1 never mints) rather than `None`, matching the starting point
    `last_plan`'s own `PlanView::default()` already represented, so the very first
    empty solve does not compare as a change either.

    **Verification.** A new test in `gungnir-intercept-service` (an unmoving track, a
    ready resource, fifty ticks, one plan throughout) pins the first fix directly.
    `gungnir-app/tests/rehearsal.rs`'s plan-count assertion is tightened back to exact
    per its own comment -- to **eight**, not the seven that comment had guessed before
    either fix existed: none of the seven scripted plans task resource 1 against
    track 39, so the live solver's own genuine, now-stable proposal for that pairing
    is a legitimate eighth entry, not a leftover duplicate to eliminate. A companion
    assertion checks the raw queue for duplicate ids as well as distinct ones, since a
    distinct-id count alone would not have caught the second defect (it collapses
    repeated ids on its own). `gungnir-app/tests/service_contracts.rs`'s existing
    `the_desktop_proposes_nothing_while_the_allocator_is_unimplemented` caught the
    `last_live_plan_id` seeding mistake on the first attempt (`None` compared unequal
    to the very first empty plan's id and published once where it should not have),
    confirming the corrected seed before this entry was written.

    **Not human-owned.** Neither `gungnir-intercept-service` nor `gungnir-app` is on
    the low-trust list; no dependency edge changed and no new external crate.

111. **GAP-060's own remaining item: the OS keystore generalised to a TLS identity**
    (2026-09-08). Item 105 named exactly what was left -- `PersistentKeyProvider::
    open_or_create_via_os_keystore` hardcoded the desktop's own service name, and item
    104's `wrapping_secret` generalisation was the pattern to mirror rather than
    invent again. It now takes `service` as a parameter the same way: `gungnir-
    security/src/keystore.rs` no longer bakes in `DESKTOP_KEYSTORE_SERVICE`
    internally, and the constant itself became `pub` (re-exported from the crate
    root) so `gungnir-app`'s one external caller keeps naming the same string rather
    than growing its own copy.

    **Two new identities, two new service names, wired where item 105 left off.**
    `gungnir-remote/src/identity.rs` gained `issue_node_serving_identity` and
    `issue_desktop_outbound_identity`, each opening a `PersistentKeyProvider` under
    its own operating-system-keystore service (`gungnir-node-tls-identity`,
    `gungnir-desktop-tls-identity` -- distinct from `gungnir-node-accounts` and
    `gungnir-desktop-keystore`, four purposes and four names now) in its own
    `tls-identity` subdirectory of the deployment's data directory, since
    `PersistentKeyProvider::open_or_create`'s file name (`keystore.sealed`) is fixed
    and two unrelated keystores sharing one directory would overwrite each other's
    file under two different wrapping keys. `gungnir-node/src/main.rs::
    spawn_tls_from_provider` (the node's serving identity, written beside the journal
    as `node-identity.pem` for operators to pin) and `gungnir-app/src/session.rs::
    link_tls_for` (the desktop's outbound identity) call these instead of building an
    ephemeral provider directly, so each survives a restart when the keystore is
    reachable.

    **A cost found and fixed within this same entry, not carried into it: attempting
    the persistent path is not free, and `link_tls_for` is called far more often than
    "this desktop is connecting to a node."** It also runs for every peer link
    `build_ingest` binds and every reconnect attempt, which means every `AppState`
    this workspace's own test suite builds -- unconditionally issuing a persistent
    identity there would touch the real operating-system keystore, and leave an entry
    in it, for every one of them. Running the full suite once this way left about 190
    real Windows Credential Manager entries behind, `KeyProviderConfig::None` desktops
    included, none of which had asked for persistence at all. `link_tls_for` now
    attempts the persistent path only when `security.key_provider` is already
    `OperatingSystemKeystore` -- the one existing signal for "this deployment already
    uses the OS keystore," reused rather than duplicated, so a desktop's TLS identity
    gets the same custody model as its journal key and nothing else changes shape for
    everyone who has not opted in. `gungnir-app/tests/encryption_status.rs`'s two
    `OperatingSystemKeystore` tests now clean up the second real entry this leaves
    behind on a reachable machine, the same way they already cleaned up the first;
    re-running the full suite after the gate confirmed none left behind.
    `spawn_tls_from_provider` carries no equivalent gate, because it has no
    deployment-wide configuration to read (it is driven by environment variables) and
    is reached only along a narrow, already-deliberately-configured path that this
    workspace's own tests do not exercise routinely.

    **Falls back honestly, and only ever fails when the fallback also does.** An
    unreachable keystore is logged as a fallback and the same ephemeral
    `issue_for_client` path issues instead -- DN-22 §5's disconnected-fallback rule
    ("an unavailable keystore yields an honest unencrypted state... never a
    claimed-but-absent encryption") applied to a TLS identity rather than to journal
    encryption. A caller sees an error only when the ephemeral path also fails,
    which for it means `rcgen` refusing the names -- the same condition that already
    made `issue_for_client` fail before either function tried a keystore at all.

    **What this deliberately still does not do, named rather than left ambiguous.**
    Item 105 posed the standing question of whether the node's serving and outbound
    roles should share one persisted identity or hold two, and called it real design
    surface rather than a two-line fix. This entry answers neither half of it:
    `host_tls`'s call to `issue_for_client` for the node's own peer-link identity is
    untouched and stays ephemeral, so the node presents a persisted identity when
    accepting connections and a fresh one every start when making them, exactly as
    unresolved as it was before this entry. Picking either -- persisting the outbound
    half too, or unifying it with the serving one -- remains not this change's to
    decide.

    **Verification.** Two new inline unit tests in `gungnir-security/src/
    os_keystore.rs` confirm `wrapping_secret`'s existing `service` parameter keeps two
    services from sharing a secret under the same account (against the mock store);
    seven new inline unit tests in `gungnir-remote/src/identity.rs` cover a
    `PersistentKeyProvider` issuing the same identity (the same key, the same public
    half) across a reopen under the same passphrase -- `issue()`'s second
    instantiation of its `KeyProvider` generic, after `P256KeyProvider` -- both new
    functions falling back to a working ephemeral identity when the keystore
    directory cannot even be created (a file standing where a directory belongs,
    deterministic on every platform, independent of whether this machine has a
    reachable keystore), the four service names being pairwise distinct, and,
    separately, the real backend round-tripping the same key on this development
    machine's Windows Credential Manager or the documented fallback firing --
    reachable here, and it did round-trip for real. `gungnir-security/tests/
    os_keystore.rs` and `gungnir-app/src/state.rs`'s one call site were updated for
    the new `service` parameter and otherwise unchanged. `gungnir-app/tests/
    encryption_status.rs`'s two `OperatingSystemKeystore` tests gained cleanup for the
    second real keystore entry this entry's own gate now creates alongside the
    existing journal-key one on a reachable machine; the full workspace suite was run
    twice, once before the gate (confirming the roughly 190-entry cost above) and once
    after (confirming none left).

    **Dependency edges: none added.** `gungnir-remote` already depended on
    `gungnir-security` at runtime (item 64); no crate gained `keyring` or
    `keyring-core` directly, so §2.9's "Used by" column for both is unchanged.

    **Human-owned crates touched: `gungnir-security` (the generalised constructor
    and constant) and `gungnir-remote/src/identity.rs` (the low-trust-listed
    transport-identity path, `docs/agentic-workflow.md`). Written and gated, not
    signed.** `gungnir-node/src/main.rs` and `gungnir-app/src/session.rs` are
    ordinary wiring at the two call sites, outside what either policy scopes as
    human-owned on its own account.

112. **GAP-098: a point cloud can be configured, loaded, and drawn, end to end**
    (2026-09-08). The capability §3 describes -- `gungnir-data`'s loaders, the CPU
    read-back into the viewport's GL context -- had no way in and no way out: no
    configuration field named a point cloud, the only `LoadRequest` either binary sent
    was `Terrain`, and `gungnir-viewport3d` held no point-cloud layer. All three
    closed the same day they were filed; GAP-024's registration engine is untouched
    and still independent, with deliberately no edge between the two gaps.

    `gungnir_config::PointCloudConfig` (`source`, `target`, `frame`) sits beside
    `TerrainConfig`; each half is a `PointCloudFileConfig` naming a path and, for a
    COPC file, the bounds its bounded reader needs. `validate_point_cloud` refuses a
    COPC file with no bounds, bounds on a file that is not COPC (checked by the
    `*.copc.laz` suffix convention, since `Path::extension` only ever returns
    `"laz"`), an unknown extension, or a frame other than `"local-enu"` -- the same
    shape of check `validate_terrain` already made for the DEM, narrower here because
    the loader never reads a LAS file's own CRS, so there is no tag to contradict.
    `gungnir_app::pointcloud` mirrors `terrain.rs` module for module: `start` sends
    both requests to a loader channel of its own, kept apart from `terrain`'s so a
    configured terrain and a configured pair load independently rather than
    contending over one channel, and `poll` drains results without blocking a frame,
    called from `update::tick` beside `terrain::poll`. The pair is all-or-nothing --
    `data.point_clouds.len()` at the moment a result arrives is what tells
    `apply_result` whether it is the source's or the target's, resting on
    `spawn_loader`'s single worker thread draining one request channel strictly in
    order (so results arrive in the order they were sent), and a cloud that loaded
    before its partner failed is discarded rather than kept as a lone, unusable
    entry. `gungnir_viewport3d::layers::PointCloudLayer` sits beside `TerrainLayer`,
    borrowing a loaded buffer's positions the same way; `draw_point_clouds_2d`
    decimates to a 20,000-point budget and draws the source and target in two new
    theme colours so a loaded pair reads as a pair rather than one cloud.

    **Positions are drawn relative to the cloud's own origin, not placed against the
    deployment's ENU origin.** `PointBuffer` carries no parsed CRS -- unlike a DEM's
    `GridCrs`, nothing here reads a LAS file's coordinate reference system -- so there
    is no placement step to mirror `TerrainMesh::placed()`. The limitation is named on
    `PointCloudLayer`'s own doc comment rather than closed over with an invented
    alignment, the same choice `terrain.rs` makes for a DEM whose tags contradict
    `"local-enu"`. PN-09's health panel is not touched: the viewport layer is the
    reachability proof the closing action asked for, and a status line was never one
    of its three pieces.

    **Verification.** Reuses GAP-023's own vendored fixtures rather than adding a
    third: `testdata/pointcloud/five-points.las` (5 points) as source, and the real
    Autzen COPC hierarchy bounded to the same box `gungnir-data/tests/pointcloud.rs`'s
    own happy-path test already queries (4767 points) as target.
    `gungnir-app/tests/pointcloud.rs` drives a real desktop tick to a loaded pair and
    checks both failure directions of the all-or-nothing rule (a missing source
    discards nothing and names its own path; a missing target discards the source
    that had already loaded); a `gungnir-config` test covers the validation rule's
    every acceptance and refusal; a `gungnir-viewport3d` test constructs a
    `PointCloudLayer` from a populated `PointBuffer` and confirms it borrows rather
    than copies (`std::ptr::eq` on the two slices). Eleven new tests, all passing; the
    rest of `cargo test --workspace` stays green. No dependency edge changed and no
    new external crate: `gungnir-viewport3d` already depended on `gungnir-data` (§3,
    §7.1), and `gungnir-app` already depended on both.

    **Not human-owned.** None of `gungnir-config`, `gungnir-app` or
    `gungnir-viewport3d` is on the low-trust list.

113. **GAP-099: a UAS's KLV metadata (MISB ST 0601) has an adapter** (2026-09-08),
    the separate job GAP-001's own closing action pointed at: "ISR video is what
    remains -- a separate MISB-shaped job whose only open question is a fixture."
    `docs/design/external-standards.md` §8.1 had already named the fixture
    (`paretech/klvdata`'s MIT-licensed worked example) and left it untaken; this item
    takes it. `gungnir-interop/src/misb0601/mod.rs` decodes the UAS Datalink Local
    Set's KLV framing (the 16-byte Universal Label and BER short/long-form length,
    independent public knowledge) and seventeen tags -- platform heading/pitch/roll,
    the platform's own position, the sensor's pointing relative to the platform and
    its slant range, the ground point it is looking at, and four identity strings --
    whose semantics are read from `klvdata/misb0601.py`, a secondary source, because
    both NGA registry pages that carry MISB ST 0601 sit behind a bot gateway that
    refused a scripted fetch again this session. Every decoded value is checked
    against klvdata's own Python, run against the identical vendored bytes rather
    than read and paraphrased, and that run's output is the oracle
    `gungnir-interop/tests/misb0601_fixtures.rs` gates on. **A genuine finding,
    stated rather than smoothed over**: the vendored fixture's own stated checksum
    (`0xAA43`) does not match what this decoder's `packet_checksum` -- reconstructed
    from `klvdata.common.packet_checksum` and confirmed against the maintainer's own
    description of its contract, which quotes MISB ST 0601.8-08's checksum-discard
    rule -- computes over its preceding bytes (`0x3E1E`). The decoder decodes the
    frame's fields regardless, since KLV framing does not depend on the checksum, and
    reports the mismatch on `Misb0601Frame::checksum_valid` rather than trusting or
    refusing a real worked example over one field that does not arithmetically close.

    `gungnir_model::UasPlatformReport` (with `EnuPoint`) is a new model type, the
    report-shaped counterpart to AIS's and ADS-B's `CooperativeReport` but placed in
    `gungnir-model` rather than beside its adapter, because its shape -- a position
    plus an orientation plus a sensor-pointing angle -- is closer to a
    `Measurement::Bearing`-style report than to a single identity claim.
    `gungnir_ingest::adapters::misb::UasMetadataAdapter` buffers a KLV byte stream
    (an elementary stream, not a self-delimited datagram, so a frame may arrive split
    across reads), places a decoded fix in the local ENU frame and emits it as a
    `DetectionView` through the real gateway exactly as every other feed's position
    does, hands every accepted frame's full report to a side-channel sink, and
    enforces MISB ST 0601.8-08's discard rule itself: a checksum-invalid frame
    produces no detection and no report, counted by name rather than silently
    accepted. No new dependency edge: `gungnir-ingest` already depends on
    `gungnir-interop` (edge (i)) and `gungnir-model`.

    **Deliberately not built**: video decode or display of any kind, out of scope by
    the design survey's own separation of a video transport from a detection
    message; a `misb_feeds` entry in `gungnir-config::ConfigBaseline` and a bound
    socket in either binary, so this feed is built and gated but not yet wired into a
    running deployment, the same distinction this register already draws for GAP-001's
    acoustic and passive-RF halves; and independent confirmation against MISB's own
    primary text, which stays blocked on the bot gateway `docs/design/
    external-standards.md` §8 already recorded.

    **Verification.** 12 unit tests in `gungnir-interop::misb0601` (BER length forms,
    the linear-map arithmetic against the fixture's own heading bytes, an error
    sentinel read as absent, the checksum algorithm against hand-computed sums, a
    self-consistent hand-built frame, an unknown tag carried raw, truncation asking
    for more rather than erroring, key-mismatch resynchronization) and 4 fixture
    tests against the vendored capture (`testdata/misb/`, `SOURCE.md` recording the
    commit, blob id, SHA-256, licence and the checksum finding, on the same
    recording-conditions pattern as `testdata/ais/`). 4 unit tests on the adapter and
    2 tests running the vendored fixture and a hand-built well-formed frame through
    the real `IngestGateway` end to end.

    **Human-owned and unsigned**: `gungnir-ingest` is the ingest gateway
    `docs/agentic-workflow.md` names as the trust boundary for external data, so this
    is written and gated, not self-signed, pending the owner's review.

116. **GAP-095: the night theme variant, and the flat theme tokens become a threaded
    `Palette`** (2026-09-08, D-35). D-35 deferred exactly this: turning
    `gungnir-ui/src/theme.rs`'s tokens from `Color32` constants read by name into a
    swappable value was its own tranche, not a corollary of the theme review that
    raised the question. This is that tranche, built.

    **The tokens.** `theme::Palette` is a `Copy` struct with one field per DS-01
    token -- geometry, typography, stroke widths, the chrome, track lifecycle and
    health, and the viewport, exactly as `design-system.md`'s four tables list them.
    `Palette::day()` is a `const fn` reproducing the values the constants held;
    `Palette::night()` is the built half of D-35's remaining action, not merely a
    second `const` set: it scales the chrome's eight surface, text and interaction
    hues (`app_background` through `focus_color`) to 70 percent of their day WCAG
    relative luminance -- decoded to linear light, scaled, and re-encoded through two
    new private functions, `srgb_to_linear`/`linear_to_srgb`, factored out of
    `relative_luminance`'s own decode step rather than duplicated -- scales the grid
    (`viewport_grid_color`) darker still at that same 70 percent factor applied a
    second time (49 percent of day), and leaves `alert_color` (and its
    `degraded_color` alias) untouched, D-35's one pinned exception. Everything else
    DS-01 lists -- lifecycle, classification, coverage, hazard, selection, geometry --
    is outside the scope either document names and is copied from `day` unchanged; a
    new test, `palette_variant_tests::everything_outside_the_named_scope_is_identical`,
    asserts every one of those fields by name so a future change to `night()` cannot
    silently widen what it touches.

    **The setting.** `gungnir_model::UiSettings` gained `theme: String` (default
    `"day"`) and a new `ThemeVariant` enum with a `parse` method, the same split
    `AssetConfig::priority` already uses so the baseline stays a readable string and
    an unrecognised spelling is a validation failure rather than a silent default.
    `gungnir-config`'s `validate_ui` rejects anything `ThemeVariant::parse` does not
    recognise, naming the bad value and what is accepted; `ConfigBaseline::
    theme_variant()` resolves the validated string for a caller, mirroring
    `AssetPriority::parse(..).unwrap_or_default()`'s own fallback-only-after-
    validation shape. `AppState::palette` resolves `Palette::for_variant` from it
    once, in the constructor, before `config` moves into the struct; nothing
    reassigns the field afterward, and no control anywhere sets it a second time --
    D-35's requirement that a shift in the picture's colours never be a mid-session
    surprise holds because there is no code path left that could cause one, not
    because nothing tries.

    **The threading, and why it is explicit rather than a global.**
    `rust-ui-architecture-coding-standards.md` §2 forbids a global mutable static and
    requires state to be threaded explicitly; a 239-call-site token set (the theme
    review's own count) is exactly the scale a global would have tempted. It was not
    239: grepping `theme::` across `gungnir-ui`, `gungnir-app` and
    `gungnir-viewport3d` before this change found 404 references spread over 29
    files, which the register's GAP-095 row now records in place of the estimate.
    Every one of them now either reads a `Palette` field directly or calls a theme
    function that takes `&Palette` (`track_color`, `numeral`, `classification_color`,
    `operations_visuals`, `install_egui_theme`); the resolved value is passed down
    as an explicit parameter from `AppState::palette` through every panel function in
    `gungnir-ui`, through `gungnir-app`'s `workspace.rs` and `main.rs`, through
    `dock.rs`'s `PanelBehavior` (which reads `self.state.palette` inside the
    `egui_tiles::Behavior` trait methods, whose signatures `egui_tiles` fixes and
    which therefore cannot themselves take a new parameter), and through
    `gungnir-viewport3d`'s `render`/`prepare_3d`/`draw_renderer_toggle` and the
    `layers`/`tracks` drawing functions they call. `gungnir-viewport3d::gl::
    SceneRenderer`, which draws through OpenGL rather than egui and so has no `Ui`
    to carry a parameter through, holds the resolved `Palette` as a field instead,
    set once in `attach` and read by `instances` on every `paint`. `cargo check
    --workspace --all-targets` is clean, which is the actual claim behind "every
    call site was converted": the compiler, not a recount, is what found each one
    while the constants still existed to be removed out from under it.

    **Scope, deliberately not exceeded.** No live theme switcher and no
    settings-panel control were built or considered; D-35 and this entry both call
    that out as the one thing that must not exist, since a variant that could change
    mid-shift is the surprise D-35 forecloses. `theme.rs`'s own tests were extended
    rather than replaced: the existing contrast, alias and pairwise-distinctness
    checks now run against both `Palette::day()` and `Palette::night()` where the
    property should hold for either, and three new tests
    (`night_scales_the_chrome_hues_to_70_percent_luminance`,
    `night_grid_is_darker_than_the_chrome_scaling`, `alert_color_never_varies`) pin
    the exact transform DS-07 and D-35 describe, computed against real WCAG
    luminance rather than asserted from the constants that produced them.

    **Verification.** `cargo check --workspace --all-targets`, `cargo test
    --workspace`, `cargo clippy --workspace --all-targets` (clean except two
    pre-existing warnings this change did not touch: `gungnir-security/src/
    authz.rs`'s missing backticks and `gungnir-app/src/sustainment.rs`'s
    too-many-lines function) and `cargo fmt --check` all pass; `cargo test -p
    gungnir-app --test architecture_compliance` passes, including
    `no_unwrap_or_expect_outside_tests_and_main` and
    `every_shared_type_has_exactly_one_definition`. One pre-existing clippy
    threshold needed a new `#[allow(clippy::too_many_arguments)]`, on
    `gungnir-app::workspace::render_reconciliation_due`, which the new `palette`
    parameter took from seven arguments to eight; the same allowance already exists
    on three functions in `gungnir-association` and `gungnir-scenario` for the same
    reason.

    **Dependency edges: none added.** `gungnir-ui`, `gungnir-app` and
    `gungnir-viewport3d` already depended on `gungnir-model` (for `ThemeVariant`) and
    on each other exactly as `ARCHITECTURE.md` §4 and §7.1 already draw; no crate
    gained a new edge, and no crate was added to `[workspace.dependencies]`.

    **Human-owned crates touched: none.** `gungnir-ui`, `gungnir-app`,
    `gungnir-viewport3d`, `gungnir-model` and `gungnir-config` are not on
    `docs/agentic-workflow.md`'s list.

114. **GAP-100: ASTERIX Category 205 direction-finder bearings, decoded and adapted**
    (2026-09-08). `docs/design/external-standards.md` §9 pinned EUROCONTROL-SPEC-0149-31
    edition 1.0 the same day, fetched and read in full; `gungnir-interop/src/asterix/
    cat205.rs` decodes every standard-UAP item Table 3 defines, typed where the
    specification fixes a meaning and carried raw where its own §4.6 calls an item
    "implementation dependent" (I205/100, /120, /170), the same treatment Category 048
    already gives items it does not interpret. **The one design question the survey
    called "the real work"**: no message type in this category states an angular error
    for a bearing at all, so `DfSite::azimuth_sigma_rad` is the deployment's own stated
    accuracy from that direction finder's Interface Control Document, supplied by the
    caller and never invented -- the same refusal `gungnir_ingest::adapters::sapient`'s
    `range_bearing` already makes and the same rule the gateway's own validation
    enforces regardless. The decoded bearing becomes `gungnir_model::Measurement::
    Bearing`, DN-27's type, confirmed present and unchanged on `main` before this was
    built rather than assumed. Message types 1 and 3 (the processing system's own
    already-resolved position) decode losslessly but are deliberately not mapped:
    turning either into this deployment's local frame needs `gungnir-geo`, which this
    crate may not depend on. `gungnir_ingest::adapters::asterix::AsterixFeedAdapter`
    gained a third category arm and an opt-in `with_df_sites` builder, so no existing
    call site changed. No real Category 205 capture exists anywhere to vendor (checked:
    neither EUROCONTROL, the `CroatiaControlLtd/asterix` repository, nor `asterix-specs`
    carries one), so `testdata/asterix/cat205.raw` is hand-built directly from the
    specification's own byte tables and documented as exactly that in
    `testdata/asterix/SOURCE.md`'s new Category 205 section. Category 129 (UAS
    Identification Reports), surveyed alongside 205 in the same note, is deliberately
    not built here: a different report shape that would not share this gap's one hard
    question. 42 new tests across `gungnir-interop` (the codec, its fixture, and the
    catalogue's own conformance and wire-coverage declarations) and `gungnir-ingest`
    (adapter routing), all passing; no regression elsewhere.

    **Human-owned crate touched (`gungnir-ingest`, the low-trust gateway); written and
    gated, not signed.** Host configuration wiring (`ConfigBaseline`, `gungnir-app`,
    `gungnir-node`) is deferred, the same shape GAP-001 deferred Category 034's host
    wiring in; a bearing this adapter produces does not yet reach an operator's screen,
    which is GAP-096 and not this gap's to fix.

115. **GAP-096: a retained bearing reaches the operator** (2026-09-08). Of the three
    things that can happen to a bearing offered to `FusionPipeline::offer_bearing`
    (DN-27 §5), the one rule 3 calls "exactly the report an operator most needs" --
    matching no track, retained for `bearing_retention_s` -- had no path to any screen:
    `retained_bearings()` had no caller outside `gungnir-fusion-async`'s own tests, and
    `PipelineStats`' five bearing counters and `gungnir-app`'s `sapient_stats` were each
    written and read by nothing.

    `gungnir_model::BearingRayView` (sensor, origin in the local ENU frame, azimuth,
    optional elevation, angular one-sigma, valid-until) is `gungnir_tracking_service::
    project_bearing_ray`'s projection of `RetainedBearing`, read through two new,
    defaulted `TrackingService` methods -- `bearing_rays`, `pipeline_stats` -- so
    `gungnir-remote`'s connected-profile backend and every existing test double answer
    with an honest empty set rather than needing a change none of them asked for.
    `gungnir_viewport3d::tracks::draw_bearing_rays_2d` draws each as a wedge from the
    sensor along the azimuth, its edges spread by the one-sigma and reaching twice the
    visible rectangle's longer side so it runs past the edge at any pan or zoom instead
    of stopping at a fixed distance nobody measured (DN-27 §7's "does not terminate"),
    wired into both `render` and `prepare_3d`'s overlay -- at parity with tracks, not
    behind them, since the three-d GL scene draws no glyphs for anything yet either
    (item 53's open row). `gungnir-app/src/bearings.rs::tick` alerts PN-08 once per
    bearing newly appearing in `bearing_rays()`, naming the sensor and the azimuth and
    nothing the sensor did not report -- DN-27 §7's own "a gunshot, bearing 037" names
    what an acoustic sensor could classify, not a field `Measurement::Bearing` has, and
    the alert does not invent one. PN-09's `SensorHealthView` gains `bearing_feeds`
    (one line per bound SAPIENT feed, off the `SapientFeedStatsSink` values `sapient.rs`
    already held) and `bearing_pipeline` (the five counters, read live).

    **One change inside `gungnir-fusion-async` beyond this gap's own doc-comment
    correction (2026-09-08, signed the same day): written and gated, not signed.**
    `FusionPipeline` runs inside `ingest_with`'s spawned task, reachable only through
    the channel it sends snapshots on, so exposing `retained_bearings()` and `stats()`
    to `LiveTrackingService` needed the channel to carry more than `Vec<TimedTrack>`.
    `ingest`/`ingest_with` now send a `PipelineSnapshot { tracks, retained_bearings,
    stats }`, all three read from the pipeline at the same point in the loop with no
    `.await` between them -- bundled into one message rather than a second channel so a
    poller can never see a track snapshot from one epoch beside a bearing snapshot from
    another, the same reasoning `TimedTrack` already carries its own estimate time for.
    No pipeline rule changed; only what already crossed an existing channel boundary
    does. `gungnir-fusion-async/tests/oos_convergence.rs` updated its channel type to
    match and is otherwise unchanged.

    **Verification.** `gungnir-tracking-service::tests::
    a_retained_bearing_appears_in_the_view_and_leaves_it_once_expired`: a bearing
    offered to a real `LiveTrackingService` holding no track surfaces in `bearing_rays()`
    with the fields it was offered under (polled in a bounded retry loop, the idiom
    `gungnir-app/tests/frame_budgets.rs` already uses for the same async-pipeline
    reason), and a second bearing offered past the first one's `valid_until` ages it out
    -- the fourth row in `docs/verification-capability-table.md` §1, in DN-27 §10's own
    shape. `gungnir-viewport3d::tracks`'s own tests prove the drawn far point lands
    outside the visible rectangle at every scale tried and that the wedge widens as the
    one-sigma grows; `gungnir-ui`'s render-probe suite (`panels/rendered.rs`) proves
    PN-09's new feed line and pipeline counters actually reach the screen from synthetic
    state, and a new `BEARING_RAY_COLOR` theme constant passes the existing contrast and
    pairwise-distinctness gates. `cargo test --workspace` and `cargo clippy --workspace
    --all-targets` are unchanged elsewhere.

    **Left open, named rather than folded in.** `gungnir-remote`'s connected profile
    carries neither a bearing nor the pipeline's counters over the v2 wire, so a
    node-backed desktop draws no ray and no bearing health line even where the node's
    own pipeline is retaining bearings; the defaulted trait methods make that an honest
    gap rather than a wrong answer, but a gap it stays, and neither DN-27 nor this
    entry's own closing action named the wire contract. `gungnir_coord::cross_bearings`
    still has no caller (DN-27 §5 rule 2), unchanged by this entry, as GAP-001's own
    closing action already recorded.

117. **GAP-024's WGSL pipeline, its GPU-vs-CPU tests, and `gungnir-app`'s caller for
    `GpuContext::new`** (2026-09-08). The four `shaders/*.wgsl` files hold real
    compute kernels now, not stage comments: `spatial_hash.wgsl` (a uniform-grid
    spatial hash, atomic slot-claim into a fixed-capacity bucket, per
    `rust-3d-data-ecosystem-build-vs-adopt.md` §3.4's own rationale for a grid over a
    tree), `correspondence.wgsl` (apply the current estimate, then a 3x3x3-cell
    nearest-neighbour search gated by distance and, when both clouds carry normals,
    by angle), `reduction.wgsl` (a workgroup tree reduction of the Kabsch
    cross-covariance's raw moments -- no floating-point atomics anywhere, since
    neither WGSL nor this crate's `Features::empty()` descriptor guarantees one),
    and `fuse_voxels.wgsl` (confidence-weighted voxel fusion, the one stage with no
    CPU oracle to check against). The GPU path stays point-to-point (Kabsch),
    calling `transform_solve::solve_rigid_transform` unmodified, so it is
    differentially checkable against `CpuIcp` byte-for-byte; point-to-plane on the
    GPU is a materially different reduction and stays a named follow-on rather than
    something `GpuFusionEngine`'s name silently implies it already does.
    `GpuFusionEngine` dropped its `<'a>` borrowed-device lifetime for owned
    `Arc<wgpu::Device>`/`Arc<wgpu::Queue>` clones -- `gungnir-app::AppState` holding
    both the device and an engine that borrows from it would be self-referential,
    which safe Rust cannot express without a crate this workspace does not carry --
    and `gungnir_render::GpuContext`'s two fields moved to the same `Arc` for the
    same reason; still exactly one `wgpu::Device` in the process (§3, §9), since
    cloning an `Arc` shares the handle rather than creating a second device.

    **Verified two different ways, and the two are not the same claim.** All four
    kernels parse and validate under `naga` -- the same front-end and validator
    `wgpu::Device::create_shader_module` runs, reached through `wgpu`'s own
    re-export, no new dependency -- in plain `cargo test`, no GPU needed
    (`gungnir-data-fusion::gpu::validation`), and the raw-moment Kabsch
    cross-covariance algebra is checked by hand against a direct centred
    computation in the same suite. Separately, the four `#[ignore]`d `gpu-tests` in
    `gungnir-data-fusion/tests/gpu_vs_cpu.rs` (`--features gpu-tests -- --ignored`)
    passed against a real `wgpu` adapter the implementing agent's own execution
    sandbox unexpectedly had -- `get_info()` names an NVIDIA GeForce RTX 5060 Ti
    over Vulkan, the same model as the registered `gungnir-rtx-5060ti` runner --
    within `verification-capability-table.md` §2's own bar (transform within 1e-3
    m/rad, inlier ratio within 0.01 of `CpuIcp`'s result). That is real hardware
    execution, not a simulation, and it is how a real bug got found and fixed
    rather than shipped: the first version defaulted the spatial hash's cell size
    to the target cloud's bounding-box diagonal, which crowded a 36-point test
    cloud into one or two cells and silently dropped points past the fixed
    per-cell capacity, caught because the simplest case -- a cloud registered
    against itself -- failed to converge in one step. It is not, however, the
    `gpu-fusion.yml` dispatch through GitHub Actions `docs/agentic-workflow.md`
    calls this gate's recorded evidence; see the PR this change shipped in for
    whether that dispatch was attempted on this branch and what it returned before
    reading this item as the recorded verification rather than a local one.

    **`gungnir-app::fusion::FusionBackend`** is the caller `GpuContext::new` had
    none of before this, and it resolves lazily rather than at construction --
    which is itself a second real bug this entry found and fixed, not a design
    taken on faith. The first version constructed the device eagerly, inside
    `AppState::with_config_and_store`, which every one of `gungnir-app`'s several
    hundred integration tests calls to build the `AppState` it tests against: that
    requested a real `wgpu` device on every single one of them, the same "never
    inside plain `cargo test`" rule this gate exists to enforce for
    `gungnir-data-fusion` -- and slow enough under the resulting contention that
    `cargo test --workspace` looked hung on an unrelated test (`fires_deconfliction.rs`,
    three tests, three assertions, normally 0.01 s; over 300 CPU-seconds with eager
    construction still in place). `FusionBackend::new` now touches no `wgpu` API at
    all; `FusionBackend::engine_for` is what actually calls `GpuContext::new`, the
    first time any caller asks it for an engine, memoized from then on. Falls back
    to `CpuIcp` on `RenderError::NoAdapter` or any `GpuInit` failure, per
    `rust-3d-data-ecosystem-build-vs-adopt.md` §3.6 rule 3. `Features::empty()`/
    `Limits::default()` needed no revisiting: every kernel stays within the default
    limits (64-wide workgroups, no floating-point atomics, at most seven
    storage-buffer bindings in any one pipeline against the default limit of
    eight). `FusionBackend::engine_for` returns a `Box<dyn PointCloudFusion>` --
    whichever backend was actually resolved -- but nothing in `update::tick` calls
    it: GAP-098 already found that no point cloud reaches `DataStore.point_clouds`,
    so there is nothing to register against yet, and building an engine against
    fabricated data to look more finished would be exactly the fake wiring this
    document's own culture refuses; today, in practice, no code path calls
    `engine_for` at all, so no test in this workspace requests a `wgpu` device
    outside the `gpu-tests`-gated differential tests that mean to. `gpu-fusion.yml`
    stays on `workflow_dispatch` permanently (D-10 as amended 2026-09-08); nothing
    here reopens that question.

118. **GAP-099's `misb_feeds`: the ISR video-metadata feed reaches a running
    deployment** (2026-09-08). Item 113 built the MISB ST 0601 KLV decoder and its
    adapter and then named, under its own "deliberately not built", exactly what was
    missing: "a `misb_feeds` entry in `gungnir-config::ConfigBaseline` and a bound
    socket in either binary, so this feed is built and gated but not yet wired into a
    running deployment". That is closed. `MisbFeedConfig` and `MisbSource` take the
    same two-variant `Tcp { addr }` / `File { path }` shape `AisSource` and
    `AdsbSource` already have rather than inventing a third convention;
    `ConfigBaseline` carries `misb_feeds: Vec<MisbFeedConfig>`; and
    `validate_misb_feeds` -- unique names, a known sensor, one feed per sensor, a
    source that parses -- joins the main `validate()` chain. Both binaries bind every
    configured entry at start: `gungnir-node`'s `bind_misb_feeds` from
    `build_gateway`, immediately after `bind_adsb_feeds` and mirroring it, and
    `gungnir-app`'s own `misb::bind_feeds` through `state.rs`'s `build_ingest`.
    Nothing in `gungnir-ingest` itself was touched.

    **One sink is attached and one deliberately is not, and the reason is a defect
    rather than a preference.** The desktop attaches a `MisbStatsSink` -- a `Copy`
    struct overwritten in place, safe to leave unread until a PN-09 row wants it --
    and attaches no `PlatformReportSink` for `UasPlatformReport`. GAP-099's closing
    action names two remaining pieces, this wiring and primary-source confirmation,
    and evidence fusion over platform reports is neither; more to the point, a report
    sink with nothing draining it is an unbounded queue that grows for as long as a
    live feed runs. The reasoning sits in `gungnir_app::misb`'s own module doc comment
    rather than only here. What remains on the gap is independent confirmation against
    MISB's own primary text, still blocked on the NSG registry's bot gateway that
    `docs/design/external-standards.md` §8 already records; that document's "what it
    does not do" row went stale the moment this landed and was corrected with it.

119. **GAP-024's registration engine is called by a real tick, and PN-09 says which
    backend ran** (2026-09-08). Item 117 built the WGSL kernels and `gungnir-app`'s
    `FusionBackend`, and stated plainly why the last step was not taken then:
    "nothing in `update::tick` calls it: GAP-098 already found that no point cloud
    reaches `DataStore.point_clouds`, so there is nothing to register against yet,
    and building an engine against fabricated data to look more finished would be
    exactly the fake wiring this document's own culture refuses". Item 112 then made
    a configured pair reach `DataStore.point_clouds` for real, which is what changed.
    `gungnir_app::pointcloud::register`, called from `update::tick` immediately after
    `pointcloud::poll`, builds the engine through `FusionBackend::engine_for` on the
    first tick a `Loaded` pair is present and steps that same engine once per tick
    from then on -- one iteration per tick, which is `PointCloudFusion::step`'s own
    documented contract, rather than running to convergence inside a frame. An
    incomplete pair opens no engine and is a no-op, holding GAP-098's all-or-nothing
    rule rather than restating it. `AppState` carries the result as `registration:
    RegistrationOutcome` (`NoPair`, `Registered { transform, converged,
    inlier_ratio }`, `Failed { reason }`) beside the engine itself.

    **A fallback that is never silent.** `PointCloudRegistrationLine`
    (`NotConfigured`, `Pending`, `Gpu`, `CpuFallback { reason }`) is derived by the
    pure `pointcloud::registration_line` and drawn on PN-09, so a deployment whose
    GPU path failed to resolve reads as CPU-with-a-reason rather than as the GPU path
    working. The lazy-resolution rule item 117 established is intact: the only two
    tests that reach a loaded pair force `FusionBackend::Cpu` before ticking, so plain
    `cargo test` still requests no `wgpu` device, and `fires_deconfliction.rs` -- the
    canary for the eager-probe regression that had cost over 300 CPU-seconds on three
    tests that normally take 0.01 s -- stayed at its baseline.

120. **Three dependency decisions taken ahead of their engineering** (2026-09-08,
    D-40, D-41, D-42). Each settles a §2.9 question a gap could not proceed without,
    and none is yet recorded in `docs/agentic-coding-standards.md` §2.9 or built: the
    decision and the engineering are separate acts, the same distinction D-39's own
    history draws for GAP-057 and GAP-060 in item 103.

    **D-40, the inference runtime GAP-077 deferred: `ort`** (MIT OR Apache-2.0). The
    2026-09-05 deferral rested on two grounds -- no model existed to need a runtime,
    and `docs/ml/architecture.md`'s own condition of a security reviewer appointed for
    it was unmet -- and neither has changed on its own; the owner un-defers it on his
    own authority as the reviewer that deferral named, because Plan 09's schedule
    needs the runtime question settled ahead of the first trained model rather than
    behind it. `ort` over `tract` for full ONNX operator coverage, since Plan 09's
    intended models are not yet known well enough to confirm they fit `tract`'s
    smaller pure-Rust subset. **`ort`'s default `download-binaries` feature is
    refused**: it fetches prebuilt ONNX Runtime from a third-party CDN and those
    binaries may carry telemetry, so the runtime is built from Microsoft's source
    instead -- the same shape of native-dependency review `rcgen` and `vtkio` already
    get here.

    **D-41, real-world CRS for DEM and point-cloud files: `proj`, full support.**
    Neither loader converts anything today: each requires the file already be in the
    deployment's local-ENU frame and refuses it by name otherwise, because the
    approved stack has never held a projection library. Most real DEM and LIDAR data
    ships in a geographic or a projected CRS, so this is a live usability limit on
    both capabilities rather than a hypothetical one. Full projection support was
    chosen over a narrower WGS84-only first step deliberately, and it reaches GAP-023
    and GAP-098 alike.

    **D-42, cloud KMS for the `ManagedService` custody profile: AWS via
    `aws-sdk-kms`, Azure via `azure_security_keyvault_keys` with `azure_identity`.**
    DN-22 §5 names `ManagedService` and stops; item 103 built the disconnected
    desktop's OS-keystore custody under D-39 and left the cloud profile's equivalent
    with neither a design nor an admitted crate. The Azure crate is named precisely,
    because `azure_security_keyvault` is a different and deprecated package and
    choosing it from memory is exactly the error this row exists to prevent. **The
    order is stated with the decision**: `ManagedService` needs its own DN-22
    amendment before either backend is written, since no custody profile in this
    system has been built from a bare name ahead of its own design note.

## Directory layout

See the workspace `Cargo.toml` for the authoritative member list and
`docs/gungnir-workspace-structure.md` for the directory tree. Each crate's `src/lib.rs`
(or `src/main.rs` for the two binaries) carries a module-level doc comment
cross-referencing the document and section it was derived from.
