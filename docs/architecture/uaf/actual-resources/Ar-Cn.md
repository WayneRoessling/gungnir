# Ar-Cn Actual resource connectivity

**UAF definition.** The actual resources connectivity view shows the physical and
logical connections between actual resources in a deployment.

**Purpose here.** The links in each profile, what they carry, and the budgets they
must meet. Read with Ar-Sr and `../../../performance-budgets.md`.

Status: first draft, 2026-09-04; link statuses checked against the code 2026-09-16.
A link marked planned is designed but not built.

Diagram: [`Ar-Cn.puml`](Ar-Cn.puml).

## Links

| Link | Profiles | Carries | Budget or rule | Status |
|---|---|---|---|---|
| AR-04 sensors → AR-01 or AR-02/AR-03 | all | native sensor protocols into adapters | 5,000 detections per second per node (MOP-17) | live ASTERIX over UDP, with recorded and simulated inputs |
| AR-01 ↔ AR-02 (LAN) | on-prem | JSON over HTTP (snapshot, submissions; the plan-decision route answers 501, as a node runs no approval queue); WebSocket event stream | detection to node publish p99 under 150 ms (MOP-02); fallback within 2 s of a lost heartbeat (MOP-11) | built (GAP-041) |
| AR-01 ↔ AR-03 (WAN) | cloud | as above over TLS | p99 under 400 ms (MOP-02) | built (GAP-041) |
| AR-02/AR-03 ↔ AR-05 peers | connected | peer tracks and warnings; products with marking; STANAG 4676 blocked on AEDP-12, which is not public | latency per agreement (D-08); staleness visible (MOP-22) | built over the v2 contract (GAP-065) |
| AR-02/AR-03 → AR-06 effectors | connected | handoff messages | under 500 ms from the decision record (MOP-32) | built (GAP-040) |
| AR-07 → AR-02/AR-03/AR-01 | all | signed images and installers | release gates (MOP-41) | planned (GAP-061) |
| AR-01 internal | all | OpenGL context; wgpu compute device; CPU point buffer between them | 60 fps, never below 30 (MOP-16) | real |

## Fan-out and capacity

- Event stream fan-out: 10 subscribed desktops per node without exceeding the
  publish budget.
- Store-and-forward: 100,000 detections per desktop while disconnected (MOP-13);
  reconciliation under 60 s for a 10-minute outage (MOP-12).

## Elements used

- AR-01 to AR-07; SD-06 to SD-08.

## Notes

- Physical media (fibre, radio, satellite) are per deployment and not modelled;
  the budgets apply to whatever medium is used.

## Traceability

- Derives from: Ar-Sr; `../../../performance-budgets.md`; `../../../gungnir-api-v1.md`.
- Feeds: Sc-Cn (which links need protection), Pm-Me (the budgets as measurements),
  the connectivity harness (GAP-056).
