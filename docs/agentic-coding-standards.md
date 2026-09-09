# Gungnir Agentic Coding & Architecture Standards

Companion reference to the workspace `ARCHITECTURE.md`, `gungnir-workspace-structure.md`,
`verification-capability-table.md`, `scenario-crate-narrative.md`, and
`gungnir-capabilities.md`. This document is written **for agents** (and their human
reviewers) doing implementation work in the Gungnir workspace. It fixes the stack —
`nalgebra`, `tokio`, `serde`, `rand`, `proptest`, `criterion`, `arrow-rs`, and `tracing`
(approved per §2.8), with the further additions listed in §2.9 pending sign-off — and
defines how code built on that stack should be structured, written, and reviewed so that
output is consistent across crates and across agent sessions, not just individually correct.

**Scope.** These standards govern the tracking core (`gungnir-core` through
`gungnir-metrics`, plus `gungnir-oracle`, `gungnir-testkit`, `gungnir-fuzz`), the service
layer (`gungnir-tracking-service`, `gungnir-intercept-service`), and the productization
layer (`gungnir-model` through `gungnir-reporting`). The UI, rendering, and 3D-data crates
follow `rust-ui-architecture-coding-standards.md`; §7 below says which rule wins where
the two documents differ. Section numbers here are cited from Rust doc comments and must
not be renumbered.

Nothing here overrides the tiered agent trust model already established for the project
(`unsafe`, concurrency correctness, and numerical-stability guarantees remain human-owned).
This document is the *day-to-day style and architecture contract* that sits underneath that
trust model.

---

## 1. Architectural principles

### 1.1 Dependency direction is one-way and matches the crate graph

Within the tracking core the chain is `gungnir-core` / `gungnir-coord` → `gungnir-filters`
→ `gungnir-association` → `gungnir-track` → `gungnir-rfs` / `gungnir-track-fusion` /
`gungnir-metrics` → `gungnir-fusion-async`, with `gungnir-allocation` depending on
`gungnir-core` only (it needs a track identifier, which `gungnir-core` owns),
`gungnir-scenario` feeding test/bench code only (never a normal dependency of a
production crate; it is a dev-dependency of `gungnir-oracle` and `gungnir-mission`), and
`gungnir-oracle` / `gungnir-fuzz` depending on whatever they verify but nothing depending
on them. `gungnir-testkit` depends on no workspace crate at all, so it can be a
dev-dependency of every core crate without a dev-dependency cycle. The workspace
`ARCHITECTURE.md` draws the full graph, including the foundation, service, productization,
deployment, and UI layers, from the actual `Cargo.toml` edges; that drawing is
authoritative where this summary is looser.

Above the core, the rule continues one-way: `gungnir-model` depends on `gungnir-core` and
`gungnir-coord` (for the primitives they own, §1.2); the service facades depend on the
core and on `gungnir-model`; the productization crates depend on `gungnir-model`, the
facades, or each other as drawn in `ARCHITECTURE.md` §7.1; the deployment crates depend
on the facades and the productization layer; and nothing in the core, the model, or the
facades depends on a productization, deployment, or UI crate.

An agent must not add a dependency edge that isn't already implied by this graph. If a task
seems to require one (e.g., `gungnir-filters` wanting something from `gungnir-association`),
that's a signal the abstraction is misplaced — stop and flag it rather than adding the edge.

### 1.2 Every public type has exactly one owning crate

No capability from the verification table is split across two crates' public APIs. If a type
needs to be shared (e.g., a `Track` struct used by both `track-manager` and `track-fusion`),
it lives in the crate lower in the dependency graph and is re-exported, never redefined.
The concrete instances of this rule today: `gungnir-core` owns `TrackId`, `TrackStatus`,
`ResourceId`, and `assert_psd`; `gungnir-coord` owns `Geodetic`; `gungnir-model` re-exports
all of them and owns the canonical views (`TrackView`, `DetectionView`, `ResourceView`,
`PlanView`) and events; the service facades re-export the model's types as their public
contract.

### 1.3 Traits define the oracle-comparable surface

Every capability row in `verification-capability-table.md` corresponds to a trait, not just a
struct — `Filter`, `Associator`, `TrackFuser`, `CoordTransform`, etc. This is what lets
`gungnir-oracle` write one differential-test harness per trait rather than per concrete type,
and it's what lets `gungnir-testkit` express property tests generically over "any `Filter`
impl" instead of duplicating proptest strategies per filter.

```rust
// gungnir-filters/src/lib.rs
pub trait Filter {
    type State;
    type Measurement;
    fn predict(&mut self, dt: f64);
    fn update(&mut self, z: &Self::Measurement);
    fn state(&self) -> &Self::State;
}
```

Concrete filters (`KalmanFilter`, `ExtendedKalmanFilter`, `UnscentedKalmanFilter`, `Imm<M>`)
implement this trait. Oracle and property tests are written once against the trait.

### 1.4 No god-crates

If a crate's `lib.rs` module list stops mapping cleanly to a single row (or tight cluster of
rows) in the capability table, that's a signal it's grown beyond its scope. `gungnir-filters`
holding KF/EKF/UKF/PF/IMM/sqrt-UDU/RTS is intentional (they're one table section); it should
never also grow association or fusion logic.

### 1.5 Feature flags express oracle/verification opt-in, not runtime configuration

`nalgebra`, `tokio`, `serde` are always-on. Anything oracle-comparison-specific (e.g., a
`proptest`-only strategy module, or a `serde`-derived debug dump used only by
`gungnir-oracle`) is gated behind a `testkit` or `oracle` feature so production builds don't
carry verification scaffolding.

---

## 2. Stack-specific conventions

### 2.1 `nalgebra`

- **Fixed-size over dynamic wherever the dimension is known at compile time.** State vectors
  and covariance matrices for a specific motion model (`CV`, `CA`, `CT`) use `SVector<f64, N>`
  / `SMatrix<f64, N, N>`, not `DVector`/`DMatrix`. This is both a performance decision
  (stack allocation, no heap churn in the hot filter loop) and a correctness one — a
  dimension mismatch becomes a compile error instead of a runtime panic.
- **`DMatrix`/`DVector` are reserved for genuinely variable-cardinality state** — PHD/CPHD and
  GLMB/LMB, where the number of targets isn't known at compile time. Don't reach for dynamic
  matrices anywhere else out of convenience.
- **Never construct a covariance matrix without going through a constructor that asserts
  symmetry and PSD-ness in debug builds.** Every `Filter` impl's `new()` and every place a
  covariance is mutated in place should call the shared `gungnir_core::assert_psd(&cov)`
  helper (debug-only, compiled out in release; it checks symmetry and attempts a Cholesky
  factorization of `P + εI`). This is cheap insurance against the exact failure mode
  `filters::sqrt/UDU` exists to prevent, and it should fire in *every* filter's tests, not
  just the sqrt-form one. It lives in `gungnir-core`, not `gungnir-testkit`, precisely so
  library code can call it.
- **Decompositions used for correctness-critical paths (Cholesky for sqrt-form, QR/SVD for
  UDU) are always taken from `nalgebra`'s own implementations, never hand-rolled.** If a
  decomposition doesn't exist in `nalgebra` for what's needed, that's an escalation to a
  human, not a reason to write a custom one inline.
- **Units are documented in doc comments, not enforced by the type system**, since a full
  units crate (e.g., `uom`) is out of scope for this stack. Every public field or function
  argument that's a physical quantity states its unit explicitly: `/// range, meters`,
  `/// bearing, radians`. An agent adding a new public numeric field without a stated unit
  has produced incomplete work.

### 2.2 `tokio`

- **`gungnir-fusion-async` is the only crate that uses the `tokio` runtime; the host binary
  owns it.** The runtime is created once by `gungnir-app` (or, on a service node,
  `gungnir-node`) and handed to `gungnir-tracking-service::LiveTrackingService::new` as a
  `tokio::runtime::Handle`, which is what drives `gungnir_fusion_async::ingest`. No other
  crate creates a runtime. Other crates may use `tokio`'s non-runtime utilities (e.g.,
  `tokio::sync` primitives) only if there's a concrete reason tied to the fusion-async data
  flow; otherwise they stay synchronous. A synchronous `Filter::update` call should never
  become `async fn` just because it's called from async code elsewhere — keep the async
  boundary at the I/O and scheduling layer (`gungnir-fusion-async`), not inside pure
  computation.
- **No blocking work inside a `tokio` task without `spawn_blocking`.** Anything that does
  nontrivial matrix work (a PHD update over a large birth/clutter set, for instance) inside
  an async context must go through `tokio::task::spawn_blocking`, not run inline on the async
  executor thread.
