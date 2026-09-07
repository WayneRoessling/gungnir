# Gungnir Workspace

Gungnir is a command-and-control (C2) desktop application and services layer written in
Rust. It fuses a verified tracking, estimation, and intercept-planning engine with an
`eframe`/`egui`/`three-d` operator interface and a 3D-data ecosystem, and wraps both in
the productization layer (data model, ingest, policy, security, persistence, API,
resilience, collaboration) that a fieldable system needs. Gungnir is one node in a
larger system of systems: the same crates run as a standalone disconnected desktop, as a
desktop connected to an on-prem service node, or as a desktop connected to a cloud
service node.

Every crate shares the `gungnir-` prefix. The `fusion-` prefix that early drafts used for
the UI and productization crates is retired and appears in this repository only in
historical notes.

**Start here:**

- [`ARCHITECTURE.md`](./ARCHITECTURE.md) is the technical reference: layers, dependency
  graph, the service boundary, GPU contexts, deployment topology (§8), the tested version
  set (§9), and what was fixed and what is still open (§10).
- [`docs/README.md`](./docs/README.md) indexes the rest of the documentation, says which
  standards document governs which crate group, and carries the glossary.
- [`docs/gungnir-capabilities.md`](./docs/gungnir-capabilities.md) describes every crate
  in business terms with its implementation status.
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) is the short form of the rules for any change.

## Layout at a glance

```
gungnir-core, gungnir-coord, gungnir-filters, gungnir-association,     Tracking core: pure
gungnir-track, gungnir-rfs, gungnir-track-fusion, gungnir-fusion-async, Rust, no UI or GPU
gungnir-allocation, gungnir-metrics, gungnir-scenario                   types, verified
gungnir-oracle, gungnir-testkit, gungnir-fuzz                           against oracles

gungnir-model                                                          Canonical views and
                                                                        events (foundation)

gungnir-tracking-service, gungnir-intercept-service                    Service facades the
                                                                        app and the node
                                                                        depend on

gungnir-eventing, gungnir-store, gungnir-config, gungnir-mission       Productization,
                                                                        foundational: event
                                                                        bus, journal, config,
                                                                        sessions

gungnir-ingest, gungnir-sensor-management, gungnir-time,               Productization, sense
gungnir-interop                                                        and ingest, interop
                                                                        schemas and codecs

gungnir-identity, gungnir-identification, gungnir-geo,                 Productization,
gungnir-analytics                                                      understand: global ID,
                                                                        classification, maps,
                                                                        line-of-sight

gungnir-policy, gungnir-command, gungnir-assessment,                   Productization, assess
gungnir-decision, gungnir-modelops                                     and decide: approval
                                                                        gate, threat scoring,
                                                                        courses of action

gungnir-security, gungnir-api, gungnir-observability,                  Productization, secure
gungnir-resilience, gungnir-collab, gungnir-workflow                   and operate: authN/Z,
                                                                        API contract, health,
                                                                        offline operation,
                                                                        multi-user, workflow

gungnir-replay, gungnir-reporting                                      Productization,
                                                                        validate: playback,
                                                                        after-action reports

gungnir-data, gungnir-data-fusion                                      3D data: file I/O, and
                                                                        wgpu compute for
                                                                        point-cloud fusion

gungnir-remote, gungnir-node                                           Remote backends and
                                                                        the headless service
                                                                        node (Linux container)

gungnir-render, gungnir-viewport3d, gungnir-ui, gungnir-app             UI and rendering, and
                                                                        the eframe desktop
                                                                        binary
```

The workspace has 50 crates: 49 members plus `gungnir-fuzz`, which is excluded from the
default build because `cargo-fuzz` needs its own toolchain. The scope is deliberately
open; a later pass locks which crates ship first (`ARCHITECTURE.md` §10).

## Status

Every crate compiles (`cargo check --workspace --all-targets`), every crate's public
surface is doc-commented with the capability and, for the tracking core, the pass
criterion it implements, and the crates that have logic have unit tests. What is real
and what is a scaffold:

