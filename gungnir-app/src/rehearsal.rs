//! A seeded session for usability rounds (GAP-089, docs/ux/usability-round-1-session.md §3).
//!
//! **Written when the live desktop had neither tracks nor plans**, and both now exist
//! (GAP-011 and GAP-029, 2026-09-06). The seed keeps its purpose: a usability round
//! needs the *same* picture in front of every participant, and a live tracker fed by
//! whatever sensors a laptop has is not that. What has changed is that the desktop's
//! own planner now proposes against the seeded tracks alongside the seeded plans, so a
//! session is a scripted picture with a real planner running on it rather than a
//! script throughout. This module puts the seed in place through the same surfaces a
//! live picture and a live planner would use, and
//! **marks everything it causes as a rehearsal**: the journal opens with a
//! `RehearsalEvent::Started` carrying the seed's hash, and PN-01 says "rehearsal" for the
//! whole session, so a seeded record can never be read as an operation.
//!
//! What it does not do: it does not produce detections (nothing would turn them into
//! tracks), it does not stop the allocator running on what it seeds, and it refuses a
//! baseline whose backend is
//! not the embedded one, because a rehearsal against a node would seed a shared picture.

use std::sync::Arc;

use gungnir_eventing::Event;
use gungnir_model::events::RehearsalEvent;
use gungnir_model::{
    Classification, DetectionView, InterceptSolutionView, MissionTime, PlanId, PlanView,
    Provenance, Quality, Releasability, ResourceId, TrackId, TrackStatus, TrackView,
};
use gungnir_tracking_service::{SubmitError, TrackingService};
use sha2::Digest;

use crate::state::AppState;

/// One scripted track: appears at `at_s`, moves at constant velocity, and goes stale
/// after `stale_after_s` if given.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SeedTrack {
    pub id: u64,
    pub at_s: f64,
    pub position_enu: [f64; 3],
    pub velocity_mps: [f64; 3],
    #[serde(default)]
    pub classification: Classification,
    #[serde(default)]
    pub stale_after_s: Option<f64>,
}

/// One scripted plan, submitted through the policy chain and the queue at `at_s`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SeedPlan {
    pub id: u64,
    pub at_s: f64,
    pub resource: u32,
    pub track: u64,
}

/// One scripted alert line at `at_s`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SeedAlert {
    pub at_s: f64,
    pub text: String,
}

/// The seed file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Seed {
    pub name: String,
    #[serde(default)]
    pub tracks: Vec<SeedTrack>,
    #[serde(default)]
    pub plans: Vec<SeedPlan>,
    #[serde(default)]
    pub alerts: Vec<SeedAlert>,
    /// Resources marked not ready at the start, so a plan against one is refused on
    /// readiness (US-02).
    #[serde(default)]
    pub resources_not_ready: Vec<u32>,
}

/// Why a rehearsal could not be installed.
#[derive(Debug, thiserror::Error)]
pub enum RehearsalError {
    #[error("cannot read the seed {path}: {reason}")]
    Unreadable { path: String, reason: String },
    #[error("the seed {path} is not a rehearsal seed: {reason}")]
    Malformed { path: String, reason: String },
    #[error("a rehearsal runs only against the embedded backend; this baseline names a node")]
    NotEmbedded,
}

/// The picture a rehearsal shows: the seed's tracks, advanced on the clock.
pub struct RehearsalPicture {
    seed: Arc<[SeedTrack]>,
    started: Option<MissionTime>,
    tracks: Vec<TrackView>,
}

impl RehearsalPicture {
    fn new(seed: Arc<[SeedTrack]>) -> Self {
        Self {
            seed,
            started: None,
            tracks: Vec::new(),
        }
    }

    fn elapsed(&mut self, now: MissionTime) -> f64 {
        let started = *self.started.get_or_insert(now);
        now.0 - started.0
    }
}

