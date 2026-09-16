# Pj-Rm Project roadmap

**UAF definition.** The projects roadmap view shows the projects that deliver the
architecture, their sequence, and their dependencies.

**Purpose here.** The two project streams: the four engineering increments that
deliver the capabilities, and the eleven documentation plans that describe, design,
and govern them. Read by the owner and plan 10 (phases E and F).

Status: first draft, 2026-09-04; plan statuses checked against their deliverables, and plan
11 added, 2026-09-16.

Diagram: [`Pj-Rm.mmd`](Pj-Rm.mmd).

## Engineering increments

| Project | Delivers |
|---|---|
| PJ-I1 Productize the core | canonical model, service contracts, session lifecycle, eventing and journal, replay |
| PJ-I2 Integrate real data | ingest adapters, time discipline, sensor management, the tracking pipeline, scenarios and metrics, harnesses, hosting |
| PJ-I3 Close the decision loop | assessment, allocator, geometry, policy model, queue, panels, warnings, effects, fires, roles in code, codecs |
| PJ-I4 Operationalize and scale | transport, connected profiles, failover, authentication for callers, encryption, releasability, peers, the assistant; the single release (D-01) |

How far each increment has got -- its gaps closed, in progress, open and planned -- is
generated from the gap data into `../../../mission/gap-analysis/closure-roadmap.md`
§Counts, and is not restated here. PJ-I1 has no gaps of its own: it is the scaffold the
others build on.

## Documentation and design plans

| Project | Produces | Depends on | Status |
|---|---|---|---|
| PJ-P02 Mission analysis | `docs/mission/` | | first draft 2026-09-04 |
| PJ-P04 Mission capabilities | `docs/mission/capabilities/` | P02 | first draft 2026-09-04 |
| PJ-P05 Capability design gaps | `docs/mission/gap-analysis/` | P04 | first draft 2026-09-04; decisions D-01 to D-16 resolved |
| PJ-P03 UAF views | this description | P02, P04 | first draft 2026-09-04 |
| PJ-P06 UX designs by role | `docs/ux/` | P02, P03 | first draft 2026-09-04 |
| PJ-P07 Test-track suite | `docs/test-tracks/`, `testdata/tracks/` | P02 | first draft 2026-09-04 |
| PJ-P01 Product business plan | `docs/business/` | P05, P06 | held outside the repository by the owner's decision of 2026-09-07 |
| PJ-P09 ML model integration | `docs/ml/` | P07 | first draft 2026-09-04 |
| PJ-P08 AI agent integration | `docs/ai/` | P06, transport | first draft 2026-09-04 |
| PJ-P10 TOGAF ADM documentation | `docs/architecture/togaf/` | P03 | first draft 2026-09-04 |
| PJ-P11 Design gap closure | `docs/design/` | P02, P04, P05, P10 | first draft 2026-09-05 |

## Dependencies between the streams

- PJ-I2 needs PJ-P07's first scenarios for verification and PJ-P03's resource views
  for engineering review; PJ-I3 needs PJ-P06's panel designs; PJ-I4 needs PJ-P08's
  egress policy and PJ-P10's governance.
- PJ-P01 sets the calendar dates the increments do not yet have.

## Elements used

- PJ-I1 to PJ-I4; PJ-P01 to PJ-P11.

## Notes

- Effort per plan is in `../../../plans/README.md`; the increments' effort is the
  gap register's effort sizes summed per increment.

## Traceability

- Derives from: `../../../gungnir-capabilities.md` §7; `../../../plans/README.md`;
  `../../../mission/gap-analysis/closure-roadmap.md`; St-Rm.
- Feeds: plan 10 phases E and F; plan 01's roadmap.