- **Channels, not shared mutable state, for the OOS buffer and multi-rate scheduler.**
  Inside the async pipeline, `tokio::sync::mpsc` (or `broadcast` where multiple consumers
  genuinely need the same detection) is the default. At the boundary between the async
  pipeline and synchronous callers (`LiveTrackingService::submit_detection` on the UI
  thread, the `tracks()` snapshot the per-frame tick reads) the scaffold uses
  `crossbeam-channel`, matching the render-thread handoff rule in
  `rust-ui-architecture-coding-standards.md` §5; that is the one sanctioned use of
  `crossbeam-channel` in this layer, pending the §2.9 sign-off. Anywhere an agent finds
  itself reaching for `Arc<Mutex<...>>` to pass state between async tasks, that's the
  trigger to stop and get human review — this is exactly the code path `loom` and
  mandatory human sign-off exist for.
- **Every `async fn` in `fusion-async`'s public API takes a cancellation-safety note in its
  doc comment** (does it hold partial state across an `.await` point that would be corrupted
  by a dropped future?). This is cheap to write and expensive to reconstruct later.

### 2.3 `serde`

- **`#[derive(Serialize, Deserialize)]` on every type that crosses the scenario
  export/replay or oracle-comparison boundary** — this is a hard requirement, not a nice-to-
  have, since `gungnir-scenario`'s round-trip-fidelity row and `gungnir-oracle`'s comparison
  harness both depend on it.
- **No `#[serde(skip)]` on anything that affects verification-relevant state.** If a field is
  skipped, it must be reconstructible on deserialize (e.g., a cached derived value), and the
  reconstruction logic needs a comment explaining why skipping is safe.
- **Schema types (ASTERIX/STANAG, JSON/Arrow interop) live in their own module with `serde`
  attributes doing the format mapping explicitly** (`#[serde(rename = "...")]` etc.) rather
  than relying on Rust field names matching the wire format by coincidence. Field renames
  should be visible in the type definition, not hidden in a separate mapping table.

### 2.4 `rand`

- **Every stochastic component (particle filter resampling, scenario clutter/Pd sampling,
  RFS birth processes) takes an explicit `&mut impl Rng` parameter — never calls
  `rand::thread_rng()` internally.** This is what makes the statistical tests
  (`scenario`'s self-check, the particle filter's KS-test comparison) reproducible: tests
  seed a fixed `StdRng` or `ChaCha8Rng` and get deterministic output.
- **`rand_distr` for named distributions** (Normal, Poisson for clutter counts, etc.) rather
  than hand-rolled sampling from a uniform draw, except where a specific non-standard
  distribution is the point of the test.

### 2.5 `proptest`

- **Shared strategies live in `gungnir-testkit`, not duplicated per crate.** A "valid PSD
  covariance matrix of size N" strategy, a "well-formed CV/CA/CT trajectory" strategy, and a
  "cost matrix, optionally degenerate/rectangular" strategy are each written once and
  imported everywhere they're needed.
- **Every property test states the invariant it's checking in a doc comment or a
  `proptest!` block's leading comment**, not just in the function name — e.g., "covariance
  stays PSD across N predict/update cycles regardless of input measurement noise" rather than
  a bare `#[test] fn prop_psd()`.
- **Property tests are the default for anything with a mathematical invariant** (PSD-ness,
  probability-vector sums to 1, assignment respects one-to-one mapping); example-based unit
  tests are for known-input/known-output oracle comparisons. An agent writing only
  example-based tests for a filter or associator has under-tested it.

### 2.6 `criterion`

- **One benchmark group per hot path named after the capability row it measures**
  (`ekf_predict_update`, `hungarian_solve_100x100`, `phd_update_dense_swarm`), so
  `bench-regression.yml`'s output maps directly back to `verification-capability-table.md`
  without translation.
- **Benchmarks use `gungnir-scenario` output for realistic-scale inputs**, per the table's
  "Performance" row — not hand-picked toy inputs that don't represent real workload shape.
- **No benchmark asserts pass/fail on absolute numbers** (per the capability table, this row
  is explicitly not a pass/fail gate) — only the separate CI regression check compares
  against the prior release.

### 2.7 `arrow-rs`

- **Arrow schemas are defined once per interop boundary and shared between the writer and
  reader**, never independently reconstructed on each side — a single `schema()` function in
  the owning crate, imported by both directions.
- **ASTERIX/STANAG mapping code is isolated from the Arrow schema code.** They're two
  different interop concerns (industry format vs. internal columnar representation) and
  mixing them in one module makes the round-trip test in the capability table harder to
  reason about.

### 2.8 `tracing` (approved stack addition)

Logging/diagnostics wasn't part of the original fixed stack and was flagged for sign-off per
§6 rule 4. **Approved.** `tracing` (+ `tracing-subscriber`, and `tracing-appender` if
file-based capture is wanted for CI artifact upload) is now part of the standard Gungnir
stack alongside `nalgebra`, `tokio`, `serde`, `rand`, `proptest`, `criterion`, and
`arrow-rs` — the eight dependencies plus the five additions signed off in §2.9 are the
full approved set. The conventions below
are binding: crates should have no logging calls outside these conventions, and
`println!`/`eprintln!`/the `log` crate should not be reached for as substitutes anywhere
`tracing` conventions apply.

**Why `tracing` specifically, not `log`:** `fusion-async` is the one crate with genuine
concurrent, multi-stage execution (OOS buffer, multi-rate scheduler, multiple sensor feeds
in flight at once). `tracing`'s span model is what makes a log line traceable back to *which*
in-flight fusion task or *which* filter cycle produced it — a flat `log`-style line doesn't
carry that context across an `.await` point. `tracing`'s async-aware instrumentation
(`#[instrument]`, span-following across task boundaries) is built for exactly this problem,
and it composes with `tokio` directly (`tokio-console` and `tracing-subscriber` both consume
its output), so it doesn't introduce a second, uncoordinated diagnostics story alongside the
existing async runtime choice.

**Levels — used for their capability-table meaning, not generic severity:**

- `error!` — reserved for the things the capability table treats as hard failures: a PSD
  violation, a NaN/Inf in a covariance or state vector, a `loom`-detected race surfaced at
  runtime outside the test harness, an association solver returning an infeasible assignment.
  An `error!` in Gungnir should always correspond to a definition-of-done criterion being
  violated, not general-purpose "something looked off."
- `warn!` — degraded-but-recoverable conditions the table treats as expected edge cases:
  a track entering coast state, a gate rejecting all candidate detections for a cycle, a
  sensor's OOS buffer approaching its configured depth limit.
- `info!` — capability-table-visible lifecycle events: track init/confirm/delete transitions,
  IMM mode switches, scenario generation start/end. Sparse enough that a human tailing
  production logs gets a readable narrative of what the tracker is doing, not a firehose.
- `debug!` — pipeline-stage detail useful when reproducing a specific test failure: per-cycle
  predict/update calls, association cost-matrix dimensions, fusion input track counts. Off by
  default; enabled per-crate when debugging a specific oracle mismatch.
- `trace!` — full numeric dumps (state vectors, covariance matrices, cost matrices). Gated
  behind a `trace-numeric` feature flag in addition to the log-level filter, since these lines
  are expensive to format and privacy/size-irrelevant here but still not something to pay for
  in a release build even when the level is filtered — the feature flag keeps the formatting
  code out of the binary entirely when unused.

**Spans, not ad hoc context strings:** every `Filter::predict`/`update`, `Associator::solve`,
and `fusion-async` task uses `#[instrument]` (or an explicit `tracing::span!` where
`#[instrument]`'s auto-derived fields aren't the right shape) rather than folding identifying
context into the log message text. Structured fields use the same names as the capability
table and the error-enum conventions in §3.1 (`cycle`, `track_id`, `min_eigenvalue`,
`sensor_id`) so a span's fields, an error variant's fields, and a proptest failure's shrunk
input can all be cross-referenced by the same names.

```rust
#[tracing::instrument(skip(self, z), fields(track_id = self.id))]
fn update(&mut self, z: &Measurement) {
    if let Some(min_eig) = self.covariance.min_eigenvalue_if_below_threshold() {
        tracing::error!(min_eigenvalue = min_eig, "covariance PSD violation");
    }
    // ...
}
```

**No logging in `core`, `coord`, or `allocation`.** These are the exact-match,
closed-form-oracle capabilities (§ table notes) — deterministic math with a single correct
answer doesn't benefit from runtime diagnostics, and adding spans there is noise that would
never get read. Tracing effort is concentrated in `filters`, `association`, `rfs`,
`fusion-async`, and `track-manager`, where behavior is statistical, stateful, or
timing-sensitive enough that a human debugging a failure actually needs the trail.