- **Real:** the canonical data model and event schema; the broadcast event bus; the
  JSON-lines journal, replay, and reporting; config loading and validation with backend
  selection; the ingest gateway with validation, quarantine, recorded and simulated
  adapters; the geofence policy, approval workflow, risk scoring, decision rationale,
  identity and classification engines, model registry, sensor registry, role-based
  authorization and audit log; geofences, line-of-sight and coverage analytics;
  store-and-forward and reconciliation; authority arbitration; role workspaces and the
  alert lifecycle; the schema catalog and Arrow interop; the egui panels and the
  viewport's 2D fallback; both binaries' startup and tick loops.
- **Scaffold:** the tracking math (filters, association, lifecycle, RFS, fusion), the
  allocator, the scenario generator, ICP, the three-d scene, the API transport, live
  protocol adapters, and the ASTERIX/STANAG codecs. Each says so through a
  `NotImplemented` error, a `todo!()` off every runtime path, or a health flag; nothing
  pretends to work.

`ARCHITECTURE.md` §10 lists what the 2026-09-04 pass fixed and what remains open.

## Running

The desktop (Windows 11, eframe with the `glow` renderer so three-d has a GL context):

```bash
cargo run -p gungnir-app
```

The headless service node (Linux x86_64 container; runs on any host for development):

```bash
cargo run -p gungnir-node -- deploy/node/config.example.json
```

Both start with a default configuration when none is given. Point the desktop at a
config baseline with the `GUNGNIR_CONFIG` environment variable; `deploy/README.md`
covers the container build and pointing a desktop at a node.

