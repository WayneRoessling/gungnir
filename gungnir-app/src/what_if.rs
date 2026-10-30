// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Dead-spot what-ifs (GAP-249, `docs/design/DN-34-sensor-dead-spots.md` amendment 4,
//! D-149 to D-152).
//!
//! A sensor manager trying one sensor changed -- switched on whatever its mode, its mast
//! raised or lowered, its sector turned, moved -- against the terrain's dead spots, without
//! editing the baseline. The live sensors' field and the field with the change are computed
//! over one grid on a worker thread, off the tick, and compared cell by cell. The map keeps
//! the live dead spots and marks over them what the change would clear and what it would
//! leave newly dark (D-150), under a line that says it is a what-if and not the live
//! picture. The controls are on PN-11 (D-152).
//!
//! **Each run is recorded on the journal with who ran it** (D-151), as
//! `PlanningEvent::DeadSpotWhatIf`, once, when its first answer is computed; so a run needs
//! somebody signed in. Recording it does not make it an act: **nothing here commands a
//! sensor**, and changing one is still PN-10's, under its own authority.

use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, TryRecvError};
use gungnir_analytics::{
    CellChange, CoverageVolume, FieldComparison, HeightField, ShadowField, ShadowMeasure,
    ShadowParameters,
};
use gungnir_model::{DeadSpotAreas, DeadSpotWhatIf, SensorId, SensorWhatIf};
use gungnir_sensor_management::{SensorMode, SensorRecord, SensorRegistry as _};
use gungnir_ui::panels::coverage_layers::WhatIfDraft;
use gungnir_viewport3d::layers::DeadSpotCell;

use crate::state::AppState;

/// The bounds a change is held to, the same as PN-11's controls: a mast is not moved by
/// kilometres, and a sensor moved beyond fifty kilometres is a laydown, not a what-if.
const MAST_M: (f64, f64) = (-200.0, 500.0);
const MOVE_M: f64 = 50_000.0;

/// What a what-if's two fields depend on.
#[derive(Debug, Clone)]
pub struct WhatIfInputs {
    /// The live searching and tracking sensors, and the same with the change.
    pub before: Vec<(SensorId, CoverageVolume)>,
    pub after: Vec<(SensorId, CoverageVolume)>,
    terrain: Arc<HeightField>,
    pub parameters: ShadowParameters,
    /// The one grid both fields are laid over: both sets' reach together.
    pub extent: [[f64; 2]; 2],
}

impl PartialEq for WhatIfInputs {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.terrain, &other.terrain)
            && self.parameters == other.parameters
            && self.extent == other.extent
            && self.before == other.before
            && self.after == other.after
    }
}

/// A what-if's answer.
#[derive(Debug, Clone)]
pub struct WhatIfAnswer {
    pub before: ShadowField,
    pub after: ShadowField,
    pub comparison: FieldComparison,
    /// The map's cells: the live dead spots, with what the change would clear and darken.
    pub codes: Vec<DeadSpotCell>,
    pub before_measure: ShadowMeasure,
    pub after_measure: ShadowMeasure,
    pub inputs: WhatIfInputs,
    pub at: Instant,
}

/// One run: the change, who ran it, and whether it is on the record yet.
#[derive(Debug, Clone)]
struct Run {
    change: SensorWhatIf,
    operator: String,
    role: String,
    describes: String,
    recorded: bool,
}

/// The what-if state on this desktop (module documentation).
#[derive(Debug, Default)]
pub struct WhatIf {
    /// The change being set on PN-11, before it is run.
    pub draft: WhatIfDraft,
    run: Option<Run>,
    /// The last answer for the run.
    pub answer: Option<WhatIfAnswer>,
    pending: Option<(WhatIfInputs, Receiver<WhatIfAnswer>)>,
    /// Why the run has no answer, when it cannot have one.
    pub not_computed: Option<String>,
    /// Why the last attempt to run was refused, before anything was computed.
    pub refused: Option<String>,
    /// The run's inputs changed since its answer and a newer one is on its way.
    pub stale: bool,
}

