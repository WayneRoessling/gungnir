# Row 7's MOP-07 is measured until the row is gated

A change to how one Draft criterion is exercised, not to the criterion: DN-31 §9 row 7's
text in [`../../verification-capability-table.md`](../../verification-capability-table.md)
is untouched and still says "under 500 ms (MOP-07)".

## What happened

`gungnir-app/tests/desktop_projection.rs`, written for row 7 in GAP-133, measured MOP-07 --
a plan proposed on the node to an approval control available on a desktop -- over tracing
spans, and asserted it under 500 ms. It measured 3.4 to 5.1 ms locally and passed on the CI
runs for #140, #143 and `main`. On the run for #146, a change that touches nothing on that
path, it measured **647.1 ms** and failed.

Nothing on the measured path waits on purpose. A `Queued` frame makes the link take the
node's queue picture at once, and the test's node publishes over an in-process bus with no
journal behind it, so no disk sync is involved. What varied is scheduling: a node thread,
the node's HTTP and WebSocket server, two desktops each with their own runtime, and a
picture fetch, all on a four-core runner that nextest was sharing with every other test
binary in the workspace. A bound checked there measures the runner. It is the defect the
laydown rehearsal had earlier the same day (GAP-136), in a different place.

## The change, and the convention it follows

`gungnir-app/tests/frame_budgets.rs` already records how this workspace handles a timing
budget that is not yet a gate: **promoting a Draft row to a gate is the owner's confirmation
under D-16 and not a test's to take**, so a Draft budget is measured and printed until it is
gated, and a timing gate, once it is one, is asserted in release builds only. Row 7 is a
Draft row. The test was enforcing it as a gate, in a debug build, on shared runners.

So the MOP-07 comparison became a printed measurement, and the three functional clauses of
row 7 -- the same queue in the same order, a decision reaching the other desktop naming who,
and neither desktop queueing, engaging or issuing for a node plan -- stay asserted as they
were. When the owner confirms row 7, the gate is the removed assertion put back with
`#[cfg(not(debug_assertions))]` on it, which is the one-line change `frame_budgets.rs` was
shaped to accept for its own rows.

## What the owner may want to decide when gating row 7

A release-only timing gate still runs on the same shared runners, only faster. Whether
MOP-07 belongs there, or with the performance budgets that run where timing means something,
is a question for the confirmation, and this change does not answer it.
