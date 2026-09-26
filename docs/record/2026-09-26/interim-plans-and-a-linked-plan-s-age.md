# Interim plans, and a linked plan's age

GAP-156 and GAP-157 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-04 §11, D-93 and D-94, taken under the owner's delegation of 2026-09-26. It raised
GAP-168 and GAP-169, and found and closed GAP-170.

## What was wrong

GAP-119 gave the planner a solve budget and made a stale plan say how old it is. What it
left, it filed. A raid toward the exact solver's limits takes seconds of solving outright
and several times that at 4 ms a tick; at the limits, sixteen tracks and eight effectors,
longer than any engagement; past them the solver refuses the picture by design. In every
one of those cases the operator was shown the last good plan -- an answer to an older
picture -- with a percentage that barely moved, for as long as the raid lasted. Honest, and
no help to the person who has to decide (GAP-156).

And on the console most operators use while linked, even that honesty was missing. The
node's plan was relayed as current whenever the link was up, so a plan the node could no
longer refresh was drawn on PN-05 with no stale line and no age. GAP-161 had made the
health strip follow the node, so PN-01 said the planner was down while PN-05 drew its plan
as if nothing were wrong (GAP-157).

## What stands in, and why this one (D-93)

The owner delegated the choice with one instruction: the best production answer for an
operator mid-raid. Four candidates were weighed.

- **The exact solve at a shorter horizon.** Still a subset dynamic program, still
  exponential in the tracks. At the limits even a horizon of one enumerates hundreds of
  millions of matchings for the full picture. It does not answer the pictures GAP-156 is
  about.
- **A greedy pass.** Fast, but it is the optimum of nothing, it has no tie rule, and on
  the uniform matrix the planner uses today it would pair effectors with tracks in an
  order the exact answer does not use -- so the moment the exact answer arrived it would
  replace the stand-in with a plan naming different effectors, for no reason an operator
  could see.
- **Refusing large pictures.** Leaves the operator exactly where GAP-156 found them.
- **The best assignment for this step alone.** The allocator's own problem at a horizon
  of one: the same rewards, the same one-to-one constraint, the same freedom to leave an
  effector idle. It is an assignment problem, so the Hungarian method solves it exactly in
  polynomial time at any size. This was taken.

`gungnir_allocation::solve_one_step` solves it and reproduces the exact solver's tie
rule -- the higher total, then more pairs, then the first matching in its enumeration
order -- so on the uniform matrix it names the pairs the exact solve names at the shipped
horizon of ten, and when the optimum arrives the pairing does not move. Rules one and two
ride inside the assignment problem as a lexicographic cost (minus the reward, minus the
pairs); rule three is applied resource by resource, and the potentials the Hungarian
method leaves are a certificate that rules most choices out without a second solve.

A one-step answer gives up the look-ahead, and the operator should know by how much it
might have cost. `gungnir_allocation::stand_in` says: a floor, the value of following the
first step with one-step answers over the rest of the horizon, which the plan can
actually reach; and a ceiling on the optimum, the smaller of every track's best reward
summed (a track is serviced once) and the horizon times the best one-step value (no step
is worth more). The exact optimum lies between them -- the oracle test checks it against
the exact solve -- so "worth at least 87% of the best plan's value" is something the
numbers support rather than an estimate.

The oracle in `tests/one_step_oracle.rs` is twofold. Inside the exact limits, the exact
solve at a horizon of one must name the same assignment on integer rewards, where ties
are everywhere, and the same value on fractional ones. Past them, a dynamic program over
resource subsets that shares nothing with the Hungarian method must reach the same total.
The one place the two can part is stated in the module: the exact solve treats totals
within 1e-9 as equal, and the stand-in compares them as computed, so two different sums
of fractions rounded to within a hair of each other can be tied in one and not the other.

A release probe on the development machine, ten steps of horizon, two hundred runs each:
four effectors and eight tracks 5 µs; six and ten 7 µs; eight and sixteen, the exact
limits, 16 µs; eight and forty 41 µs; sixteen and sixty-four 0.24 ms. The stand-in is
computed outside the solve budget for that reason: budgeting it would make a stand-in that
could itself fail to arrive.

## When it stands in (D-93)

