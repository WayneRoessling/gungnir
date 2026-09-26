# A console that may not publish says so once

GAP-146 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-18 §13, D-75 and D-76, taken under the owner's delegation of 2026-09-25. It raised
GAP-150.

## What was wrong

GAP-145's test published nothing until it signed in as a Supervisor, and that is how this
was found. `PUBLISH_EXCHANGE` belongs to Commander, IntelligenceAnalyst and Supervisor, so
a console signed in as an Operator -- the ordinary case on a watch floor -- is refused
`403` on every publish. The link's `flush_exchange` read every answer but a `2xx` as "try
again next tick", so it offered the same refused batch four times a second for as long as
the console ran; `queue_exchange` bounded nothing, so every handoff issued added a batch
behind it; and nothing on any panel said a partner was not being told about engagements
this deployment holds. DN-18 §11 and §12 said a `507` backlog was visible on PN-09, and no
line on PN-09 read the outbox.

## What was decided, and why

**D-75: the answer is read for what it says about the caller.** Delivered; not now (no
answer, `408`, `425`, `429`, a `5xx`); not you (`401`, `403`); not that (any other
`4xx`). Only "not now" is retried, and on an interval that doubles from one forward tick to
thirty seconds, so a register that stays full for an hour is asked about a hundred and twenty times
rather than fourteen thousand. "Not you" stops the link offering anything until it signs
in again.

The alternatives, and why not:

- *Retry a `403` on a timer.* It asks the same session the same question. A role does not
  change on a timer; it changes with a sign-in, and every connection signs in afresh, so
  the connection is the trigger that can actually make a difference.
- *Have the Operator's console not queue at all.* The desktop has a copy of the authority
  matrix, but the node answers for the session the link holds, and the two can differ.
  Holding the newest set is also what a Supervisor who signs in at that console expects to
  be sent, without issuing another handoff to prompt it.
- *Raise an alert.* A refusal that stands for a whole watch is a state, not an event. It
  belongs on the health panel in one sentence that stays said, not in the alert stream,
  where it would either repeat or be acknowledged away while still true.

**D-76: one set per item, the newest.** Every set is its producer's whole current set, so
a newer one makes an older one waiting for the same item obsolete. The outbox replaces it
in place and counts the replacement. That bounds the outbox at one set per item and loses
nothing the node should end up with. The detection outbox's rule -- a capacity, oldest
dropped -- is right for detections, each its own fact, and wrong for sets: it would keep
obsolete sets, drop by age across items, and post every obsolete set on reconnection.

**GAP-121 was considered and left alone.** `StoreAndForwardQueue` is that same
first-in-first-out rule over `Envelope`s, and adopting it here would have taken the wrong
rule, a different type, and a `gungnir-remote` to `gungnir-resilience` manifest edge
`ARCHITECTURE.md` §7.1 does not show. Whether it should carry what a desktop records during
an outage, or be retired, is still GAP-121's question.

## What was built

- `gungnir-remote/src/link.rs`: `publish_answer` classifies a status; `ExchangePublishing`
  holds the counts, the refusal, the retry and its next instant; `queue_exchange` replaces
  a waiting set for the same item and counts it; each set carries a generation so a set
  replaced while in flight is not lost when the old one lands; a new connection clears the
  refusal and the backoff (`ExchangePublishing::signed_in_again`).
- `gungnir-ui`'s PN-09 gains a "Coalition exchange" line (`ExchangeLine`,
  `ExchangeStanding`), drawn only for a linked console.
- `gungnir-app/src/exchange.rs` composes the sentence, including the roles that may
  publish, read from `gungnir_security::authz::role_permits` over `Role::ALL`. The
  reconnection edge now publishes launch warnings as well as handoffs
  (`exchange::republish_all`), because a sign-in on a linked desktop builds a new link and
  the old link's held sets go with it.

No `gungnir-api` write path, no `gungnir-security` rule and nothing on `gungnir-remote`'s
TLS identity path was touched: the node's `403` already carried its reason in the problem
body.

## What the tests hold

- `gungnir-remote/tests/transport.rs::an_operator_s_console_is_refused_once_and_holds_one_set_per_item`:
  an Operator's link queues fifty handoff sets and a warning set against a real node; a
  proxy counting publish request lines on the wire sees **one** publish in the next two
  seconds, where the old link offered the batch again on every 250 ms tick; the outbox
  holds two sets, with
  forty-nine replacements counted. The link then signs in as a Supervisor at its next
  connection, and the node takes the newest set, sixty handoffs, with two more publishes.
- `a_set_queued_while_cut_off_is_delivered_when_the_link_returns`: a Supervisor's console
  publishes normally, holds the newest set through a cut, and delivers it on reconnection;
  an unreachable node is not a refusal.
- `a_node_that_says_not_now_is_asked_again_less_often`: a full register's `507` is retried,
  and three attempts take at least the sum of the first two intervals, whatever the
  machine's load.
- `gungnir-app/tests/cut_off_and_reconnected.rs::an_operator_s_console_says_once_on_pn09_that_it_may_not_publish`:
  a real desktop as an Operator issues twenty handoffs; PN-09, rendered, carries the
  refusal exactly once with the node's reason and the matrix's roles; the link made one
  request; one set is held; and a Supervisor signing in at the same console publishes all
  twenty with no handoff issued in between.
- Unit tests for the classification, the backoff, the generation rule and the sentences.

## What it found

The mission report is published when PN-13 generates or exports it and at no other time.
It lives in `SustainmentState`, which is window state outside `AppState`, so the tick
cannot republish it when the link comes back: a node that restarted, or a new link built
by a sign-in, holds no report from this console until someone generates one again. Filed
as GAP-150 rather than moving the report into mission state here.