impl WhatIf {
    /// The running what-if in words, or `None` when none is running.
    #[must_use]
    pub fn describes(&self) -> Option<&str> {
        self.run.as_ref().map(|r| r.describes.as_str())
    }

    /// Whether the run's answer is being computed.
    #[must_use]
    pub fn computing(&self) -> bool {
        self.pending.is_some()
    }

    /// Take the what-if off the map; the record keeps what it found.
    pub fn clear(&mut self) {
        let draft = self.draft;
        *self = Self {
            draft,
            ..Self::default()
        };
    }
}

/// Run the what-if PN-11 has set (D-151: recorded with who ran it, so somebody must be
/// signed in). Nothing is computed here; the tick hands it to the worker.
///
/// # Errors
///
/// In words, when nobody is signed in, no sensor is chosen, the sensor is not declared, or
/// the change is outside what a what-if may try. Nothing is recorded for a refusal.
pub fn run(state: &mut AppState) -> Result<(), String> {
    let Some(session) = state.signed_in() else {
        return Err("nobody is signed in, and a what-if is recorded with who ran it".into());
    };
    let draft = state.dead_spots.what_if.draft;
    let Some(id) = draft.sensor else {
        return Err("no sensor is chosen".into());
    };
    let Some(record) = record_of(state, SensorId(id)).cloned() else {
        return Err(format!("S{id} is not a sensor this baseline declares"));
    };
    let finite = [draft.mast_m, draft.turn_deg, draft.east_m, draft.north_m]
        .iter()
        .all(|v| v.is_finite());
    if !finite
        || !(MAST_M.0..=MAST_M.1).contains(&draft.mast_m)
        || !(-180.0..=180.0).contains(&draft.turn_deg)
        || draft.east_m.abs() > MOVE_M
        || draft.north_m.abs() > MOVE_M
    {
        return Err(format!(
            "the change is outside what a what-if tries: a mast from {} to {} m, a turn \
             within 180 degrees, and a move within {} km",
            MAST_M.0,
            MAST_M.1,
            MOVE_M / 1_000.0
        ));
    }
    let change = SensorWhatIf {
        sensor: record.id,
        mast_m: draft.mast_m,
        turn_rad: if record.azimuth_sector.is_some() {
            draft.turn_deg.to_radians()
        } else {
            0.0
        },
        east_m: draft.east_m,
        north_m: draft.north_m,
    };
    let describes = describe(state, &record, &change);
    let what_if = &mut state.dead_spots.what_if;
    what_if.clear();
    what_if.run = Some(Run {
        change,
        operator: session.operator.0.to_string(),
        role: format!("{:?}", session.role),
        describes,
        recorded: false,
    });
    state.dead_spots.revision += 1;
    Ok(())
}

/// The registry's record of `sensor`.
fn record_of(state: &AppState, sensor: SensorId) -> Option<&SensorRecord> {
    state.sensors.sensors().iter().find(|s| s.id == sensor)
}

/// A change in words: the sensor, and each part of the change that is not nothing.
fn describe(state: &AppState, record: &SensorRecord, change: &SensorWhatIf) -> String {
    let mut parts = Vec::new();
    if !matches!(record.mode, SensorMode::Search | SensorMode::Track) {
        parts.push(format!(
            "switched on from {}",
            format!("{:?}", record.mode).to_lowercase()
        ));
    }
    if change.mast_m.abs() > f64::EPSILON {
        parts.push(format!("mast {:+.0} m", change.mast_m));
    }
    if change.turn_rad.abs() > f64::EPSILON {
        parts.push(format!("turned {:+.0} deg", change.turn_rad.to_degrees()));
    }
    if change.east_m.abs() > f64::EPSILON || change.north_m.abs() > f64::EPSILON {
        parts.push(format!(
            "moved {:.0} m east, {:.0} m north",
            change.east_m, change.north_m
        ));
    }
    if parts.is_empty() {
        parts.push("as it stands".into());
    }
    format!(
        "{} {}",
        crate::dead_spots::sensor_label(state, record.id),
        parts.join(", ")
    )
}

