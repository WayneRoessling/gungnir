// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Terrain dead spots on the desktop (GAP-246, `docs/design/DN-34-sensor-dead-spots.md`
//! §6.5 and §7).
//!
//! **Computed off the tick.** A field over the sensors' reach costs tens to hundreds of
//! milliseconds, which is a recompute's budget (MOP-14, under ten seconds) and never a
//! frame's. So [`tick`] takes a snapshot of what the field depends on -- the searching and
//! tracking sensors placed in the frame, the resampled terrain, the height and the cell --
//! and when it differs from what the shown field was computed under, hands it to a worker
//! thread and keeps drawing the old field, faded and saying it is recomputing, until the
//! new one arrives. A stale shadow that says so is a better claim than none for the
//! seconds a recompute takes.
//!
//! **No terrain, no field, said in words** (DN-34 §4 rule 4): with no DEM configured,
//! loading, or refused, or no local frame to place the sensors in, nothing is computed and
//! the reason is what the viewport and PN-11 say. A flat-earth field is never drawn: on a
//! flat earth there is no shadow, and an empty layer over a sector with a ridge in it would
//! be the layer lying.
//!
//! **Desktop only** (D-148): the node loads no terrain and computes no field. A linked
//! desktop computes its own from the sensors it holds and its own DEM, and says so.
//! **Nothing is journaled**: a field is derived from the baseline, the registry and the
//! DEM, all on the record already.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, TryRecvError};
use gungnir_analytics::{
    CellSight, CoverageVolume, HeightField, ShadowField, ShadowMeasure, ShadowParameters,
};
use gungnir_model::SensorId;
use gungnir_viewport3d::layers::{DeadSpotCell, DeadSpotField, DeadSpotLayer};

use crate::state::AppState;
use crate::terrain::TerrainStatus;

/// What a field depends on. Two fields computed from equal inputs are the same field, so
/// a field whose inputs still equal the current ones is current.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub sensors: Vec<(SensorId, CoverageVolume)>,
    /// The resampled terrain, compared by identity: a new load is a new surface, and
    /// comparing a million heights a frame to learn that would cost more than the frame.
    terrain: Arc<HeightField>,
    pub parameters: ShadowParameters,
}

impl PartialEq for Inputs {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.terrain, &other.terrain)
            && self.parameters == other.parameters
            && self.sensors == other.sensors
    }
}

/// A field, what it was computed under, and when.
#[derive(Debug, Clone)]
pub struct Computed {
    pub field: ShadowField,
    /// Every active sensor's cells as the map draws them, folded on the worker so the
    /// tick that receives a field of a million cells does not fold it.
    pub codes: Vec<DeadSpotCell>,
    /// The field's measure, likewise computed once on the worker.
    pub measure: ShadowMeasure,
    pub inputs: Inputs,
    pub at: Instant,
    /// How long the worker took, for PN-09 and the MOP-14 budget test.
    pub took: Duration,
}

/// The dead-spot layer's state on this desktop (module documentation).
#[derive(Debug, Default)]
pub struct DeadSpots {
    /// Index into `analytics.dead_spot_heights_agl_m` of the height shown. Session state,
    /// like the layer toggle.
    pub selected_height: usize,
    /// One sensor's own shadow, or every active sensor's.
    pub for_sensor: Option<SensorId>,
    /// The last field the worker finished.
    pub computed: Option<Computed>,
    /// The chosen sensor's own cells and measure, folded from `computed` when one is
    /// chosen; empty when every active sensor's are shown.
    sensor_cells: Vec<DeadSpotCell>,
    sensor_measure: Option<ShadowMeasure>,
    /// Changes whenever the cells drawn do, so the viewport rebuilds its texture once.
    pub revision: u64,
    /// Why there is no field, when there is none.
    pub not_computed: Option<String>,
    /// The worker computing the next field, and what it was given.
    pending: Option<(Inputs, Receiver<Computed>)>,
    /// The operator asked for a recompute.
    pub recompute_requested: bool,
    /// What `cells` was folded from: the field's computation time and the sensor shown.
    folded: Option<(Instant, Option<SensorId>)>,
    /// Set by [`tick`]: the shown field's inputs have changed since it was computed.
    pub stale: bool,
    /// Set by [`tick`]: this desktop is linked to a node.
    pub linked: bool,
    /// The selected laydown option's own field, for PN-16 (GAP-248).
    pub option: OptionField,
}

impl DeadSpots {
    /// Whether a worker is computing a field now.
    #[must_use]
    pub fn computing(&self) -> bool {
        self.pending.is_some()
    }

    /// The cells the map draws: the chosen sensor's, or every active sensor's.
    #[must_use]
    pub fn shown_cells(&self) -> &[DeadSpotCell] {
        match (&self.computed, self.for_sensor) {
            (Some(_), Some(_)) => &self.sensor_cells,
            (Some(c), None) => &c.codes,
            (None, _) => &[],
        }
    }