**Test and CI integration:** `gungnir-oracle`'s differential-test harness captures the
`trace!`-level span output (via `tracing-subscriber`'s `EnvFilter` and a test-scoped
subscriber, not a global one — global subscribers in test code cause cross-test interference)
for any comparison that fails, and attaches it to the CI failure artifact. This means a failed
oracle-diff run comes with the actual predict/update sequence that produced the mismatch,
rather than requiring the failure to be reproduced locally with logging manually turned on
after the fact.

**What this section does not cover:** metrics/observability for a production deployment
(dashboards, alerting thresholds) is out of scope for a library — Gungnir emits `tracing`
spans and events; what a downstream binary does with them (write to a file, forward to an
OpenTelemetry collector, ignore them entirely) is the downstream application's decision, not
something this workspace should couple itself to. Within this workspace that downstream
application is `gungnir-observability`, which turns `tracing` output into operator-facing
health (`gungnir-capabilities.md` §5.5); the library crates still emit spans and events
only.

### 2.9 Approved stack additions (signed off 2026-09-04)

Beyond the eight dependencies above, the following are part of the approved stack. Each
was in use before sign-off and was approved when the scope was opened; the sign-off is
recorded here the way §2.8 records `tracing`.

| Crate | Used for | Used by |
|---|---|---|
| `thiserror` | `#[derive(Error)]` on every crate-local error enum | Every crate with an error type |
| `crossbeam-channel` | Synchronous channels across the render/UI boundary, the async-pipeline boundary, and the event bus | `gungnir-fusion-async`, `gungnir-tracking-service`, `gungnir-eventing`, `gungnir-data`, `gungnir-app` (the loader thread's channels, GAP-023) |
| `serde_json` | JSON config baselines, the JSON-lines journal, recorded feeds, report export; the wire types in the conformance suite, the generator's `metadata.json`, and a test-track set read for a dataset | `gungnir-config`, `gungnir-store`, `gungnir-ingest`, `gungnir-reporting`, `gungnir-interop`, `gungnir-scenario`, `gungnir-ml` |
| `rand_distr` | Named distributions for clutter and detection sampling, per §2.4 | `gungnir-scenario`, `gungnir-filters` (particle filter) |
| `tracing-subscriber` | Subscriber setup in the two binaries and the test-scoped subscriber in `gungnir-oracle` | `gungnir-app`, `gungnir-node`, `gungnir-oracle` |

The UI stack (`eframe`, `egui`, `three-d`, `wgpu`, `gltf`, `vtkio`, `las`) is governed by
`rust-ui-tech-stack-summary.md` and pinned in the same workspace `Cargo.toml`.

#### Docking crate (signed off 2026-09-05, D-19)

D-17 (`../ARCHITECTURE.md` §10 item 25) adopted docking within each role's layout and
detaching the viewport, approval queue, and replay timeline to a second window. It left
the crate open. The owner chose `egui_tiles` on 2026-09-05.

| Crate | Used for | Used by |
|---|---|---|
| `egui_tiles` | The dock tree behind `gungnir_workflow::WorkspaceLayout::for_role`: tabs, splits, and drag-to-rearrange within a role's layout | `gungnir-app` only (since GAP-075, 2026-09-05) |

`gungnir-ui` gained a `harness` Cargo feature in the same increment: a headless render
probe over `egui::Context` so a test can assert on what a panel drew rather than on the
view it was handed. It adds no dependency -- it is egui and nothing else.

`gungnir-app` enables it from `[dev-dependencies]`, and it stays out of the shipped
binary because the workspace sets `resolver = "2"`: under resolver 1 a dev-dependency's
features unify into the normal build and the probe would have been compiled into the
desktop. Checked rather than assumed --
`cargo tree -p gungnir-app -e features,no-dev` does not mention the feature, while
`-e features` does. If the workspace ever moves off resolver 2, this moves with it.

Why this one, and what was checked:

1. **It is a tree of containers, not a fixed dock widget.** `WorkspaceLayout::for_role`
   already hands the app an ordered list of `PanelId`s per role; a tile tree is the shape
   that maps onto without inventing a second layout model. `egui_dock` was the
   alternative and is simpler, but its container model is a less direct fit for the
   detached-window half of D-17.
2. **The pin is `0.10`, and that is not the newest.** `egui_tiles` 0.10 depends on
   `egui ^0.29`, which is the workspace pin; 0.11 moved to `egui ^0.30`. Taking the newer
   one would pull a second `egui` into the binary. Resolution was checked on the pinned
   1.98 toolchain against `egui` 0.29 and `eframe` 0.29: `cargo tree -d` reports no
   duplicate `egui`, `eframe`, `emath`, or `epaint`. When the workspace moves to egui
   0.30 this pin moves with it, in the same change.
3. **Detached panels need no crate.** egui 0.29's native viewports already provide the
   second window; `egui_tiles` is only for the in-window docking.

It entered `gungnir-app`'s manifest under GAP-075 on 2026-09-05. **`gungnir-ui` does
not depend on it**, which this table said it would: an `egui_tiles::Behavior` has to
render panes, and rendering a pane means reaching the app's own state, so the tree
belongs to the binary. `gungnir-ui` keeps its egui-and-`gungnir-model` diet, and the
panels stay callable without a docking crate anywhere in sight.

The persisted arrangement is **not** `egui_tiles`' own serialized `Tree`, for the reason
that pin exists: a configuration baseline is version-gated, validated and hand-edited,
and a serialized library structure is none of those. `gungnir_model::LayoutNode`
describes an arrangement in the design's vocabulary -- panels, tabs, splits, shares --
and `gungnir-app` converts both ways.

#### API transport stack (signed off 2026-09-05, D-18)

The transport `gungnir-api-v1.md` specifies: JSON over HTTP for request and response, a
WebSocket carrying the event stream, and mutual TLS for machine identities (D-02). None of
**Entered the manifests on 2026-09-05 under GAP-041**, except the four TLS and
middleware crates, which wait for GAP-060: `axum` is in `gungnir-api`, and `reqwest`,
`tokio-tungstenite` and `futures-util` are in `gungnir-remote`. The Used-by column below
records what each is *for*; the "Landed" column records where it actually is.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `axum` (`ws`) | The node's HTTP surface and the WebSocket event stream; its `ws` feature is the server side, so no separate server WebSocket crate | `gungnir-node`, `gungnir-api` | `gungnir-api` (2026-09-05); the node uses it through that crate |
| `tokio-tungstenite` | The desktop's WebSocket **client**, which axum's `ws` feature does not provide | `gungnir-remote` | 2026-09-05 |
| `reqwest` (`json`, `rustls`, no default features) | The desktop's HTTP client. The feature is `rustls`, not `rustls-tls`: reqwest renamed it in 0.13 | `gungnir-remote` | 2026-09-05 |
| `futures-util` (no default features) | The `Sink` and `Stream` traits `tokio-tungstenite`'s socket implements; sending and receiving a frame needs them in scope. **Added 2026-09-05 under GAP-041**, not part of D-18's original list: it was already in the tree beneath `tokio-tungstenite`, and this row is for naming it directly | `gungnir-remote` | 2026-09-05 |
| `rustls` | TLS, including the client-certificate verification mutual TLS needs, and the client configuration the desktop's event stream is spoken over. Also the PEM reader: `rustls::pki_types::pem` is rustls's own re-exported type crate, and it **reads certificates and keys; never holds or logs the key material** (`gungnir-security` owns custody, DN-22). It replaced `rustls-pemfile` on 2026-09-07 when RUSTSEC-2025-0134 marked that crate unmaintained, which removed a crate from the graph and added none | `gungnir-api`, `gungnir-node`, `gungnir-remote` | 2026-09-06 (GAP-060, both sides); PEM reader swap 2026-09-07 |
| `tokio-rustls` | rustls over tokio streams: the node's acceptor, and the desktop's connector under `tokio-tungstenite` | `gungnir-api`, `gungnir-remote` | 2026-09-06 (GAP-060, both sides) |
| `tower-http` (`trace`, `limit`) | Request tracing and body-size limits on the node's HTTP surface, which is an untrusted-input boundary | `gungnir-node` | Not yet: GAP-060 |

The existing `tokio` pin also gained the **`net`** feature on 2026-09-05, which is what
binds the listener. Recorded here as well as in §9 because it is the one change to the
signed version set GAP-041 needed.

**The single-copy claim was re-checked on 2026-09-05** by resolving the whole set against
the registry of that day: one `rustls` (0.23.43), one `tungstenite` (0.29.0), one
`tower-http` (0.6.11), one `hyper` (1.11.1). The pins hold.

Five things about this set are deliberate and should not be changed without a new sign-off:

1. **rustls, not `native-tls`.** Neither the Linux node container nor the Windows desktop
   then needs a system OpenSSL, and one TLS implementation appears in the SBOM, which is
   what the vulnerability-response objective in `release-governance.md` is measured on.
2. **The pins avoid duplicate linkage.** `tokio-tungstenite` is 0.29 because that is what
   `axum` 0.8 resolves to, and `tower-http` is 0.6 because that is what `reqwest` 0.13
   resolves to. Taking the newer of either pulls a second copy into the binary. Resolution
   was checked on the pinned 1.98 toolchain: one `rustls` (0.23), one `tungstenite`, one
   `tower-http`, and the only duplicates left are `getrandom` and `syn`, both transitive
   and both build-time.
3. **`tokio-tungstenite` takes no TLS feature of its own.** Its `rustls-tls-webpki-roots`
   feature would bundle Mozilla's root store, which mutual TLS against our own certificate
   authority has no use for, and it pulls a second `webpki-roots`. The client instead
   establishes a `tokio-rustls` stream with our roots and our client certificate and hands
   the finished stream to `client_async`. That keeps the TLS configuration in one place —
   ours — rather than splitting it between two crates' feature flags.
4. **`hyper` and `tower` are transitive, not approved for direct use.** They arrive under
   `axum` and `reqwest`. Naming either type directly in our code is a new §2.9 row.
5. **gRPC is not in this sign-off**, and was taken separately: `tonic` and `prost` were
   signed off as D-21 on 2026-09-05 and have their own subsection below. This rule stands
   as the record that D-18 did not cover them.

#### Cryptography (signed off 2026-09-05, D-20)

Raised by DN-23 §9. **No cryptographic crate was in this workspace at all**: D-02 chose
the credential mechanism in 2026-09-04 and never chose the libraries, exactly as D-18 had
to be raised before the transport could be written.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `argon2` (with `password-hash`) | Verifying an operator's passphrase against a local account store. Memory-hard, so a stolen store is expensive to attack offline; the current password-hashing competition winner and the OWASP default | `gungnir-security` | Not yet: GAP-057 |
| `hmac` + `sha2` | Integrity of the short-lived session tokens a node issues (D-02); `sha2` alone for the seed hash a rehearsal journals (GAP-089) and the content hash that identifies a dataset (GAP-079) | `gungnir-security`, `gungnir-node`, `gungnir-app`, `gungnir-ml` (`sha2`) | 2026-09-06 (GAP-057's node half, GAP-089, GAP-079) |
| `subtle` | Constant-time comparison. Already transitive under the above; named directly so a comparison is obviously constant-time rather than incidentally so | `gungnir-security` | Not yet: GAP-057 |

Three things are deliberate:

1. **A symmetric MAC, not a public-key signature, for session tokens.** D-02 has the node
   issue and verify its own tokens, so there is no third party to verify a signature.
   Choosing a public-key scheme would put a private key in the process, which is precisely
   the conflict that stopped TLS in GAP-041 against DN-22's `seal`/`unseal` boundary. If a
   peer C2 system must one day verify a Gungnir operator token itself, that is a different
   decision to be taken then.
2. **One family.** All RustCrypto, so the workspace has one cryptographic audit surface
   rather than several, which is the same argument §2.9 rule 1 makes for rustls over
   `native-tls`.
3. **Resolution was checked**, not assumed, on 2026-09-05 against the whole transport
   stack: one copy each of `argon2` 0.5.3, `password-hash` 0.5.0, `hmac` 0.12.1, `sha2`
   0.10.9, `subtle` 2.6.1, and of the shared `digest`, `generic-array`, `crypto-common`
   and `base64ct` beneath them.

**Two purposes are still unnamed, and they are not covered by this sign-off.** DN-22's
`KeyProvider` needs an authenticated cipher for `KeyPurpose::JournalAtRest` and a
signature scheme for `KeyPurpose::BaselineSigning`, and neither DN-22 nor the register
names an algorithm, let alone a crate. Nothing above supplies either: `hmac` authenticates,
it does not encrypt. Those two rows were **GAP-060 and GAP-084's to raise**, and the
recommendation to argue there was AES-256-GCM (`aes-gcm`) and ECDSA P-256 (`p256`), both
FIPS-approved algorithm choices for a system whose documents keep an accreditor in view,
and both RustCrypto so this family stays one family. Both were taken as D-22 below:
`aes-gcm` and `rcgen` on 2026-09-05 with GAP-060, and `p256` the same day once the owner
took the third row that had been left open because no gap needed it yet.

#### Data protection (signed off 2026-09-05, D-22)

Raised by GAP-060, which found DN-22 unusable as written: the note designs a custody
boundary and names no algorithm. D-20 covered authentication and explicitly not this --
`hmac` authenticates and does not encrypt.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `aes-gcm` | AES-256-GCM behind `KeyProvider::seal`/`unseal`: the journal at rest, and anything else a provider protects | `gungnir-security` | 2026-09-05 |
| `rcgen` | Making certificates **in tests**, so mutual TLS can be verified at all; and, **since D-29 (2026-09-06), issuing the node's own TLS identity at runtime** through its `SigningKey` trait over `P256KeyProvider::sign`, so the private half never leaves the provider (`gungnir-node/src/identity.rs`, DN-22 amendment 1). A normal dependency of **`gungnir-remote`** alone, with `default-features = false` and the `aws_lc_rs` and `pem` features, so no second `ring` enters. It moved there from `gungnir-node` on 2026-09-06 (GAP-060) so that both binaries issue an identity from the same code rather than from two copies; see point 2 below for the rule that replaced "ships in the node and nowhere else" | `gungnir-remote` (runtime, D-29 as amended); `gungnir-api` (tests) | 2026-09-05; runtime 2026-09-06 |
| `p256` (`ecdsa`, `ecdh`, `pem`) | ECDSA P-256 for `KeyPurpose::BaselineSigning` and the transport identity, and ECDH for escrow (DN-22 §11). **The third row, signed off 2026-09-05** after standing open because no gap needed it; the `ecdh` feature added 2026-09-06 under GAP-084, as §11 said it would be, with no new crate | `gungnir-security` | 2026-09-06 (GAP-084's provider; unwired) |

Four things are deliberate:

1. **AES-256-GCM rather than ChaCha20-Poly1305.** Both targets have AES hardware
   acceleration, and AES-GCM is FIPS-approved where ChaCha20-Poly1305 is not -- which
   matters for a system whose documents keep an accreditor in view. The nonce is
   per-operation and never reused under one key, which is the discipline that makes
   AES-GCM safe rather than catastrophic; DN-22 amendment 1 (b) specifies the sealed form
   that carries it.
2. **No code path may build a certificate over private key material that has left a
   `KeyProvider`** (2026-09-06, superseding "`rcgen` ships in the node and nowhere else",
   which superseded "it never ships").

   **The rule is now about the key, because the previous one protected the wrong thing.**
   "A certificate generator in a deployed binary is a way to mint an identity" is only true
   given a signing key. Here every certificate is self-signed over a key generated inside a
   provider and signed through the provider's `sign`, so the control is the provider and
   not the absence of `rcgen`. Two consequences follow, and both were hidden by the old
   wording: the old rule forbade something nearly harmless, and it did not forbid the thing
   that would actually be harmful -- reading a private key out of a provider and building
   a certificate over it, which the old rule permits and this one does not.

   The new rule is also **checkable**, which the old one was not, and as of 2026-09-06 it
   is checked: `gungnir-app/tests/architecture_compliance.rs` carries
   `no_path_exports_private_key_material` and
   `the_key_provider_surface_is_the_one_the_certificate_path_relies_on`.

   **The rule holds by construction today, and that is precisely why it needed a test.**
   `KeyProvider` has six methods -- `active`, `state`, `seal`, `unseal`, `rotate`, `sign`
   -- and not one returns a private key; the concrete providers add only `generate`,
   `public_key_der` and `public_key_sec1`; nothing in `gungnir-security` can hand a caller
   private key material at all. So a certificate over a key that has left custody cannot
   be built, because such a key cannot be obtained. **An invariant that holds because
   nobody has yet added a convenience getter is one afternoon from not holding**: the day
   somebody adds a private-key exporter for a backup feature or to move a key between
   machines, the rule dies and every certificate path stays green, because nothing
   downstream would change. Both tests were checked by adding such an exporter and
   watching them fail; the messages name this rule and say the amendment is the owner's
   decision rather than something to relax.

   `rcgen` is therefore a normal dependency of `gungnir-remote`, which owns
   `identity::issue` and `LinkTls`, and a dev-dependency elsewhere. It moved there from
   `gungnir-node` on 2026-09-06 under GAP-060, because the desktop needs the same thing and
   two binaries cannot depend on each other: `gungnir-remote` is the only crate both
   already depend on at runtime that already carries the rustls stack.

   **A note on what this cost, because it is the interesting part.** `LinkTls` used to hold
   the client identity as PEM -- a certificate *and its private key* as text -- which is
   the one thing a provider exists to prevent, and which is why the desktop half of GAP-060
   could not be built by simply handing it a better string. `reqwest::Identity::from_pem`
   accepts nothing else. The way through was `tls_backend_preconfigured`, which takes a
   whole `rustls::ClientConfig`, so the HTTP client and the event stream are now given the
   same configuration built once over a client-certificate resolver, and no private key
   appears in a `String` anywhere.
3. **Checked for duplicate linkage** on 2026-09-05, as D-18 requires of every addition:
   one copy each of `aes-gcm` 0.10.3, `aes`, `ctr`, `universal-hash`, `aead`, and no
   second `rustls` or `ring` beneath `rcgen`. For `p256`: one copy each of `p256` 0.13.2,
   `elliptic-curve` 0.13.8, `ecdsa` 0.16.9, `primeorder` 0.13.6 and `sec1` 0.7.3, and no
   second copy of the `sha2`, `digest`, `generic-array`, `crypto-common`, `base64ct` or
   `subtle` already beneath D-20's crates.

   **How an unused entry is checked, because it is not obvious.** A
   `[workspace.dependencies]` entry no member uses is never resolved: it is absent from
   `Cargo.lock`, and `cargo tree -i <crate>` reports that the package does not exist. The
   crate must be added to a member temporarily, `cargo metadata` run, the lockfile read,
   and the temporary edge removed. That is how the numbers above were obtained, and it is
   how the next unused addition should be checked.
4. **P-256 rather than Ed25519**, and a signature scheme at all rather than reusing
   `hmac`. DN-22 amendment 1 (a) makes the second argument: `seal`/`unseal` cannot produce
   a signature over a TLS transcript, so a cloud deployment whose private key never leaves
   a hardware module or a managed key service needs `sign` rather than a getter, and a MAC
   cannot serve because there is no shared secret with the peer. P-256 because it is
   FIPS-approved, which is what the accreditor these documents keep in view will ask;
   Ed25519 is the better curve on most other grounds and loses on that one. RustCrypto, so
   the family stays one family.

   **`p256` is in the manifest and used by nothing.** No gap builds the asymmetric
   provider, and none may depend on this crate until one does — the same condition D-21
   put on `tonic`. What the sign-off unblocked is the decision, which is what GAP-060's
   remaining half was waiting on.

#### OS keystore (signed off 2026-09-08, D-39)

DN-22 amendment 3 (§12) named the condition exactly: the passphrase-sealed keystore
stands "until a §2.9 decision admits an OS-keystore crate." GAP-084's action said the
same thing from the register's side, and GAP-057's and GAP-060's remaining items both
reduce to this same decision seen from authentication and from transport identity.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `keyring` (`v1`, no default features) | `KeyProvider`'s persistent custody unlocked at operator login rather than a typed passphrase (DN-22 §5): Windows Credential Manager, macOS Keychain, or Linux Secret Service, chosen by target and reached through the crate's own backward-compatible `v1::Entry` facade | `gungnir-security` | 2026-09-08 |
| `keyring-core` (no features; `keyring`'s own dependency, named directly) | Production's route to the store `keyring` selected, addressed directly because `v1::Entry` cannot be redirected to a test double (point 2 below); its always-on `mock` module -- a platform-independent, in-memory credential store needing no feature flag, unlike the fancier `sample` store `keyring`'s `cli` feature would pull -- is then the same route a test takes to a store that needs no real Secret Service session, which a headless Linux CI runner does not have | `gungnir-security` (a normal dependency, not test-only) | 2026-09-08 |

Four things are deliberate:

1. **`default-features = false`, `v1` only.** `keyring`'s `cli` feature (off by default,
   named here so nobody reaches for it later without reading this) exists for the crate's
   own demonstration binary: it pulls every platform backend regardless of target,
   `keyring-core`'s fancier `sample` store, and `dbus-secret-service-keyring-store` with a
   **vendored OpenSSL** -- none of which a library dependency wants. `v1` pulls exactly
   the three backends DN-22 §5 needs (Windows, macOS, Linux) and nothing to demonstrate
   them with.
2. **`v1::Entry`, not `keyring_core::Entry`, in production code -- and the reason is
   mechanical, not stylistic.** `v1::Entry::new`'s first call latches the real
   platform-native store as `keyring-core`'s process-wide default and never checks whether
   one is already set, so whichever caller creates the first entry in a process decides
   the backend for every entry after it. A test that wants the mock store cannot go
   through `v1::Entry` at all -- it has already lost the race to the real backend the
   moment any `v1::Entry` exists in that binary. `gungnir-security/src/os_keystore.rs`
   therefore forces the real backend once (`keyring::v1::Entry::store_status()`, which is
   exactly what that method exists for) and then talks to `keyring_core::Entry` directly,
   so its tests can install `keyring_core::mock::Store` first instead and exercise the
   same code path a production caller does. This is the pattern `keyring-core`'s own test
   suite (`src/mock.rs`) uses for the identical reason.
3. **Checked for duplicate linkage on 2026-09-08, and the finding did not come back
   clean, which is worth recording precisely rather than rounding off.** On the Windows
   target -- both binaries' primary target today -- `cargo tree -p gungnir-security -d`
   shows no duplicate beneath `keyring` at all: the only repeated package anywhere in the
   crate's tree is `syn` (v2 for `thiserror-impl`/`tracing-attributes`, v3 for
   `serde_derive`), pre-existing and build-time only, the same finding D-18 recorded.
   **On the Linux x86_64 target the Secret Service backend costs two real duplicates.**
   `zbus-secret-service-keyring-store` pulls `secret-service` 5.2.0, which depends on a
   newer generation of RustCrypto than D-20/D-22 pinned for the rest of the workspace --
   a second `aes` (0.9.3 beside 0.8.4), `digest` (0.11.3 beside 0.10.7), `hmac` (0.13.0
   beside 0.12.1), `sha2` (0.11.0 beside 0.10.9), `cipher`, `cpufeatures`, `const-oid` and
   `block-buffer` -- because the Secret Service D-Bus protocol encrypts its own session
   and `secret-service` brings the crypto for that, independent of whatever the rest of
   this workspace already pins. **And `zbus` itself duplicates, which an earlier review of
   this decision believed it would not**: `gungnir-app`'s accessibility stack
   (`accesskit_unix` on Linux, reached through `eframe`) already carries `zbus` 4.4.0, and
   `zbus-secret-service-keyring-store` pulls `zbus` 5.19.0 -- a second major version, not a
   reused one. Checking found this wrong before it was written down as right rather than
   after, which is the entire point of checking. **Accepted for now, on Linux only,
   because neither duplicate is this workspace's to fix**: the RustCrypto generation is
   `secret-service`'s own pin, and the `zbus` version is `accesskit_unix`'s and
   `zbus-secret-service-keyring-store`'s respective pins, not a choice available from
   `keyring`'s feature set. Neither package appears on the Windows or macOS target at all,
   since neither the native credential-manager nor the Keychain backend touches D-Bus.
4. **`gungnir-app` only, not `gungnir-node`.** DN-22 §5 assigns the operating system's
   keystore to the disconnected desktop specifically, the same row `PassphraseSealedFile`
   answers; the node's own row is `ManagedService`, unbuilt for the same reason it always
   was. `gungnir-node/src/main.rs`'s `seal_journal` therefore keeps no arm for
   `KeyProviderConfig::OperatingSystemKeystore`, exactly as it already keeps none for
   `PassphraseSealedFile`.

#### ONNX inference runtime (signed off 2026-09-08, D-40)

GAP-077 built `gungnir-ml`'s `Model`/`FeatureExtractor` traits and `ModelSet` on
2026-09-06 but deliberately deferred the runtime itself: no model existed yet to run,
and `docs/ml/architecture.md`'s own condition for a native ML dependency -- a security
reviewer appointed for it -- had not been met. The owner un-deferred it on 2026-09-08 on
their own authority as the reviewer the deferral named, ahead of a model actually
existing, because Plan 09's own schedule needs the runtime question settled before
GAP-080 can plan around it.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `ort` (default features disabled; `download-binaries` specifically refused) | The `Model` trait's real backend: loading and running an ONNX graph | `gungnir-ml` | Not yet: no gap builds it |

Two things about this one:

1. **`ort` over `tract`, for op coverage, not for safety.** `tract` is a pure-Rust
   ONNX-subset interpreter and would have sidestepped a native dependency entirely, but
   nobody has checked whether Plan 09's intended models fit inside that subset, and `ort`
   wraps the real ONNX Runtime with the full operator set. If a future gap finds `tract`
   covers what is actually needed, revisiting this pin costs nothing that has been built
   yet -- nothing has.
2. **`download-binaries` is refused, not merely left off by default.** That feature
   fetches a prebuilt ONNX Runtime from a third-party CDN, and those binaries may carry
   telemetry -- not acceptable in this system regardless of convenience. Whoever builds
   GAP-077's runtime must build ONNX Runtime from source instead, the same shape of
   native-dependency handling this stack already gives `rcgen` (§2.9 point 2) and
   `vtkio` (GAP-023): a build-time cost accepted in exchange for not trusting a binary
   this project did not build. **Not yet checked**: `ort`'s exact resolved version, its
   duplicate-linkage footprint against the rest of the workspace, and whether a C++
   toolchain needs adding to `ci.yml` for the from-source build -- all owed by GAP-077's
   own implementation, not decided here.

#### Coordinate reference system projection (signed off 2026-09-08, D-41)

Neither the DEM loader (GAP-023) nor the point-cloud loader (GAP-098) converts a file's
coordinates -- each requires the file already be in the deployment's local-ENU frame,
because the approved stack had never admitted a projection library, and refuses
anything else by name rather than placing it wrong. Most real DEM and LIDAR data ships
in a real-world CRS (geographic WGS84 or a projected system such as UTM), so this was a
live usability limit on both capabilities, not a hypothetical one, and the owner closed
it on 2026-09-08.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `proj` | Converting a DEM's or a point cloud's declared coordinate reference system -- geographic or projected -- to the deployment's local-ENU frame | `gungnir-data` | Not yet: no gap builds it |
| `proj-sys` (`proj`'s own dependency, binding `libproj` v9.6.x) | Finds a system `libproj` install or builds one from source when none is found -- the same two-path shape `vtkio`'s and `rcgen`'s own native dependencies already follow | `gungnir-data` (a normal dependency, not test-only) | Not yet: no gap builds it |

`proj` and `libproj` are both permissively licensed (`proj`: MIT OR Apache-2.0;
`libproj`: X/MIT, an OSGeo project), so this is a build-and-review cost, not a licensing
one. **Full projection support was chosen over a WGS84-geographic-only first step**: a
narrower cut would have left the common case of a projected DEM (UTM and similar) still
refused, and `gungnir-coord`'s existing `Wgs84` tangent-plane conversion does not extend
to a projected CRS regardless -- it solves a different problem (geographic lat/lon to a
local tangent plane), not general reprojection. **Not yet checked**: `proj-sys`'s build
behaviour on a CI runner with no system `libproj` present, what `validate_terrain` and
`validate_point_cloud` need to change to accept a declared CRS beyond `"local-enu"`, and
what a wrong or unsupported EPSG code should do (refuse by name, matching every other
malformed-input rule this stack already follows, is the expected answer but is GAP-023's
and GAP-098's own decision to confirm, not this one's).

#### Cloud KMS for the ManagedService custody profile (signed off 2026-09-08, D-42)

GAP-084 built the disconnected desktop's OS-keystore custody (D-39) but the cloud
deployment profile's equivalent, `ManagedService`, has never been designed: DN-22 §5
names the row and stops. The owner named AWS and Azure as the two targets to support on
2026-09-08; neither GCP KMS nor HashiCorp Vault is in scope, and a deployment needing
either is a future decision, not a gap in this one.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `aws-sdk-kms` | `ManagedService`'s AWS KMS backend | `gungnir-security` | Not yet: no gap builds it |
| `azure_security_keyvault_keys` (pre-1.0, `0.9.0`) | `ManagedService`'s Azure Key Vault backend | `gungnir-security` | Not yet: no gap builds it |
| `azure_identity` | Authentication for `azure_security_keyvault_keys` | `gungnir-security` | Not yet: no gap builds it |

Two things about this one:

1. **`azure_security_keyvault_keys`, never the older `azure_security_keyvault`.**
   The two names are easy to confuse: `azure_security_keyvault` is an earlier, unofficial
   crate of a similar name that Microsoft has said will not be updated further now that
   the official per-service crates (`_keys`, `_secrets`, `_certificates`) exist.
   `_keys` specifically is the one this workspace wants -- key custody, not secret or
   certificate storage.
2. **`0.9.0` is pre-1.0, and that is recorded rather than smoothed over.** A pre-1.0
   crate's API may still move under a later dependency bump; `aws-sdk-kms` (Apache-2.0,
   maintained by AWS directly) carries no equivalent caveat. **Not yet designed at all**:
   `ManagedService` itself has no DN-22 amendment describing its shape, so building
   against these two crates needs that design note first, the same discipline every other
   custody profile in DN-22 already had before its own code.

#### gRPC as the second transport (signed off 2026-09-05, D-21)

Rule 5 below and D-18 both left `tonic` a later question. The owner took it on 2026-09-05.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `tonic` | The gRPC surface `gungnir-api-v1.md` plans as a second transport for peer C2 systems that require it | `gungnir-api`, `gungnir-node` | Not yet: no gap builds it |
| `prost` | The protobuf codec `tonic` generates against | `gungnir-api` | Not yet: no gap builds it |

Three things about this one:

1. **It shares the HTTP stack rather than duplicating it.** Checked on 2026-09-05:
   `tonic` 0.14.6 and `prost` 0.14.4 resolve alongside `axum` 0.8.9 with exactly one
   `hyper` (1.11.1), one `tower` (0.5.3) and one `http` (1.5.0). A gRPC stack that pulled
   a second HTTP implementation into the SBOM would have failed the same test D-18 set.
2. **It is a second transport, never a replacement.** The v2 contract stays the primary
   surface, and gRPC exists for peers that cannot speak it. Two transports must not become
   two contracts: whatever is generated is generated from the same `gungnir-model` types.
3. **No gap owns it yet.** These crates are in the workspace manifest and **no member
   depends on them**, which is where `rustls` sat between D-18 and GAP-041. A gap has to
   be opened before a `.proto` schema or a `tonic-build` build-dependency is added; the
   build-time codegen crate is not part of this sign-off.

**Next additions, not yet signed off** (reviewed 2026-09-05):

- **An inference runtime**, for the ONNX models plan 09 specifies. GAP-077 owns that
  sign-off and says so; it is a native dependency and deliberately deferred.
- **A signature scheme for `KeyPurpose::BaselineSigning`.** D-22's third row, left
  open on 2026-09-05 because no gap needs it yet: nothing signs a configuration baseline.
  ECDSA P-256 (`p256`) is the recommendation to argue when one does, for the same
  FIPS-approved reason `aes-gcm` was chosen.
- The young 3D-data crates named in `rust-3d-data-ecosystem-build-vs-adopt.md` §1.2
  (`copc-rs`, `pasture-*`, `oxigdal-3d`), and `puffin` for profiling.

#### Identity (signed off 2026-09-04 as D-11, landed 2026-09-05 under GAP-069)

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `uuid` | UUID v7 for `GlobalEntityId`. `gungnir-model` takes it **without** the `v7` feature -- it owns the identity and therefore the textual form of one, and formatting needs no generator; `gungnir-identity` takes `v7`, because minting is its job | `gungnir-model`, `gungnir-identity` | 2026-09-05 |

Two things are deliberate:

1. **v7 rather than v4.** The leading 48 bits are a millisecond timestamp, so identities
   sort in the order they were minted -- which is the order an after-action review reads
   them in, and what keeps an index over them from degenerating. `GlobalEntityId` gained
   `Ord` for that reason and the ordering means something.
2. **The representation did not change.** `GlobalEntityId` was and remains a `u128`, so
   every journal that stored one still reads. An identity minted by the counter this
   replaced is not a UUID of any version and reads back rather than being rejected: a
   journal recorded before the change is still a journal.

#### Terrain (decided 2026-09-06, GAP-023, D-25)

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `tiff` | Decoding the raster of a GeoTIFF DEM. The image-rs TIFF codec, pure Rust, MIT. It names the GeoTIFF tag numbers and interprets none of them: `gungnir-data` reads `ModelPixelScale`, `ModelTiepoint`, `GeoKeyDirectory` and `GDAL_NODATA` itself, per OGC GeoTIFF 1.1, so the georeferencing is this project's reading of the standard and not a dependency's | `gungnir-data` | 2026-09-06 |

Two things are deliberate:

1. **`tiff` rather than a GDAL binding or `oxigdal-3d`.** A GDAL binding brings a C
   library and its build; `oxigdal-3d` was named by the 3D-data plan and never pinned. A
   TIFF decoder plus this project's own forty lines of tag reading is the smallest thing
   that reads what terrain actually arrives as, and it is testable against a fixture the
   repository generates itself (`testdata/dem/SOURCE.md`).
2. **The ASCII grid needs no crate** and was built in the same change, so the loader has
   a format whose every byte is legible in a diff.

#### YAML for the test-track library (decided 2026-09-06, GAP-016, D-31)

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `yaml_serde` | Reading the four plan-07 files (`scenarios.yaml`, `classes.yaml`, `sensors.yaml`, the catalogue and its includes) into typed data, so a TT set can be regenerated from Rust and a docs change that breaks the generator's inputs fails a test (`gungnir-scenario/src/library.rs`). The yaml organisation's continuation of `serde_yaml` (same API, MIT OR Apache-2.0, <https://github.com/yaml/yaml-serde>), which is what the deprecation notice on `serde_yaml` points at | `gungnir-scenario` (test/bench crate only) | 2026-09-06 |

Two things are deliberate:

1. **`yaml_serde` rather than `serde_yml` or `yaml-rust2`.** `serde_yaml`'s deprecation
   notice names its successor under the YAML organisation, and the API is unchanged, so
   the two candidates the register listed would have been a second parser or a second API
   for no gain. It vendors a Rust port of libyaml with `unsafe` inside; that is the same
   code `serde_yaml` shipped, and it is confined to a crate no production binary depends
   on.
2. **No production crate reads YAML.** The baseline is JSON (§2.9's first table) and stays
   JSON; YAML is the documentation's format for the test-track library, and the loader
   lives in the test-only crate that consumes it.

#### ADS-B differential oracles (decided 2026-09-06, GAP-010)

The first pair of dependencies in this table that exist **because no specification could
be bought or read**. No ADS-B document is both free to obtain and permissively licensed
(`design/external-standards.md` §4 lists what was tried and rejected, including
the freely circulating DO-260B draft, which carries RTCA copyright and is refused by
name), so the owner took GAP-010's open-source-consensus route: build the decoder and gate
it against two independently written MIT Rust decoders over two independently recorded
permissively licensed captures.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `rs1090` (`default-features = false`) | Differential oracle for `gungnir_interop::adsb` over `testdata/adsb/`. Default features off because its only default, `bds-infer`, adds plausibility assertions that would make the oracle refuse frames rather than decode them | `gungnir-interop` (`[dev-dependencies]` only) | 2026-09-06 |
| `adsb_deku` | The second oracle, from a different author with a different bit-layout transcription | `gungnir-interop` (`[dev-dependencies]` only) | 2026-09-06 |

Five things are deliberate:

1. **Test-only, and the rule is stronger than "not shipped".** Neither crate may become a
   normal dependency of any member. A binary that decoded through one of them would
   inherit that project's reading of a standard this repository has not pinned, and the
   whole point of the exercise is that this project owns its own reading and states what
   backs it.
2. **Exact `=` pins, unlike every other row in this section.** An oracle whose version
   floats is an oracle whose answers change without a change in this repository; GAP-010's
   closing action asks for the decoders "at recorded commits". Moving either pin is a
   change to `tests/adsb_fixtures.rs`'s recorded disagreements in the same commit.
3. **`adsb_deku` is 0.7.1 and not the current 0.8.0, because 0.8.0 does not build.** It
   requires `deku ^0.19`; the only 0.19 the registry still offers is 0.19.1, whose derive
   macro is stricter than the 0.19.0 it was written against, and sixteen of its enums fail
   with "`id_type` must be specified on non-unit variants". 0.19.0 itself is yanked. The
   fix is on the project's master branch and unreleased, so taking it would mean this
   workspace's **first git dependency**, which is an owner's decision and was not taken.
   0.7.1 is the newest published version that compiles, and it is the better oracle in one
   respect anyway: 0.8.0 reads the surface-position movement field as two bits where the
   56-bit message gives it seven.
4. **Duplicate linkage checked with `cargo tree -d`, as D-18 requires.** The pair brings
   two `deku`/`deku_derive` (0.16.0 under `adsb_deku`, 0.20.3 under `rs1090`), two
   `tokio-tungstenite` and `tungstenite` (0.28.0 under `rs1090` against the workspace's
   0.29.0), and second copies of the proc-macro-time set `darling`, `proc-macro-crate`,
   `strsim`, `toml_edit`, `toml_datetime` and `winnow` beneath `deku_derive` 0.16.
   `bytes` and `regex` gain a second **feature resolution** at one version rather than a
   second version. **`cargo tree -d -e normal,build` names none of them**, which is the
   check that matters: every copy is beneath a dev-dependency of one crate and nothing is
   linked into `gungnir-app` or `gungnir-node`. Recorded in full rather than summarised,
   because this is the largest duplicate set any addition in this section has brought and
   the reader is entitled to weigh it.
5. **A green gate here is not conformance**, and the verification-capability-table row's
   oracle column says "open-source consensus, not normative" so it cannot be read as one.
   The two oracles are only partly independent. The two parts of the build that are
   checkable by arithmetic — the parity polynomial and the CPR position algorithm — are
   gated against arithmetic and published worked examples instead, in
   `gungnir-interop/tests/adsb_crc.rs` and `adsb_cpr.rs`, and that is where a misreading
   shared by both oracles would be caught.

**Signed off and not yet in a manifest**, which needs no further approval, only the work:
`rustls`, `tokio-rustls`, `rustls-pemfile` (since replaced by `rustls::pki_types::pem`) and `tower-http` (D-18) enter under GAP-060.
The docking crate left this list on 2026-09-05: `egui_tiles` was signed off as D-19 and
has its own subsection above.

Each is added under §6 rule 4 and recorded in this table when it lands. Agents must not
add any crate not listed here.

#### Model checker (written 2026-09-08 under GAP-061, **not signed**)

Gate 4 has required `loom` since the workflow was written (`agentic-workflow.md`, and §5
below). What it did not have until 2026-09-08 was a dependency, which is why twenty-two
green runs model-checked nothing: `RUSTFLAGS=--cfg loom` set a cfg no line of source
read. This entry records the dependency; it is not a new decision about the tool.

| Crate | Used for | Used by | Landed |
|---|---|---|---|
| `loom` (feature `futures`) | Gate 4's model checks -- `gungnir-fusion-async/src/loom_model.rs`, over the channel shim in `src/sync.rs`. The `futures` feature supplies `loom::future::block_on`, which is what drives the real `async fn ingest_with` inside a loom execution rather than a transcription of it | `gungnir-fusion-async`, from `[target.'cfg(loom)'.dev-dependencies]` and nowhere else | 2026-09-08 |

Three things are deliberate:

1. **`[target.'cfg(loom)'.dev-dependencies]`, not a plain `[dev-dependencies]` entry.**
   Cargo evaluates that table's cfg against the rustflags in force, so an ordinary
   `cargo build`, `cargo test`, `cargo deny` or `cargo about` run never resolves,
   downloads or compiles `loom` at all: it reaches no SBOM and no release-target graph,
   and this row therefore adds nothing to what ships. Verified rather than assumed --
   `cargo tree -p gungnir-fusion-async -e dev` shows no `loom` line, and the same command
   under `RUSTFLAGS=--cfg loom` shows `loom v0.7.2`. It is the pattern `tokio` uses for
   its own loom support.
2. **The code is gated on `all(test, loom)`, not on `loom`.** A dev-dependency is linked
   only into test targets, so a plain `cargo build --lib` under the flag would meet
   `use loom::..` with no `loom` crate to resolve. `tokio` gates its shim the same way
   for the same reason. `cfg(loom)` is declared in `[workspace.lints.rust]`'s
   `check-cfg`, because it is a real cfg this workspace sets and not a typo.
3. **A channel of our own under the flag, rather than `loom::sync::mpsc`.** loom's mpsc
   never reports disconnection -- its `try_recv` returns a message or `Empty` and
   delegates otherwise to a blocking `recv` -- and `ingest_with` terminates on
   `Disconnected`. The shim therefore builds crossbeam's contract out of `loom::sync`
   primitives, and `sync.rs` states in full what that does and does not buy: the loop,
   the pipeline and the consumer protocol are real, the channel beneath them is a model,
   and nothing here is evidence about `crossbeam-channel`'s own implementation.

**Not signed.** `gungnir-fusion-async` is a human-owned, low-trust crate
(`agentic-workflow.md`), so this row and the model checks it serves are written and
gated and await the owner's review. §5's sign-off requirement is not satisfied by this
entry existing.

---

## 3. General Rust standards

### 3.1 Errors

- Every fallible public function returns `Result<T, E>` with a crate-local error enum
  derived with `thiserror` (§2.9). A capability that is not implemented yet returns an
  explicit variant (`NotImplemented`, `TransportNotImplemented`) rather than panicking on
  a runtime path; `todo!()` is reserved for functions nothing calls yet. Adding any
  further error-handling crate (`anyhow`, `eyre`) is a stack change that needs explicit
  sign-off, not an agent's unilateral choice.
- `unwrap()` / `expect()` are permitted only in: test code, `main()`/example binaries, and
  `debug_assert!`-style internal invariant checks explicitly gated to debug builds. Any other
  `unwrap()` in library code is a defect, not a style nit — flag it in review.
- Errors carry enough context to reconstruct which oracle-comparison row failed and why
  (e.g., `FilterError::CovarianceNotPsd { cycle: usize, min_eigenvalue: f64 }`), since these
  messages end up in CI logs that a human is debugging against the capability table.

### 3.2 Naming

- Types match the capability table's terminology exactly (`ExtendedKalmanFilter`, not `Ekf`
  or `EKFilter`) so that grep-ing the table's language against the codebase works.
- Generic parameters are named for what they represent, not just `T`/`U` — `Filter<S: State>`
  reads better than `Filter<T>` in a numerically dense codebase where the reader is already
  tracking several type parameters.

### 3.3 Module structure within a crate

- `lib.rs` re-exports the public trait(s) and top-level types; implementation detail lives in
  submodules named after the capability, not generic names like `impl.rs` or `utils.rs`.
- Every submodule that corresponds to a capability-table row opens with a doc comment linking
  back to the row: `//! Verifies against verification-capability-table.md: "Extended Kalman
  Filter (EKF)". Relative error < 1e-4 vs. filterpy/trackingEKF.`

### 3.4 Documentation

- Every public item has a doc comment. For anything appearing in the capability table, the
  doc comment states the pass criterion inline, so the two documents never drift silently out
  of sync.
- Doc examples (`/// ```` blocks) are required on every public trait and are run as doctests —
  they double as cheap regression coverage for the API surface.

### 3.5 Lints

- The lint policy is centralized in the workspace `Cargo.toml` (`[workspace.lints]`), and
  every crate opts in with `[lints] workspace = true`: `unsafe_op_in_unsafe_fn` denied,
  `clippy::all` denied, `clippy::pedantic` warned, with a short commented allow-list for
  pedantic lints that don't fit numerical code (`clippy::many_single_char_names`, since
  `f`, `q`, `h`, `r` are the standard filter notation). A crate-level allow beyond that
  list needs a one-line justification, not a blanket suppression.
- `cargo fmt` is non-negotiable and runs in `ci.yml` on every PR
  (`gungnir-workspace-structure.md`). Agents should not hand-format code differently
  from what `rustfmt` would produce.

---

## 4. `unsafe` policy

Per the existing tiered trust model: agents may **propose** `unsafe` code (e.g., for a
performance-critical SIMD path in a hot filter loop) but every such block:

- Is isolated to the smallest possible scope, wrapped in a safe function with documented
  preconditions (`# Safety` doc section — mandatory, not optional).
- Triggers `miri.yml` automatically and requires human sign-off before merge, per the
  workspace's CI gate mapping — an agent should never merge its own `unsafe` code, and
  should say so explicitly in the PR description rather than assuming the reviewer will
  notice.
- Is justified with a benchmark showing the safe alternative was measured and found
  insufficient — `unsafe` for unmeasured, assumed performance gain is not acceptable.

## 5. Concurrency policy

Same posture as `unsafe`: agents may write `fusion-async` code, including code using
`tokio::sync` primitives, but:

- Any PR touching `gungnir-fusion-async` or `gungnir-tracking-service` triggers
  `loom.yml` automatically.
- The PR description must state, in plain language, what interleaving the change could
  introduce or affect — this is for the human reviewer's benefit and should not be skipped
  even when the change looks small.
- Human sign-off is mandatory before merge regardless of green CI, per the existing gate.
- **New cross-task state goes through `gungnir_fusion_async::sync` or Gate 4 cannot see
  it.** That module is the shim the gate model-checks through: `crossbeam-channel` in
  every ordinary build, a loom-instrumented channel under `--cfg loom`. A channel
  constructed straight from `crossbeam_channel` in that crate is invisible to loom, and
  invisible is exactly how this gate spent 22 runs certifying nothing (GAP-061). The
  model checks themselves are `gungnir-fusion-async/src/loom_model.rs`, and its module
  documentation states what they reach and what they do not.

---

## 6. Agent operating rules

1. **Read the capability table row before writing the code**, not after. The pass criterion
   should shape the implementation (e.g., knowing IMM's tolerance is on mode probabilities as
   well as state should influence how mode probability normalization is implemented, not be
   discovered afterward when a test fails).
2. **Write the oracle-comparison test alongside the implementation**, in the same PR, not as
   follow-up work. A capability without a passing differential test against its named oracle
   is not done, regardless of how complete the implementation looks.
3. **Never widen a pass criterion to make a test pass.** If relative error is coming in at
   1e-3 against an EKF's 1e-4 requirement, the fix is in the implementation (or an escalation
   explaining why the criterion itself needs revisiting), not a quiet tolerance edit.
4. **Flag architectural questions instead of guessing.** If a task seems to require crossing
   a dependency-graph boundary (§1.1) or adding a new stack dependency beyond the approved
   set (§2.1–§2.9), stop and ask rather than proceeding on an assumption.
5. **Self-review against this document before requesting human review** — specifically the
   `unwrap()` policy (§3.1), the `nalgebra` fixed-vs-dynamic rule (§2.1), and the `tokio`
   blocking-work rule (§2.2), since these are the three most common places agent-written
   numerical/async code tends to drift from the standard.

---

## 7. Where this document and the UI standards differ

`rust-ui-architecture-coding-standards.md` was written for the UI crates and this document
for the tracking core; the service and productization crates sit between them. The rules
below settle the differences.

| Topic | This document | UI standards | Rule |
|---|---|---|---|
| Error types | Crate-local enum (§3.1) | Project-level `AppError` with `thiserror` (§4) | Crate-local enums everywhere. `AppError` exists only in `gungnir-app` and wraps the crate enums it receives. |
| `unwrap()`/`expect()` | Tests, `main()`, debug-only invariant checks (§3.1) | `main.rs` bootstrap and tests (§4) | Same rule; the debug-only invariant case applies to every crate. |
| Shared mutable state across threads | Channels; `Arc<Mutex>` is a review trigger (§2.2) | `Arc<Mutex>` or channels for cross-thread state (§7) | Channels by default in every crate. `Arc<Mutex>` is permitted in the UI, data, and app crates for cross-frame async results, and is a mandatory review trigger in the tracking core and `gungnir-fusion-async`. |
| Lints | `clippy::all` denied, `clippy::pedantic` warned, documented allows (§3.5) | `clippy::all` with `-D warnings` (§6) | One workspace policy for every crate: `clippy::all` denied, `clippy::pedantic` warned (`Cargo.toml` `[workspace.lints]`). |
| Logging | `tracing` with capability-table level semantics (§2.8) | `tracing` with generic levels (§4) | `tracing` everywhere. The §2.8 level semantics apply to the tracking core; UI and data crates use ordinary severity semantics. |
| Profiling | Not addressed | `puffin` suggested (§5) | `puffin` is not in the approved stack (§2.9); use `tracing` spans until it is signed off under §6 rule 4. |
| Which crates | Tracking core, foundation, service layer, productization layer, deployment crates | `gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`, `gungnir-app`, and the GPU and render-thread rules for `gungnir-data`, `gungnir-data-fusion` | As listed; `docs/README.md` carries the same mapping. |

---

## Cross-references

- `ARCHITECTURE.md` (workspace root) — the full dependency graph §1.1 summarizes, the
  deployment topology that decides which binary owns the `tokio` runtime (§2.2), and the
  known scaffold defects this document refers to by item number.
- `verification-capability-table.md` — the pass criteria this document's conventions exist to
  make easy to satisfy and easy to verify.
- `scenario-crate-narrative.md` — why the five scenarios exist; §2.5/§2.4 here explain how
  test code should consume that generator's output deterministically.
- `gungnir-workspace-structure.md` — the repository layout and the CI gate table §4/§5 here
  point back to.
- `gungnir-capabilities.md` — the "why it matters" layer that motivates why some of these
  standards (unit documentation, PSD assertions, deterministic RNG) exist as hard requirements
  rather than style preferences.
- `rust-ui-architecture-coding-standards.md` — the standards for the UI, rendering, and
  3D-data crates; §7 above reconciles the two.
- `agentic-workflow.md` — the trust tiers and review pipeline these standards are applied
  through.
