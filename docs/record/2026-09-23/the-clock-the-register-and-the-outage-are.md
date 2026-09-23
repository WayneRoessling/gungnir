# The clock, the register and the outage are decided

D-69, D-70 and D-71
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
for GAP-145, GAP-140 and GAP-142.

## What this item is for

Three gaps built on 2026-09-23 each rested on a question that was the owner's rather than
the build's, and each was filed open with the built shape as its proposal. The three
record items written with them say what was built; this one says where the proposals
stopped being proposals.

**The owner answered all three as proposed on 2026-09-23.** Nothing about the built
behaviour changed with the answers. What changed is that the register, the countdown and
the outage now have decided contracts behind them.

## What each one settles

**D-69, the exchange register's lifecycle.** Nothing expires: a handoff is a decision that
was taken, and dropping it because the console that issued it went quiet would delete a
true thing to hide an unknown one. `as_of` carries the node time the least recently
refreshed producer wrote, which is what a partner can be told without learning how many
consoles this deployment runs, and a desktop republishes its whole set the tick its link
comes back. The bound stands: a sixty-fifth producer is refused rather than evicting a set
a partner is being served from.

**D-70, whose clock a deadline is in.** The node stamps its own clock on the snapshot as
it answers; the desktop keeps the pair of readings rather than the difference, so a
countdown advances on its own clock between snapshots while staying on the node's scale;
PN-01 says the difference above one second, because that is when a countdown drawn to the
second changes a digit. Nothing sets or steers a clock.

**D-71, a node that has never answered.** Silent, measured from the link's own start, and
the sentence says which it is: "has not answered since this desktop signed in" rather than
"silent for 12 s". A desktop that signed in to an unreachable node used to sit on a remote
backend for ever, neither linked nor fallen back.

## What was signed with them

The three designs, and one piece of human-owned code: `DecisionRecord::to_event` now puts
the queue item a decision answered and whether it was an override on the journal, which is
what a desktop reads back after a restart and exactly what `accepted` and `plan` left out.
What the owner has signed is [`../../signatures.md`](../../signatures.md), which is the
only place that says so.

## What it does not settle

The two gaps this work raised stand as filed: GAP-146, a console whose role may not
publish queues its handoffs for ever, and the register's own memory -- a node restart
still loses it until each desktop's link cycles. And a decision whose plan was never
proposed in the same session cannot be rebuilt after a restart; PN-18 says so and counts
it rather than forwarding a partial outage.
