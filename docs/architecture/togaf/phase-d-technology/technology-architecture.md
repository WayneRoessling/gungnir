# Technology architecture

Status: first draft, 2026-09-04. Phase D. **The content lives in the UAF actual-resources
and standards views and in `ARCHITECTURE.md` §8 and §9; this document says what phase D
concluded and where the technology gaps are.** Engineering reviewer sign-off outstanding.

## 1. Platforms

| Element | Decision |
|---|---|
| Language and toolchain | Rust, pinned to 1.98 by the toolchain file, with a workspace lint policy every crate opts into |
| Desktop platform | Windows x86_64 for the released desktop binary; the workspace builds on Linux |
| Node platform | Linux x86_64 containers, unprivileged runtime user, pinned base image |
| Concurrency | The async runtime is created by the host binary, never by a library |
| Graphics | Two contexts, kept separate: OpenGL for the interface and viewport, `wgpu` for compute |
| Packaging | An OCI container image for the node, a signed executable for the desktop, a bill of materials for both |

## 2. Deployment profiles

Three profiles configure one crate set. This is principle AP-05 and it is the single most
consequential technology decision in the architecture.

| Profile | Services layer | System of record | Network |
|---|---|---|---|
| Disconnected desktop | Embedded in the desktop binary, with a local journal and local ingest | The desktop's journal | None required |
| On-prem connected | One or more node instances on the local network | The node | Local area network |
| Cloud connected | The same node binary, hosted | The node | Wide area network, higher latency budget, stricter posture |

The profile is chosen by the desktop's configuration baseline. In the connected profiles
the desktop runs remote service implementations that subscribe to the node's event
stream, keep a local projection, and forward detections and decisions. **Today the
transport is not in the workspace, so the connect call reports a not-implemented
transport and the desktop falls back to embedded operation with an alert.** That fallback
is AP-02 working: a connected profile that cannot connect says so instead of appearing
healthy.

Where each crate group runs is `ARCHITECTURE.md` §8.3; degraded behaviour is §8.4;
security posture per profile is §8.5.

## 3. The version set

Twenty pinned dependencies, all in one place, all recorded in the standards document
§2.1 to §2.9. The compliance assessment checked the manifest against that record and
found no unrecorded crate.

| Group | Pins |
|---|---|
| Numerical and runtime | `nalgebra` 0.33, `tokio` 1, `serde` and `serde_json` 1, `rand` 0.8, `rand_distr` 0.4, `thiserror` 1, `crossbeam-channel` 0.5 |
| Verification | `proptest` 1, `criterion` 0.5 |
| Interoperability and diagnostics | `arrow` 53, `tracing` 0.1, `tracing-subscriber` 0.3 |
| Interface and rendering | `eframe` and `egui` 0.29, `three-d` 0.18, `wgpu` 22 |
| 3D input and output | `gltf` 1, `vtkio` 0.6, `las` 0.9 |

Two known issues carried openly: `vtkio` pulls transitive crates the compiler flags as
future-incompatible, and it is upgraded when a release drops them; the young point-cloud
crates named in the 3D-data plan are unpinned until the change that first uses one.

**Four sign-offs are pending**, each named rather than implied: the transport crates for
the interface, the identifier crate decided by D-11, a docking crate for the layouts D-17
adopted, and the inference runtime the machine-learning plan needs. Each enters through
the same recorded procedure (AP-14).

## 4. The two graphics contexts

The interface and the 3D viewport draw through one OpenGL context owned by the
application shell's `glow` backend. Point-cloud fusion runs on a separate headless
compute device; on Windows that selects DirectX 12 or Vulkan. **There is no buffer
sharing between the two**; fusion results cross to the viewport through a CPU point
buffer.

This is worth restating in a technology architecture because the natural assumption is
the opposite, and an engineer who assumes a shared device will write code that cannot
link. The earlier claim that one device served both was retired; it was only true of a
graphics stack this project did not adopt.

## 5. Standards and interoperability

Fifteen standards in the standards information base, five of them real today and ten
planned, each with the gap or decision that carries it. The interface contract defines a
snapshot, an event stream, detection submission, and plan decisions, over JSON on HTTP
with a WebSocket event stream, with transport-layer security 1.3 on every crossing and
mutual authentication for machine identities.

Version negotiation is exact-match in the schema catalogue. Minor-version tolerance
becomes a policy decision when a second version exists, and not before.

## 6. Assurance

The release governance document is the technology assurance track: an allow-listed
licence policy, advisory scanning, dependency provenance restricted to one registry, a
bill of materials per release, auditable binaries carrying their own dependency list,
keyless signatures, and a promotion step that verifies the signature and records which
bill of materials was deployed.

The vulnerability-response objective is critical advisories within seven days of
publication, high within thirty, lower at the next release.

## 7. Technology gaps

| Gap | What is missing |
|---|---|
| GAP-041 | The transport. It gates the peer, effector, coalition, failover, authentication, encryption, and conformance work |
| GAP-060 | Encryption in transit and at rest |
| GAP-057 | Authentication implementation against the mechanism D-02 fixed |
| GAP-022 | The 3D scene is not created from the shell's graphics context; a 2D fallback stands in |
| GAP-023, GAP-024 | Data loaders per format, and point-cloud registration on both CPU and GPU |
| GAP-056 | The performance harnesses. The budgets are confirmed as provisional gates and nothing measures them |
| GAP-061 | The release workflow has never run; it is run against the hosted repository under this gap (D-10 as amended 2026-09-07: GitHub) |
| GAP-069 | The identifier crate |
| GAP-075 | The docking crate for the adopted layouts |
| GAP-077 | The inference runtime, its sign-off, and the crate that hosts it |

GAP-056 deserves emphasis. Every latency and throughput figure in this architecture is a
budget, not a measurement, and no harness exists to turn one into the other.

## Traceability

`../../../../ARCHITECTURE.md` §8 and §9; `../../uaf/actual-resources/`;
`../../uaf/standards/Sd-Tx.md`, `Sd-Rm.md`; `../../../performance-budgets.md`;
`../../../release-governance.md`; `../../../gungnir-api-v1.md`;
`../../../rust-ui-tech-stack-summary.md`.
