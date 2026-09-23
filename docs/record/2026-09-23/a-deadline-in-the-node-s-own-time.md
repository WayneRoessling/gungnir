# A deadline in the node's own time

GAP-140 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-31 §14, D-70.

## What was wrong

`QueueItemView::expires_at` is the node's mission time. PN-06 drew the countdown as
`expires_at` minus **this desktop's** clock. Both machines run a wall clock, so both
numbers are seconds since the epoch and they agree only as far as the two machines'
clocks do -- and nothing measured the difference or carried the node's own reading. A
console a minute fast showed every item on the node's queue a minute closer to expiry
than it was; one a minute slow showed a window still open after it had closed.

The node refuses a late decision either way (`409 Expired`, DN-31 §6.3), so nothing was
decided that should not have been. What was wrong was what the operator was told, on the
one countdown they work to under saturation.

GAP-008's `ClockSkewEstimator` could not answer this: it measures sensor sources from
ingest events and never sees a node.

## What was built

**The node's clock travels with its picture.** `SnapshotResponse::node_time` is stamped
where the route answers rather than where the loop publishes, so the reading a desktop
takes is as close to the transit as the node can make it. Additive and defaulted: a node
that does not send one leaves a desktop drawing against its own clock, which is what it
did before.

**A desktop keeps the pair of readings, not the difference.** What the node said, and
what this desktop's clock said when that was read. The countdown then advances on this
desktop's own clock between snapshots while staying on the node's scale, because two
clocks that tick at the same rate keep the offset they had when it was measured. What
changes an offset is a machine's clock being set, and a desktop that reconnects measures
it again -- which is why the reading is refreshed when the node's own value changes
rather than every frame, where it would re-measure against a snapshot that had not moved
and drift by exactly that snapshot's age.

**PN-01 says the difference above a second**, and nothing when the two agree or when this
desktop has never been told the node's clock. A countdown is drawn to the second, so a
smaller difference cannot change what a person reads; a larger one means the console and
the node disagree about a deadline by a digit on the screen. The sentence says which way
and what the rows follow, so nobody has to do the arithmetic.

**Nothing sets or steers a clock.** This measures what the two machines say and draws the
node's deadlines in the node's terms; discipline between them is a deployment's business.

## What the test holds

`a_node_s_deadline_is_drawn_against_the_node_s_clock`
(`gungnir-app/tests/desktop_projection.rs`) is the extreme case, and it was not contrived
for the test: that harness's node keeps a stated mission clock from 100 s while its
desktops keep the wall clock. Before this change every row on PN-06 there read as expired
by about fifty-five years. The assertion is the node's own six-hundred-second window,
which is the only number that means anything on either machine, and the skew is asserted
large and negative with PN-01's sentence saying "behind".

A unit test in `gungnir-ui` holds the wording itself: which way, in whole seconds, and
that the rows follow the node.

## What it did not settle

The offset is measured from the snapshot, which a desktop fetches on connection. A link
that stays up for a day keeps the offset it measured at the start; clock drift between
two machines over that time is far below the second this reports at, but a clock **set**
mid-connection is not seen until the next reconnection. Nothing here asks for a fresh
reading on a timer, because the queue route returns a bare list with nowhere to put one,
and a route change for it would be a contract change this gap does not need.
