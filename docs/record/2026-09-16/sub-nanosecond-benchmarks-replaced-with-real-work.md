# Sub-nanosecond benchmarks replaced with real work

Four of the twelve benchmarks Gate 6 (`bench-regression.yml`) compares measured a
nanosecond or less. As of 2026-09-16 three measure real work, and the fourth is folded
into one of them. No pass criterion, threshold or budget changes. GAP-093 stays open.

**How it was found.** Two noise-floor measurements on 2026-09-16 each ran Gate 6 three
times with unchanged benchmark code. The first was unpinned. The second ran on the
branch `claude/gate6-pcore-pinning`, with the benchmarks restricted to the runner's
performance cores. Pinning did not narrow the spread: 8 of 12 benchmarks varied more.
The raw criterion output then showed why part of the table could never be read:

| benchmark | time | what it was |
|---|---|---|
| `ekf_predict_update` | ~0.41 ns | `black_box(0_u64)`, a placeholder from before the EKF existed |
| `phd_update_dense_swarm` | ~0.75 ns | the length of a 200-element `Vec`, a placeholder from before the PHD existed |
| `snapshot_calls/tracks` | ~0.77 ns | `tracks().len()` on a snapshot that was **empty** |
| `snapshot_calls/is_healthy` | ~1.1 ns | a field read |

A ratio of two timer readings is not a measurement, and those four produced some of
the widest swings in both tables (phd 1.07 to 3.00, `is_healthy` 0.75 to 1.59).

**What each measures now** (local timings, not baselines):

- `ekf_predict_update`: 100 predict/update cycles of a six-state constant-velocity EKF
  with a range/azimuth/elevation radar, over a Scenario 1-shaped flight (straight,
  3 deg/s turn, straight). About 17 µs.
- `phd_update_dense_swarm`: one predict/update of a GM-PHD filter settled on a
  200-target grid 50 m apart, with 20 clutter detections. `max_components` is 400
  because the default 100 would truncate the swarm on every scan. The setup refuses to
  run unless the warm-up settled near 200 targets. About 23 ms.
- `snapshot_calls/panel_read`: `is_healthy()`, then `tracks()`, reading every track's
  position and covariance, the way a panel uses them. About 43 ns over 36 tracks.

**The empty snapshot was a defect, not only noise.** The benchmark ran 300 frames and
measured immediately, but the tracking pipeline is asynchronous and had produced no
tracks yet. An assertion added with this change failed on its first run, which is how
this was found. The benchmark now waits for tracks the way
`snapshot_calls_meet_their_budget` in `gungnir-app/tests/frame_budgets.rs` already did.
That test, which asserts the 1 ms budget for each call separately, is unchanged.

**Why fixed arithmetic and not `gungnir-scenario`.** `benches/README.md` asks for
scenario-generated inputs. `gungnir-scenario` depends on `gungnir-filters` and
`gungnir-rfs` through `gungnir-fusion-async`, so using it would add a dev edge back into
the crates being measured. The inputs follow the Hungarian benchmark's precedent:
deterministic, identical every run, and shaped after the named scenario. The README now
says so.

**What this does not settle.** The first comparison after this merges has no baseline
for `panel_read` and new work under two old names, so its ratios for those three mean
nothing. The noise floor needs measuring again on the benchmarks that remain. The eight
that already measured real work also swung by up to 1.9 in ratio, so this is necessary
and not sufficient.
