# A register that says how old it is

GAP-145 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-18 §12, D-69. It raised GAP-146.

## What was wrong

GAP-137 gave the exchange register a producer per writer and left it with no lifecycle,
which is the gap that change filed against itself. The register lives in the node's
memory, and a desktop publishes only when it issues a handoff. So a node that restarted
held an empty register until some console's next decision: a partner reading
`GET /v3/exchange/handoffs` was served an empty deployment while the consoles held
engagements they believed were published, and nothing on the response said the list was
short. In the other direction nothing forgets a producer that has gone away, and an
ephemeral desktop (D-67) is a new producer every run.

## What was built

**Each producer's set carries the node time it was written**, and `as_of` on both
`ExchangeResponse` variants is the oldest of those. A merged answer is only as current as
its quietest contributor, and the producers themselves stay off the wire (§11), so the age
is the one thing a partner can be told about them without learning how many consoles this
deployment runs. An item nothing has been published for has no age at all, only a reason.

**A desktop publishes its whole handoff set the tick its link comes back.** The edge, not
the state: a publish is a replacement, so repeating it every tick would be a write a
second for nothing, and waiting for the next decision is exactly what left the gap. A
desktop holding none publishes none -- an empty set is the claim "there are none here",
which a console that has issued nothing has no business making, and it keeps quiet
consoles out of a register where each would take a producer slot.

**Nothing expires, and nothing is evicted.** A handoff is a decision that was taken.
Dropping it because the console that issued it went quiet would delete a true thing to
hide an unknown one, and a partner would watch a list shrink for a reason no field
explains. The bound still refuses a sixty-fifth producer rather than dropping a set a
partner is served from; a deployment running ephemeral identities reaches it after
sixty-four restarts and gets a `507` and a visible backlog, which points at the key
custody that is the actual fault.

## What the test holds

`a_desktop_publishes_its_handoffs_again_when_its_link_comes_back`
(`gungnir-app/tests/cut_off_and_reconnected.rs`) runs a real node behind the cuttable
proxy that row 8 already uses. A Supervisor issues one handoff and the node's register
holds it; the register is then emptied for that producer, which is the state a restart
leaves as far as this desktop can tell; the proxy cuts and restores; and the set comes
back with **no decision taken in between**, which the test asserts by counting the
records. Against the old behaviour it times out after sixty seconds, because nothing else
would have sent it.

Three unit tests in `gungnir-api/src/transport.rs` hold the age itself: that it is the
oldest producer's write rather than the newest, that a republish moves it off the producer
that wrote it without dropping the quiet producer's products, and that an item nothing has
been published for carries no age.

## What it found

`PUBLISH_EXCHANGE` belongs to Commander, IntelligenceAnalyst and Supervisor, not to an
Operator. The test above published nothing at all until it signed in as a Supervisor --
which is how GAP-146 was found: an Operator's console is refused `403` on every publish,
`flush_exchange` keeps the batch and retries it, `queue_exchange` bounds nothing, and the
outbox grows one batch per handoff for as long as that console runs, in silence. A
refusal that cannot succeed is not a node that is unreachable, and the link treats them
the same.

## What it did not settle

The register is still memory. A node restart still loses every producer's set until each
desktop's link cycles, which for a desktop whose link never drops means until its next
handoff; what this change guarantees is that a reconnection repairs it, not that a restart
is invisible.