Not at once. An ordinary picture -- four effectors, eight tracks -- finishes its exact solve
a few ticks after the picture changes, and a stand-in on every overrun would put an interim
plan in the queue every time a track appeared, followed a moment later by the optimum.
The wait is `plan_stand_in_after_ms` of mission time, 500 ms by default: MOP-07's figure,
the time the deployment allows between a plan being proposed and a person being able to
decide it. An exact answer later than that has already cost the operator more than the
whole decision path may; from then on a labelled answer to the picture on the screen serves
them better than the last answer to an older one. It is measured from when the planner fell
behind the picture, not from when the current solve began, because a raid whose picture
changes every few ticks drops each solve for the next and would otherwise never reach it.

A picture past the exact solver's limits is answered at once: no wait brings an answer the
solver will never give. A deadline taken from the tracks' time to impact was weighed; the
planner does not have it (that is the assessment's), and a wait that moved with the picture
would make the stand-in arrive at times nobody could predict.

## How it is kept from being taken for the optimum (D-93)

The label is on the plan: `PlanView::basis`, `Exact` or `OneStep`. A plan travels -- into
the approval queue, onto the journal, over the link, inside a queue item's view -- and a
label kept anywhere beside it would be lost at the first of those. The planner answers
`PlanOutcome::Interim` with the bound and the reason and stays unhealthy, so the status
strip and the record say it is not giving its own answer. PN-05 draws "INTERIM" above the
plan, with the bound and the reason; under a stale line it still labels an interim plan,
because "stale" alone does not say what the stale plan was. PN-06 marks the row. PN-07
names the planner's interim standing and, separately, the item's own basis, so accept
waits on an acknowledgement of exactly that -- D-82's gate for a stale plan, for the same
reason: during an engagement the stand-in may be the best recommendation there is, and a
control that vanished would be worked around rather than read.

One pairing stays one plan across an interim answer, by GAP-097's rule. When the exact
solve finishes with a different assignment, that is a new plan. When it finishes with
the same assignment, the interim plan stands, and PN-05 says the full solve has since
reached it. A stand-in that recommends what the plan in force already recommends keeps
that plan, under the interim standing. The first draft of this change minted a new
plan in both cases, reasoning that a plan's label should not be left on a plan it no
longer describes. CI then showed what that costs. `gungnir-app/tests/rehearsal.rs`
plans on the machine's clock and jumps fifteen seconds between ticks. On a slow runner
its live planner fell behind, and the stand-in and then the exact answer were each
minted as a plan for the pairing already in force: 9 queued where 8 are expected. With
the planner's clock stepped at 100, 70, 50 or 30 microseconds a reading, the first
draft fails that way every time (11 at 30); the rule now taken passes at every speed
from 0 to 1 ms. A second queue item for one recommendation is the flooding GAP-097
closed, and a plan's basis records how it was reached, which does not change. So the
label stays, and it is PN-05's words that change once the pairing is confirmed.

## What the node says, and how a linked desktop draws it (D-94)

`PlanStandingView` -- current; interim with its bound and reason; stale since
`computed_at` with its reason; or no plan -- goes out as `InterceptEvent::PlanStanding`
when it changes, published after the plan it describes, and in the snapshot's
`plan_standing` for a desktop that connects mid-stall. It carries no progress figure. A
solve's progress changes every tick; carrying it would put an event on the stream and the
journal at the tick rate for as long as a solve ran. So the planner now keeps its progress
beside its reason rather than inside it, and the reason that travels does not change while
the picture does not.

`RemoteInterceptService` answers what the node said, each reason prefixed "on the node".
The node's `computed_at` is on the node's clock; the service converts it to the desktop's
through the offset it measures per connection, pairing the node's reading with the tick's
own clock the first time it sees it -- GAP-140's rule, the one the app uses for the queue's
deadlines. The age PN-05 draws is therefore the age on the node's clock. The end-to-end test
sets the desktop's clock a hundred seconds from the node's, so an age taken across the two
clocks would be a hundred seconds wrong and fail.

Both additions are a defaulted field and a new enum variant, which the interface's
rules call compatible, so `SCHEMA_VERSION` stands as it did for `node_time` and `queue`.
The snapshot a coalition partner reads withholds the standing with the plan it describes.

## What the tests hold

`gungnir-allocation/tests/one_step_oracle.rs`, as above. In
`gungnir-intercept-service/src/lib.rs`: a solve that cannot advance is stale with the last
good plan through the wait, publishing an unchanging standing; half a second behind, an
interim plan that is a new plan where its pairing differs, labelled, with its bound and
reason and the solve's
progress, and the planner unhealthy; the same picture again, the same interim plan; the
exact solve running again and reaching the same pairing, the same plan, now fresh; a
stand-in that agrees with the plan in force keeps it. A picture that grows every call
still reaches its stand-in. Twenty tracks, or nine effectors, are answered at once. The
two GAP-119 tests that advanced mission time by whole seconds now hold the stand-in off
explicitly, or plan inside the wait, since they are about the exact solve carrying on.

`gungnir-app/tests/interim_plan.rs` runs the same sequence through the tick the binary
runs, with the mission clock replayed: PN-05, PN-06 and PN-07 say what they should, accept
waits on the acknowledgement, and the label leaves PN-05 when the optimum arrives while
staying on the interim item.

`gungnir-app/tests/linked_plan_standing.rs` runs it over the real link: a node built by
`gungnir_node::picture`'s own builders and stepped in `main.rs`'s order, serving over
mutual TLS; a desktop signed in through PN-20's path. The desktop draws current, then
STALE with the age on the node's clock, then INTERIM with the node's bound and the node's
queue item marked, then current again.

## What it found

GAP-168: the `gungnir-intercept-service` row's degradation clause says an over-budget solve
returns the last good plan, which now holds only for the stand-in wait. It is a criterion
cell, and the owner's.

GAP-169: a desktop built before `PlanView::basis` reads a newer node's interim plan as the
optimum, and would let it be accepted without the acknowledgement. The interface's rules
call the field compatible and also call a change a client could act on wrongly a reason to
move a version; which applies is a release decision.

GAP-170, found and closed here. A full run of `gungnir-app`'s tests failed once in
`cut_off_and_reconnected.rs`'s
`an_operator_s_console_says_once_on_pn09_that_it_may_not_publish`. It passed alone and in
three reruns, so the single test was looped under parallel load: one failure in 30 runs,
then one in 140. The failure text was the same both times: PN-09 said "20 older sets
were replaced" where the test expects 19.

A diagnostic was added to the test. It printed whether the desktop's own tick had read its
link as connected before the first handoff was issued, and the link's publishing counters
at the end. All 139 passing runs had seen the link come up and replaced 19 sets. The
failing run had not, and it replaced 20: one post, no session renewal, generation 21. So
there was no reconnection, no second publish, and no second PN-09 line. The node refused
once, as it should.

The mechanism is the test's wait. `until` ticks the desktop and then reads
`NodeLink::connected`. The link task sets that on its own thread, so it can become true
after a tick that read it false. The wait then ends while the desktop has not yet seen the
link come up. The first tick after the first handoff sees it, and
`failover::republish_exchange_on_reconnect` republishes the console's whole set. That is
correct behaviour, and the replacement is harmless. But it means 21 sets are queued where
the test counted 20. This is the GAP-136 pattern: the wait read a state the system had not
yet reported through the path the assertion depends on.

This was on main before this change. On origin/main at ab0ca325, a copy of the test was
changed to wait on the link task alone, with no tick in between, which forces this
ordering. It then failed every time with the same text and the same counters. It did not
reproduce by chance in 98 unforced runs there, which is consistent with a one-in-a-hundred
race. The fix is in the test: its wait now also requires `AppState::link_was_connected`,
the desktop's own reading. 98 runs of the fixed test under the same load passed. No
product change was needed. A reconnection cannot slip between two frames, because the
link waits two seconds before it reconnects.

## Ownership

`gungnir-allocation` is reached by the numerical-stability clause, and `one_step.rs` is new
arithmetic in it, so the change is human-owned; its standing is in `../../signatures.md`.
`gungnir-policy` is human-owned too, and three of its test fixtures gained the new field
and nothing else; that is human-owned by the same rule, and in the same ledger. The
`gungnir-api` change is to the snapshot, a read path, and touches no write path.