    /// The measure of what the map draws.
    #[must_use]
    pub fn shown_measure(&self) -> Option<ShadowMeasure> {
        let computed = self.computed.as_ref()?;
        Some(match self.for_sensor {
            Some(_) => self.sensor_measure.unwrap_or(computed.measure),
            None => computed.measure,
        })
    }
}

/// The inputs the live field should be computed under now: the registry's searching and
/// tracking sensors.
///
/// # Errors
///
/// In words, when there is no terrain or no frame to compute a field in.
pub fn inputs(state: &AppState) -> Result<Inputs, String> {
    let (terrain, frame) = context(state)?;
    let sensors = gungnir_analytics::coverage_from_registry(
        &state.sensors,
        state.config.analytics.coverage_min_elevation_rad,
        &frame,
    );
    inputs_for(state, terrain, sensors)
}

/// The inputs a laydown option's field should be computed under (GAP-248, DN-34 §7 item 5):
/// the sensors it places that are searching or tracking, each where it places them, from
/// the same `laydown_coverage_volumes` PN-16's coverage column reads, so the option's
/// dead spots and its gaps are about the same placements.
///
/// # Errors
///
/// In words, when there is no terrain or no frame to compute a field in.
pub fn option_inputs(state: &AppState, laydown: &gungnir_model::Laydown) -> Result<Inputs, String> {
    let (terrain, frame) = context(state)?;
    let sensors = crate::sustainment::laydown_coverage_volumes(
        laydown,
        &state.config.sensors,
        state.config.analytics.coverage_min_elevation_rad,
        &frame,
    );
    inputs_for(state, terrain, sensors)
}

/// The resampled terrain and the local frame every field needs, or why there are none.
fn context(state: &AppState) -> Result<(Arc<HeightField>, gungnir_model::LocalFrame), String> {
    let terrain = match (&state.terrain, state.height_field.as_ref()) {
        (TerrainStatus::Loaded { .. }, Some(field)) => Arc::clone(field),
        (TerrainStatus::NotConfigured, _) => {
            return Err("no DEM is configured in this baseline (terrain)".to_owned())
        }
        (TerrainStatus::Loading { path }, _) => return Err(format!("{path} is still loading")),
        (TerrainStatus::Failed { path, reason }, _) => {
            return Err(format!("{path} did not load: {reason}"))
        }
        (TerrainStatus::Loaded { path, .. }, None) => {
            return Err(format!("{path} is loaded but its surface is not resampled"))
        }
    };
    let Some(frame) = crate::sustainment::local_frame(state) else {
        return Err("this deployment has declared no local frame origin (origin)".to_owned());
    };
    Ok((terrain, frame))
}

/// A field's inputs for these sensors: the height PN-11 has chosen, and the cell the
/// baseline states or the DEM's own, coarsened to the budget over these sensors' reach.
fn inputs_for(
    state: &AppState,
    terrain: Arc<HeightField>,
    sensors: Vec<(SensorId, CoverageVolume)>,
) -> Result<Inputs, String> {
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
    let (cell_m, _) = gungnir_analytics::dead_spot_cell_m(&sensors, wanted);
    Ok(Inputs {
        sensors,
        terrain,
        parameters: ShadowParameters {
            height_agl_m,
            cell_m,
        },
    })
}

/// Start, poll and retire the worker; on the tick (module documentation).
pub fn tick(state: &mut AppState) {
    let wanted = match inputs(state) {
        Ok(wanted) => wanted,
        Err(reason) => {
            let dead = &mut state.dead_spots;
            dead.pending = None;
            if dead.computed.take().is_some() {
                dead.sensor_cells.clear();
                dead.sensor_measure = None;
                dead.revision += 1;
            }
            dead.folded = None;
            dead.not_computed = Some(reason);
            tick_option(state);
            return;
        }
    };
    let is_linked = linked(state);
    let dead = &mut state.dead_spots;
    dead.not_computed = None;
    dead.linked = is_linked;

    // A worker's answer for the inputs it was given is kept even if they moved on since:
    // it is the newest field there is, and it is drawn stale until the next one lands.
    if let Some((_, results)) = &dead.pending {
        match results.try_recv() {
            Ok(done) => {
                dead.computed = Some(done);
                dead.pending = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                dead.pending = None;
                dead.not_computed = Some("the dead-spot worker stopped without an answer".into());
            }
        }
    }

    let current = dead.computed.as_ref().is_some_and(|c| c.inputs == wanted);
    let pending_matches = dead.pending.as_ref().is_some_and(|(p, _)| *p == wanted);
    if (dead.recompute_requested || !current) && !pending_matches {
        dead.recompute_requested = false;
        match spawn(wanted.clone()) {
            Ok(results) => dead.pending = Some((wanted, results)),
            Err(reason) => dead.not_computed = Some(reason),
        }
    } else if dead.recompute_requested && pending_matches {
        dead.recompute_requested = false;
    }

    dead.stale = dead.pending.is_some() || !current;
    fold(dead);
    tick_option(state);
}

