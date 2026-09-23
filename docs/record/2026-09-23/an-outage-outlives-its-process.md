# An outage outlives its process

GAP-142 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-31 §15, D-71.

## What was wrong

The fallback lived in memory. A desktop that restarted while cut off -- or after its node
answered and before a person switched back -- came up with no outage at all: what it had
decided offline stayed in its own journal, never reached the node's record, was never
compared with the node's engagements by track, and MOE-11 fell below 1.0 with nothing
saying so.

And a desktop that started with its node already unreachable never fell back either,
because silence was measured from the last thing heard and nothing had ever been heard. It
sat on a remote backend for as long as it ran, with a queue it could not read and a
decision path it could not use.

## What was built

**The outage comes back from the journal**: the last `FellBack` with no `SwitchedBack`
after it. Its bounds, its endpoint, the sequence the node's history is asked from and the
lapse are on the record, so they return whole -- the sequence only because the event
carries it now, which is the point of putting it there. The session the outage was
journaled into returns with them, because the merge reads this desktop's half out of the
journal and the session in progress after a restart is a new one: reading that would
report that this desktop did nothing while it was cut off, which is the opposite of what
happened.

**Its decisions are rebuilt from the record, or none of them is forwarded.** A decision
comes back from its `Decided` event and the `PlanProposed` view it names. The event now
carries the queue item it answered and whether it was an override -- exactly what
`accepted` and `plan` left out, and without them a decision read back off the journal is
not a record the node's forwarded route can take. A decision the journal cannot describe
whole is counted rather than guessed at, and then the batch does not go at all: an outage
reaches the node whole or not at all (DN-31 §6.8), and PN-18 says how many of each.

**A sign-in during a recovered outage builds the link the outage never had.** The process
that fell back is gone, so there is nothing to hand the credential to. The desktop builds
a link and leaves everything else where it is -- its own services, its own queue, its own
backend -- because an outage ends when a person switches back (D-15), not when a link
appears. The services and the tee are the same ones `fall_back` installs; they moved into
one function so the two paths cannot come to differ.

**A node that has never answered is silent** (D-71), measured from the link's own start
until there is something later to measure from. The sentence says which it is: "has not
answered since this desktop signed in" rather than "silent for 12 s", because an operator
reads those two differently.

## What the tests hold

`an_outage_outlives_the_desktop_that_fell_into_it`
(`gungnir-app/tests/cut_off_and_reconnected.rs`) is a real restart: a desktop falls back
behind the cuttable proxy, decides on its own queue, and is dropped -- and a second
`AppState` over the same data directory reads the journal the first one wrote. It asserts
the bounds, the session, the decision in the batch the node will be sent, that the batch is
owed rather than held back, and that the strip says so the moment the desktop starts.

`a_node_that_never_answers_is_silent_rather_than_pending` cuts the proxy before anybody
signs in, so the link never hears anything at all, and asserts both the fall-back and the
sentence that says this node was never reached.

## What it did not settle

A decision whose plan was never proposed in the same session, and a journal written before
the item and the override were on the event, cannot be rebuilt. That is stated on PN-18
and counted; it is not silently dropped and it is not guessed at. What a deployment should
do with such an outage -- reconstruct it by hand, or accept that its decisions stay on the
desktop's own record -- is a question for whoever reads that panel, not for this change.
