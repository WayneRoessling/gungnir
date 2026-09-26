# DN-04 Effector layer and cost model

Closes GAP-030. Status: **implemented and wired** (GAP-030, 2026-09-06). **Amendment 1 (§9)**: the closing speed GAP-031's geometry needs. What the owner has signed of this note is in [`../signatures.md`](../signatures.md).

## 1. The gap and the thread step it blocks

MT-01 step 6 asks for the cheapest adequate response. `ResourceConfig` and `ResourceView`
carry an identifier, a position, a capacity, and a ready flag, and nothing else. There is
no layer, no cost, no magazine, and no reserve, so the cheapest-adequate rule has nothing
to read and MOE-03 cannot be computed, let alone enforced.

## 2. The owning component

`gungnir-model` owns `EffectorLayer` and the extended `ResourceView`; `gungnir-config`
owns the extended `ResourceConfig` and its validation. Both already depend on the model
only, so **no new edge**.

`gungnir-allocation` and `gungnir-intercept-service` read the new fields through the
reward matrix `gungnir-assessment` builds. The allocator itself is unchanged by this note:
it optimizes a matrix it is given, which is the boundary that keeps it verifiable.

## 3. Types

```rust
/// Defence layer, ordered outermost first. MOE-03 is defined by this field:
/// the fraction of propeller-drone engagements taken by `Point` or `SelfDefence`
/// rather than `Area` must be at least 0.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
         serde::Serialize, serde::Deserialize)]
pub enum EffectorLayer {
    Area,
    Point,
    SelfDefence,
    /// Non-kinetic: jammers, spoofers, directed effects. Kept distinct because
    /// policy and warning obligations differ, not because the geometry does.
    NonKinetic,
}

/// What using one round of this resource costs, relative to the others in the
/// same deployment. Deliberately unitless: see section 5.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct RelativeCost(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Magazine {
    pub rounds_available: u32,
    /// Rounds held back from automatic recommendation. The allocator may not
    /// propose a resource whose remaining rounds are at or below this.
    pub reserve: u32,
}

impl Magazine {
    /// Rounds the allocator may plan against.
    pub fn allocatable(&self) -> u32 { self.rounds_available.saturating_sub(self.reserve) }
}
```

`ResourceView` gains three fields: `layer: EffectorLayer`, `cost: RelativeCost`, and
`magazine: Option<Magazine>`. The magazine is optional because a non-kinetic effector and
a sensor-cued camera have no rounds, and modelling them with a fictitious count would make
`allocatable` meaningless.

## 4. Edges

**None.**

## 5. Behaviour

**Layer is mandatory; cost is optional and relative.** This is a design call taken against
MOE-03, which is defined by layer and not by money: "fraction of propeller-drone
engagements made by the point or self-defence layers rather than area-defence
interceptors, at least 0.9". A currency figure would put a procurement question in front
of every customer before the system could compute its own headline measure, and would be
wrong the moment a contract changed.

So:

- `layer` has no default and fails validation if absent, because MOE-03 depends on it.
- `cost` defaults to 1.0, meaning "no preference expressed", and only ever breaks ties
  between resources of the same layer that are both adequate.

The cheapest-adequate rule, stated once here so DN-05 and the allocator agree:

1. A resource is **adequate** for a track if it is ready, has allocatable rounds, and the
   policy chain does not deny it.
2. Among adequate resources, prefer the **innermost** layer that is adequate, because that
   is what MOE-03 measures.
3. Within a layer, prefer the lower `cost`.
4. Never propose a resource at or below its reserve. A reserve breach is a decision for a
   person, so the recommendation panel shows the resource as unavailable with the reason
   rather than proposing it and relying on refusal.

Rule 4 is the one that would be easiest to soften later and must not be: a system that
proposes eating the reserve under saturation has quietly changed who decides.

**When a resource has no layer**, which can only happen with a baseline written before this
change: validation rejects the baseline. It does not default to `Area`, because a wrong
layer silently degrades the measure the product advertises.

## 6. Configuration and interface delta

`ResourceConfig` gains `layer: String`, `cost: Option<f64>`, `rounds_available:
Option<u32>`, `reserve: Option<u32>`.

Validation rules added:

1. `layer` parses to a known variant; unknown is an error.
2. `cost`, when present, is finite and positive.
3. `reserve` is not greater than `rounds_available`, and both are present or both absent.

`SUPPORTED_CONFIG_VERSION` stays 1 for the optional fields. `layer` is mandatory but new;
a baseline without it is rejected with a message naming the resource, which is the correct
behaviour for a field the headline measure depends on.

