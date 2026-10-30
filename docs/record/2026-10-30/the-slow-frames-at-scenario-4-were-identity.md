# The slow frames at Scenario 4 were identity resolution

`the-per-frame-gate-timed-an-empty-picture.md` measured `update()` at Scenario 4's track
count, p99 4.33 to 5.30 ms release against the 4 ms budget, and left open what in the tick
cost that much. This item answers it and fixes the stage that did.

## How it was measured

A scratch probe, not committed: a lap after every step of `gungnir_app::update::tick`
(51 of them, plus the planner call alone), and a test that drove Scenario 4 (seed 4,
`DenseSwarm { target_count: 200 }`) through `drive_to_scenario_4_counts`, then timed 3 000
frames twice: with 1 ms between frames, as the per-frame gate does, and back to back.
Release profile. The slow set is every frame at or above the p99, 31 frames of 3 000.

Other sessions were compiling on this machine throughout, and its 8 P-cores and 12 E-cores
already part timings by about two times, so the absolute figures run higher than the
earlier item's. The before and after figures below were taken interleaved, one binary and
then the other, three rounds, so they share whatever the machine was doing.

## Where the slow frames went

Before the fix, 1 ms between frames, one run (the other runs agree to within 15%):

| Stage | Mean, all frames | Mean in the slow 1% |
|---|---|---|
| `identity::tick` | 503 µs | **11.07 ms** |
| `tracking.poll` | 36 µs | 222 µs |
| `journal_pending` | 21 µs | 175 µs |
| `engagements::sweep` | 11 µs | 26 µs |
| `decisions::sweep` | 7 µs | 8 µs |
| `plan_and_queue` | 5 µs | 5 µs |
| `anomaly::tick`, `warnings::tick` | under 1 µs | under 5 µs |

**Every frame in the slow set was one where a fresh pipeline snapshot had landed**, and in
every one `identity::tick` was all but the whole of it. Of the other candidates:

- `plan_and_queue` is 5 µs, the planner call inside it 2 µs. This fixture's baseline names
  no resource, so the planner has nothing to solve; the question
  `../../performance-budgets.md` asks of the DP solve is not exercised by this scenario.
  At 340 tracks the exact solver would refuse the picture as too large and a one-step
  answer would stand in (GAP-156); that path was not timed here.
- `warnings::tick` returns at once: no defended asset in this baseline carries a warning.
- `tracking.poll` and the journal drain are a fraction of a millisecond each.

## Why identity resolution cost that much

`identity::tick` resolves every live track on every frame. A track id the resolver has not
seen is compared with **every lineage it has ever held**
(`InMemoryIdentityResolver::best_match`), to find out whether it is an entity seen before
(GAP-019). The dense swarm's false tracks churn: in the slow frames a fresh snapshot brought
48 to 158 new track ids, and every one of them that matched nothing minted a lineage, about
60 a second of mission time, from 1 500 to 6 800 across the timed window. Each comparison ran `similarity` in full,
`exp` included, over a lineage a few hundred bytes from the next in a `HashMap`. The worst
frame was 76 new ids against 4 109 lineages: 43 ms.

## What changed

- **`gungnir-identity` keeps each lineage's last state packed beside the lineages**, and
  compares a new track with those, through a necessary condition first
  (`similarity::may_match`): the same gap test, then each axis's share of the distance
  against the most a match above the merge threshold can have, by multiplication alone.
  Every pair it passes is still decided by `similarity`, and a value it cannot judge is
  passed rather than refused. So the answer is the one a comparison with every lineage
  gives, and a test holds it to that: 6 000 tracks under three thresholds, through mints,
  correlations, re-sightings and merges, against the comparison it replaced.
- **`identity::tick` borrows the picture instead of copying it.** The copy was some 340
  track views a frame, each with its provenance's allocations, and spiked to 3 ms in a few
  frames on its own.

The node resolves through the same resolver (`gungnir-node/src/entities.rs`) and has the
same scan, so it has the same gain, though it resolves each track once rather than every
frame.

## After

Interleaved with the build before the fix, three rounds each:

| | Before | After |
|---|---|---|
| p99, 1 ms between frames | 8.70 to 9.52 ms | 1.70 to 2.09 ms |
| p99, back to back | 7.79 to 8.50 ms | 1.02 to 1.04 ms |
| Mean, 1 ms between frames | 649 to 676 µs | 374 to 402 µs |
| `identity::tick`, mean in the slow 1%, 1 ms between frames | 10.6 to 11.7 ms | 1.59 to 1.98 ms |

After the fix the slow 1% is spread over `identity::tick` (about 2 ms), the journal drain
(0.1 to 0.6 ms) and `tracking.poll` (0.1 to 0.3 ms). Two runs after the fix also had one
frame of 4.1 to 4.4 ms in the journal's `sync_if_due`, the D-04 fsync, which runs on the
frame thread; it set those runs' worst frame, not their p99.

## For the owner

1. **The 4 ms criterion is unchanged and was not touched.** At Scenario 4's count the
   frame now measures p99 under it in every run on this machine, by about two times. The
   earlier item's question stands: whether the per-frame budget should hold at Scenario 4
   as well as at Scenario 3.
2. **Identity resolution still grows with the session.** The change makes each comparison
   cheap; it does not make them fewer. Every new track id is still compared with every
   lineage the session has minted, and this raid mints about 60 a second, so an hour of it
   is some 200 000 lineages and frames that cost tens of milliseconds again. The remedies
   all change what the product does, so none is taken here: resolve only confirmed tracks
   (the tentative churn is most of the new ids and most of the lineages); resolve off the
   frame thread, as the tracker runs; or stop comparing against lineages last seen longer
   ago than `max_gap_s`, which `similarity` already refuses, and which is safe to drop for
   good only if mission time never runs backwards inside one resolver. Whether the fold at
   start guarantees that was not established.
3. **The journal fsync runs on the frame thread.** 4.1 to 4.4 ms when it landed here. It is under
   the p99 today because it is rare; a deployment that fsyncs more often would move it.
