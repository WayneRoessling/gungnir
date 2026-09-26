# DN-02 Trajectory prediction and closest point of approach

Closes GAP-020. Status: first draft 2026-09-05; **implemented the same day** (`gungnir-assessment/src/prediction.rs`) and **wired on 2026-09-06** (the desktop predicts every frame, PN-04 and the viewport draw it, DN-03 reads it). The filter predictor landed with GAP-011. §7's approach corridors -- the first-engagement range PN-16 compares laydowns on -- are specified by §9, Amendment 1 (D-45, D-107 to D-109), and **built 2026-09-26** (`gungnir-assessment/src/first_engagement.rs`).

## 1. The gap and the thread step it blocks

MT-01 step 4 and MT-02 step 3 order a raid by time to impact. MT-04 warns a port before a
surface craft arrives. `ClosingSpeedAssessor` divides range by closing speed against one
point, which is a straight-line approximation to a single target and produces nothing at
all for a craft that will pass close rather than arrive.

## 2. The owning component

`gungnir-assessment`, which already owns `RiskScore` and `time_to_impact_s`. The
prediction itself uses the filter state that `TrackView` already carries, so nothing new
crosses a crate boundary.

## 3. Types

In `gungnir-assessment`:

```rust
/// A predicted position with the uncertainty that goes with it. Reported at the
/// horizon the caller asked for, never extrapolated further silently.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PredictedPoint {
    pub time_ahead_s: f64,
    pub position_enu: [f64; 3],
    /// One-sigma along-track and cross-track, metres, grown from the track
    /// covariance. The UI draws it; the score does not use it beyond gating.
    pub sigma_along_m: f64,
    pub sigma_cross_m: f64,
}

/// Closest point of approach to one asset. Distinct from time to impact:
/// a craft that will pass a port at 400 m never "impacts" it and still matters.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClosestApproach {
    pub asset: AssetId,
    pub time_ahead_s: f64,
    pub distance_m: f64,
}

pub struct Prediction {
    pub track: TrackId,
    pub points: Vec<PredictedPoint>,
    pub approaches: Vec<ClosestApproach>,
}

pub trait TrajectoryPredictor: Send + Sync {
    /// Predict each track forward to the given horizons.
    fn predict(&self, tracks: &[TrackView], horizons_s: &[f64]) -> Vec<Prediction>;
}
```

`AssetExposure` from DN-01 gains `closest_approach_m: Option<f64>` so that a score can
express "will pass close" as well as "will arrive".

## 4. Edges

**None.** `gungnir-assessment` depends on `gungnir-model`, which owns `AssetId` after
DN-01.

## 5. Behaviour

Two predictors, and the distinction between them is the honest-status question:

- **`ConstantVelocityPredictor`** propagates the state vector and grows the covariance
  linearly. It needs nothing from the tracking core, works today, and is what the design
  assumes until the pipeline exists.
- **`FilterPredictor`** uses the pipeline's own motion model, so a turning track predicts
  as a turn rather than as a tangent. It arrives with GAP-011.

The predictor in use is reported on the prediction, and the panel says which one produced
the line it draws. A constant-velocity prediction of a manoeuvring drone is wrong in a way
the operator can compensate for **only if they know that is what they are looking at**.

Closest approach is computed analytically against each asset extent from DN-01: for a
point, the minimum of the range function along the predicted segment; for a circle, that
minimum less the radius, floored at zero.

**Rules that keep the output honest:**

1. A prediction beyond the configured horizon is not produced. It is not clamped and
   presented as if it were computed.
2. A stale track is not predicted at all. Extrapolating a track nobody has observed for
   thirty seconds produces a confident line to a place nothing is.
3. When the covariance is not positive semi-definite, which the tracking core's own
   invariant forbids but which this crate must survive, the sigmas are reported as
   non-finite and the panel draws no ellipse rather than a nonsense one. `TrackView`'s
   existing `position_sigma` already takes this approach.

## 6. Configuration and interface delta

`PolicySettings` gains nothing. Prediction horizons belong with the assessment settings:
`ConfigBaseline.assessment.prediction_horizons_s: Vec<f64>`, defaulting to a short set,
validated as finite, positive, and ascending.

Interface: `RiskScore` already appears in derived views; `Prediction` is added to the
snapshot as `predictions: Vec<Prediction>` behind the same additive rule.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-02 Viewport | Predicted track lines with an uncertainty ribbon, drawn differently from observed history so the two are never confused |
| PN-04 Track detail | Time to impact per asset and closest approach, with the predictor named |
| PN-05 Recommendation panel | Time remaining, which is what orders the queue |
| PN-16 Planning panel | Approach corridors, which are the aggregate of predictions over a rehearsal: each approach's first-engagement range per laydown, the worst case over the run (§9) |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-2.8 Predict trajectory and approach | Closed-form comparison on synthetic straight and turning tracks, plus a replay | Predicted position error against truth is within tolerance for straight-line motion; closest approach matches the analytic minimum to 1e-6; no prediction is produced for a stale track or beyond the horizon; the predictor in use is reported on every prediction | TT-01 and TT-04 sample sets, which carry truth |

## 9. Amendment 1 -- first-engagement range over a rehearsal (2026-09-26)

