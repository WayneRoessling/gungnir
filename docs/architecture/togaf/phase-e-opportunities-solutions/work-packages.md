# Work packages

Status: first draft, 2026-09-04. Phase E. The gap register grouped into deliverable
bundles, so that phase F can sequence work rather than eighty individual items.

**The gap register is the source.** Nothing is invented here: a work package is a set of
gaps that share an owner, a layer, and a reason to be built together. Severity, effort,
priority, and target increment all come from
[`../../../mission/gap-analysis/gap-register.md`](../../../mission/gap-analysis/gap-register.md)
and are not restated per gap.

## 1. Increment 2, integrate real data

| Package | Gaps | Owner | Why these together |
|---|---|---|---|
| WP-01 Tracking pipeline and its oracles | GAP-016, GAP-048, GAP-011, GAP-013 | Tracking engineer, human-owned | The generator and the metrics are the instruments; the pipeline is meaningless without them, and fusion follows immediately |
| WP-02 The live data path | GAP-064, GAP-001, GAP-002, GAP-003, GAP-008, GAP-010, GAP-066 | Services and security engineers | Codecs, adapters, authentication, the sensor registry, skew, cooperative identity, and the thicker service contract are one path from a wire to a view |
| WP-03 Harnesses and test data | GAP-056, GAP-046, GAP-076, GAP-023 | Services and data engineers | Everything that turns a budget into a measurement and a scenario into a corpus |
| WP-04 Identity and session foundations | GAP-069, GAP-051 | Services engineer | Two small changes the later increments assume |
| WP-05 Build and release pipeline | GAP-061 | Owner | The gates cannot gate until they run |
| WP-18 Architecture compliance automation | GAP-081, GAP-082 | Owner | New in plan 10: the contract checks run by hand today |

## 2. Increment 3, close the decision loop

| Package | Gaps | Owner | Why these together |
|---|---|---|---|
| WP-06 Dense raids | GAP-015 | Tracking engineer, human-owned | Random finite sets, once the pipeline exists |
| WP-07 The decision loop | GAP-026, GAP-027, GAP-030, GAP-029, GAP-031, GAP-028, GAP-032 | Services and tracking engineers | Asset list, lethality, effector layers, allocator, geometry, wiring, alternatives: a chain in that order |
| WP-08 Authority and policy | GAP-052, GAP-033, GAP-058, GAP-034, GAP-035, GAP-018, GAP-039 | Security engineer, human-owned | Policy configuration gates all of it, and the no-execution-without-decision test is the product's central claim |
| WP-09 Role workspaces and decision panels | GAP-055, GAP-068, GAP-072, GAP-038, GAP-071, GAP-073, GAP-075, GAP-070, GAP-074 | User-interface engineer, plan 06 lead | Plan 06's designs in one bundle, plus the roles in code they assume |
| WP-10 Picture enrichment | GAP-004, GAP-005, GAP-006, GAP-007, GAP-012, GAP-017, GAP-019, GAP-020, GAP-021, GAP-022, GAP-036, GAP-037, GAP-042, GAP-043 | Services and user-interface engineers | Everything the picture needs beyond the tracker: tasking, coverage, staleness, correlation, prediction, anomalies, the scene, fires, re-tasking, warning, effect assessment |
| WP-11 Audit and sustainment | GAP-059, GAP-045, GAP-047, GAP-053, GAP-067 | Security and services engineers, owner | Audit wiring, replay through the live pipeline, measures from the journal, model governance, and promoting the second-section verification rows to gates |
| WP-12 Machine-learning data foundation | GAP-079, GAP-083 | Plan 09 lead, architecture agent | The dataset pipeline, and requirement identifiers traceable from code |

## 3. Increment 4, operationalize and scale

| Package | Gaps | Owner | Why these together |
|---|---|---|---|
| WP-13 Transport and the connected profiles | GAP-041, GAP-050, GAP-057, GAP-060, GAP-063 | Services and security engineers | One sign-off unlocks all five; nothing here can start before it |
| WP-14 The outside world | GAP-009, GAP-040, GAP-065, GAP-062 | Services and security engineers | Peers, effectors, coalition exchange, and the releasability enforcement they need |
| WP-15 Analyst and intelligence products | GAP-025, GAP-049, GAP-054, GAP-014, GAP-024 | Services and data engineers | The cluster phase B found least served |
| WP-16 The assistant | GAP-044 | Plan 08 lead | One package, because its safety boundary is one design |
| WP-17 Machine-learning models | GAP-077, GAP-078, GAP-080 | Plan 09 lead | The crate and runtime, the manifests as baselines, and the first two models |

## 4. Sequencing constraints

Four dependencies determine most of the order, and they are checked by the generator that
produces the closure roadmap rather than asserted here:

1. **The generator and the metrics gate the pipeline**, which gates every Understand and
   Decide closure. WP-01 is the first work of increment 2.
2. **Effector layers and costs gate the allocator and the geometry**, which gate the
   policy checks, the wiring, the panel, and the handoff. WP-07 runs in that internal
   order.
3. **Policy configuration gates** staleness policy, identification thresholds, weapons
   control status, and through them pre-delegation and per-class authorization. WP-08
   starts with GAP-052.
4. **The transport gates** peer ingestion, effector handoff, failover, authentication,
   encryption, conformance, and coalition exchange. WP-13 is a prerequisite for WP-14.

The generator also checks that no gap targets an earlier increment than a gap it depends
on, and that the dependency graph has no cycles. Three cycles were found and fixed during
plan 05 by moving items between increments, never by removing the check.

## 5. Packages that are opportunities rather than gaps

Two items in the set are not repairs to a shortfall but additions the plans proposed:
WP-16 the assistant and WP-17 the models. Both are marked lower severity in the register
and higher reach, which is exactly the profile of a feature rather than a defect. If the
schedule slips, they are the two packages to cut, and cutting them costs no capability
that a mission thread requires.

## Traceability

`../../../mission/gap-analysis/gap-register.md`, `closure-roadmap.md`;
`../../../plans/README.md`; `transition-architectures.md` for what each increment leaves
behind; `../phase-f-migration/implementation-and-migration-plan.md` for the schedule.
