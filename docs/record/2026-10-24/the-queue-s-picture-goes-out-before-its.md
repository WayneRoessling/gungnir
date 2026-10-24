# The queue's picture goes out before its stream

GAP-201. A linked desktop could be told an item was queued, read the node's queue, find
it missing, and then show nothing until the queue moved again. Found by CI on PR #205
(run 40094267726, attempt 1), which changed none of the code involved:
`gungnir-app/tests/linked_plan_standing.rs` timed out waiting for the interim plan's
queue item. It did not reproduce in 32 concurrent local runs.

## The race

The node's tick, in `gungnir-node/src/main.rs`, appended the tick's envelopes to the
journal, offered them to every linked desktop, and only then published the picture.
`GET /v3/queue` serves that picture's `queue` (`NodeApi::queue`). A desktop's link reads
the route as soon as a `Queued` or `Escalated` frame marks its projection stale
(`refresh_queue` in `gungnir-remote/src/link.rs`). A fast desktop could read the queue in
the gap between the offer and the publish. `NodeQueue::take_picture` took the old queue
as the truth and cleared the stale mark, and nothing asked again.

`gungnir-remote/src/queue.rs` already stated the rule this broke: anything queued before
the picture is in it, and anything after is on the stream. That holds only if the node
publishes a tick's picture before it streams that tick's events.

## Why the node and not the desktop

On the node the fix is an ordering change, and it makes the desktop's stated rule true.
The desktop alternative was to keep the stale mark set while an item the stream announced
is missing from the picture and has not been decided. That works, but it puts a second
bookkeeping of the queue on every desktop to make up for an order the node controls.

The order the node keeps now:

1. Every step that publishes onto the bus, as before.
2. The journal append. Nothing reaches a client before it is on disk here. The picture
   comes after the append too.
3. The picture.
4. The tick's envelopes, offered to the transport.
5. The health event. It still goes on the bus after the drains, so it is journaled and
   offered on the next tick, as before. Moving it between steps 2 and 4 would have
   offered an envelope the journal did not yet hold.

Reading the picture ahead of its events is the case every desktop already handled. A
desktop that signs in reads a snapshot, then subscribes from its last sequence number,
and the backlog replays events the snapshot already reflects. Track and plan events
applied over a picture that holds them change nothing. A `Queued` for an item already in
the picture only takes the picture again.

## How it is held

`picture::publish_picture` now takes the tick's envelopes and offers them itself, after
the snapshot. The order is in one function instead of in every copy of the loop:

- `main.rs` calls it with `api_rx.try_iter()`.
- `backend_parity.rs`, `linked_plan_standing.rs` and `node_audit_on_pn20.rs` copy the
  loop's order, and call it the same way.
- `health_follows_flags.rs` passes no envelopes, because it reads the bus itself.

`linked_plan_standing.rs` had also announced health before its drain, unlike `main.rs`.
It now announces it after.

`gungnir-node/tests/queue_picture_before_stream.rs` forces the window instead of timing
it. `publish_picture` draws each envelope from its iterator just before offering it, so
the test's iterator works as a hook. It reads `NodeApi::queue` exactly when each `Queued`
and `Escalated` is about to reach a desktop. It checks two things:

- The item is in the queue when its `Queued` goes out.
- The item is offered to the new role when its `Escalated` goes out.

With the snapshot and offer steps swapped back to the old order, the test fails on the
first assertion, against an empty queue, every time.
