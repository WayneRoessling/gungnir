# DN-13 Sensor re-tasking recommendation

Closes GAP-037. Status: first draft, 2026-09-05. **Design only; no code exists.**

## 1. The gap and the thread step it blocks

MT-07 step 3 asks the sensor manager to restore coverage after losing a sensor to jamming
or a fault, under time pressure, by changing the modes of what remains. `gungnir-decision`
is domain-neutral and proposes nothing about sensors, so the step is manual arithmetic at
the worst possible moment.

## 2. The owning component

`gungnir-decision`, which owns the recommendation and what-if surface. It already produces
courses of action for engagements; a sensor plan is another course of action with a
different cost function.

## 3. Types

In `gungnir-decision`:

```rust
/// A proposed set of sensor mode changes, with the coverage it buys.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorPlan {
    pub changes: Vec<SensorModeChange>,
    /// Gaps before the changes and after them: the rationale, in the only form
    /// that means anything to a sensor manager.
    pub gaps_before: Vec<CoverageGap>,
    pub gaps_after: Vec<CoverageGap>,
    pub cost: SensorPlanCost,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorModeChange {
    pub sensor: SensorId,
    pub from: SensorMode,
    pub to: SensorMode,
}

/// What the plan gives up. A mode that closes one gap usually opens another.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorPlanCost {
    /// Approach metres uncovered after, minus before. Negative is an improvement.
    pub delta_uncovered_m: f64,
    /// Approach metres that drop from redundant to single-sensor.
    pub redundancy_lost_m: f64,
    pub sensors_changed: usize,
}

pub trait SensorPlanner: Send + Sync {
    /// Candidate plans, best first. Empty when no change improves coverage.
    fn recommend(
        &self,
        sensors: &[(SensorId, SensorRecord)],
        approaches: &[&[[f64; 3]]],
    ) -> Vec<SensorPlan>;
}
```

## 4. Edges

**One: `gungnir-decision` to `gungnir-analytics`.**

`gungnir-decision` depends on `gungnir-model`, `gungnir-assessment`, and `gungnir-policy`.
It needs `combined_coverage` and `CoverageGap` from DN-12 to compute and to express the
rationale. Passing gaps in from the binary was the alternative; the edge is taken because
the recommender must call the coverage function **inside its search**, once per candidate
plan, not once on the caller's behalf. A search that cannot evaluate its own candidates is
not a search.

Acyclic: `gungnir-analytics` depends on coord, data, geo, and, after DN-12,
sensor-management. None of those is decision or assessment or policy.

Recorded in [`dependency-edges.md`](dependency-edges.md).

## 5. Behaviour

**The search.** Enumerate candidate mode changes over sensors that are not offline,
respecting the mode transition rules `SensorMode::can_transition_to` already enforces, and
score each candidate by the cost above. This is a small combinatorial search over a handful
of sensors, not an optimizer; a greedy pass with a bounded candidate set is enough and is
what the design assumes.

Rules:

1. **A candidate that violates a transition rule is never proposed.** The registry's state
   machine is the constraint, not a preference. A sensor coming out of offline must pass
   through standby, and a recommendation that skips it would be rejected at execution and
   waste the operator's time.
2. **Every plan reports what it costs as well as what it buys.** `redundancy_lost_m` exists
   because the tempting plan under pressure is to swing everything toward the hole and
   leave the rest single-sensor. The sensor manager must see that trade, not discover it.
3. **An empty result is a real answer.** When no change improves coverage, the recommender
   returns nothing and the panel says no change helps, rather than proposing the least bad
   change as though it were an improvement.
4. **It recommends; it does not task.** Accepting a sensor plan issues the tasks through
   DN-11, each of which is an authorized action with its own record. There is no path from
   this crate to a sensor (AP-01).

**Degradation.** When coverage cannot be computed, because the registry is unavailable or
no approaches are configured, the recommender returns nothing and says why. It does not
fall back to proposing changes with an unstated rationale.

## 6. Configuration and interface delta

`ConfigBaseline.analytics` gains `max_sensor_plan_candidates: usize`, bounding the search,
defaulted and validated positive.

Interface: `GET /v2/sensor-plans` returning `Vec<SensorPlan>`, authorization action
`sensor.task`, because seeing the recommendation is part of the tasking role rather than
of the general picture.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-11 Coverage layer controls | Before-and-after comparison for a selected plan, which is what the panel's before-and-after control was designed for |
| PN-10 Sensor management | Candidate plans with their cost, and an accept that issues the tasks through DN-11 |
| PN-08 Alerts | A coverage loss raises an alert; the alert links to the recommendation rather than embedding it |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-3.9 Sensor re-tasking | Property tests over generated sensor sets, plus an MT-07 replay with a scripted loss | No proposed change violates a mode transition rule; every plan's `gaps_after` matches an independent evaluation of the same coverage function; no plan is returned when none improves coverage; accepting a plan produces one authorized task per change and no direct sensor call | Generated layouts; TT-07 sample set |

## Traceability

GAP-037; CAP-3.9; MT-07 step 3; depends on DN-12 for coverage and DN-11 for issuing;
`../ux/wireframes/WF-11-coverage-layers.puml`, `WF-10-sensor-management.puml`; principles
AP-01, AP-02.
