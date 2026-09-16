# Gungnir Agentic Coding & Architecture Standards

Companion reference to the workspace `ARCHITECTURE.md`, `gungnir-workspace-structure.md`,
`verification-capability-table.md`, `scenario-crate-narrative.md`, and
`gungnir-capabilities.md`. This document is written **for agents** (and their human
reviewers) doing implementation work in the Gungnir workspace. It fixes the stack —
`nalgebra`, `tokio`, `serde`, `rand`, `proptest`, `criterion`, `arrow-rs`, and `tracing`,
with the approved additions in §2.9 — and
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
  `crossbeam-channel` in this layer (§2.9). Anywhere an agent finds
  itself reaching for `Arc<Mutex<...>>` to pass state between async tasks, that's the
  trigger to stop and get human review — this is exactly the code path `loom` and
  mandatory human sign-off exist for.
- **Every `async fn` in `fusion-async`'s public API takes a cancellation-safety note in its
  doc comment** (does it hold partial state across an `.await` point that would be corrupted
  by a dropped future?). This is cheap to write and expensive to reconstruct later, and
  `gungnir-app/tests/architecture_compliance.rs` fails an `async fn` that has none.

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

`tracing` is the logging and diagnostics crate everywhere, with `tracing-subscriber` in
the two binaries and in test-scoped subscribers (§2.9). `println!`, `eprintln!` and the
`log` crate are not substitutes for it. A binary's own command-line output, such as a
usage message, is output and not logging.

**Why `tracing` specifically, not `log`:** `fusion-async` is the one crate with genuine
concurrent, multi-stage execution (OOS buffer, multi-rate scheduler, multiple sensor feeds
in flight at once). `tracing`'s span model is what makes a log line traceable back to *which*
in-flight fusion task or *which* filter cycle produced it — a flat `log`-style line doesn't
carry that context across an `.await` point. It composes with `tokio` directly
(`tokio-console` and `tracing-subscriber` both consume its output), so it doesn't introduce
a second, uncoordinated diagnostics story alongside the async runtime.

**Levels in the tracking core carry the capability table's meaning.** The UI, data and
productization crates use ordinary severity (§7). In the core, and in
`gungnir-fusion-async` above all, where most of the core's logging is:

- `error!` — a definition-of-done criterion violated at run time, never "something looked
  off": a covariance that stopped being PSD, a NaN or infinity in a state, an infeasible
  assignment, a track initiated with no filter behind it, a filter refusing its own
  settings.
- `warn!` — degraded but recoverable, and expected as an edge case: a detection the
  reorder buffer refused, a scan whose association failed, a consumer that has gone.
- `info!` — lifecycle a person tailing the log should be able to read as a narrative: a
  pipeline starting and stopping, a mode engaging.
- `debug!` — per-cycle detail for reproducing one failure: a bearing that refined a track,
  or matched nothing and was retained.
- `trace!` — full numeric dumps of states, covariances and cost matrices, for a local
  debugging session.

**Structured fields, not context folded into the message.** Fields are named the way the
error enums in §3.1 name them (`cycle`, `track_id`, `min_eigenvalue`, `sensor_id`), so a
log line, an error variant and a shrunk proptest input can be cross-referenced by the same
names.

**No logging in `core`, `coord`, or `allocation`.** These are the exact-match,
closed-form-oracle capabilities: deterministic math with a single correct answer doesn't
benefit from runtime diagnostics, and a span there is noise nobody reads.

**What this section does not cover:** metrics and observability for a production
deployment (dashboards, alerting thresholds). The library crates emit spans and events;
what a binary does with them is the binary's decision. Within this workspace that consumer
is `gungnir-observability`, which turns `tracing` output into operator-facing health
(`gungnir-capabilities.md` §5.5).

### 2.9 Approved stack

