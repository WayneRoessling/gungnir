# GAP-015: the dense group off the executor, and live on a linked desktop

The previous record item of the same day
(`gap-015-the-dense-group-on-the-picture.md`) left GAP-015 with two items, both of which
the owner asked to have completed on 2026-10-26:

- the `spawn_blocking` plumbing, which coding standard §2.2 asks for before the mode could
  be on by default;
- a live update for a desktop linked to a node, which until then saw a new group only at
  its next snapshot.

## Off the executor thread

The standard is explicit. A PHD update over a large set "must go through
`tokio::task::spawn_blocking`, not run inline on the async executor thread". An engaged
epoch at the specified raid size was measured at hundreds of milliseconds, and
`ingest_with` ran every epoch inline.

- **With the mode configured,** `ingest_with` now hands each submission to the blocking
  pool, together with whatever else is already waiting, up to `OFF_EXECUTOR_BATCH` (256).
  The pipeline goes with the batch and comes back with the result, and the end-of-stream
  flush goes the same way. The executor thread only polls the channel and awaits. The batch
  limit means a feed that never pauses still lets the ingest future be polled, and dropped,
  between batches.
- **With the mode off,** the loop is exactly what it was. That is every deployment that
  has not asked for the mode, and every loom model. A hop to the blocking pool per
  detection would buy nothing for a pipeline whose per-epoch work is a few Kalman updates.
  This keeps the concurrency change confined to the deployments that need it.
- **The loom shim.** The hand-off goes through `crate::sync::off_executor`, beside the
  channel and the idle back-off. Under `--cfg loom` it runs the work in place, because a
  loom execution has no blocking pool. No model reaches it, since every model runs with the
  mode off.
- **Cancellation safety** is restated in `ingest_with`'s doc comment. While a batch runs
  the pipeline is not in the future. Dropping the future there lets the batch finish, with
  each submission applied whole and each snapshot sent whole, and then drops the pipeline
  with the result. That is the same guarantee the inline loop gives.

**How it was proved, without timing anything.** A test-only record of which threads ran a
dense-group step lets `the_dense_group_step_runs_off_the_executor_thread` check where the
work went. It runs `ingest_with` with the mode on a current-thread runtime, whose executor
is the test's own thread, and asserts that no step was recorded on that thread. Its control
runs the same pipeline in lock-step on the test thread and finds the step recorded there.
Disabling the hand-off by hand made the test fail with "a dense-group step ran on the
thread driving the executor", and it passed again once restored.

**The loom gate was run locally,** as the workflow runs it: `RUSTFLAGS=--cfg loom`,
`cargo clippy -p gungnir-fusion-async --release --lib --tests`, then
`cargo test ... --lib -- loom_`. All four models pass, including the negative check that
must find an interleaving.

**What this does not change: the mode is still off by default.** The plumbing that had to
come first is built. Whether the default should now be on is a decision about cost, and it
is the owner's. Off the executor, that cost is no longer a stalled runtime but a picture
about an epoch's work late during a raid. `PipelineSettings::dense_group` and
`docs/performance-budgets.md` now say so.

## Live on a linked desktop

`TrackingEvent` gains `DenseGroup(Option<DenseGroupView>)`, added under the API's
compatibility rule ("Enum variants may be added"), as `InterceptEvent::PlanStanding` was.

- **The node.** Its `Announcer` publishes the group whenever it differs from the one last
  published, and publishes `None` when it is released. A tick on which nothing changed
  publishes nothing, so the record says when a raid outgrew the tracker and when it
  stopped, without a copy of the same group on every tick between.
- **The link.** It applies the event to its projection, so a connected desktop's group
  follows the node's live, as its tracks do.
- **Partners.** A partner's stream already carried only what `NodeApi::releases` names, so
  the event stays home without a code change. The doc comment and a test now say so.
- **Other readers.** The desktop's cross-session identity fold ignores the event, because
  it names no track. The report counts it nowhere, through the "not this watch's work" arm
  that already covers the planner's standing.

**Tests.**
- The announcer publishes once per change and once for the release, and nothing on a tick
  with no change.
- A desktop connected over the real v3 transport receives a group announced after it
  connected, then the next epoch's group in its place, then the release, with no new
  snapshot.
- A party's stream carries a released track and never the group.
- The model's every-variant round trip covers the new variant, `Some` and `None`.

`gungnir-fusion-async` is human-owned twice over: concurrency, and the numerical
stability clause over what it drives. Its standing is in `../../signatures.md`.
