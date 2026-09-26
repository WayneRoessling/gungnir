# A rehearsal decides and tracks under the deployment's baseline

GAP-182, GAP-183 and GAP-184 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-113 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
[`../../design/DN-02-prediction-and-approach.md`](../../design/DN-02-prediction-and-approach.md)
§9 and [`../../design/DN-32-re-observation-for-a-laydown.md`](../../design/DN-32-re-observation-for-a-laydown.md)
§14. Built on 2026-09-26 under the owner's delegation of that day. It follows
[`first-engagement-range-over-a-rehearsal.md`](first-engagement-range-over-a-rehearsal.md),
whose "not changed" section named the shortfall this item closes.

## What was wrong

A rehearsal's throwaway desktop was built from `ConfigBaseline::default()` with the
laydown's sensors and resources placed in it. Every other field was a default, and the
defaults are the strictest reading (DN-08):

- **Policy.** Every layer was at hold, with no authority rule, no decision expiry or
  escalation, and no geofence. The chain therefore denied every plan a rehearsal
  proposed, whatever the deployment's weapons control status. PN-16's "decisions raised
  (expired)" read 0 (0) for every laydown and every recording. That reads as a queue
  that never saturated, when it was a queue that never received anything.
- **First engagement (GAP-020, merged the same day).** Predictions were taken from plans
  the planner *proposed*, so the policy never touched them. A deployment at hold was
  shown an engagement range it would never offer anyone. A no-go fence was ignored, so an
  intercept point inside it counted.
- **Tracker.** The pipeline ran the default algorithm baseline and the default one-second
  late-data buffer, not the deployment's promoted baseline and its late-data policy.

## What was decided (D-113)

The throwaway desktop takes from the deployment's baseline everything that decides or
tracks: the whole `policy` (control status, authority, decision timeouts,
identification, staleness, delegation, fires), the allocation horizon, and the tracker
configuration (algorithm candidates, mission and tracking profiles, the active profile,
and `time.late_data`). Geofences are re-expressed about the recording's origin, the
arrangement DN-32 §5.5 already reads a laydown as. A deployment with geofences and no
origin is refused by name.

It takes nothing that reaches outside the process: endpoints, a node, peers, feeds,
exchange partners, accounts, and resources' handoff endpoints. It also does not take the
validity window, which is on the live mission clock.

**A first engagement counts only from a plan the policy offered for decision** (queued),
which amends D-107's "proposed". Plans that were proposed and not offered are counted,
with the denial reasons the chain recorded, in PN-06's own words. When nothing was
offered, each approach reads "not computable" and says so.

Rejected:

- *Keeping proposals and marking denied ones.* That draws an engagement the deployment
  forbids, beside a caption nobody reads.
- *Cloning the whole baseline and blanking the dangerous fields.* A field added later
  would reach a rehearsal by default, and some of those reach outside the process.
- *Carrying the live desktop's runtime state* (delegations, a signed-in role). A laydown
  is judged under the baseline, not under whoever happens to be at the console.

## What it showed at once (GAP-183, filed, not built)

Rehearsed under its own policy, round 1 offers nothing. Its area layer is at hold, and
the planner tasks the area battery in every plan: 292 of 296 plans were denied for
control status and 4 for the upper Vell no-go fence. DN-09 denies a plan if any solution
is denied, so the point layer's weapons-free engagements are never offered either. That
holds on the live desktop as much as in a rehearsal. Whether the planner withholds
resources at hold, or the chain offers the permitted part of a plan, is a decision about
DN-09 and DN-04, not a rehearsal fix, so it is filed as GAP-183.

## Tests

`gungnir-app/tests/laydown_rehearsal.rs`:

- `a_deployment_at_hold_rehearses_to_no_engagement_and_says_why`: TT-01 at point-hold
  forms the same tracks as at point-free, offers nothing, raises nothing, and reads not
  computable naming the control status.
- `round_1s_forward_radar_changes_its_own_detections_and_nothing_else`: round 1 under
  its own policy offers nothing (area hold, the no-go fence). A weapons-free variant
  keeps GAP-020's comparison: `c` first engages 3.0 km farther out.

Unit test: `a_rehearsal_decides_and_tracks_under_the_deployments_own_baseline`. PN-16's
drawing of the plans not offered is tested in `gungnir-ui/src/panels/rendered.rs`.
