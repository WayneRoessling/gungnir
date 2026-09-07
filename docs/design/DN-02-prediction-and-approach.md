# DN-02 Trajectory prediction and closest point of approach

Closes GAP-020. Status: first draft 2026-09-05; **implemented the same day** (`gungnir-assessment/src/prediction.rs`) and **wired on 2026-09-06** (the desktop predicts every frame, PN-04 and the viewport draw it, DN-03 reads it). The filter predictor waits on GAP-011.

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
| PN-16 Planning panel | Approach corridors, which are the aggregate of predictions over a rehearsal |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-2.8 Predict trajectory and approach | Closed-form comparison on synthetic straight and turning tracks, plus a replay | Predicted position error against truth is within tolerance for straight-line motion; closest approach matches the analytic minimum to 1e-6; no prediction is produced for a stale track or beyond the horizon; the predictor in use is reported on every prediction | TT-01 and TT-04 sample sets, which carry truth |

## Traceability

GAP-020; CAP-2.8; MOP-28 monotonicity with DN-01; MT-01 step 4, MT-02 step 3, MT-04;
depends on DN-01 for assets and GAP-011 for the filter predictor;
`../ux/wireframes/WF-02-viewport.puml`, `WF-04-track-detail-evidence.puml`; principle
AP-02. Read by DN-03 and DN-06.
