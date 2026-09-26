# First-engagement range over a rehearsal

GAP-020, GAP-045 and GAP-087 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-45, D-107, D-108 and D-109 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
[`../../design/DN-02-prediction-and-approach.md`](../../design/DN-02-prediction-and-approach.md)
§9 and [`../../design/DN-32-re-observation-for-a-laydown.md`](../../design/DN-32-re-observation-for-a-laydown.md)
§13. Human-owned: `gungnir-fusion-async` (concurrency), whose ingest loop body this change
extracts into a function; what the owner has reviewed is in
[`../../signatures.md`](../../signatures.md). Built on 2026-09-26 under the owner's
delegation of that day.

## What was missing

DN-02 §7 gave PN-16's approach corridors one line: "the aggregate of predictions over a
rehearsal". D-45 settled the aggregate on 2026-09-15 -- the worst case, the minimum over
the run, labelled and carrying its count -- and left open what the predictions are, which
approach one belongs to, and what its range is measured from. Three entries stopped at it:
GAP-020 (whose predictor half was built), GAP-045 and GAP-087.

## What was decided, and why

**D-107: a prediction is one recorded target's first engagement.** The planner proposes
plans during a rehearsal, and every pairing carries DN-04 §9's earliest constant-velocity
intercept point: the track taken forward on DN-02's constant-velocity prediction to the
moment the effector reaches it. That is the prediction the note aggregates, and the first
one for each target is the first engagement. Three alternatives were rejected.

- *Every pairing in every plan.* A target re-planned every second would be counted every
  second, and n would be a count of replans, not of threats.
- *Every track's first pairing.* This was built first and measured: round 1's two
  short-range radars form tracks from their own false alarms beside the harbour, the
  planner pairs them, and the worst case over them was 106 m on every laydown, a figure
  about false alarms that no laydown changes. Confirmed tracks alone did no better (64
  confirmed clutter tracks, worst case 30 m) and dropped TT-01 entirely, whose single
  long-range radar confirms nothing at 110 km.
- *Truth association by fixed distance.* No one distance suits both a 20 m track beside
  a harbour and a track at 110 km.

What was taken is the 0.999 gate of the track's own position covariance about where the
recording's truth says a target was. A rehearsal is the one place the system holds truth,
and scoring against it is how `scenario_truth_replay.rs` already judges the tracker.
Clutter paired is counted and shown on PN-16, so its absence from the figure is stated.

**The range is ground distance from the predicted intercept point to the approach's inner
end**, its last declared point (D-107). Alternatives: distance to the nearest defended
asset (approaches name no asset, and a rehearsal's throwaway desktop declares none);
along-axis distance (hides a lateral miss); slant range (altitude noise on a single-radar
track moved intercept points hundreds of kilometres vertically in TT-01). `ApproachConfig`
now states the order its points are in, outer end first.

**D-108: an approach declares a corridor half-width, and without one takes no
prediction.** The alternatives were a baseline-wide default width, or assigning every
target to its nearest axis. A default is a mission-analysis answer nobody gave, and nearest
axis puts a raid down an undeclared axis into a declared one's figure. The target is
placed by where the recording says it was, not where its track said: round 1's first track
of drone R1-001 was a tentative one six kilometres off the axis with a 1.8 km sigma, which
the covariance gate rightly accepts as the drone and a track-position test threw off the
approach.

**D-109: a rehearsal runs in lock-step.** The first end-to-end test found two runs of one
laydown proposing different plans. The throwaway desktop's tracker was a task on another
thread, and each tick planned against however far it had got. The existing determinism
test passed only because its resource never had a plan queued, and the decision counts a
rehearsal already reported were a property of scheduling too. Alternatives: waiting on the
channel to empty (the task can hold the last detection mid-update while the channel reads
empty), driving a current-thread runtime with paused time (tokio's `test-util` in a
production binary, and a dependence on the loop yielding only when idle), and copying the
loop body into the tracking service (two drivers that can drift). What was taken extracts
`ingest_with`'s body into `gungnir_fusion_async::step` and `end_of_stream`, which the live
loop now calls, and adds `LiveTrackingService::lockstep`, which calls them on the caller's
thread. The planner measures its budget on `SteppedClock::new(Duration::ZERO)`, so no solve
is cut short by the machine. The live desktop and node are unchanged.

## What the tests show

- TT-01's committed sample set, a ridge radar and one effector: moving the effector 72 km
  up the eastern axis moves the worst case from 116 km to 130 km (n = 2), and the sea
  approach, down which the radar sees nothing, reads not computable on both rows.
- Round 1's committed laydowns against the test-written raid down the upper Vell approach:
  `current` 10.2 km, `b` the same prediction (it moves a battery, and the worst track had
  no velocity yet, so its intercept point is where it stood), `c` 13.2 km, 3.0 km farther
  out, all n = 3; clutter is counted on every row.
- Round 1's current laydown against the committed TT-01 set: no recorded target comes in
  reach, the planner pairs only the radars' clutter, which is counted, and the upper Vell
  column reads not computable rather than a range of a few metres.
- All ten sample sets through the lock-step service match the offline batch to 1e-6, and
  two lock-step runs agree at every poll. The loom model checks pass unchanged.

## What is not changed

No verification row: D-45 and DN-02 §7 specify none. The rehearsal still re-observes with
the throwaway desktop's default policy, not the deployment's. Round 1's committed
recordings still bring no target within reach of its radars (GAP-147), so a round-1
session rehearsing them reads the column as not computable; the US-15 session card does
not mention the column, and changing the card is the round's, not this change's.