/// `record`'s volume under `change` (D-149): counted as searching, its position moved and
/// its mast raised in the frame, its declared sector turned, its band as declared.
fn changed_volume(
    record: &SensorRecord,
    change: &SensorWhatIf,
    floor_rad: f64,
    frame: &gungnir_model::LocalFrame,
) -> Result<(SensorId, CoverageVolume), String> {
    let base = frame.to_enu(record.position);
    let enu = [
        base[0] + change.east_m,
        base[1] + change.north_m,
        base[2] + change.mast_m,
    ];
    let at = frame.to_geodetic(enu);
    let sector = record
        .azimuth_sector
        .map(|s| gungnir_model::AzimuthSector::new(s.boresight_rad + change.turn_rad, s.width_rad))
        .transpose()
        .map_err(|e| format!("the turned sector is refused: {e}"))?;
    let band = gungnir_analytics::band_or_default(record.elevation_band, floor_rad);
    let volume = gungnir_analytics::volume_in_frame(frame, at, record.max_range_m, sector, band);
    Ok((
        record.id,
        CoverageVolume {
            // Exactly where the change puts it, not through the round trip.
            sensor_enu: enu,
            ..volume
        },
    ))
}

/// The inputs the run's answer should be computed under now.
///
/// # Errors
///
/// In words, when there is no terrain or frame, or the sensor is no longer declared.
pub fn inputs(state: &AppState, change: &SensorWhatIf) -> Result<WhatIfInputs, String> {
    let (terrain, frame) = crate::dead_spots::context(state)?;
    let floor = state.config.analytics.coverage_min_elevation_rad;
    let before = gungnir_analytics::coverage_from_registry(&state.sensors, floor, &frame);
    let Some(record) = record_of(state, change.sensor) else {
        return Err(format!(
            "S{} is no longer a sensor this baseline declares",
            change.sensor.0
        ));
    };
    let changed = changed_volume(record, change, floor, &frame)?;
    let mut after: Vec<_> = before
        .iter()
        .filter(|(id, _)| *id != change.sensor)
        .copied()
        .collect();
    after.push(changed);
    let extent = gungnir_analytics::union_extent(
        gungnir_analytics::dead_spot_extent(&before),
        gungnir_analytics::dead_spot_extent(&after),
    )
    .ok_or_else(|| "no sensor has a range to reach anywhere".to_owned())?;
    let heights = &state.config.analytics.dead_spot_heights_agl_m;
    let Some(&height_agl_m) = heights
        .get(state.dead_spots.selected_height)
        .or_else(|| heights.first())
    else {
        return Err("the baseline names no dead-spot height (analytics)".to_owned());
    };
    let wanted = state
        .config
        .analytics
        .dead_spot_cell_m
        .unwrap_or(terrain.cell_m);
    let (cell_m, _) = gungnir_analytics::terrain::fit_cell(
        extent[1][0] - extent[0][0],
        extent[1][1] - extent[0][1],
        wanted,
        gungnir_analytics::DEAD_SPOT_CELL_BUDGET,
    );
    Ok(WhatIfInputs {
        before,
        after,
        terrain,
        parameters: ShadowParameters {
            height_agl_m,
            cell_m,
        },
        extent,
    })
}