/// The selected laydown option's field (GAP-248): one at a time, for the option PN-16 has
/// selected, on the same worker as the live field. Nothing is computed while no option is
/// selected, because every option is a full recompute (DN-34 §7 item 5).
#[derive(Debug, Default)]
pub struct OptionField {
    /// The option this field is for.
    pub laydown: Option<gungnir_model::LaydownId>,
    /// The last field the worker finished for it.
    pub computed: Option<Computed>,
    pending: Option<(Inputs, Receiver<Computed>)>,
    /// Why there is none, when there is none.
    pub not_computed: Option<String>,
    /// The option's inputs changed since `computed` and a newer field is on its way.
    pub stale: bool,
}

impl OptionField {
    /// Whether a worker is computing this option's field now.
    #[must_use]
    pub fn computing(&self) -> bool {
        self.pending.is_some()
    }
}

/// Start, poll and retire the selected option's worker.
fn tick_option(state: &mut AppState) {
    let selected = state.selected_laydown().cloned();
    let laydown = selected
        .as_ref()
        .and_then(|id| state.config.laydowns.iter().find(|l| &l.id == id));
    let Some(laydown) = laydown else {
        state.dead_spots.option = OptionField::default();
        return;
    };
    let wanted = option_inputs(state, laydown);
    let option = &mut state.dead_spots.option;
    if option.laydown.as_ref() != Some(&laydown.id) {
        *option = OptionField {
            laydown: Some(laydown.id.clone()),
            ..OptionField::default()
        };
    }
    let wanted = match wanted {
        Ok(wanted) => wanted,
        Err(reason) => {
            option.pending = None;
            option.computed = None;
            option.not_computed = Some(reason);
            return;
        }
    };
    option.not_computed = None;
    if let Some((_, results)) = &option.pending {
        match results.try_recv() {
            Ok(done) => {
                option.computed = Some(done);
                option.pending = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                option.pending = None;
                option.not_computed = Some("the dead-spot worker stopped without an answer".into());
            }
        }
    }
    let current = option.computed.as_ref().is_some_and(|c| c.inputs == wanted);
    let pending_matches = option.pending.as_ref().is_some_and(|(p, _)| *p == wanted);
    if !current && !pending_matches {
        match spawn(wanted.clone()) {
            Ok(results) => option.pending = Some((wanted, results)),
            Err(reason) => option.not_computed = Some(reason),
        }
    }
    option.stale = option.pending.is_some() || !current;
}

/// Hand `inputs` to a worker thread.
fn spawn(inputs: Inputs) -> Result<Receiver<Computed>, String> {
    let (send, results) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("gungnir-dead-spots".into())
        .spawn(move || {
            let started = Instant::now();
            let field = gungnir_analytics::shadow_field(
                &inputs.sensors,
                &inputs.terrain,
                inputs.parameters,
            );
            let codes = field.cells.iter().copied().map(cell).collect();
            let measure = field.measure();
            // A worker whose answer nobody waits for any more has nobody to tell.
            let _ = send.send(Computed {
                field,
                codes,
                measure,
                inputs,
                at: Instant::now(),
                took: started.elapsed(),
            });
        })
        .map_err(|e| format!("the dead-spot worker could not start: {e}"))?;
    Ok(results)
}

/// Note a new field or a new choice of sensor, once each: the revision moves so the
/// viewport rebuilds its texture, and a chosen sensor's own cells and measure are folded
/// from the field. Every active sensor's were folded on the worker.
fn fold(dead: &mut DeadSpots) {
    let Some(computed) = &dead.computed else {
        return;
    };
    let key = (computed.at, dead.for_sensor);
    if dead.folded == Some(key) {
        return;
    }
    if let Some(sights) = dead.for_sensor.and_then(|s| computed.field.for_sensor(s)) {
        dead.sensor_measure = Some(gungnir_analytics::measure_cells(
            &sights,
            computed.field.parameters.cell_m,
        ));
        dead.sensor_cells = sights.into_iter().map(cell).collect();
    } else {
        // No sensor chosen, or one this field was not computed with: every active
        // sensor's field is what is drawn.
        dead.for_sensor = None;
        dead.sensor_cells = Vec::new();
        dead.sensor_measure = None;
    }
    dead.revision += 1;
    dead.folded = Some((computed.at, dead.for_sensor));
}

