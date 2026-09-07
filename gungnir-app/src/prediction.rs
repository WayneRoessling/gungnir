// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Trajectory prediction on the desktop (GAP-020, docs/design/DN-02-prediction-and-approach.md).
//!
//! `gungnir-assessment` owns the predictor; this module is the wiring: the horizons
//! from the baseline, the asset anchors from the local frame, the tracks from the
//! picture, and the honest states in between. Every line the viewport draws and every
//! approach PN-04 lists says which predictor produced it, because a constant-velocity
//! prediction of a manoeuvring drone is wrong in a way an operator can compensate for
//! only if they know that is what they are looking at (DN-02 §5).

use gungnir_assessment::{FilterPredictor, Prediction, PredictorKind, TrajectoryPredictor};
use gungnir_model::TrackId;

use crate::state::AppState;

/// Predictions for this frame's picture, or why there are none.
#[derive(Debug, Clone, PartialEq)]
pub enum PredictionOutcome {
    Predicted(Vec<Prediction>),
    /// Nothing was predicted, and why. Distinct from an empty picture.
    NotPredicted {
        reason: &'static str,
    },
}

impl PredictionOutcome {
    #[must_use]
    pub fn predictions(&self) -> &[Prediction] {
        match self {
            PredictionOutcome::Predicted(p) => p,
            PredictionOutcome::NotPredicted { .. } => &[],
        }
    }

    #[must_use]
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            PredictionOutcome::Predicted(_) => None,
            PredictionOutcome::NotPredicted { reason } => Some(reason),
        }
    }
}

/// Predict every track in the picture to the baseline's horizons, against the
/// declared assets. Approaches need the assets placed in the frame; the lines do not,
/// so a deployment with no origin still gets lines and no approaches, and says so.
#[must_use]
pub fn predict(state: &AppState) -> PredictionOutcome {
    let horizons = &state.config.assessment.prediction_horizons_s;
    if horizons.is_empty() {
        return PredictionOutcome::NotPredicted {
            reason: "the baseline declares no prediction horizons",
        };
    }
    let anchors = crate::sustainment::asset_anchors(state).unwrap_or_default();
    // GAP-020: the model the tracker runs, with the process noise it runs it with, so a
    // predicted ellipse is the filter's own uncertainty rather than a second opinion
    // about it. `PredictorKind::Filter` says which on every prediction.
    PredictionOutcome::Predicted(
        FilterPredictor::new(state.pipeline.process_noise_psd).predict(
            state.tracking.tracks(),
            horizons,
            &anchors,
        ),
    )
}

/// One approach line for PN-04.
#[derive(Debug, Clone, PartialEq)]
pub struct Approach {
    pub asset: String,
    pub time_ahead_s: f64,
    pub distance_m: f64,
    pub arrives: bool,
    pub predictor: PredictorKind,
}

/// The approaches one track makes, by asset name, arriving first then nearest.
#[must_use]
pub fn approaches(state: &AppState, outcome: &PredictionOutcome, track: TrackId) -> Vec<Approach> {
    let Some(p) = outcome.predictions().iter().find(|p| p.track == track) else {
        return Vec::new();
    };
    let mut lines: Vec<Approach> = p
        .approaches
        .iter()
        .map(|a| Approach {
            asset: state
                .config
                .assets
                .iter()
                .find(|c| c.id == a.asset.0)
                .map_or_else(|| format!("asset {}", a.asset.0), |c| c.name.clone()),
            time_ahead_s: a.time_ahead_s,
            distance_m: a.distance_m,
            arrives: a.distance_m <= 0.0,
            predictor: p.predictor,
        })
        .collect();
    lines.sort_by(|a, b| {
        b.arrives
            .cmp(&a.arrives)
            .then(a.distance_m.total_cmp(&b.distance_m))
    });
    lines
}

/// A predicted line for the viewport: the points, owned, so the layer can borrow them.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedPath {
    pub track: u64,
    pub points: Vec<[f64; 3]>,
    /// False when any point's uncertainty is not drawable (DN-02 §5 rule 3), so the
    /// layer draws the line and no ribbon.
    pub uncertainty_drawable: bool,
    pub predictor: PredictorKind,
}

/// The lines, one per predicted track with at least two points.
#[must_use]
pub fn paths(outcome: &PredictionOutcome) -> Vec<PredictedPath> {
    outcome
        .predictions()
        .iter()
        .filter(|p| p.points.len() >= 2)
        .map(|p| PredictedPath {
            track: p.track.0,
            points: p.points.iter().map(|pt| pt.position_enu).collect(),
            uncertainty_drawable: p
                .points
                .iter()
                .all(gungnir_assessment::PredictedPoint::uncertainty_is_drawable),
            predictor: p.predictor,
        })
        .collect()
}

/// The predictor's name for a panel.
#[must_use]
pub fn predictor_name(kind: PredictorKind) -> &'static str {
    match kind {
        PredictorKind::ConstantVelocity => "constant-velocity prediction",
        PredictorKind::Filter => "filter prediction",
    }
}
