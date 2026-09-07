# DN-05 Fires plan type and deconfliction

Closes GAP-036. Status: first draft, 2026-09-05. **Design only; no code exists.**
D-07 put fires in the first release, so this is release content and not a domain extension.

## 1. The gap and the thread step it blocks

MT-06 steps 5 to 7 cue fires against a located battery. The plan and approval machinery is
domain-neutral and works, but there is no fires plan, no deconfliction against friendly
positions or airspace measures, and no handoff, so those three steps happen outside the
system.

## 2. The owning component

`gungnir-model` owns the plan variant, because the plan is the canonical recommendation
type that policy, command, the interface, and the interface all carry.
`gungnir-policy` owns the deconfliction rules, because they are denials.

Both already depend on what they need. `gungnir-policy` depends on `gungnir-model` and
`gungnir-geo`, and geo is where airspace measures and no-fire areas live. **No new edge.**

## 3. Types

In `gungnir-model`:

```rust
/// What a plan proposes. `Intercept` is today's behaviour, unchanged.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlanKind {
    Intercept(Vec<InterceptSolutionView>),
    Fires(FiresPlan),
}

/// A fires task against a located ground target. Distinct from an intercept
/// because the target does not move toward us, the effect is on the ground, and
/// the deconfliction question is about who else is there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FiresPlan {
    pub target: TrackId,
    pub target_position: Geodetic,
    /// One-sigma target location error, metres. Fires against a poorly located
    /// target is a different decision from fires against a well located one, and
    /// the operator must see which they are being asked to approve.
    pub location_error_m: f64,
    pub firing_unit: ResourceId,
    /// Requested time on target, if the task is time-constrained.
    pub time_on_target: Option<MissionTime>,
    pub deconfliction: DeconflictionResult,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeconflictionResult {
    pub checks: Vec<DeconflictionCheck>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeconflictionCheck {
    pub kind: DeconflictionKind,
    pub passed: bool,
    /// Why, in words the panel shows. Never empty on a failure.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeconflictionKind {
    FriendlyPosition,
    NoFireArea,
    AirspaceMeasure,
    InterceptorTrajectory,
    /// The target's location error exceeds what the policy permits for fires.
    LocationAccuracy,
}
```

`PlanView.solutions` is replaced by `PlanView.kind: PlanKind`. This is **not** additive:
it changes an existing field's type, which under the interface compatibility rules
requires a new schema version.

**Decided by the owner on 2026-09-05: replace it outright.** `SCHEMA_VERSION` goes to 2
and the interface path goes to `/v2`. No deprecated mirror of `solutions` is kept. The
contract's rule that `v1` is removed only after every known client has moved is satisfied
because the set of known clients is empty: the transport is not in the workspace and
nothing is deployed. Recorded in
[`model-and-schema-deltas.md`](model-and-schema-deltas.md) §3, which is the one breaking
change in this design set.

## 4. Edges

**None.**

## 5. Behaviour

`gungnir-policy` gains a `FiresDeconflictionPolicy` that runs every check and returns a
`PolicyVerdict`. The rules:

1. **Friendly position.** The target's error ellipse must not contain a known friendly
   position. Checked against positions the picture holds, not against a separate list.
2. **No-fire area.** The error ellipse must not intersect a no-fire geofence. `Geofence`
   already exists with a `no_go` flag; fires needs a distinct `no_fire` meaning, because an
   area interceptors may not enter is not the same as an area artillery may not strike.
   `LayerKind` gains a `NoFireArea` variant.
3. **Airspace measure.** The trajectory must not violate a declared airspace measure.
4. **Interceptor trajectory.** The fires trajectory must not conflict with an intercept
   solution in the same plan or in a plan in force.
5. **Location accuracy.** `location_error_m` must be within the policy's limit for fires.

**Every check runs and every result is reported, including the ones that passed.** A
verdict that says only "denied: no-fire area" hides that the friendly-position check also
failed, and the operator who clears the first obstacle then believes the task is clean.

A check that **cannot** be evaluated, because the data is absent, does not pass. It fails
with a detail saying the data was missing. That is AP-02 applied to a safety check: an
unevaluated check reported as passed is the worst possible output of this component.

Nothing about fires changes the authority model. A fires plan is a recommendation, goes
through the same approval workflow, and needs a recorded human decision (AP-01).

## 6. Configuration and interface delta

`PolicySettings` gains `fires: FiresSettings { max_location_error_m: f64,
minimum_separation_m: f64 }`, validated as finite and positive.

Interface: `POST /v2/plans/{plan_id}/decision` is unchanged; the plan it carries gains the
variant. The fires handoff message itself is DN-07, which defines one handoff shape for
both intercept and fires rather than two.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-05 Recommendation panel | Fires plans show the target, its location error, the firing unit, and every deconfliction check with its result. Failed checks are text, not colour alone |
| PN-07 Decision dialog | A fires task cannot be accepted while any check has failed or could not be evaluated; the control is disabled with the reason beside it |
| PN-02 Viewport | No-fire areas as a layer, and the target error ellipse |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-3.8 Fires tasks | Unit tests per check plus a replay of MT-06 | Every check appears in the result whether it passed or failed; a check with missing data fails with a stated reason and never passes; a plan with any failed check cannot be accepted; a target whose error ellipse touches a no-fire area is denied | TT-06 sample set; generated friendly positions and areas |

## Traceability

GAP-036; CAP-3.8; D-07; MT-06 steps 5 to 7; depends on DN-04 for the firing unit's layer
and DN-07 for delivery; `../ux/wireframes/WF-05-recommendation.puml`,
`WF-07-decision-dialog.puml`; principles AP-01, AP-02.
