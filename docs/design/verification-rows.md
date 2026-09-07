# Verification rows

Status: **agreed by the owner 2026-09-05.** The rows this design set contributed to
[`../verification-capability-table.md`](../verification-capability-table.md) §2, and the
map from each row back to the note that produced it.

**The criteria themselves are not repeated here.** They live in the verification table,
which is their one home. This file is the mapping, so that a row can be traced to the
design that justified it and to the crate that will implement it.

## Why the criteria were written first

Twenty-three rows across twenty-two capabilities were written before any of their code
exists, so no criterion could be fitted to an implementation. That is AP-17, and it is the
rule most likely to erode quietly under delivery pressure. Having the owner agree them at
design time rather than at review time is what makes contract C-17 enforceable: a later
change to any of these is a change request under phase H, not an edit.

## Row to note to crate

| Capability | Crate the row sits under | Design note |
|---|---|---|
| CAP-3.1 Defended-asset list | `gungnir-assessment` | [DN-01](DN-01-defended-assets.md) |
| CAP-2.8 Predict trajectory and approach | `gungnir-assessment` | [DN-02](DN-02-prediction-and-approach.md) |
| CAP-4.5 Warn assets and authorities | `gungnir-workflow` | [DN-03](DN-03-warning.md) |
| CAP-3.3 Assignment recommendation | `gungnir-intercept-service` | [DN-04](DN-04-effector-model.md) |
| CAP-3.8 Fires tasks | `gungnir-policy` | [DN-05](DN-05-fires.md) |
| CAP-4.6 Track engagements and effects | `gungnir-intercept-service` | [DN-06](DN-06-engagement-and-effect.md) |
| CAP-4.4 Handoff with provenance | `gungnir-api` | [DN-07](DN-07-handoff.md) |
| CAP-5.6 Baselines and plans | `gungnir-config` | [DN-08](DN-08-policy-configuration.md) |
| CAP-3.6 Rules of engagement | `gungnir-policy` | [DN-09](DN-09-authority-and-control-status.md) |
| CAP-3.7 Queue under saturation | `gungnir-command` | [DN-10](DN-10-queue-expiry-and-escalation.md) |
| CAP-1.3 Sensor modes and tasking | `gungnir-sensor-management` | [DN-11](DN-11-sensor-control-and-tasking.md) |
| CAP-2.12 Requirements and tasking concurrence | `gungnir-workflow` | [DN-11](DN-11-sensor-control-and-tasking.md) |
| CAP-1.4 Coverage and gaps | `gungnir-analytics` | [DN-12](DN-12-coverage-and-gaps.md) |
| CAP-3.9 Sensor re-tasking | `gungnir-decision` | [DN-13](DN-13-sensor-retasking.md) |
| CAP-2.5 Hazard and barrier layer | `gungnir-geo` | [DN-14](DN-14-hazard-layer.md) |
| CAP-2.9 Anomalies | `gungnir-analytics` | [DN-15](DN-15-anomaly-detectors.md) |
| CAP-1.6 Peer early warning | `gungnir-ingest` | [DN-16](DN-16-peer-sources.md) |
| CAP-6.6 Releasability | `gungnir-security` | [DN-17](DN-17-releasability.md) |
| CAP-7.4 Peer and coalition exchange | Cross-layer | [DN-18](DN-18-coalition-exchange.md) |
| CAP-2.12 Order of battle and pattern of life | `gungnir-reporting` | [DN-19](DN-19-order-of-battle.md) |
| CAP-5.3 Reports and measures, review part | `gungnir-workflow` | [DN-20](DN-20-after-action-review.md) |
| CAP-5.8 Battle rhythm | `gungnir-reporting` | [DN-21](DN-21-battle-rhythm.md) |
| CAP-6.4 Data protection | `gungnir-security` | [DN-22](DN-22-key-management.md) |
| CAP-7.4 SD-16 exchange over a bearer with no party | `gungnir-remote` | [DN-25](DN-25-cursor-on-target.md) |
| CAP-3.8 Friendly-set provenance in fires deconfliction | `gungnir-policy` | [DN-25](DN-25-cursor-on-target.md) |
| CAP-1.6 Self-reported positions through the gateway | `gungnir-ingest` | [DN-25](DN-25-cursor-on-target.md) |

CAP-2.12 takes two rows because two notes touch it from different directions: the tasking
half is a workflow question and the product half is a reporting one.

**The last three rows are later than the rest (2026-09-06, D-33) and were agreed
separately.** DN-25 is not part of the plan-11 set. CAP-7.4, CAP-3.8 and CAP-1.6 each already
carried an agreed row from 2026-09-05, and none of those three was touched: DN-25 adds a
second bearer, a second source of friendly positions, and a second kind of inbound thing, so
the new rows govern what the old ones never spoke to. Editing the older three to cover the
new cases would have been the widening the workspace forbids, arrived at by the
honest-looking route -- and in CAP-1.6's case it would also have blurred the distinction the
row exists to hold, that a peer's track and a report about the sender itself are not the same
claim.

**CAP-1.6 now takes two rows, for the same reason CAP-2.12 does**: two notes touch it from
different directions. DN-16 governs a track another node fused and sent; DN-25 governs a
position an entity states about itself, which is never a track.

Six rows are marked human-owned in the table, matching the low-trust tier: fires
deconfliction, handoff, authority and control status, queue expiry, releasability, and key
custody.

## The five criteria that carry the most weight

Each fails only if somebody later takes a shortcut that looks reasonable at the time.

1. **No configuration causes an expiry to accept** (CAP-3.7). Verified by exhaustive
   search over the settings space for any path producing an acceptance with no operator.
   This is contract C-01, which is not dispensable.
2. **Removing a sensor never shrinks the gap set** (CAP-1.4). The monotonicity property
   that catches most coverage-routine bugs.
3. **No hazard contributes to a policy verdict** (CAP-2.5). A negative test that fails if
   anyone wires the hazard layer into the authority chain.
4. **The marking is checked independently of the agreement** (CAP-7.4). Proves the two
   gates are independent; a shortcut would check only the agreement.
5. **A replayed session reproduces its scheduled products** (CAP-5.8). Proves the scheduler
   runs on mission time; a wall-clock implementation passes every other check.

## What these rows do not cover

Each row verifies its own closure. None verifies that the closure was the right design,
which is the domain reviewer's job. Every note's section 1 names the mission thread step it
serves, so a wrong design shows up as a mismatch rather than as a plausible document.

None of these rows is a gate yet. A row becomes one when its test lands and its status
moves in `../architecture.md`, which is GAP-067's work.

## Traceability

[`../verification-capability-table.md`](../verification-capability-table.md) §2, where the
criteria live; section 8 of each design note, which is where each was drafted;
`../mission/measures.md` for MOE-03, MOP-27, MOP-28, MOP-38; GAP-067; principle AP-17;
contract C-17.
