# Transition architectures

Status: first draft, 2026-09-04. Phase E. What the architecture actually is at the end of
each increment: what became real, what is still scaffold, and what the health flags say.

The rule that keeps these honest is AP-02. A transition architecture that describes an
intended state is a plan; one that describes the observable state, including the parts
that report themselves broken, is an architecture. `ARCHITECTURE.md` §10 is the ledger
each of these is written against.

## T1, current: the productized scaffold

**Reached 2026-09-04.** The baseline every later state is measured from.

| Aspect | State |
|---|---|
| Real | The canonical model, the event bus, the journal, configuration, session lifecycle, the ingest gateway with validation and quarantine, time, interoperability catalogue, identity, identification, geospatial analytics, policy structure, command structure, assessment, decision, model operations, security traits, observability, resilience, collaboration, workflow, replay, reporting: twenty-five productization crates implemented and tested |
| Real | Both binaries start and run without sensors; the desktop draws a track table, an intercept panel, health, alerts, and a 2D viewport |
| Scaffold | The tracking mathematics, association, lifecycle, random finite sets, fusion, allocation, intercept planning, the industry codecs, the interface transport, the 3D scene |
| Health says | The pipeline flag is false and gates the tracking service's health; the connect call reports a not-implemented transport and the desktop falls back to embedded with an alert; seventeen not-implemented variants are reachable and honest |
| Governance | Eighty-three gaps registered with owners and targets; seventeen engineering decisions resolved; the architecture description generated and checked |

**What a customer could do with T1: nothing operational.** It is a verifiable skeleton and
a demonstrable data path, which is precisely what the business plan sells at this stage.

## T2, end of increment 2: real data in, measured

| Aspect | Target state |
|---|---|
| Becomes real | The tracking pipeline against its oracles; the scenario generator; tracking metrics; track-to-track fusion; live adapters for the lead mission's sensor classes; ASTERIX and STANAG decoding; source authentication; the sensor registry wired; clock-skew detection; cooperative identity decoders; data loaders; the performance harnesses; the test-track corpus replaying end to end; the release workflow running on hosted runners |
| Still scaffold | The allocator, intercept geometry, random finite sets, the decision panels, the transport, the 3D scene |
| Health should say | The pipeline flag becomes true only when the pipeline is verified against its oracles, not when it runs. Every measure claimed by increment 2 has a harness and a recorded number |
| Exit criterion | Multiple real or recorded sources enter through governed adapters, are normalized, and retain traceable provenance to the display |
| Closes | WP-01 to WP-05, WP-18; eighteen gaps plus two new |

The risk to watch at T2 is a green tracking-core build being read as readiness. That is
exactly what GAP-067 exists to prevent, and it is why the second-section verification rows
are promoted to gates in increment 3 rather than left draft.

## T3, end of increment 3: the loop closes

| Aspect | Target state |
|---|---|
| Becomes real | The defended-asset list; threat scoring against it; effector layers and costs; the allocator; intercept geometry; the decision crates wired; alternatives and what-if; policy configuration; weapons control status; per-class and per-layer authorization; escalation and timeout; the approval queue and decision panels; the eight roles in code; role workspaces; audit wiring; measures from the journal; the 3D scene; random finite sets |
| Still scaffold | The transport and everything behind it; peer and coalition exchange; effector handoff; the assistant; the models |
| Health should say | Nothing executes without a recorded human decision, and a test proves it (GAP-039). Releasability markings appear on views, reports, and the contract, unenforced and visibly so |
| Exit criterion | The system recommends, never executes, policy-constrained actions with a rationale and a complete audit trail |
| Closes | WP-06 to WP-12; forty-three gaps plus two new |

T3 is the increment where the product becomes the product. It is also the largest, and
the work-packages document says which two packages are cuttable if it slips.

## T4, end of increment 4: operational and connected

| Aspect | Target state |
|---|---|
| Becomes real | The interface transport; mid-session failover and reconciliation; authentication; encryption in transit and at rest; the conformance suite; peer ingestion; effector handoff; coalition exchange; releasability enforcement per caller; pattern of life and order of battle; after-action review; battle rhythm; registration evidence; point-cloud registration; the assistant; the first two models |
| Still scaffold | Nothing that a mission thread requires. Anything remaining is named in the ledger with a reason |
| Health should say | All three profiles report their true state; a desktop that loses its node says so, stores and forwards, and reconciles with conflicts reported |
| Exit criterion | All three deployment profiles with measurable reliability, performance, security, and recoverability, and a desktop that survives losing its node |
| Closes | WP-13 to WP-17; fifteen gaps plus three new |

## What each transition must leave behind

Not optional, and the reason the increments have a closing ritual at all:

1. This document updated with what actually became real, as against what was targeted.
2. A compliance assessment run and its findings filed.
3. `ARCHITECTURE.md` §10 moved: resolved items out of Open, new open items in.
4. Every measure the increment claims, with a harness and a number.
5. The gap register regenerated, so the counts in every ledger agree.

## Honest note on these four states

T1 is observed. T2, T3, and T4 are targets derived from the gap register's increment
assignments, which are themselves the drafting agent's proposals awaiting the engineering
reviewer. **No date is attached to any of them here.** The business plan carries the
schedule and its own confidence marks; putting dates in a transition architecture would
give the sequence a false precision it has not earned.

## Traceability

`../../../../ARCHITECTURE.md` §10; `../../../gungnir-capabilities.md` §7;
`work-packages.md`; `../../../mission/gap-analysis/closure-roadmap.md`;
`../phase-g-implementation-governance/compliance-assessment.md`;
`../../../business/roadmap.md` for the commercial milestones on top of these.
