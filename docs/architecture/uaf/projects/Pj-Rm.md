# Pj-Rm Project roadmap

**UAF definition.** The projects roadmap view shows the projects that deliver the
architecture, their sequence, and their dependencies.

**Purpose here.** The two project streams: the four engineering increments that
deliver the capabilities, and the ten documentation plans that describe, design,
and govern them. Read by the owner and plan 10 (phases E and F).

Status: first draft, 2026-09-04.

Diagram: [`Pj-Rm.mmd`](Pj-Rm.mmd).

## Engineering increments

| Project | Delivers | Gaps closed (count, `../../../mission/gap-analysis/closure-roadmap.md`) | Status |
|---|---|---|---|
| PJ-I1 Productize the core | canonical model, service contracts, session lifecycle, eventing and journal, replay | the scaffold as it stands | in progress |
| PJ-I2 Integrate real data | ingest adapters, time discipline, sensor management, the tracking pipeline, scenarios and metrics, harnesses, hosting | 17 | planned |
| PJ-I3 Close the decision loop | assessment, allocator, geometry, policy model, queue, panels, warnings, effects, fires, roles in code, codecs | 38 | planned |
| PJ-I4 Operationalize and scale | transport, connected profiles, failover, authentication for callers, encryption, releasability, peers, the assistant; the single release (D-01) | 15 | planned |

## Documentation and design plans

| Project | Produces | Depends on | Status |
|---|---|---|---|
| PJ-P02 Mission analysis | `docs/mission/` | | first draft 2026-09-04 |
| PJ-P04 Mission capabilities | `docs/mission/capabilities/` | P02 | first draft 2026-09-04 |
| PJ-P05 Capability design gaps | `docs/mission/gap-analysis/` | P04 | first draft 2026-09-04; decisions D-01 to D-16 resolved |
| PJ-P03 UAF views | this description | P02, P04 | first draft 2026-09-04 |
| PJ-P06 UX designs by role | `docs/ux/` | P02, P03 | not started |
| PJ-P07 Test-track suite | `docs/test-tracks/`, `testdata/tracks/` | P02 | not started |
| PJ-P01 Product business plan | `docs/business/` | P05, P06 | not started |
| PJ-P09 ML model integration | `docs/ml/` | P07 | not started |
| PJ-P08 AI agent integration | `docs/ai/` | P06, transport | not started |
| PJ-P10 TOGAF ADM documentation | `docs/architecture/togaf/` | P03 | not started |

## Dependencies between the streams

- PJ-I2 needs PJ-P07's first scenarios for verification and PJ-P03's resource views
  for engineering review; PJ-I3 needs PJ-P06's panel designs; PJ-I4 needs PJ-P08's
  egress policy and PJ-P10's governance.
- PJ-P01 sets the calendar dates the increments do not yet have.

## Elements used

- PJ-I1 to PJ-I4; PJ-P01 to PJ-P10.

## Notes

- Effort per plan is in `../../../plans/README.md`; the increments' effort is the
  gap register's effort sizes summed per increment.

## Traceability

- Derives from: `../../../gungnir-capabilities.md` §7; `../../../plans/README.md`;
  `../../../mission/gap-analysis/closure-roadmap.md`; St-Rm.
- Feeds: plan 10 phases E and F; plan 01's roadmap.
