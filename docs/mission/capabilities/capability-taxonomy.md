# Capability taxonomy

Status: first draft, 2026-09-04. Seven areas, each with leaf capabilities derived
from the steps of the ten mission threads (`../mission-threads.md`). Identifiers are
stable: new leaves are appended within an area, never renumbered. The same list is
the `capabilities` section of `../../architecture/uaf/model/elements.yaml`.

Each leaf is a mission capability, written as an outcome the force needs, not a
feature the software has. The statements, measures, and maturity targets are in
`capability-statements.md`.

## CAP-1 Sense

Bring every observation the sector can obtain into one governed, time-disciplined
stream.

| Id | Leaf capability |
|---|---|
| CAP-1.1 | Ingest observations from every sensor class in the sector into one canonical observation stream with provenance |
| CAP-1.2 | Validate, authenticate, and quarantine every external input before it reaches the picture |
| CAP-1.3 | Manage sensor readiness, modes, tasking, and calibration |
| CAP-1.4 | Know what each sensor can see over terrain and where the gaps are |
| CAP-1.5 | Keep time discipline across sources: source versus receipt time, late data, clock health |
| CAP-1.6 | Receive early warning, tracks, and reports from peers and higher command |
| CAP-1.7 | Receive cooperative identity from own and civil platforms |

## CAP-2 Understand

Turn observations into one honest picture with identities, uncertainty, and
context.

| Id | Leaf capability |
|---|---|
| CAP-2.1 | Maintain one multi-sensor track picture per domain from sources at different rates and latencies, with stated uncertainty |
| CAP-2.2 | Keep tracks through gaps and dropouts, and make quality and staleness visible |
| CAP-2.3 | Register sensors against each other so systematic bias does not corrupt the fused picture |
| CAP-2.4 | Track dense groups and estimate how many targets are present |
| CAP-2.5 | Maintain a clutter-tolerant surface picture of a port and its approaches |
| CAP-2.6 | Classify and identify tracks from fused evidence with stated confidence |
| CAP-2.7 | Keep one identity for an entity across gaps, sorties, sessions, and peers |
| CAP-2.8 | Predict where a track is going: trajectory, time to impact, closest point of approach to defended assets |
| CAP-2.9 | Detect anomalies in tracks and feeds |
| CAP-2.10 | Fuse terrain, imagery, point clouds, and map layers into the picture's context |
| CAP-2.11 | Answer geometric questions: line of sight, viewshed, coverage volumes, route deconfliction |
| CAP-2.12 | Maintain pattern of life and the adversary order of battle across sessions |

## CAP-3 Decide

Turn the picture into a policy-checked recommendation for a human.

| Id | Leaf capability |
|---|---|
| CAP-3.1 | Maintain the defended-asset list with priorities and warning obligations |
| CAP-3.2 | Score threat and priority for every track against the defended-asset list |
| CAP-3.3 | Recommend the cheapest adequate resource-to-track assignment within readiness, geometry, and policy |
| CAP-3.4 | Compute intercept geometry for each assignment |
| CAP-3.5 | Offer alternatives, what-if analysis, and a rationale for every recommendation |
| CAP-3.6 | Enforce rules of engagement: identification criteria, weapons control status, engagement authority, restrictions, escalation and timeout |
| CAP-3.7 | Manage the recommendation queue under saturation |
| CAP-3.8 | Recommend fires tasks against land targets with deconfliction |
| CAP-3.9 | Recommend sensor re-tasking when the picture degrades |

## CAP-4 Act (recommend and authorize)

Put the recommendation in front of the person who holds authority, record what they
decide, and hand off; never act without that record.

| Id | Leaf capability |
|---|---|
| CAP-4.1 | Present each recommendation with its verdict, rationale, alternatives, cost, and time remaining |
| CAP-4.2 | Record every decision with who, what, when, and the evidence and verdict at the time |
| CAP-4.3 | Guarantee that nothing is executed without a recorded human decision; record self-defense engagements after the fact |
| CAP-4.4 | Hand off decided assignments and fires tasks to effector systems with provenance |
| CAP-4.5 | Warn defended assets, units, and civil authorities of inbound threats |
| CAP-4.6 | Track engagements, assess effects, and supersede plans |
| CAP-4.7 | Assist every role with explanations, answers, and drafts that carry provenance and no authority |

## CAP-5 Sustain

Keep the sector operating, learning, and honest over time and through outages.

| Id | Leaf capability |
|---|---|
| CAP-5.1 | Journal every event durably as the system of record |
| CAP-5.2 | Replay any session deterministically and rehearse plans against scenarios |
| CAP-5.3 | Produce after-action reports and measures from the journal |
| CAP-5.4 | Operate disconnected, store and forward, and reconcile on reconnection with conflicts reported |
| CAP-5.5 | Report health honestly, correlate alerts, and run the alert lifecycle |
| CAP-5.6 | Manage configuration baselines and mission plans: validate before apply, audit every change |
| CAP-5.7 | Govern algorithm and model baselines: validate, promote, roll back |
| CAP-5.8 | Support the battle rhythm: handover, reporting cycle, maintenance windows |
| CAP-5.9 | Provide role-based workspaces and the operator workflow |
| CAP-5.10 | Meet the performance budgets of each deployment profile |

## CAP-6 Secure

Control who can see and do what, and prove it afterwards.

| Id | Leaf capability |
|---|---|
| CAP-6.1 | Authenticate operators and API callers |
| CAP-6.2 | Authorize by role, refined by threat class and effector layer |
| CAP-6.3 | Audit every decision, configuration change, model promotion, and assistant exchange |
| CAP-6.4 | Protect data in transit and at rest as each profile requires |
| CAP-6.5 | Assure the software and model supply chain |
| CAP-6.6 | Mark and enforce releasability of pictures and products |
| CAP-6.7 | Treat every external input as untrusted, including free text presented to the assistant |

## CAP-7 Integrate

Be one node in a system of systems.

| Id | Leaf capability |
|---|---|
| CAP-7.1 | Expose a versioned interface for desktops, peers, and analytics: snapshot, event stream, submission, decision |
| CAP-7.2 | Speak the interoperability standards of the domain through a governed schema catalogue |
| CAP-7.3 | Run as a disconnected desktop, an on-prem node, or a cloud node from one crate set |
| CAP-7.4 | Exchange pictures, warnings, and products with peers and coalition partners |

## Counts

| Area | Leaves |
|---|---|
| Sense | 7 |
| Understand | 12 |
| Decide | 9 |
| Act | 7 |
| Sustain | 10 |
| Secure | 7 |
| Integrate | 4 |
| Total | 56 |
