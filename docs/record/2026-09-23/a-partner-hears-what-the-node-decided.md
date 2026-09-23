# A partner hears what the node decided

GAP-137 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-18 §11, D-68. It raised GAP-145.

## What was wrong

`ApprovalHost::republish_handoffs` was a no-op on a node, and had been since GAP-132 gave
the node an approval queue of its own. A coalition partner reading
`GET /v3/exchange/handoffs` was told about every engagement a desktop decided and about
none the node decided, with nothing on the response saying the list was partial -- the
silence DN-17 §5 rule 3 exists to prevent.

The no-op was deliberate and its doc comment said why. `NodeApi::publish_exchange`
replaced the held set for an item, so two writers would each have erased the other, and
which set a partner saw would have depended on tick order. GAP-133 removed one of the two
writers it was waiting on and found another in the same place: a desktop that falls back
keeps its `NodeLink`, decides on its own queue while cut off, and queues its whole handoff
set for `flush_exchange` to deliver on reconnect (DN-31 §6.7, GAP-134). Wiring the node in
against that register would have made the node's set the one that disappeared after any
outage -- the same failure, one layer along.

## What was built

**The register is keyed by item and then by producer.** A publish replaces that producer's
set and touches no other; a read concatenates every producer's products, the node's own
first, and sums what each of them had withheld. An item answers `NotHeld` only when no
producer holds any, and then it carries what each said rather than one reason at random.
`NodeHost::republish_handoffs` publishes this node's whole current set under
`ExchangeProducer::Node`, exactly as the desktop's host does over the link.

**The producer is the name the connection was verified under, never a field in the
request.** GAP-141 made that name worth keying on the day before: `desktop-` and sixteen
hex digits of the desktop's own key (D-67). A field in the request would have let any
caller publish as any producer, which is the authority the `origin` check on forwarded
decisions already refuses. The node's own set is a variant rather than a string, so no
desktop name can collide with it. A link with no client certificate names nobody: every
such writer shares one set and they overwrite each other, which is what the whole register
did until now and is what mutual TLS buys.

**The node's opening claim for handoffs was false and is not any more.** It read "handoffs
are issued on a desktop from a recorded decision; this node holds none", which stopped
being true the day DN-31 moved the queue. It now publishes an empty set -- "I keep these
and have issued none yet" -- which is a different claim from withholding and the true one.
That claim moved out of `main.rs` into `approval::claim_exchange_items` so the test asserts
it rather than a copy of it.

## What the tests hold

`a_partner_with_an_agreement_receives_a_handoff_this_node_issued`
(`gungnir-node/tests/approval_queue.rs`) drives the real transport and the real loop: the
node claims an empty set, an operator decides a queued plan, and the partner's read holds
exactly that decision. Run against the old no-op it fails, `left: []` against the decision
id, which is the gap's own sentence.

`two_desktops_publishing_one_item_do_not_overwrite_each_other` and
`an_operator_holding_the_action_replaces_its_own_set_and_no_other`
(`gungnir-api/tests/exchange.rs`) do it over real mutual TLS with two client certificates:
each desk replaces its own set, one desk emptying its set leaves the other's and the node's
alone, and what a partner reads is the merge.

Three unit tests in `gungnir-api/src/transport.rs` hold the merge itself: producer order,
every producer's reason travelling when none holds any, and the bound.

## What was decided, and what was not

The register question is D-68, and the shape above is what this change proposes: one set
per producer, merged on read. The node as the sole writer would have needed a collision
rule for two desktops claiming one product, and that rule would have lived in the
transport. Appending per handoff, keyed by product id, cannot express a withdrawal: a
desktop that no longer holds a handoff has no way to say so, and "partial delivery is
reported" (DN-18 §5) turns on a partner being told what is no longer current as much as
what is.

**Bounded at sixty-four producers an item, refusing rather than evicting.** A producer
already in the register always writes. A new one beyond the bound is refused with `507` and
the reason, so the desktop's link keeps the batch and the backlog shows on PN-09; evicting
a set to make room would have served a partner a stale list and said nothing.

## What it left open

The register has no lifecycle, and giving it a producer per writer is what made that
visible (GAP-145). It lives in memory: a node restart loses every producer's set, and no
desktop republishes until it next issues a handoff, so a partner reads an empty deployment
while desktops hold handoffs they believe are published. In the other direction nothing
forgets a producer that has gone away, and an ephemeral desktop is a new producer every
run. What a partner may believe about a producer that has gone quiet is a policy, not a
transport change.
