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
system of systems. §9 records the tested version set and the two GPU contexts. §10 points
at the record of what was fixed and decided, and at the generated sources for what is
still open.

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
| `gungnir-policy`, `gungnir-command` | Yes, local operator approval | `gungnir-policy` only, on every fresh plan. A node runs no approval queue: plans are decided on a desktop, and its decision route refuses with 501, so no decision reaches a node's record (GAP-129) |
| `gungnir-collab` | Linked by no binary. The arbitration rule it re-exports lives in `gungnir_model::arbitration`, which the desktop's reconciliation applies (D-53) | Linked by no binary |
| `gungnir-resilience` | Reconciliation on reconnect. Its `StoreAndForwardQueue` has no caller: the desktop's outbox is `gungnir-remote`'s (GAP-121) | Not linked: the node accepts forwarded detections through `gungnir-api` and serves the history a desktop reconciles against |
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
- Detections recorded while disconnected are store-and-forward: `RemoteTrackingService`
  queues them (bounded by `OUTBOX_CAPACITY`, oldest dropped and counted) and the link
  forwards them once the node answers. Operator decisions and audit entries stay on the
  desktop's own journal. `gungnir_resilience::StoreAndForwardQueue` has no caller
  (GAP-121).
- Reconciliation: `gungnir_resilience::reconcile` merges the local and node journals by
  mission time, drops exact duplicates, and reports conflicting decisions on the same
  plan, an expiry against a decision included. D-03's rule, `gungnir_model::arbitration`
  (a real decision beats an expiry; otherwise the higher recorded role wins, and the
  earlier decision on equal rank), resolves every conflict it can rank as soon as the
  reconciliation is computed, and journals its verdict as `LinkEvent::ConflictArbitrated`.
  A conflict it cannot rank, because a side's role was never recorded, waits on PN-18 for
  a person permitted `plan.decide`, and the switch back waits with it (D-53,
  `docs/record/2026-09-04/018.md`). No build puts a decision on a node's record yet,
  since a node runs no approval queue, so outside the tests that place one there,
  reconciliation meets no conflicting decision (GAP-129).

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

**This section keeps no items of its own since 2026-09-16.** Every item it held is a
file under [`docs/record/`](docs/record/README.md) with its wording unchanged, and a
record file is not edited once it has merged. Items 1 to 136 and 96A keep their
numbers, so "§10 item N" still names exactly one file, `docs/record/<date>/NNN.md`,
and the record's README lists them. A new item is a new file named for its date and
its subject, so two changes filed on the same day cannot collide on a number.

What this section used to say about the present is now read from sources that are
generated or checked, so it cannot fall behind the code:

| Question | Source |
|---|---|
| What is not built yet | [`docs/unbuilt.md`](docs/unbuilt.md), generated from every `NotImplemented` error the code returns |
| What is undecided | The open rows of [`docs/mission/gap-analysis/decisions-needed.md`](docs/mission/gap-analysis/decisions-needed.md) |
| What is broken or unfinished | The entries of [`docs/mission/gap-analysis/gap-register.md`](docs/mission/gap-analysis/gap-register.md) that are not Closed |
| What the owner has signed | [`docs/signatures.md`](docs/signatures.md) |
| Why something was done | The record item, and the pull request that landed it |

## Directory layout

See the workspace `Cargo.toml` for the authoritative member list and
`docs/gungnir-workspace-structure.md` for the directory tree. Each crate's `src/lib.rs`
(or `src/main.rs` for the two binaries) carries a module-level doc comment
cross-referencing the document and section it was derived from.