impl TrackingService for RehearsalPicture {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        // A rehearsal picture is scripted; a detection changes nothing and says so.
        Err(SubmitError::PipelineGone)
    }

    fn poll(&mut self, now: MissionTime) {
        let elapsed = self.elapsed(now);
        self.tracks = self
            .seed
            .iter()
            .filter(|t| t.at_s <= elapsed)
            .map(|t| {
                let dt = elapsed - t.at_s;
                let mut state = nalgebra::SVector::<f64, 6>::zeros();
                for i in 0..3 {
                    state[i] = t.position_enu[i] + t.velocity_mps[i] * dt;
                    state[i + 3] = t.velocity_mps[i];
                }
                let is_stale = t.stale_after_s.is_some_and(|s| dt >= s);
                TrackView {
                    id: TrackId(t.id),
                    status: TrackStatus::Confirmed,
                    state,
                    covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 25.0,
                    classification: t.classification,
                    provenance: Provenance::default(),
                    quality: Quality {
                        is_stale,
                        ..Quality::default()
                    },
                    mission_time: now,
                    releasability: Releasability::default(),
                }
            })
            .collect();
    }

    fn tracks(&self) -> &[TrackView] {
        &self.tracks
    }

    fn is_healthy(&self) -> bool {
        // Honest: the picture is a script, and the health flag says the tracker is not
        // a tracker. PN-09 shows the rehearsal state beside it.
        false
    }
}

/// The rehearsal in progress.
#[derive(Debug)]
pub struct Rehearsal {
    pub seed: Seed,
    pub seed_hash: String,
    started: Option<MissionTime>,
    next_plan: usize,
    next_alert: usize,
}

impl Rehearsal {
    /// The seed's name and hash, for PN-01.
    #[must_use]
    pub fn label(&self) -> String {
        format!("rehearsal: {} ({})", self.seed.name, &self.seed_hash[..12])
    }
}

/// Parse a seed file.
///
/// # Errors
///
/// `RehearsalError::Unreadable` or `Malformed`, naming the path.
pub fn load_seed(path: &std::path::Path) -> Result<(Seed, String), RehearsalError> {
    let bytes = std::fs::read(path).map_err(|e| RehearsalError::Unreadable {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let seed: Seed = serde_json::from_slice(&bytes).map_err(|e| RehearsalError::Malformed {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
    Ok((seed, hash))
}

/// Install a rehearsal: swap the picture for the seed's, mark the resources, and arm the
/// schedule. The journal mark is written on the first tick, when there is a bus to write
/// it to.
///
/// # Errors
///
/// `RehearsalError::NotEmbedded` when the baseline names a node.
pub fn install(state: &mut AppState, seed: Seed, seed_hash: String) -> Result<(), RehearsalError> {
    if !matches!(state.backend, gungnir_config::BackendConfig::Embedded) {
        return Err(RehearsalError::NotEmbedded);
    }
    for r in &mut state.resources {
        if seed.resources_not_ready.contains(&r.id.0) {
            r.ready = false;
        }
    }
    state.tracking = Box::new(RehearsalPicture::new(seed.tracks.clone().into()));
    state.rehearsal = Some(Rehearsal {
        seed,
        seed_hash,
        started: None,
        next_plan: 0,
        next_alert: 0,
    });
    Ok(())
}

/// The tick step: the mark once, then the plans and alerts the schedule brings due.
pub fn tick(state: &mut AppState) {
    let now = state.clock.now();
    let Some(rehearsal) = state.rehearsal.as_mut() else {
        return;
    };
    let started = *rehearsal.started.get_or_insert(now);
    let first =
        rehearsal.started == Some(now) && rehearsal.next_plan == 0 && rehearsal.next_alert == 0;
    let elapsed = now.0 - started.0;
    let mut plans = Vec::new();
    while let Some(p) = rehearsal.seed.plans.get(rehearsal.next_plan) {
        if p.at_s > elapsed {
            break;
        }
        plans.push(p.clone());
        rehearsal.next_plan += 1;
    }
    let mut alerts = Vec::new();
    while let Some(a) = rehearsal.seed.alerts.get(rehearsal.next_alert) {
        if a.at_s > elapsed {
            break;
        }
        alerts.push(a.text.clone());
        rehearsal.next_alert += 1;
    }
    if first {
        let event = RehearsalEvent::Started {
            seed: rehearsal.seed.name.clone(),
            seed_sha256: rehearsal.seed_hash.clone(),
            at: now,
        };
        crate::update::publish(state, now, Event::Rehearsal(event));
    }
    for p in plans {
        let plan = PlanView::intercept(
            PlanId(p.id),
            now,
            vec![InterceptSolutionView {
                resource: ResourceId(p.resource),
                track: TrackId(p.track),
                intercept_point: None,
                time_to_intercept_s: None,
            }],
            0.0,
        );
        crate::update::publish(
            state,
            now,
            Event::Intercept(gungnir_model::events::InterceptEvent::PlanProposed(
                plan.clone(),
            )),
        );
        state.last_plan = plan.clone();
        let outcome = crate::decisions::submit(state, plan);
        tracing::info!(?outcome, plan = p.id, "rehearsal plan submitted");
    }
    state.alerts.extend(alerts);
}