/// What the map draws for one cell (DN-34 §4): a dead spot, single-sensor, no data, or
/// nothing.
#[must_use]
pub fn cell(sight: CellSight) -> DeadSpotCell {
    match sight {
        CellSight::Shadow { .. } => DeadSpotCell::Shadow,
        CellSight::Single { .. } => DeadSpotCell::Single,
        CellSight::NoData => DeadSpotCell::NoData,
        CellSight::Unclaimed | CellSight::Fused { .. } => DeadSpotCell::Clear,
    }
}

/// Whether this desktop is linked to a node now.
fn linked(state: &AppState) -> bool {
    state
        .link
        .as_ref()
        .is_some_and(gungnir_remote::link::NodeLink::connected)
}

/// A sensor's name on the map and on PN-11.
#[must_use]
pub fn sensor_label(state: &AppState, id: SensorId) -> String {
    use gungnir_sensor_management::SensorRegistry as _;
    state
        .sensors
        .sensors()
        .iter()
        .find(|s| s.id == id)
        .map_or_else(
            || format!("S{}", id.0),
            |s| format!("S{} {}", id.0, s.modality),
        )
}

/// The viewport's dead-spot layer for this frame. `label` holds the shown sensor's name,
/// which the layer borrows. Takes the dead-spot state rather than the whole of it, so the
/// viewport can borrow its own state mutably in the same call (as `crate::terrain::layer`).
#[must_use]
pub fn layer<'a>(dead: &'a DeadSpots, label: &'a str) -> DeadSpotLayer<'a> {
    if let Some(reason) = &dead.not_computed {
        return DeadSpotLayer::NotComputed { reason };
    }
    let Some(computed) = &dead.computed else {
        return DeadSpotLayer::Computing;
    };
    let field = &computed.field;
    DeadSpotLayer::Field(DeadSpotField {
        revision: dead.revision,
        origin_enu: field.origin_enu,
        cell_m: field.parameters.cell_m,
        rows: field.rows,
        columns: field.columns,
        cells: dead.shown_cells(),
        height_agl_m: field.parameters.height_agl_m,
        age_s: computed.at.elapsed().as_secs_f64(),
        stale: dead.stale,
        sensor: dead.for_sensor.map(|_| label),
        linked: dead.linked,
    })
}

/// The shown sensor's label for [`layer`], or empty for every active sensor.
#[must_use]
pub fn shown_label(state: &AppState) -> String {
    state
        .dead_spots
        .for_sensor
        .map(|s| sensor_label(state, s))
        .unwrap_or_default()
}

/// PN-11's measure sentence for the shown field, or `None` when there is no field.
#[must_use]
pub fn measure_sentence(state: &AppState) -> Option<String> {
    let dead = &state.dead_spots;
    let computed = dead.computed.as_ref()?;
    let measure = dead.shown_measure()?;
    Some(measure.sentence(computed.field.parameters.height_agl_m))
}

/// One line for PN-09: the worker's state and the last field's cost.
#[must_use]
pub fn health_line(state: &AppState) -> String {
    let dead = &state.dead_spots;
    if let Some(reason) = &dead.not_computed {
        return format!("dead spots not computed: {reason}");
    }
    match (&dead.computed, dead.computing()) {
        (Some(c), computing) => format!(
            "dead spots: {} by {} cells of {:.0} m at {:.0} m above ground, computed in {} ms{}",
            c.field.columns,
            c.field.rows,
            c.field.parameters.cell_m,
            c.field.parameters.height_agl_m,
            c.took.as_millis(),
            if computing { "; recomputing" } else { "" }
        ),
        (None, true) => "dead spots: computing the first field".to_owned(),
        (None, false) => "dead spots: waiting for the first tick".to_owned(),
    }
}

/// Apply an operator's choice from PN-11.
pub fn apply(state: &mut AppState, action: gungnir_ui::panels::coverage_layers::LayerAction) {
    use gungnir_ui::panels::coverage_layers::LayerAction;
    match action {
        LayerAction::ShowDeadSpots(on) => state.viewport.layers.dead_spots = on,
        LayerAction::DeadSpotHeight(i) => {
            if i < state.config.analytics.dead_spot_heights_agl_m.len() {
                state.dead_spots.selected_height = i;
            }
        }
        LayerAction::DeadSpotsFor(sensor) => {
            state.dead_spots.for_sensor = sensor.map(SensorId);
            fold(&mut state.dead_spots);
        }
        LayerAction::RecomputeDeadSpots => state.dead_spots.recompute_requested = true,
        LayerAction::ShowRings(on) => state.viewport.layers.rings = on,
        LayerAction::ShowGaps(on) => state.viewport.layers.gaps = on,
        LayerAction::ShowHazards(on) => state.viewport.layers.hazards = on,
        LayerAction::ShowGeofences(on) => state.viewport.layers.geofences = on,
    }
}
