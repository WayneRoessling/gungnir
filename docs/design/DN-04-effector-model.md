# DN-04 Effector layer and cost model

Closes GAP-030. Status: signed and **implemented and wired** (GAP-030, 2026-09-06). **Amendment 1 (§9), signed by the owner 2026-09-06**: the closing speed GAP-031's geometry needs.

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

## 9. Amendment 1 -- a closing speed, for the intercept geometry (**signed by the owner 2026-09-06**)

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

## Traceability

GAP-030; GAP-031 (§9); CAP-3.3; MOE-03; MT-01 step 6, MT-02, MT-03;
`../ux/wireframes/WF-05-recommendation.puml`; principle AP-17 for the rule that the
measure defines the field rather than the reverse. Read by DN-05, DN-06, DN-07.