/// Start, poll and record the run; on the tick, after the live field's.
pub fn tick(state: &mut AppState) {
    let Some(change) = state.dead_spots.what_if.run.as_ref().map(|r| r.change) else {
        return;
    };
    let wanted = inputs(state, &change);
    let what_if = &mut state.dead_spots.what_if;
    let wanted = match wanted {
        Ok(wanted) => wanted,
        Err(reason) => {
            what_if.pending = None;
            if what_if.answer.take().is_some() {
                state.dead_spots.revision += 1;
            }
            state.dead_spots.what_if.not_computed = Some(reason);
            return;
        }
    };
    what_if.not_computed = None;
    let mut arrived = None;
    if let Some((_, results)) = &what_if.pending {
        match results.try_recv() {
            Ok(answer) => {
                arrived = Some(answer);
                what_if.pending = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                what_if.pending = None;
                what_if.not_computed = Some("the what-if worker stopped without an answer".into());
            }
        }
    }
    if let Some(answer) = arrived {
        let first = what_if.run.as_ref().is_some_and(|r| !r.recorded);
        if first {
            record(state, &answer);
        }
        state.dead_spots.what_if.answer = Some(answer);
        state.dead_spots.revision += 1;
    }
    let what_if = &mut state.dead_spots.what_if;
    let current = what_if.answer.as_ref().is_some_and(|a| a.inputs == wanted);
    let pending_matches = what_if.pending.as_ref().is_some_and(|(p, _)| *p == wanted);
    if !current && !pending_matches {
        match spawn(wanted) {
            Ok(pending) => what_if.pending = Some(pending),
            Err(reason) => what_if.not_computed = Some(reason),
        }
    }
    what_if.stale = what_if.pending.is_some() || !current;
}

/// Hand both fields to a worker thread.
fn spawn(inputs: WhatIfInputs) -> Result<(WhatIfInputs, Receiver<WhatIfAnswer>), String> {
    let (send, results) = crossbeam_channel::bounded(1);
    let kept = inputs.clone();
    std::thread::Builder::new()
        .name("gungnir-dead-spot-what-if".into())
        .spawn(move || {
            let over = |sensors: &[(SensorId, CoverageVolume)]| {
                gungnir_analytics::shadow_field_over(
                    sensors,
                    &inputs.terrain,
                    inputs.parameters,
                    inputs.extent,
                )
            };
            let before = over(&inputs.before);
            let after = over(&inputs.after);
            // Laid over one extent at one cell, the two are one grid; a worker whose
            // fields were not would have nothing to compare, and says so by sending none.
            let Some(comparison) = gungnir_analytics::compare_fields(&before, &after) else {
                return;
            };
            let codes = comparison
                .cells
                .iter()
                .zip(&before.cells)
                .map(|(change, live)| match change {
                    CellChange::Cleared => DeadSpotCell::WouldClear,
                    CellChange::Darkened => DeadSpotCell::WouldDarken,
                    CellChange::Same => crate::dead_spots::cell(*live),
                })
                .collect();
            let before_measure = before.measure();
            let after_measure = after.measure();
            // A worker whose answer nobody waits for any more has nobody to tell.
            let _ = send.send(WhatIfAnswer {
                before,
                after,
                comparison,
                codes,
                before_measure,
                after_measure,
                inputs,
                at: Instant::now(),
            });
        })
        .map_err(|e| format!("the what-if worker could not start: {e}"))?;
    Ok((kept, results))
}

/// Put the run's first answer on the record (D-151), and mark the run recorded.
fn record(state: &mut AppState, answer: &WhatIfAnswer) {
    let Some(run) = state.dead_spots.what_if.run.as_mut() else {
        return;
    };
    run.recorded = true;
    let areas = |m: &ShadowMeasure, watched_m2: f64| DeadSpotAreas {
        claimed_m2: m.claimed_m2,
        shadow_m2: m.shadow_m2,
        watched_m2,
    };
    let now = state.clock.now();
    let event = DeadSpotWhatIf {
        change: run.change,
        operator: run.operator.clone(),
        role: run.role.clone(),
        revision: state.config.revision,
        height_agl_m: answer.inputs.parameters.height_agl_m,
        cell_m: answer.before.parameters.cell_m,
        before: areas(&answer.before_measure, answer.comparison.watched_before_m2),
        after: areas(&answer.after_measure, answer.comparison.watched_after_m2),
        cleared_m2: answer.comparison.cleared_m2,
        darkened_m2: answer.comparison.darkened_m2,
        at: now,
    };
    if let Err(err) = state.events.publish(
        now,
        gungnir_eventing::Event::Planning(gungnir_model::events::PlanningEvent::DeadSpotWhatIf(
            event,
        )),
    ) {
        tracing::error!(%err, "dead-spot what-if publish failed");
        state
            .alerts
            .push(format!("A dead-spot what-if is not on the record ({err})"));
    }
}
