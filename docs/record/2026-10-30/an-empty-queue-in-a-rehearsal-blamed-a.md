# An empty queue in a rehearsal blamed a tracker that is not there

GAP-250. The owner's screenshots of 2026-10-30, taken while looking at the three-d scene
in a round-1 rehearsal (`the-owner-s-look-at-the-three-d.md`), showed nine tracks, plan 1206
proposed and plan 1183 decided. Under them PN-06 read: "Empty because no plan has been
produced to decide on: the tracking pipeline is not running, so there are no tracks to plan
against".

**Why it said so.** `decisions::queue_empty_reason` checked the tracking health flag first.
A rehearsal's tracking service is `RehearsalPicture`, whose flag is false on purpose: the
picture is a script, not a tracker, and the rule is that a health flag never claims a
subsystem works when it does not. The flag was honest. The sentence built on it was not,
because the cause it named (no tracks) was false on screen.

**What changed.** The flag is untouched. Each place that reads it to explain something was
decided on its own:

- `queue_empty_reason`: during a rehearsal the flag is not a cause. The question is whether
  the seed has put a track up yet, and if it has, the reason comes from the checks after
  it (the planner's health, whether it has produced a plan, the denial history).
- PN-07's degraded conditions (`workspace::degraded_conditions`): a rehearsal's picture
  *is* being updated, by its seed, so "the picture is not being updated" was false too.
  The condition stays, under the same subsystem key, so a decision still acknowledges it.
  It now says the picture is a rehearsal's script and no sensor is updating it.
- The status strip's tracking dot and PN-09's indicator are left as they are. They state
  that the tracking pipeline is not running, which is true in a rehearsal, and explain no
  absence. PN-09 shows the rehearsal beside it.
- `laydown_rehearsal.rs` reads the flag as the pipeline's end-of-stream signal, not as an
  explanation.
- The node's copy of the condition (`gungnir-node/src/approval.rs`) is unaffected, since a
  rehearsal runs only on the embedded backend.

**The second false sentence.** With the tracking check out of the way, the screenshot's state
fell through to the denial history. Plan 1181 is refused on readiness by design, so the
count was above zero, and the queue would have said that policy denied every plan and
nothing reached a human. That was false as well: 1183 had reached a person and been
decided. `AllDenied` now also requires that no plan has reached the queue, read from the
approval workflow's decision history, which holds every decision and expiry. This applies
to the live desktop as well. Any session where one plan was denied and a later one was
decided showed the same sentence.

**The test.** `gungnir-app/tests/rehearsal.rs`,
`a_rehearsals_empty_queue_names_neither_missing_tracks_nor_universal_denial`. It installs the
round-1 seed, runs to 76 seconds with all nine tracks up, rejects every queued item as a
participant would, and asserts the empty queue's sentence names neither missing tracks nor
the tracking pipeline, and is not `AllDenied`.