**Raised by GAP-020, whose predictor half was built and whose PN-16 half stopped at §7's
one line**: an approach corridor's first-engagement range is "the aggregate of predictions
over a rehearsal", and nothing said which predictions or how they aggregate. D-45 (the
owner, 2026-09-15) settled the aggregate; D-107, D-108 and D-109 (2026-09-26, under the
owner's delegation) settled what it is taken over. Reasoning:
`../record/2026-09-26/first-engagement-range-over-a-rehearsal.md`.

**The rule, whole.**

1. **A prediction is one recorded target's first engagement** (D-107). During a
   rehearsal (DN-32) the throwaway desktop's planner proposes plans; each pairing carries
   DN-04 §9's earliest constant-velocity intercept point, which is this note's
   constant-velocity prediction of the track taken to the moment the effector reaches it.
   A target's first engagement is the first pairing proposed for a track that *is* that
   target and carries an intercept point. A track is a target's when its position lies
   within the 0.999 gate of its own position covariance (χ², three degrees of freedom,
   16.27) about where the recording's truth says the target was at the track's estimate
   time; nearest target first. A paired track that is no target is clutter the pipeline
   formed from false alarms: it is counted and shown, never measured, because a worst case
   over clutter paired beside a radar is a figure about the radar's false alarms. A target
   paired only with no intercept point (no closing speed, or outrun) is counted against
   its approach and given no range.
2. **It belongs to the approach whose corridor the target was in** when the pairing was
   proposed -- where the recording says the target was, not where its track said, since
   which approach a target came down is a fact of the recording (D-108). An approach
   declares its corridor as `corridor_half_width_m`, the ground distance either side of its
   axis; nearest axis wins where corridors overlap. **An approach with no declared width
   takes no prediction** and says so: guessing which targets came down it would be a
   default standing in for a mission-analysis answer. A target in no corridor is counted.
3. **Its range is the ground distance from the predicted intercept point to the
   approach's inner end** (D-107): the last point the approach declares, which
   `ApproachConfig` now states is where the approach leads (outer end first).
4. **The approach's first-engagement range, for one laydown, is the worst case: the
   minimum over its last rehearsal's predictions** (D-45), shown as "worst N km (n = k)"
   with k the predictions it is over, and against the current laydown's in words
   ("farther out", "closer in") when both were rehearsed against the same recording and
   seed.
5. **No prediction is not zero.** An approach nothing was engaged on, one with no
   corridor, and a deployment with no approaches or no origin each read *not computable*
   with the reason. A laydown never rehearsed reads *not rehearsed*.
6. **Provenance travels with the figure**: which rehearsal (the laydown's last), which
   recording and seed, when it was run (the desktop's mission time), and behind the worst
   case the target, its track, the effector and when in the recording it was proposed.
7. **A rehearsal is a measurement of the recording, not of the machine** (D-109). The
   throwaway desktop's tracker runs in lock-step on the rehearsal's own thread
   (`LiveTrackingService::lockstep`, through the same `gungnir_fusion_async::step` the
   live ingest task calls), and its planner measures its solve budget on a clock that
   never advances. Before this, the mid-run picture was however far the pipeline task on
   another thread had got, so the plans -- and every figure read off them -- differed
   between two runs of one laydown.

**Owning components.** The aggregate is `gungnir-assessment` (this note's §2), a pure
function over a run's first pairings and the approaches in one frame. The run's pairings
are `gungnir-app/src/laydown_rehearsal.rs`'s, which alone holds the recording's truth. The
corridor width is `gungnir-config`. PN-16's column and the rehearsal section's account are
`gungnir-ui`. The lock-step driver is `gungnir-fusion-async` (the loop body extracted, not
copied) and `gungnir-tracking-service`. **No dependency edge is added.**

**Frame.** The approaches are placed with the deployment's frame and the run's positions
are in the recording's; DN-32 §5.5 reads a laydown as an arrangement about the recording's
origin, and the approaches are part of the same arrangement.

**Configuration delta.** `ApproachConfig.corridor_half_width_m: Option<f64>`, absent by
default, finite and positive when present; a baseline written before it stays valid and
reads its approaches as declaring no corridor. `SUPPORTED_CONFIG_VERSION` unchanged.
`testdata/usability/round-1.json` declares 3 km for the upper Vell approach.

**User-interface delta.** PN-16's options table gains one column per declared approach,
"<approach>: first engagement (worst case)", and a caption stating the rule; each row's
rehearsal cell says when it was run; the rehearsal section lists each approach's corridor,
worst case, count and the target behind it, and counts targets in no corridor and clutter
paired.

**Verification.** D-45 and §7 specify no verification row, and none is added. The rule is
tested as a pure function in `gungnir-assessment/src/first_engagement.rs`; end to end
against the committed TT-01 sample set with two laydowns in
`gungnir-app/tests/laydown_rehearsal.rs`'s
`first_engagement_is_the_worst_case_over_a_committed_recording_per_laydown` (an approach
nothing came down reads not computable), and against round 1's committed laydowns over TT-11, round 1's committed recording (GAP-147,
D-112), in `round_1s_forward_radar_changes_its_own_detections_and_nothing_else` (laydown
`c` first engages farther out than `current`, `b`'s moved battery predicts its own
intercepts; clutter is counted and not measured) and, over a plan-07 recording that never
reaches round 1's radars,
`round_1_against_a_plan_07_recording_is_not_computable_and_says_why`; the lock-step
driver against the offline batch over all ten sample sets in
`gungnir-tracking-service/tests/sample_set_replay.rs`; PN-16's drawing in
`gungnir-ui/src/panels/rendered.rs`.

## Traceability

GAP-020; CAP-2.8; MOP-28 monotonicity with DN-01; MT-01 step 4, MT-02 step 3, MT-04;
depends on DN-01 for assets and GAP-011 for the filter predictor;
`../ux/wireframes/WF-02-viewport.puml`, `WF-04-track-detail-evidence.puml`; principle
AP-02. Read by DN-03 and DN-06.
