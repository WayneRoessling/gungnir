# GAP-244: an alternative is timed on the plan's clock

Found from one failure on main's CI. Run 40715701906 (job 122912658690, commit 46a10829)
failed `gungnir-app/tests/alternatives.rs`'s
`every_alternative_carries_a_verdict_from_the_desktops_own_chain` with four options under
one chain and three under the other. The next main run and every pull-request run passed,
and the merge before it (GAP-213, fuzz targets) touched none of this code.

## The mechanism

Two defects, each harmless without the other.

**The alternatives timed themselves on the machine.** `gungnir-app`'s `SnapshotPlanner`
builds a fresh `DpInterceptService` for each alternative and for the what-if, with the
baseline's 4 ms budget (GAP-119) measured on a new `MonotonicClock`. The live planner can
be put on a `SteppedClock` that never advances, and every test of the decision path and
every laydown rehearsal (D-109) does that, but the snapshot planner could not be. Its
budget was always measured on the machine's clock.

**An alternative that overran was dropped without a word.**
`PlanAlternatives::recommend` skipped any alternative whose solve returned
`PlanUnavailable`. `decisions.rs` said the opposite: that an over-budget option "is
reported unavailable with the planner's own sentence". That was true for the
recommendation and the what-if, and not for an alternative.

So the number of options the same picture offered depended on whether the thread was
preempted during one of four solves. Each solve in this fixture takes about 70 µs on the
development box. A Windows scheduler quantum is about 15.6 ms, and on a CI runner sharing
its cores with the rest of the suite a single preemption is enough to overrun 4 ms.

## Evidence

- Sixty runs of the unchanged test under 28 busy processes on the development box all
  passed. The timing needed is rare, as main's history shows.
- A throwaway probe calling `decisions::alternatives` 200 times on the same desktop, with
  the baseline budget tightened to 0.05 ms, gave four options 198 times, three once (one
  alternative overran and was dropped: the CI failure exactly) and one once (the
  recommendation overran). At 4 ms and at 0.1 ms it gave four every time.
- With the drop restored, the new unit test fails with `left: 3, right: 4`, the shape of
  the CI failure. With the old clock restored, the new desktop test fails with
  `left: 4, right: 1`.

## What changed

- `InterceptService::solve_clock`, defaulting to none and returning the embedded
  planner's clock. The snapshot planner measures on it, and falls back to the machine's
  monotonic clock only for a desktop that relays its node's plan, which has no local
  clock to share.
- `recommend` keeps an alternative it could not compute as the empty plan under its own
  heading ("Alternative, if resource N were unavailable."), with the allocator's reason.
  The chain denies an empty plan (`EmptyPlan`), so the entry ranks last and cannot be
  acted on. It is left out of the duplicate check, because an uncomputed alternative has
  no plan to duplicate.
- `alternatives.rs`' desktop puts its planner on a clock that never advances, so the
  file's assertions are about the picture and not the host.

## Tests

- `gungnir-decision`: `an_alternative_the_allocator_could_not_compute_is_said_not_dropped`.
  One alternative's solve refuses and the others answer. The list is as long as when all
  of them answer, and the refused one is last, denied, and names the reason.
- `gungnir-app/tests/alternatives.rs`:
  `the_alternatives_measure_their_budget_on_the_planners_clock`. The planner's clock
  advances twice the budget at every reading. On the planner's clock the picture offers
  the one course saying the budget was spent. On a clock of the alternatives' own it
  would offer four.

## What this does not do

- On a deployed desktop under load an alternative can still fail to finish inside the
  budget. That is GAP-119's design and is unchanged. What changes is that the operator is
  told, where before the option disappeared.
- A desktop linked to its node still times its rehearsals on the machine's clock. It has
  no local planner clock to share, and nothing that needs determinism runs linked.