Checks:

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets
```

## Deployment profiles

Three profiles are supported by design, all built from the same crates. See
`ARCHITECTURE.md` §8 for the full description and `deploy/README.md` for the mechanics.

| Profile | Where the services layer runs | Who hosts durable mission state |
|---|---|---|
| Disconnected desktop | Embedded inside `gungnir-app` | The desktop's local `gungnir-store` journal |
| On-prem connected | A `gungnir-node` container on the local network, behind `gungnir-api` | The node |
| Cloud connected | The same `gungnir-node` image hosted in the cloud | The node |

The desktop selects the backend from its config baseline and switches through the
`TrackingService` and `InterceptService` traits, so no UI code knows which profile is
active. The API contract is decided (`docs/gungnir-api-v1.md`); its transport is the next
addition, until which a configured remote endpoint falls back to embedded with an alert.

## Stack

The fixed dependency stack is defined in `docs/agentic-coding-standards.md` §2 and §2.9
(tracking, services, productization) and `docs/rust-ui-tech-stack-summary.md` (UI and
rendering). The single resolved version set lives in the workspace `Cargo.toml`, the
toolchain in `rust-toolchain.toml`, and both are reproduced with their caveats in
`ARCHITECTURE.md` §9. Two facts about the rendering stack matter for anyone touching the
UI crates: the 3D viewport draws through three-d's OpenGL context inside eframe's `glow`
backend, and `wgpu` 22 is used only for compute in `gungnir-data-fusion`.

## Suggested implementation order

This is a recommendation, not an enforced build order. Steps 1 and 2 are the
dependency-driven minimum; the rest follow the priorities in
`docs/gungnir-capabilities.md` §7 and §8.

1. `gungnir-core`, `gungnir-coord`, `gungnir-filters` (closed-form checks and the linear
   Kalman filter first, per the verification table's "validated first" note), with the
   oracle fixtures `gungnir-oracle` compares against.
2. `gungnir-scenario` Scenario 1 and the `gungnir-fusion-async` pipeline for one sensor
   and one target, flipping `PIPELINE_IMPLEMENTED` when it passes; `LiveTrackingService`
   then reports healthy and the desktop shows real tracks.
3. `gungnir-allocation`'s Bellman/DP solver against the textbook oracle, then intercept
   geometry so `InterceptSolutionView` carries a point and time.
4. Wire the approval gate (`gungnir-policy`, `gungnir-command`) and `gungnir-assessment`
   rewards into the desktop and node ticks; then identity, classification, security,
   and the role-based layouts from `gungnir-workflow`.
5. The `gungnir-api` transport, which makes `gungnir-remote` and `gungnir-node`
   connectable and unlocks the connected profiles, then mid-session failover and the
   reconciliation harness.
6. Attach the three-d scene to eframe's GL context; `gungnir-viewport3d::scientific`
   (the VTK bridge, smallest scope) first, then live glyphs in 3D.
7. `gungnir-render::GpuContext` on a GPU host, `gungnir-data-fusion::cpu_reference`, then
   the GPU ICP pipeline.
8. Live protocol adapters and the ASTERIX/STANAG codecs; the remaining tracking-core
   rows (UKF, IMM, JPDA, MHT, RFS, track fusion) in verification-table order.
9. `gungnir-viewport3d::streaming` (3D Tiles) last, and only once a panel needs
   site-scale streamed terrain.

## Licensing

Gungnir is free software under the **GNU Affero General Public License, version 3 or
later** ([`LICENSE`](./LICENSE)), with additional terms under AGPL section 7 covering
attribution, origin marking, and trademarks
([`LICENSE-ADDITIONAL-TERMS.md`](./LICENSE-ADDITIONAL-TERMS.md)).

Copyright (C) 2026 Roessling Digital Solutions LLC.

You may run, study, modify, redistribute, and self-host Gungnir at no charge, including
commercially. Two obligations are worth stating plainly because of how this system is
deployed:

- **Section 13, the network clause.** If you modify Gungnir and let users interact with
  your version over a network — which is what the on-prem and cloud service-node
  profiles in [`ARCHITECTURE.md`](./ARCHITECTURE.md) §8 are — you must offer those users
  the corresponding source of your modified version. Running an unmodified copy carries
  no such obligation.
- **Attribution survives.** The notice in [`NOTICE`](./NOTICE) and the copyright header
  on each source file must be preserved, and where a derivative work has an interactive
  user interface, the attribution must appear in its Appropriate Legal Notices.

**Commercial licensing.** Where AGPL terms are incompatible with a program's
requirements, Roessling Digital Solutions LLC offers Gungnir under a separate commercial
license on negotiated terms: wayne.roessling@roesslingdigital.com.

**Contributions** require a sign-off and a relicensing grant ([`CLA.md`](./CLA.md));
[`CONTRIBUTING.md`](./CONTRIBUTING.md) has the mechanics.

**Not everything here is AGPL.** Test fixtures under `testdata/` are third-party
material redistributed under their own licenses and are never linked into or shipped
with a binary — notably `testdata/asterix/`, which is **GPL-2.0**
(`testdata/asterix/COPYING`). [`NOTICE`](./NOTICE) lists every exception with its
license and text. Rust dependencies are permissively licensed and gated against the
allow-list in [`deny.toml`](./deny.toml).

## Documentation

| Document | Purpose |
|---|---|
| `LICENSE` | GNU Affero General Public License v3 |
| `LICENSE-ADDITIONAL-TERMS.md` | Attribution, origin, and trademark terms under AGPL §7 |
| `NOTICE` | Attribution text, and every third-party license in the repository |
| `CLA.md` | Contributor License Agreement and sign-off requirement |
| `ARCHITECTURE.md` | Technical reference for the whole workspace |
| `CONTRIBUTING.md` | The rules that apply to any change |
| `CLAUDE.md` | Working notes for coding agents |
| `docs/README.md` | Index, reading order, standards-to-crate mapping, glossary |
| `docs/gungnir-capabilities.md` | Business-analyst capability reference, all crates |
| `docs/verification-capability-table.md` | Pass/fail matrix |
| `docs/scenario-crate-narrative.md` | The five generator scenarios |
| `docs/architecture.md` | Crate to verification-row map |
| `docs/gungnir-workspace-structure.md` | Repository layout and CI-gate mapping |
| `docs/agentic-coding-standards.md` | Standards for the tracking, service, and productization crates |
| `docs/rust-ui-architecture-coding-standards.md` | Standards for the UI, rendering, and 3D-data crates |
| `docs/agentic-workflow.md` | Agent trust tiers, gates, review pipeline |
| `docs/rust-ui-tech-stack-summary.md` | UI stack decision record |
| `docs/rust-3d-data-ecosystem-build-vs-adopt.md` | 3D data build-versus-adopt plan |
| `docs/gungnir-api-v1.md` | API interface control document |
| `docs/performance-budgets.md` | End-to-end SLO drafts |
| `docs/release-governance.md` | Assurance and release policy |
| `deploy/README.md` | Building and running the two binaries |
| `benches/README.md` | Benchmark groups and rules |
