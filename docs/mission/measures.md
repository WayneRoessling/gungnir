# Measures of effectiveness and performance

Status: first draft, 2026-09-04. Measures of effectiveness (MOE) describe mission
outcomes; measures of performance (MOP) describe system behaviour that produces
them. Values marked *budget* come from `../performance-budgets.md`; values marked
*confirmed 2026-09-04* were proposed by the drafting agent and confirmed or adjusted by
the owner that day (D-16 in `gap-analysis/decisions-needed.md`); values marked
*doctrine* have a public doctrinal or analytical source noted in the domain documents. Every measure names
how it is measured, because the after-action review (MT-09) is where these are
actually computed from the journal.

## 1. Measures of effectiveness

| Id | Measure | Threads | Definition | Target | Source |
|---|---|---|---|---|---|
| MOE-01 | Defended-asset protection | MT-01, MT-02, MT-04 | Fraction of threats predicted to reach a listed asset that are engaged before reaching it or whose asset is warned with the agreed lead time | 0.95 for priority-1 and priority-2 assets | confirmed 2026-09-04 |
| MOE-02 | No fratricide, no civil engagement | MT-01, MT-02, MT-03, MT-04, MT-08 | Count of engagements of tracks later shown friendly or civil | 0 | doctrine (absolute) |
| MOE-03 | Cost discipline | MT-01, MT-02 | Fraction of propeller-drone engagements made by the point or self-defense layers rather than area-defense interceptors | at least 0.9 | confirmed 2026-09-04 |
| MOE-04 | Decision timeliness | MT-01, MT-02, MT-03, MT-04 | Time from a track meeting policy criteria for engagement to a recorded decision, as a fraction of the track's remaining time to impact | p95 under 0.3 | confirmed 2026-09-04 |
| MOE-05 | Decision completeness | all | Fraction of engagements with a recorded decision, verdict, and rationale | 1.0 | doctrine (absolute) |
| MOE-06 | Picture honesty | MT-07, MT-10 | Count of decisions taken on tracks or backends whose degraded state was not shown | 0 | doctrine (absolute); confirmed 2026-09-04 |
| MOE-07 | Surface picture completeness | MT-05 | Fraction of vessels in the approaches carried as a track with identity or anomaly flag | 0.98 | confirmed 2026-09-04 |
| MOE-08 | Fires timeliness | MT-06 | Fraction of time-sensitive fires tasks decided within the target's window | 0.9 | confirmed 2026-09-04 |
| MOE-09 | Identity continuity | MT-06, MT-08 | Fraction of entities that keep one global identity across an observation gap in the scenario | 0.9 | confirmed 2026-09-04 |
| MOE-10 | Degradation recovery | MT-07 | Time from sensor loss to coverage recomputed and shown; time to formal acceptance or restoration | 30 s to show; battle rhythm to accept | confirmed 2026-09-04 |
| MOE-11 | Continuity under disconnection | MT-10 | Fraction of offline decisions and detections that reach the node's record after reconnection; conflicts resolved rather than overwritten | 1.0 reached; 1.0 resolved | doctrine (absolute); confirmed 2026-09-04 |
| MOE-12 | Rehearsal effect | MT-09 | Fraction of shifts that rehearse; gaps found in rehearsal versus in action | rehearse every plan change; the rehearsal-versus-action ratio is observed without a target | confirmed 2026-09-04 |

## 2. Measures of performance

| Id | Measure | Threads | Definition | Target | Source |
|---|---|---|---|---|---|
| MOP-01 | Detection to display | all live | Source time to glyph update on the desktop, embedded profile | p99 under 250 ms | budget |
| MOP-02 | Detection to node publish | connected | Source time to event-stream publish on the node | p99 under 150 ms on-prem, 400 ms cloud | budget |
| MOP-03 | Track continuity | MT-01, MT-04, MT-06 | Fraction of true tracks maintained without a break through scenario dropouts | per scenario row in `../verification-capability-table.md` | table |
| MOP-04 | False-track rate | MT-05 | False tracks per hour above the display threshold in Scenario 2 clutter | under 1 per hour | confirmed 2026-09-04; carried into the Scenario 2 association and lifecycle rows when they are promoted |
| MOP-05 | Identification timeliness | MT-01, MT-08 | Time from track initiation to a classification at policy confidence | p90 within the first third of the warning time | confirmed 2026-09-04 |
| MOP-06 | Plan recompute | MT-01, MT-02 | Time for the planner to return a plan for a snapshot of N tracks and M resources | p99 under the per-frame budget of 4 ms for the embedded profile; off-thread with last-good-plan return beyond | budget |
| MOP-07 | Decision path latency | MT-02 | Time from plan proposed to the approval control being available with verdict and rationale | under 500 ms | confirmed 2026-09-04 |
| MOP-08 | Health reporting latency | MT-07 | Time from a sensor or pipeline fault to the health flag and correlated alert | under 5 s | confirmed 2026-09-04 |
| MOP-09 | Clock skew detection | MT-07 | Time to flag a GNSS-dependent source whose clock skew exceeds the late-data policy | under 30 s | confirmed 2026-09-04 |
| MOP-10 | Journal durability | all | Time from an accepted envelope to its being on disk | under 100 ms | budget |
| MOP-11 | Fallback to embedded | MT-10 | Time from last successful heartbeat to embedded operation with a banner | under 2 s | budget |
| MOP-12 | Reconciliation time | MT-10 | Time to merge and report after a ten-minute outage | under 60 s | budget |
| MOP-13 | Store-and-forward capacity | MT-10 | Detections held per desktop while disconnected before the oldest are dropped | 100,000 | budget |
| MOP-14 | Coverage recompute | MT-07, MT-09 | Time to recompute coverage over terrain for the sector after a sensor change | under 10 s | confirmed 2026-09-04 |
| MOP-15 | Replay determinism | MT-09 | Two replays of one journal produce identical event sequences | exact | table |
| MOP-16 | Frame rate with the viewport open | all live | Sustained frame rate at Scenario 4 track counts | 60 fps, never below 30 | budget |
| MOP-17 | Ingest throughput, node | MT-01, MT-05 | Detections per second through validation and quarantine | 5,000 per second | budget |
| MOP-18 | Alert correlation | MT-07 | Correlated incidents shown per raw alerts received | never more incidents than raw alerts; operator-rated usefulness in plan 06 tests | table |

## 3. How they are measured

- MOE-01 to MOE-04, MOE-07 to MOE-09: from the journal against the scenario's ground
  truth (test tracks carry truth; live sessions need the analyst's reconstruction).
- MOE-02, MOE-05, MOE-06, MOE-12, MOE-13: from the journal alone (engagement, decision,
  health, replay, review and rhythm events; `gungnir-reporting::measures`, GAP-047).
  MOE-05 counts an accepted decision's missing rationale until the course of action
  reaches the record (GAP-032). MOE-11: from the journal once reconciliation events
  exist (GAP-050).
- MOP-01, MOP-02, MOP-06, MOP-07, MOP-08, MOP-10 to MOP-14: from `tracing` spans and
  the tick harness in `../performance-budgets.md`.
- MOP-03, MOP-04, MOP-15: from the verification-table rows.
- MOP-16 to MOP-18: from the benchmark and load harnesses.

## 4. Open items

- Every value was confirmed or set by the owner on 2026-09-04; a change needs a
  recorded reason. Reviewers may still propose doctrinal values where those exist and
  are releasable.
- The intelligence function's product measure is MOE-13 in
  `capabilities/measures-catalogue.md`; requirement satisfaction stays unmeasured until
  the tasking workflow (GAP-005) exists.
