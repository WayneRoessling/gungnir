# Benchmarks

Criterion benchmarks live inside the crate they measure, under `<crate>/benches/`,
one file per hot path, named after the capability-table row it measures
(`docs/agentic-coding-standards.md` §2.6) so `bench-regression.yml` output maps
straight back to `docs/verification-capability-table.md`.

| Benchmark group | Crate | Row | State |
|---|---|---|---|
| `ekf_predict_update` | `gungnir-filters` | Extended Kalman Filter (EKF) | **Real, 2026-09-16**: 100 predict/update cycles of a Scenario 1-shaped flight, range/azimuth/elevation, in 17 µs. It timed `black_box(0)` until then |
| `hungarian_solve_100x100` | `gungnir-association` | Hungarian / Jonker-Volgenant | **Real, 2026-09-05**: 100x100 in 309 µs, 100x400 in 105 µs, 200x200 in 1.34 ms |
| `phd_update_dense_swarm` | `gungnir-rfs` | PHD / CPHD filter | **Real, 2026-09-16**: one predict/update of a settled 200-target swarm with 20 clutter detections, in 23 ms. It timed a folded `Vec` length until then |
| `tick`, `snapshot_calls`, `journal_append_50`, `startup` | `gungnir-app` | The desktop rows of `docs/performance-budgets.md` | **Real, 2026-09-05**: fed from `gungnir-scenario` output through the ingest gateway `snapshot_calls` became one `panel_read` benchmark on 2026-09-16, walking the populated snapshot; before that it timed two field reads of an empty one |

The `gungnir-app` groups are the tick harness `docs/performance-budgets.md` calls for
(GAP-056). They are named after the budgets rather than after a §1 capability row,
because the budgets are what they measure; the rule below about naming a group after a
capability-table row still holds for the tracking-core groups. What the numbers currently
mean, and which of them is not yet the budgeted quantity, is in the harness's own module
documentation and in `gungnir-app/tests/frame_budgets.rs`.

Run everything with:

```bash
cargo bench --workspace
```

Rules:

- Inputs come from `gungnir-scenario` output at realistic scale, not hand-picked
  toy inputs. The generator exists as of 2026-09-05 and the `gungnir-app` groups use
  it. The tracking-core groups build fixed inputs shaped after a named scenario
  instead, because `gungnir-scenario` depends on the crates they measure; none is a
  placeholder since 2026-09-16.
- A benchmark that measures a nanosecond or less is measuring the timer. Gate 6 cannot
  read a percentage change in one, so time the work a caller actually does.
- No benchmark asserts pass/fail on absolute numbers. The separate CI regression
  check compares against the previous release's baseline (`bench-regression.yml`).
- Add a new group only for a capability-table row, and name it after that row.