The crates in §2.1 to §2.8 and in the table below are the whole approved stack for the
crates these standards govern. The UI stack (`eframe`, `egui`, `three-d`, `wgpu`, `gltf`,
`vtkio`, `las`) is governed by `rust-ui-tech-stack-summary.md` and pinned in the same
workspace `Cargo.toml`. **Agents must not add a crate that is not listed.** A proposed
addition goes to the owner under §6 rule 4, and its row lands in the same change as the
manifest line; `gungnir-app/tests/architecture_compliance.rs` fails a
`[workspace.dependencies]` entry these documents do not name.

A row holds only what stays true while the crate is in the stack: what it is for, the
terms it was admitted on, and the decision that admitted it. The argument for each,
including the duplicate-linkage check it passed at the time, is in the linked section of
[`record/2026-09-16/stack-sign-offs.md`](record/2026-09-16/stack-sign-offs.md), moved
there unchanged, or in the pull request named. Which members depend on a crate is for the
manifests to say (`cargo tree -i <crate>`), not this table.

**A pull request that adds a crate here, or changes a crate's version or features in
`[workspace.dependencies]`, states its duplicate-linkage check.** Its description carries
a line beginning `Duplicate linkage:` with the `cargo tree -d` result on the release
targets, and a line beginning `Decision:` naming the decision or sign-off that admits the
change. `.github/workflows/pr-rules.yml` fails such a pull request when either line is
missing.

