# The owner's walk of 2026-09-26

Four rows of [`../../verification-capability-table.md`](../../verification-capability-table.md)
whose criterion cells had gone stale while the build moved on. Each was filed as an owner
gap because a criterion cell is the owner's to change. The owner decided all four on
2026-09-26, and the wording below is what the owner approved.

| Gap | Row | What the owner decided |
|---|---|---|
| GAP-154 | Cross-layer, Disconnected reconciliation (already gated) | Replace "No build puts a decision on a node's record yet ... (GAP-129)" with what now holds: a node records every decision taken on its queue and every decision forwarded after an outage, and an outage's conflicts are caught by track (D-58) and, for journals written before UUID v7, by plan (D-53). Criterion and gate unchanged. |
| GAP-159 | `gungnir-analytics`, Coverage accuracy | **Gated** against `gungnir-analytics/tests/coverage_accuracy.rs`. The stale "no test compares a computed volume ..." is replaced. |
| GAP-168 | `gungnir-intercept-service`, Plan determinism and degradation | **Criterion amended**: an over-budget solve returns the last good plan, flagged stale, until the stand-in wait (500 ms) expires, then an interim plan labelled as not optimal, replaced when the exact solve finishes (D-93). Not gated by this change. |
| GAP-173 | `gungnir-time`, Late-data policy | **Gated** against `gungnir-fusion-async/tests/late_data_policy.rs` and `gungnir-app/tests/late_data_policy.rs`. The stale "nothing consumes `LateDataPolicy`" is replaced. |

## Evidence the gates rest on

Run on main at the walk, before the edit:

- **`coverage_accuracy.rs`: 4 passed.** It recovers each fixture volume's stated range
  with rays sampled at 1 percent of the range and holds it to `RANGE_TOLERANCE_FRACTION =
  0.01`. It recovers each sector edge and elevation floor and ceiling with finer sampling
  and holds them to `ANGLE_TOLERANCE_DEG = 0.1`. Elevation is measured against the
  vertical at each sensor (GAP-158); the worst elevation error measured was 0.055 degree.
  These are the row's own agreed values (D-16).
- **`gungnir-fusion-async/tests/late_data_policy.rs`: 7 passed.** One test per variant:
  `Reject` drops and counts, `BufferAndReorder` reorders inside the bound and drops and
  counts beyond it, `AcceptAsIs` processes as delivered. Also a refused bound and bearings.
- **`gungnir-app/tests/late_data_policy.rs`: 3 passed.** The baseline's `time.late_data`
  reaches the desktop's tracker, with no policy named it defaults to a one-second buffer,
  and the tracker a desktop falls back to runs the baseline's policy.

The ledger entries for these criterion changes follow once this is on main, each at the
main commit that holds it.
