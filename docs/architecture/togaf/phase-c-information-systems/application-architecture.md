# Application architecture

Status: first draft, 2026-09-04. Phase C, applications. **The content lives in the UAF
services and resources views; this document says what phase C concluded about the
application structure and where the technical gaps sit by layer.** Engineering reviewer
sign-off outstanding.

## 1. The structure

Fifty crates, of which two are binaries, in seven layers with one-way dependencies.

| Layer | Crates | Views |
|---|---|---|
| Tracking and estimation core | 14, from the primitives through the filters, association, lifecycle, random finite sets, fusion, allocation, metrics, and the three verification crates | `../../uaf/resources/Rs-Tx.md`, `Rs-Sr.md` |
| Canonical model | 1 | `../../uaf/information/If-Sr.md` |
| Service facades | 2, live tracking and intercept planning | `../../uaf/services/Sv-Tx.md`, `Sv-Sr.md`, `Sv-Pr.md` |
| Productization | 25, from eventing and the journal through ingest, time, interoperability, identity, geospatial analytics, policy, command, assessment, decision, model operations, security, the interface, observability, resilience, collaboration, workflow, replay, and reporting | `Rs-Cn.md`, `Rs-Pr.md`, `Rs-St.md` |
| 3D data | 2, loaders and GPU fusion | `Rs-Cn.md` |
| Deployment | 2: the remote backends and the headless node binary | `../../uaf/actual-resources/Ar-Sr.md`, `Ar-Cn.md` |
| User interface | 4: rendering, the viewport, the panels, and the desktop binary | `Ar-Sr.md` |

The dependency graph is drawn in `ARCHITECTURE.md` from the actual manifests. The
compliance assessment walked all 147 crate-to-crate edges and found none the graph does
not show.

## 2. The service boundary

Two facades stand between the tracking core and everything above it. They exist so that
no application crate ever depends on a filter, an associator, or an allocator directly,
which is what keeps the numerical core replaceable and independently verifiable.

Both facades speak the canonical model's types as their public contract. The live
tracking service wires real channels and spawns the ingest task; the task drains and logs
until the pipeline exists, and the health flag says so rather than reporting healthy.

The service contract is thinner than the picture the user interface needs, which is
GAP-066: today it exposes what the scaffold happened to require rather than what the
capability statements specify.

## 3. What is real, what is scaffold

Restated from the status the repository already carries, not re-judged here:

| Group | Status |
|---|---|
| Productization layer, 25 crates | Implemented and tested |
| The canonical model, eventing, journal, configuration, session lifecycle | Implemented and tested |
| Tracking mathematics, association, lifecycle, random finite sets, fusion, allocation | **Scaffold.** Trait surfaces with `todo!()` or not-implemented bodies; the pipeline flag is false |
| Intercept planning | **Scaffold.** Returns not-implemented |
| Interoperability codecs | **Scaffold.** ASTERIX and STANAG return not-implemented |
| Interface transport | **Scaffold.** The contract is written, the crates are not signed off |
| User interface | Track table, intercept panel, health, alerts, and a 2D viewport are implemented; the 3D scene and the decision panels are designed and not built |
| Both binaries | Start and run without sensors |

## 4. Technical gaps by layer

Fifty-nine of the eighty-three gaps are technical. Grouped as the technical gap map
groups them:

| Layer | Gaps | The shape of the problem |
|---|---|---|
| Tracking core | GAP-011, GAP-013, GAP-015, GAP-016, GAP-029, GAP-031, GAP-048 | The mathematics. Every one is human-owned, and the scenario generator and the metrics gate the rest |
| Productization, sensing and time | GAP-001 to GAP-005, GAP-008, GAP-010, GAP-064 | Adapters, codecs, authentication, tasking, skew |
| Productization, picture and identity | GAP-006, GAP-007, GAP-012, GAP-014, GAP-017 to GAP-021, GAP-024, GAP-025 | Coverage, staleness, prediction, anomalies, correlation |
| Productization, decision and policy | GAP-026 to GAP-028, GAP-030, GAP-032 to GAP-037, GAP-052 | The decision loop is scaffolded and unwired; policy configuration gates most of it |
| Security | GAP-057 to GAP-060, GAP-062, GAP-068 | Authentication, authorization, audit wiring, encryption, releasability, the adopted roles |
| Deployment and interface | GAP-009, GAP-040, GAP-041, GAP-050, GAP-063, GAP-065 | All wait on the transport sign-off |
| User interface and 3D data | GAP-022, GAP-023, GAP-038, GAP-055, GAP-071 to GAP-075 | The scene, the loaders, the role workspaces, the panels the designs specify |
| Verification and sustainment | GAP-045 to GAP-049, GAP-051, GAP-053, GAP-054, GAP-056, GAP-061, GAP-066, GAP-067, GAP-076 | The harnesses and the workflows that make everything else measurable |
| Machine learning and the assistant | GAP-044, GAP-077 to GAP-080 | New crates, new sign-offs, new governance |

Three dependencies dominate the ordering: the scenario generator and the tracking metrics
gate the pipeline; policy configuration gates the authority work; the transport gates
everything that crosses a network.

## 5. Application principles applied here

| Principle | How it shows up |
|---|---|
| AP-10 one-way dependencies | 147 edges, checked; the graph is generated from the manifests |
| AP-11 traits define the surface | Every verification row is a trait, including the assistant's provider and the inference runtime |
| AP-12 explicit not-implemented | Seventeen variants; nothing returns a plausible default |
| AP-13 binaries wire | Both binaries construct and connect; domain logic stays in libraries |
| AP-06 one owning crate | Nine shared types, one definition each |

## 6. Two new application areas

The assistant (`../../../ai/`) and machine-learning inference (`../../../ml/`) are
application-architecture additions with the same rules and one extra constraint each: the
assistant may not reach a state-changing crate, and inference may not produce a decision.
Both are enforced structurally rather than by policy text, and both are new crates with
new stack sign-offs (GAP-044, GAP-077).

## Traceability

`../../uaf/services/`, `../../uaf/resources/`, `../../uaf/actual-resources/`;
`../../../../ARCHITECTURE.md` §1 to §7; `../../../architecture.md` for the crate map;
`../../../mission/gap-analysis/technical-gap-map.md`;
`../../../mission/capabilities/capability-to-crate-matrix.md`.
