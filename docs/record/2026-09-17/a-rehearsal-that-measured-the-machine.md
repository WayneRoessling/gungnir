# A rehearsal that measured the machine

GAP-136 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
found by CI in the middle of DN-31's build series, in code neither that series nor the
commit under test had touched.

## What happened

The "fmt, clippy, test" check failed on a commit whose whole diff was two files under
`docs/`. The failing assertion was
`gungnir-app/tests/laydown_rehearsal.rs`'s: TT-01's fixture declares twelve entities and
408 detections, and the run had formed **no tracks at all**. The same content passed
locally, passed on the branch before it, and passed in a full workspace run on another
machine, and the merge commit's own run on `main` passed afterwards. Everything about it
said "flake", which is the word that ends an investigation early.

## What it actually was

A rehearsal replays a committed fixture through a throwaway desktop: it advances a replay
clock one frame at a time and ticks, then reads `state.tracking.tracks()` and reports the
count. The fusion pipeline is not in that loop. It runs on its own task
(`gungnir_fusion_async::ingest_with`), and a replay's 14,400 frames cost about a second of
wall clock, so at the end of the loop the task has whatever it has: on an idle machine most
of the run, on a CI runner with every core busy, possibly nothing. The number a rehearsal
reported was therefore partly a measurement of how busy the machine was -- offered to an
operator as a property of the laydown.

This was not unknown. The test file itself recorded that five runs of one input gave
`[4, 5, 4, 4, 4]` tracks under `cargo test --workspace`'s parallelism and agreed exactly
under `--test-threads=1`, and put it down to scheduling variance inside the pipeline. The
measurement was right and the diagnosis was wrong: the variance was the reader, not the
pipeline. `> 0` was chosen as an assertion weak enough to survive it, and a weak assertion
against a misdiagnosed cause is how a defect keeps its cover for a year.

Underneath it sat a second, quieter loss. A pipeline holds its last reorder horizon until
the stream ends -- "the stream's end is a flush, not a truncation" -- so the tail of every
rehearsal was never processed at all, on any machine, however idle. Waiting for the
counters to stop moving, the first fix tried, found that out by never settling: 406 of 408
detections taken, and the last two waiting for a later detection that the fixture does not
contain.

## What changed

- **`TrackingService` has a `finish`**, defaulted to nothing. The desktop holds its tracker
  as a `Box<dyn TrackingService>` and so could not reach `LiveTrackingService`'s inherent
  `finish` at all; a backend with no pipeline behind it has nothing to flush, which is why
  the default does nothing rather than being required of everyone. That inherent method's
  own documentation had drifted onto the function below it and is back where it belongs.
- **A rehearsal ends its stream and waits for the flush** before it reads anything. The
  flush is both the point at which the last horizon is finally processed and the one
  unambiguous signal that everything before it already was. A pause says nothing, and the
  old code read one as if it did.
- **It reports nothing rather than a low number** when the flush does not arrive:
  `RehearsalError::DidNotSettle`, carrying how many detections the pipeline had taken of
  how many it was fed, after a bound of about ten seconds. The bound is a deadlock guard,
  not a performance assertion -- `gungnir-tracking-service/tests/sample_set_replay.rs`'s
  reasoning, and its shape.
- **The outage tee forwards what it wraps.** `TeeTracking`, the wrapper that tees
  detections to the node link during a fallback, took the trait's defaults for
  `bearing_rays` and `pipeline_stats`. For the length of every outage PN-02 drew no bearing
  ray and PN-09 counted zero while the embedded pipeline behind the wrapper produced both.
  Found by adding `finish` to the trait and asking which other defaults this wrapper was
  silently taking.

## What it is now

Deterministic, by construction rather than by luck: the fixture's detections arrive in one
order, epoch boundaries follow from their source times rather than from when the task got
to them, and the run reads the state after the flush. Nine runs against a machine loaded
to saturation and three against an idle one all reported five tracks.
`two_runs_of_one_fixture_report_the_same_thing` pins that, and
`the_outage_tee_reports_what_the_tracker_under_it_reports` pins the wrapper. The workspace
is 1964 tests, none failing.

## Considered and not done

**The desktop still does not end its stream when a session closes.** `on_exit` saves and
closes the session (D-04, GAP-051); the tracker is dropped with everything else, and the
last horizon is not fused into the picture. Ending the stream there would mean flushing,
polling for the final snapshot and journaling it, on the way out of the process, and
nothing reads that picture afterwards -- the detections themselves are already journaled,
so a replay reconstructs the tracks the live picture never showed. It is written down here
rather than filed, because "the desktop keeps this service for the life of a session" is a
decision `LiveTrackingService::finish` already states, and undoing it deserves more than a
line in a fix for something else.

## The process note

The check that failed was read after the merge, not before it. `main` was green in the end,
by luck rather than by care: had the failure been real, it would have been merged.
