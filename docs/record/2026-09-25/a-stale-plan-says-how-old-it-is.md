# A stale plan says how old it is

GAP-119 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
DN-04 §10, D-81 and D-82, taken under the owner's delegation of 2026-09-25. It raised
GAP-156 and GAP-157.

## What was wrong

The GAP-067 walk held the `gungnir-intercept-service` row of
`../../verification-capability-table.md` §2 because its degradation clause -- "over-budget
solve returns the last good plan, flagged" -- named a budget that did not exist. The
planner ran the exact dynamic program inline in the tick for as long as it took. The
stale path the row describes existed only for a solve that failed, and its one test
compared the returned plan with an empty one, which any bug that lost the plan would also
have produced. Determinism was shown on a one-by-one plan.

Behind that, two things an operator would meet. A stale plan looked exactly like a fresh
one on PN-05: the plan in force was kept and drawn unchanged, and the only sign was a
health flag on another panel. And PN-07 said the plan "may be stale" with no age and no
reason, so the acknowledgement that gates accept was given against a guess.

## The number

MOP-06 (`../../mission/measures.md` §2) already names the planner's target: "p99 under the
per-frame budget of 4 ms for the embedded profile; off-thread with last-good-plan return
beyond". So the default is 4 ms and nothing was invented. `plan_solve_budget_ms` in the
baseline sets it per deployment; validation refuses zero, negatives, non-finite values and
anything past 100 ms. The ceiling is there because the budget is also how long a tick may
be held, and on the node that tick carries detections to the event stream inside MOP-02's
150 ms on-prem.

## Why the solve is sliced

The first version built here stopped an over-budget solve and threw it away. A probe
before going further showed what that would do: on the development machine, in release, at
the default horizon of ten, eight tracks against three effectors took 10 ms, four
effectors and eight tracks 39 ms, four and ten 355 ms. A solve that is discarded when the
budget runs out and started again next tick never finishes if it needs more than one
budget, so an ordinary raid would have been stale for as long as it lasted -- worse than
the unbudgeted planner, which at least answered late.

MOP-06's own answer is "off-thread". A thread would have kept the frame free, and brought
a result that lands at a time nothing chose, a solve for a picture that has since moved,
and an overrun that no test could decide without sleeping. The answer taken (D-81) serves
the same purpose in the tick: `gungnir_allocation::ExactSolve` fills the value function a
slice at a time and keeps its place -- the layers done, the state part-walked, the best
matching found in it -- and the planner carries it to the next call. Each call spends at
most the budget. The walk inside a state became an explicit stack so it can stop between
two matchings; it makes the same choices in the same order and performs the same additions
and subtractions in the same sequence as the recursion it replaced, and a property test
holds the sliced solve to the recursive one bit for bit over random problems and random
places to stop. The oracle comparison of the §1 allocation row runs unchanged and passes.
Not materialising every matching of a state also made the solve about three times faster:
the same pictures now take 4, 13 and 128 ms.

Two rules keep the slicing honest. An unchanged problem -- the same tracks and adequate
resources in the same order, and the same rewards -- is not solved again, because the
allocator sees nothing else and the answer is already known; the planner answers it
fresh without spending anything. A problem that changes while its solve is under way
drops that solve, because its answer would be to a question nobody is asking.

The budget is read through `gungnir_intercept_service::SolveClock`. The machine's
monotonic clock in service; `SteppedClock`, which advances a set step per reading, in the
tests, so "this solve overruns" is a statement a test makes rather than a race it hopes to
win.

## What the operator sees (D-82)

PN-05 draws the plan's standing above the plan: nothing when it is current; "STALE", when
it was computed, how long before the planner was last asked, and why, when it is not; and
"NO PLAN" with the reason when the planner has never answered. The plan is still drawn
under a stale line -- it is the last good plan, and hiding it would take the best
available recommendation away. PN-07 names the stale plan among the degraded conditions
with the same age and reason, and accept waits on the operator acknowledging that
condition, as it does for every other. Approving a stale plan is allowed after that
acknowledgement. Refusing it outright was weighed and rejected: during an engagement the
last good plan may be the best recommendation there is, the reject and override paths
stay open, and a control that vanishes is one an operator works around rather than one
that tells them anything. MOE-06 counts decisions taken under a degraded state that was
not shown; this one is shown, acknowledged, and preceded on the journal by the health
change.

## What the tests hold

In `gungnir-intercept-service/src/lib.rs`: two fresh planners given the same three tracks
and three ready resources agree with every reward tied, falling to the documented tie rule
with identifiers that differ from positions, and again with distinct rewards whose unique
optimum is not the diagonal; they differ only in the plan identifier, as D-56 requires. A
solve at t = 1, then a solve at t = 2 that does not finish inside its budget, returns
`Stale` carrying the t = 1 plan itself, stamped t = 1, with `is_healthy()` false, and the
next in-budget call recovers. The t = 2 picture has a fourth track, because an unchanged
picture is not solved again. A solve longer than one budget carries on across calls, its
progress never falling, and finishes with exactly the plan an unhurried planner computes.

In `gungnir-app/tests/solve_budget.rs`, through the tick the binary runs: the same
sequence drawn on PN-05 and PN-07, the stale line and the degraded condition appearing
and then clearing, and the baseline's budget reaching every planner the desktop builds.

The row itself is unchanged. Its criterion is the owner's, and it waits for the owner's
walk against these tests.

## What it found

Consolidating how the desktop builds its planner showed that the two planners it builds
after start -- on falling back from its node, and on signing out of one -- were built with
the horizon alone and no local frame, so a desktop that had fallen back paired effectors
with tracks and placed no intercept point. The node's planner had never had the frame
either. All are now built with it. On the node that matters beyond PN-05: the geofence
engine checks an intercept point when there is one, and the node's plans had none to
check.

GAP-156: a picture toward the solver's size limits takes seconds of solving, and so far
longer at 4 ms a tick; what should stand in for an optimum that cannot be reached in time
is a change to what the allocation row promises, and the owner's.

GAP-157: while a desktop is linked, `RemoteInterceptService` relays the node's plan as
current whenever the link is up, whatever the node's own planner says. Carrying the node's
standing across needs its computed-at time on the wire.

## Ownership

`gungnir-allocation` is not a human-owned crate, but numerical stability is human-owned,
and this change restructured how the solve walks its matchings in a crate the
numerical-stability clause has reached since 2026-09-06. The arithmetic is unchanged and
bit-identical by test; the change is human-owned all the same, and its standing is in
`../../signatures.md`.
