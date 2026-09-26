# The late-data policy governs the tracker

GAP-114 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-98 and D-99 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
GAP-173. Human-owned: `gungnir-fusion-async` (concurrency) is where the policy is applied;
what the owner has reviewed is in [`../../signatures.md`](../../signatures.md). The
`gungnir-ingest` gateway is not changed. Built on 2026-09-26 under the owner's delegation
of that day.

## What was wrong

`gungnir_time::LateDataPolicy` had three variants and one reader: the clock-skew estimate
(GAP-008, MOP-09), which called a source out of sync "because its detections will be
dropped as late by the policy the deployment chose". Nothing dropped them by that policy.
The fusion pipeline's reorder buffer decided every late detection by a horizon of its own,
`PipelineSettings::reorder_horizon_s`, which no baseline set, so every deployment ran one
second. Two consequences followed.

- **The two disagreed.** `WallClockAuthority::default()` buffered for two seconds, the
  pipeline for one, so a source lagging between the two lost detections with no
  out-of-sync flag.
- **The bound was not a bound.** The pipeline refused only what was behind its processed
  cursor, and the cursor moves only when something newer arrives. A detection three
  seconds late was dropped in busy traffic and kept in sparse traffic.

The GAP-067 walk split the `gungnir-time` row on 2026-09-16, gated replay determinism, and
held the late-data clause on GAP-114 because no delivery could honour or break a policy
nothing read.

## What was decided

**D-98: the pipeline applies the policy, and one value in the baseline sets it.** The
choice was between the ingest gateway, which knows when a message was received, and the
fusion pipeline, which knows what has already been processed. "Late" means earlier than
data already taken, which only the pipeline can answer, and receipt time minus source time
mixes transit delay with a source's clock error, which the skew estimate already reports
on its own and which has no meaning in a replay. Re-scoping the row to "the horizon is the
policy" was rejected: it would have kept a policy type that no deployment could set and a
horizon that no governance record named.

So `LateDataPolicy` moved down to `gungnir-core`, re-exported by `gungnir-model` and from
there `gungnir-time` (§1.2 of the coding standards: one owner, re-exported upward), since
the pipeline sits below `gungnir-time` and could not name the type. No dependency edge was
added. The baseline carries it in a new `time` section, a sensing section under D-91, and
both binaries hand the same value to the pipeline and to the clock authority.

**D-99: what a baseline may choose.** `buffer-and-reorder` with a bound above zero and at
most ten seconds, or `reject`. `accept-as-is` is refused: its own documentation says replay
and testing only, and it folds a stale measurement into a current estimate at full weight.
Absent, the policy is a one-second buffer, which is what every deployment already ran, so
an upgrade changes nothing the picture does. The clock's default moved from two seconds to
one for the same reason.

## How it is built

`PipelineSettings::late_data` replaces `reorder_horizon_s`, which is now a method derived
from it. `FusionPipeline::push` applies the policy:

| Policy | Out of order, inside the bound | Beyond the bound, or behind the cursor |
|---|---|---|
| `BufferAndReorder { max_lateness_s }` | held and processed in source-time order; `reordered` | refused; `too_late` |
| `Reject` | nothing is held; refused; `too_late` | refused; `too_late` |
| `AcceptAsIs` | applied at the newest time already taken, never retrodicted; `accepted_late` | the same |

Lateness is measured in source time behind the newest detection taken, so the bound holds
whatever the traffic. A detection whose time is not a number was counted as `too_late`;
it has `not_finite` now. The four counters are on `PipelineStats`, on the wire through
`PipelineStatsView` (`serde(default)`, so an older peer's snapshot still reads), and on
PN-09 beside the clock line, which names the policy for a pipeline this console runs and
says the node's baseline sets it for one the console only reads.

Bearings do not enter the reorder buffer. Their window around the cursor is now the
reorder horizon with a one-second floor, the window they always ran under; without the
floor, `Reject`'s zero horizon would refuse every bearing not stamped at the cursor's
instant. Under `Reject` a bearing behind the cursor is refused as `BearingRefusal::Late`.

`PipelineSettings::with_late_data` refuses a bound that is not a finite positive number,
and both binaries apply it in every case: promoted algorithm baseline, refused baseline or
none.

## What the tests found on the way

Every path that puts a running desktop back on its own services (falling back from a
silent node, signing in to an outage, signing out of a link) built its tracker with
`LiveTrackingService::new` and the defaults. A desktop that rejected late data would have
started buffering it the moment it fell back, and the fallback tracker also ran the
default filter with no sensor positions, whatever algorithm baseline was in force. These
paths now call `state::embedded_tracker`, which builds the tracker the desktop starts
with, as `embedded_planner` already did for the planner (GAP-119).

The three replay suites in `gungnir-tracking-service` size their bound from the
recording's latency spread and assert nothing is refused. They still pass under the
stricter bound, so no detection in them was more than the bound behind the front.

## Tests

- `gungnir-fusion-async/tests/late_data_policy.rs`: one test per clause of GAP-114's
  action. `Reject` drops and counts; `BufferAndReorder` reorders inside the bound and
  matches the offline batch exactly, and drops beyond the bound a detection that is still
  ahead of the cursor; `AcceptAsIs` processes as delivered at the front. Also: the
  counters reach the ingest task's snapshot, a bad bound is refused, and a late bearing
  under `Reject`.
- `gungnir-config`: the default and documented shapes parse; `accept-as-is`, zero,
  negative, non-finite and over-ceiling bounds are refused; the section is sensing.
- `gungnir-app/tests/late_data_policy.rs`: the baseline's policy governs the desktop's
  tracker at start and after a fallback, the clock judges skew by the same policy, and
  PN-09's line reads the pipeline's counts.
- `gungnir-ui`: PN-09 draws the policy and its counters.
- The loom model checks (`RUSTFLAGS="--cfg loom"`, as `loom.yml` runs them) pass. The
  change adds no channel, lock or await; the snapshot carries the new counters inside
  `PipelineStats` as before.

## Left for the owner

The `gungnir-time` late-data row in `../../verification-capability-table.md` §1 still says
nothing consumes `LateDataPolicy`, which is no longer true. Its criterion and its gate are
the owner's, so the row is unchanged and GAP-173 asks for the walk. The pipeline change is
human-owned code.