| Crate | For | Terms of admission | Decision | Argued in |
|---|---|---|---|---|
| `thiserror` | `#[derive(Error)]` on every crate-local error enum | The only error-handling crate: `anyhow` or `eyre` would be a new row (§3.1) | Sign-off of 2026-09-04 | [Approved stack additions](record/2026-09-16/stack-sign-offs.md#approved-stack-additions-signed-off-2026-09-04) |
| `crossbeam-channel` | Synchronous channels at the render and UI boundary, the async-pipeline boundary and the event bus | Inside `gungnir-fusion-async`, a channel goes through its `sync` shim or Gate 4 cannot see it (§5) | Sign-off of 2026-09-04 | [Approved stack additions](record/2026-09-16/stack-sign-offs.md#approved-stack-additions-signed-off-2026-09-04) |
| `serde_json` | JSON configuration baselines, the JSON-lines journal, recorded feeds and report export | | Sign-off of 2026-09-04 | [Approved stack additions](record/2026-09-16/stack-sign-offs.md#approved-stack-additions-signed-off-2026-09-04) |
| `rand_distr` | Named distributions for clutter and detection sampling (§2.4) | | Sign-off of 2026-09-04 | [Approved stack additions](record/2026-09-16/stack-sign-offs.md#approved-stack-additions-signed-off-2026-09-04) |
| `tracing-subscriber` | Subscriber setup in the two binaries and in test-scoped subscribers (§2.8) | | Sign-off of 2026-09-04 | [Approved stack additions](record/2026-09-16/stack-sign-offs.md#approved-stack-additions-signed-off-2026-09-04) |
| `loom` (`futures`) | Gate 4's model checks of `gungnir-fusion-async` | Only from `[target.'cfg(loom)'.dev-dependencies]`, so no ordinary build, `cargo deny` run or SBOM resolves it | GAP-061; the owner's signature is in `signatures.md` | [Model checker](record/2026-09-16/stack-sign-offs.md#model-checker-written-2026-09-08-under-gap-061-signed-by-the-owner-2026-09-15), [PR #61](https://github.com/WayneRoessling/gungnir/pull/61) |
| `yaml_serde` | Reading the plan-07 test-track library into typed data | Test and bench crates only: no production crate reads YAML | D-31 | [YAML for the test-track library](record/2026-09-16/stack-sign-offs.md#yaml-for-the-test-track-library-decided-2026-09-06-gap-016-d-31) |
| `uuid` | UUID v7 for `GlobalEntityId` | The `v7` feature only where identities are minted | D-11 | [Identity](record/2026-09-16/stack-sign-offs.md#identity-signed-off-2026-09-04-as-d-11-landed-2026-09-05-under-gap-069) |
| `tiff` | Decoding the raster of a GeoTIFF DEM | The GeoTIFF georeferencing tags are read by `gungnir-data` itself, not by a dependency | D-25 | [Terrain](record/2026-09-16/stack-sign-offs.md#terrain-decided-2026-09-06-gap-023-d-25) |
| `egui_tiles` | The dock tree behind `gungnir_workflow::WorkspaceLayout::for_role` | Pinned to the line built on the workspace's `egui`, and moves with that pin; a layout is persisted as `gungnir_model::LayoutNode`, never as the library's own tree | D-19 | [Docking crate](record/2026-09-16/stack-sign-offs.md#docking-crate-signed-off-2026-09-05-d-19) |
| `axum` (`ws`) | The node's HTTP surface and its WebSocket event stream | The `ws` feature is the server WebSocket, so no second server crate | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `tokio-tungstenite` | The desktop's WebSocket client | The version `axum` resolves to; no TLS feature of its own, since it is handed a finished `tokio-rustls` stream | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `reqwest` (`json`, `rustls`) | The desktop's HTTP client | No default features; configured with the same `rustls::ClientConfig` as the event stream, so no private key is ever held as text | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `futures-util` | The `Sink` and `Stream` traits a `tokio-tungstenite` socket implements | No default features | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `rustls` | TLS with client-certificate verification, and the PEM reader | Not `native-tls`: one TLS implementation in the SBOM; reads keys and never holds or logs key material | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `tokio-rustls` | `rustls` over `tokio` streams, on the node's acceptor and the desktop's connector | | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `tower-http` (`trace`, `limit`) | Request tracing and body-size limits on the node's HTTP surface | The version `reqwest` resolves to; `hyper` and `tower` stay transitive, and naming either type directly is a new row | D-18 | [API transport stack](record/2026-09-16/stack-sign-offs.md#api-transport-stack-signed-off-2026-09-05-d-18) |
| `tonic`, `prost` | A second transport, gRPC, for peers that cannot speak the v2 contract | Never a second contract: generated from the same `gungnir-model` types. No member depends on either until a gap owns a `.proto` schema, and `tonic-build` is not covered | D-21 | [gRPC as the second transport](record/2026-09-16/stack-sign-offs.md#grpc-as-the-second-transport-signed-off-2026-09-05-d-21) |
| `argon2`, `password-hash` | Verifying an operator's passphrase against a local account store | RustCrypto, so the workspace keeps one cryptographic audit surface | D-20 | [Cryptography](record/2026-09-16/stack-sign-offs.md#cryptography-signed-off-2026-09-05-d-20) |
| `hmac`, `sha2` | Session-token integrity; `sha2` alone for seed and dataset hashes | A symmetric MAC for session tokens, not a public-key signature, because a node issues and verifies its own | D-20 | [Cryptography](record/2026-09-16/stack-sign-offs.md#cryptography-signed-off-2026-09-05-d-20) |
| `subtle` | Constant-time comparison, named so it is obviously constant-time | | D-20 | [Cryptography](record/2026-09-16/stack-sign-offs.md#cryptography-signed-off-2026-09-05-d-20) |
| `aes-gcm` | AES-256-GCM behind `KeyProvider::seal` and `unseal` | A nonce is never reused under one key | D-22 | [Data protection](record/2026-09-16/stack-sign-offs.md#data-protection-signed-off-2026-09-05-d-22) |
| `rcgen` (`aws_lc_rs`, `pem`) | Issuing a host's TLS identity through its `KeyProvider`, and certificates in tests | No default features. **No code path may build a certificate over private key material that has left a `KeyProvider`**, which `architecture_compliance.rs` checks | D-22, D-29 | [Data protection](record/2026-09-16/stack-sign-offs.md#data-protection-signed-off-2026-09-05-d-22) |
| `p256` (`ecdsa`, `ecdh`, `pem`) | ECDSA P-256 for baseline signing and the transport identity; ECDH for escrow | | D-22 | [Data protection](record/2026-09-16/stack-sign-offs.md#data-protection-signed-off-2026-09-05-d-22) |
| `keyring` (`v1`) | Persistent key custody in the operating system's keystore | No default features and never `cli`; production forces the native backend once, then addresses `keyring_core::Entry` so a test can install the mock store | D-39 | [OS keystore](record/2026-09-16/stack-sign-offs.md#os-keystore-signed-off-2026-09-08-d-39), [PR #24](https://github.com/WayneRoessling/gungnir/pull/24) |
| `keyring-core` | The route to the store `keyring` selected, and the mock store tests use | Named directly for that reason alone | D-39 | [OS keystore](record/2026-09-16/stack-sign-offs.md#os-keystore-signed-off-2026-09-08-d-39) |
| `ort` (`load-dynamic`, `api-27`) | Running an ONNX model behind `gungnir-ml`'s `Model` trait | `download-binaries` refused; only behind `gungnir-ml`'s default-off `onnx-runtime` feature, because `ort` panics when no compatible ONNX Runtime loads | D-40 | [ONNX inference runtime](record/2026-09-16/stack-sign-offs.md#onnx-inference-runtime-signed-off-2026-09-08-d-40), [PR #59](https://github.com/WayneRoessling/gungnir/pull/59) |
| `proj` | Converting a DEM's or a point cloud's coordinate reference system | No default features, so `network` and `tiff` stay refused; only behind `gungnir-data`'s default-off `crs` feature, because `proj-sys` cannot build on Windows MSVC | D-41, D-51 | [Coordinate reference system projection](record/2026-09-16/stack-sign-offs.md#coordinate-reference-system-projection-signed-off-2026-09-08-d-41), [PR #59](https://github.com/WayneRoessling/gungnir/pull/59) |
| `aws-sdk-kms`, `aws-config` | The `ManagedService` custody profile on AWS KMS, with the default credential chain | No default features; `sso` and `credentials-process` stay off | D-42 | [Cloud KMS](record/2026-09-16/stack-sign-offs.md#cloud-kms-for-the-managedservice-custody-profile-signed-off-2026-09-08-d-42), [PR #59](https://github.com/WayneRoessling/gungnir/pull/59) |
| `azure_security_keyvault_keys`, `azure_identity`, `azure_core` | The `ManagedService` custody profile on Azure Key Vault | Never the older `azure_security_keyvault`; the managed identity credential, never a developer's | D-42 | [Cloud KMS](record/2026-09-16/stack-sign-offs.md#cloud-kms-for-the-managedservice-custody-profile-signed-off-2026-09-08-d-42) |
| `rs1090`, `adsb_deku` | Differential oracles for the ADS-B decoder | Dev-dependencies of `gungnir-interop` only, never a normal dependency; exact `=` pins | GAP-010 | [ADS-B differential oracles](record/2026-09-16/stack-sign-offs.md#ads-b-differential-oracles-decided-2026-09-06-gap-010) |

**Not approved, and each would need a row first:** `puffin`, `copc-rs`, the `pasture-*`
crates, `oxigdal-3d`, `anyhow` or `eyre` (§3.1), `native-tls`, direct use of `hyper` or
`tower`, `tonic-build`, and any git dependency.

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
- Every submodule that corresponds to a capability-table row opens with a doc comment naming
  the row: `//! Verifies against verification-capability-table.md: "Extended Kalman Filter
  (EKF)".` The criterion itself is not restated there: it lives in the table alone, and a
  copy in a comment is a copy that drifts when the table changes.

### 3.4 Documentation

- Every public item has a doc comment. For anything appearing in the capability table, the
  doc comment names its row (§3.3); the criterion is read from the table. Comments written
  before 2026-09-16 still restate criteria inline, and where one disagrees with the table,
  the table is right.
- A doc comment does not say whether its code is signed, or that it waits for the owner.
  That is `signatures.md`'s to say, and `docs/tools/signatures.py check` refuses the second.

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