Interface: `ResourceView` appears inside `SnapshotResponse`; the added fields are additive.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-05 Recommendation panel | Layer and remaining rounds per proposed resource; the rationale states which layer was chosen and why an inner layer was not available |
| PN-09 System health | Magazine state per resource, with reserve marked |
| PN-14 Configuration editor | The resource section gains layer, cost, magazine, and reserve |
| PN-17 Commander summary | Layer mix of accepted engagements, which is MOE-03 as the commander sees it |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-3.3 Assignment recommendation | Scenario replay with mixed-layer resources, plus a property test on the preference rule | The innermost adequate layer is always preferred; no plan proposes a resource at or below its reserve; MOE-03 computed over a TT-01 replay is at least 0.9 with a correctly configured laydown | TT-01 and TT-03 sample sets; generated resource sets |

## 9. Amendment 1 -- a closing speed, for the intercept geometry

**Raised by GAP-031.** The geometry solver needs to know how fast an effector closes on
a track, and §3 gives a resource a layer, a cost and a magazine and no motion at all. A
solver written against that would compute against an input that does not exist. The
owner decided on 2026-09-06 (D-26) that the model gains the one number the simplest
useful solution needs, and no more.

**The field.** `ResourceView` and `ResourceConfig` gain `intercept_speed_mps:
Option<f64>`, a closing speed in metres per second for a **constant-velocity
closest-approach** solution: the effector departs its position at that speed the moment
the plan is decided, the track continues at its current velocity, and the solver finds
the earliest point at which the two coincide. That is the least model that gives the
geofence policy (GAP-088) a point to check and PN-05 a time to show. It is not an
envelope: no minimum range, no maximum, no turn rate, no altitude band. Those are a
later amendment when a solver that reads them exists, and they would be layered on this
field rather than replace it.

**Optional, and what `None` means.** A jammer has no closing speed; a deployment may
not have characterised a new effector yet. Either is `None`, and the solver reports
"no geometry for this resource" as an outcome rather than defaulting a speed and
producing a point that is fiction. A plan against such a resource is still a plan; it
carries no intercept point and no time, exactly as every plan does today.

**Validation.** When present, finite and greater than zero; anything else is rejected
naming the resource, like the cost rule in §6.

**Configuration and interface delta.** `ResourceConfig.intercept_speed_mps`, optional,
`SUPPORTED_CONFIG_VERSION` unchanged. `ResourceView` inside `SnapshotResponse` gains the
field additively; a journal recorded before it reads back with `None`.

