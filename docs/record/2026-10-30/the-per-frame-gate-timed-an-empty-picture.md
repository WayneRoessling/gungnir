# The per-frame gate timed an empty picture

GAP-226 found that the egui and snapshot gates in `gungnir-app/tests/frame_budgets.rs`
stopped waiting at the first track (`gap-226-the-track-table-draws-only-the.md`). This item
asks the same question of the third gate in that file, `per_frame_update_meets_its_budget`,
which asserts the per-frame `update()` p99 under 4 ms in release.

## What the budget is stated against

`../../performance-budgets.md` states it as "Per-frame `update()` time (ingest + poll + plan +
journal), p99 under 4 ms", exercised by **Scenario 3** (three sensors, mismatched rates). It
has no row in `../../verification-capability-table.md`. The test replays Scenario 3, seed 7,
so the scenario was right; the egui and snapshot budgets beside it name Scenario 4, and this
one does not.

## What the test was timing

It ran 3 000 frames from the replay's start back to back: 100 s of mission time in about
10 ms of wall time. The tracking pipeline is asynchronous and delivered nothing inside that
window, so **every timed frame held no track**. The poll and the plan were timed against an
empty picture. The figure the owner read at gating, p99 16 microseconds, was measured that
way.

## The release figures

Development machine, release profile, three runs each unless stated.

| What was timed | Tracks in the picture | `update()` p99 |
|---|---|---|
| Scenario 3, as the test took it | 0 in every frame | 8.4 to 9.1 µs |
| Scenario 3, convoy in the picture, frames back to back | 4 to 10 | 13.5 to 15.9 µs |
| Scenario 3, convoy in the picture, 1 ms between frames (a probe) | 4 to 11 | 70.7 to 80.7 µs |
| Scenario 3, the changed gate itself, all of `frame_budgets.rs` running | 4 to 10 | 84.7 to 103.1 µs |
| Scenario 4, the first 100 s from the start | 0 rising to 345 | 1.04 to 1.67 ms |
| Scenario 4 at 200 or more tracks, back to back | 216 to 349 | 4.33 to 4.50 ms |
| Scenario 4 at 200 or more tracks, 1 ms between frames | 216 to 349 | 4.89 to 5.30 ms |

The gap between frames matters. Back to back, the pipeline has no wall time to deliver, so
the picture is frozen and almost no frame swaps in a fresh output or re-plans. The desktop
leaves it 33 ms a frame.

## What changed

The test now drives Scenario 3 until the picture holds the convoy's four vehicles, through
`drive_to_track_count` (which `drive_to_scenario_4_counts` now calls). It then leaves 1 ms
between timed frames, and it fails if any timed frame holds no track. The scenario, the
3 000 frames, the release-only assertion and the 4 ms criterion are unchanged.

## For the owner

1. **At Scenario 4's count, `update()` measures p99 4.3 to 5.3 ms**, over 4 ms in every run
   on this machine. The budget is stated against Scenario 3, so no gate fails. But the
   egui row's note in `performance-budgets.md` splits a 60 fps frame at Scenario 4 between
   the egui pass, the viewport and `update()`, and the 3-D frame-rate budget is stated at
   Scenario 4. Whether this budget should also hold at Scenario 4's count is the owner's
   decision. If it should, the frame is not inside it today, and what in the tick costs
   that much at 300 tracks has not been looked into.
2. **`performance-budgets.md`'s "What has been measured" table is stale on this row.** Its
   "Gate?" column says **No** and explains why it stays ungated, but the row has been gated
   since 2026-09-15 (`../2026-09-15/132.md`). Its 22.3 µs figure, measured 2026-09-07, was
   taken on the same empty picture. A correction would say: gated 2026-09-15; the figures
   up to 2026-10-30 were measured with no track in the picture; p99 85 to 103 µs over 4 to
   10 tracks since. This item proposes that edit and does not make it.