**User-interface delta.** PN-14's resource section shows the speed where stated. PN-05
is unchanged until the solver exists, and then shows the time to intercept the solver
produces (GAP-031's own delta).

**Verification.** A baseline with a zero, negative or non-finite speed is rejected and a
positive one reaches the view (`gungnir-config` unit test, 2026-09-06). The solver's
closed-form case belongs to GAP-031.

**What landed 2026-09-06**: the field on both types, the validation, and the test. The
solver did not: it waits on an assignment to solve for (GAP-029, Area A). The field is
read by nothing until then, and its doc comment says so.

## 10. Amendment 2 -- a solve budget, and a plan that says how old it is

**Raised by GAP-119**, which the GAP-067 walk filed when it held the
`gungnir-intercept-service` row of `../verification-capability-table.md` §2: the row's
degradation clause names an over-budget solve, and no budget existed. Decided under the
owner's delegation of 2026-09-25 as D-81 (the budget) and D-82 (approving a stale plan).

**The budget.** A planning call spends at most `plan_solve_budget_ms` solving; the default
is MOP-06's 4 ms (`../mission/measures.md` §2), and validation refuses anything not above
zero or past 100 ms (`gungnir_config::MAX_PLAN_SOLVE_BUDGET_MS`), the ceiling sized so a
node's tick keeps the rest of MOP-02's 150 ms. The time is read through
`gungnir_intercept_service::SolveClock`, so a test decides an overrun with a stepped clock
rather than a sleep.

**An overrun keeps its work.** The exact solve of an ordinary picture -- eight tracks and
four effectors at the default horizon of ten -- takes about thirteen milliseconds on the
development machine in release (and took about forty before the solve was restructured
for slicing), so a budget that threw an unfinished solve away would
leave that picture stale for as long as it lasted. `gungnir_allocation::ExactSolve` fills
the value function a slice at a time and keeps its place, and the planner carries it to
the next call. MOP-06's "off-thread beyond" is served the same way -- the tick is never
held past the budget -- without a second thread or a result that lands at a time nothing
chose. A solve advanced to its end is bit-identical to the one-pass solve however it was
sliced (`bellman::tests`, a property test), so the oracle comparison of the §1 allocation
row still measures the same function.

**An unchanged problem is not solved again.** The allocator sees the reward matrix alone,
and its rows and columns stand for the adequate resources and the tracks in the order they
were given, so an equal problem has the same answer and the planner answers it fresh
without spending budget. A problem that changes while its solve is under way drops that
solve and begins the new one: finishing an answer to a question nobody is asking would
spend the budget on nothing.

**While the solve runs**, the planner answers `PlanOutcome::Stale` with the last plan it
did compute, when it computed it, and how far the current solve has got, and
`is_healthy()` is false. The first call whose solve finishes answers fresh and healthy.

**Determinism.** Two planners given the same tracks, resources and rewards produce the
same recommendation; only the plan identifier differs, by D-56's design. Ties fall to the
rule written in `gungnir-allocation/src/bellman.rs`'s module documentation: the higher
total, then more pairs, then the first matching in enumeration order.

**User-interface delta.** PN-05 draws the plan's standing above it: nothing when current;
"STALE", when it was computed, how long before the planner was last asked, and why, when
not; "NO PLAN" and why when the planner has never answered. PN-07 names the stale plan
among the degraded conditions with the same age and reason. Accept stays available after
the operator acknowledges that condition, which is D-82: blocking it would leave the
operator no way to act on the best available recommendation during an engagement, and the
acknowledgement plus the health change on the journal is what MOE-06 counts.

**Verification.** The row is unchanged and waits for the owner's walk. The tests it will
be walked against: `gungnir-intercept-service/src/lib.rs`
(`two_fresh_planners_agree_when_every_reward_ties`,
`two_fresh_planners_agree_when_the_rewards_are_distinct`,
`an_over_budget_solve_returns_the_last_good_plan_stale`,
`a_solve_longer_than_one_budget_carries_on_across_calls`), and
`gungnir-app/tests/solve_budget.rs` for what the operator sees.

**What it leaves open.** A picture toward the exact solver's size limits takes seconds of
solving -- six effectors and ten tracks at the default horizon took 1.3 s in the probe --
and so far longer at 4 ms a tick, and at the limits themselves longer than any
engagement lasts; it is answered stale with its progress meanwhile, and whether a bounded
answer should stand in for it is GAP-156. A desktop linked to a node is told the node's
planner is unhealthy (GAP-161) but not how old the node's plan is or why, so PN-05 draws
it without a stale line; that is GAP-157.

## 11. Amendment 3 -- an interim answer, and the node's word on its plan

**Raised by GAP-156 and GAP-157**, which §10's build filed. Decided under the owner's
delegation of 2026-09-26 as D-93 (what stands in, when, and how it is labelled) and D-94
(what a node says about its plan, and how a linked desktop draws it).

**What stands in (D-93).** The best assignment for this step alone:
`gungnir_allocation::solve_one_step`, the exact answer of the allocator's own problem at a
horizon of one, found by the Hungarian method in polynomial time and with no size limit,
under the same tie rule as the exact solve (the higher total, then more pairs, then the
first matching in its enumeration order). It is not a heuristic dressed as the optimum: it
is an optimum of a stated, smaller problem, and inside the exact solver's limits it names
the same assignment the exact solve names at a horizon of one
(`gungnir-allocation/tests/one_step_oracle.rs`; the module documentation of
`gungnir-allocation/src/one_step.rs` states the one place they can part, two different
sums of fractions that round to within the exact solve's tie tolerance of each other). `gungnir_allocation::stand_in` adds how
good it is known to be over the full horizon: a floor, the value of following it with
one-step answers (a policy the plan can actually be continued with), and a ceiling on the
optimum, the smaller of every track's best reward summed and the horizon times the best
one-step value. The exact optimum lies between them, so "worth at least 87% of the best
plan's value" is a claim the numbers support. On a uniform matrix -- the one the desktop
and the node plan on today -- the stand-in and the exact first step name the same pairs.
It costs microseconds: 16 µs at the exact solver's limits (eight effectors, sixteen
tracks) and 0.24 ms at sixteen and sixty-four, in release on the development machine.

**When (D-93).** Once the planner has been behind the picture for `plan_stand_in_after_ms`
of mission time -- 500 ms by default, MOP-07's figure (`../mission/measures.md` §2) --
measured from the first call it could not answer since its last fresh answer, so a raid
whose picture changes every few ticks still reaches it; and at once for a picture past
`MAX_TRACKS` or `MAX_RESOURCES`, which the exact solver refuses and no wait would answer.
Validation keeps the wait finite, not negative and at most a minute. Inside the wait the
planner answers exactly as §10 says, stale with the last good plan: an ordinary picture
finishes inside it and never shows a stand-in, so the queue is not given an interim item
for every track that appears.

**How it is labelled (D-93).** The plan carries `PlanBasis::OneStep` (`gungnir-model`),
which travels wherever the plan does -- the approval queue, the journal, the link, a queue
item's view. The planner answers `PlanOutcome::Interim` with the bound and the reason, and
stays unhealthy, so the status strip and the record say the planner is not giving its own
answer (MOE-06). PN-05 draws "INTERIM" above the plan with the bound and the reason, and
the plan's own label under a stale line if an interim plan later goes stale; PN-06 marks the
item's row INTERIM; PN-07 names the planner's interim standing and, separately, the item's
own basis among the degraded conditions, so accept waits on an acknowledgement exactly as
D-82 has it for a stale plan.

One pairing is one plan, across an interim answer as everywhere else (GAP-097). When
the exact solve finishes with a different assignment, that is a new plan; with the same
assignment, the plan in force stands. A stand-in that recommends what the plan in force
already recommends keeps that plan too, under the interim standing. A plan's basis is
how it was reached and never changes; PN-05 says so beside it, and says so differently
once the planner's full solve has reached the same assignment for the picture on screen
-- then PN-07 asks nothing more about it either. An earlier draft minted a new plan for
the same pairing in both directions; `gungnir-app/tests/rehearsal.rs` failed on a slow
runner with the live pairing queued three times, and
`the_seed_queues_the_same_plans_however_slow_the_planner_is` now runs that session at
eight planner speeds.

**What a node says (D-94).** `gungnir_model::PlanStandingView` -- current, interim with the
bound and reason, stale since `computed_at` with the reason, or no plan -- published by
the node as `InterceptEvent::PlanStanding` when it changes, after the plan it describes,
and carried in the snapshot's `plan_standing` for a desktop that connects mid-stall. It
carries no progress figure: a solve's progress changes every tick, and a standing that did
would put an event on the stream and the journal at the tick rate for as long as a solve
ran. The reason that does travel is the stable one, and the progress stays on the node;
`PlanOutcome` therefore carries the progress beside the reason, not inside it.

**How a linked desktop draws it (D-94).** `RemoteInterceptService` answers what the node
said: current is fresh, interim is interim with the node's bound, stale is stale, no plan is
no plan, each reason prefixed "on the node". A `computed_at` the node stamped is converted to
the desktop's clock through the offset the service measures per connection by GAP-140's
rule, so the age PN-05 draws is the age on the node's clock, as the queue's deadlines are
drawn (D-70; DN-31 §14). A node that sends no standing is taken as before: its plan, fresh,
with its health flag beside it (GAP-161).

**Verification.** Three rows are drafted in `../verification-capability-table.md` §2 under
"Rows drafted from DN-04 §11", Draft and not agreed. The tests:
`gungnir-allocation/tests/one_step_oracle.rs`; `gungnir-intercept-service/src/lib.rs`
(`an_interim_answer_stands_in_once_the_planner_has_waited`,
`a_picture_that_keeps_changing_still_reaches_its_stand_in`,
`a_picture_past_the_exact_limits_is_answered_at_once`);
`gungnir-app/tests/interim_plan.rs` for what the operator sees; and
`gungnir-app/tests/linked_plan_standing.rs`, end to end over the real link, with the
desktop's clock a hundred seconds from the node's. The `gungnir-intercept-service` row's
criterion is unchanged; that its degradation clause now describes only the first half a
second of an overrun is GAP-168, for the owner's walk.

**What it leaves open.** A desktop built before `PlanView::basis` existed reads a newer
node's interim plan as the optimum, because a defaulted field is compatible by the
interface's rules; whether that warrants a schema version is GAP-169.

## Traceability

GAP-030; GAP-031 (§9); GAP-119 (§10); GAP-156, GAP-157 (§11); CAP-3.3, CAP-7.3; MOE-03; MT-01 step 6, MT-02, MT-03;
`../ux/wireframes/WF-05-recommendation.puml`; principle AP-17 for the rule that the
measure defines the field rather than the reverse. Read by DN-05, DN-06, DN-07.
