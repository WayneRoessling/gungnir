// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The picture's layers, assembled when their inputs change and lent to every frame
//! (GAP-257; `docs/review/2026-10-30-ui-review.md` §4.3 finding 16).
//!
//! `main.rs` assembled the coverage circles, the coverage report, the hazards, the
//! geofences, the predicted lines, the laydown preview and the dead-spot label on every
//! frame, twice over when the viewport was detached, and `tests/frame_budgets.rs` carried
//! a third copy of the same sixty lines, kept in step by hand. Every one of those is a
//! function of state that changes at the snapshot rate at most, and most of it changes
//! only when the baseline is promoted; `rust-ui-architecture-coding-standards.md` §5 says
//! what is drawn every frame is rebuilt only when what it reads changes, and the viewport's
//! own glyph list (`gungnir_viewport3d::tracks::glyphs_need_rebuild`) shows the pattern.
//!
//! [`PictureCache`] owns what the layers point at and recomputes each group when, and only
//! when, the state it reads differs from what it was computed from. Each group has its own
//! key, so a track that moved recomputes the predictions and nothing else, and a sensor
//! that changed mode recomputes the coverage and nothing else. The keys name every input
//! their producers read; a producer that starts reading something new has to add it here,
//! which is the same obligation `glyphs_need_rebuild` puts on `TrackGlyph`.
//!
//! [`PictureFrame`] is what one frame borrows: the viewport's `LayerInputs` are borrowed
//! views (`GapPolyline<'a>`, `HazardOutline<'a>`, ...), so a frame builds the four small
//! vectors of views that point into the cache, and lends them. Those vectors are one
//! allocation each, of a pointer and a few words per item; what the cache saves is the work
//! and the allocations behind the pointers, which were the filter predictor over every
//! track at every horizon, the coverage report's key and clone, and a placed copy of every
//! hazard and geofence, per frame.
//!
//! The desktop and the egui gate (`tests/frame_budgets.rs`) draw the picture through the
//! same two calls, [`PictureCache::sync`] and [`render_projection`], so the gate times the
//! assembly the operator's frame runs and cannot drift from it.

use crate::dead_spots::DeadSpots;
use crate::pointcloud::PointCloudStatus;
use crate::state::AppState;
use crate::terrain::TerrainStatus;
use crate::{dead_spots, geofences, hazards, pointcloud, prediction, sustainment, terrain};
use gungnir_analytics::{CoverageReport, HeightField};
use gungnir_model::{DenseGroupView, LaydownId, PlanView, SensorId, TrackView};
use gungnir_sensor_management::{CoverageRegion, SensorRegistry};
use gungnir_tracking_service::TrackingService;
use gungnir_ui::theme::Palette;
use gungnir_viewport3d::layers::{
    CoverageCircle, CoverageLayer, DeadSpotLayer, GapPolyline, GeofenceOutline, HazardOutline,
    LaydownPreview, LayerInputs, PointCloudLayer, PredictedPath, TerrainLayer,
};
use gungnir_viewport3d::ViewportState;
use std::sync::Arc;

/// The picture's layers, owned here and recomputed by group when their inputs change.
///
/// `Default` is an empty cache: the first [`sync`](Self::sync) computes every group.
#[derive(Debug, Default)]
pub struct PictureCache {
    coverage: Option<Coverage>,
    rules: Option<Rules>,
    predictions: Option<Predictions>,
    preview: Option<Preview>,
    dead_spot_label: Option<DeadSpotLabel>,
    rebuilds: Rebuilds,
}

/// How many times each group has been computed since the cache was made. A frame that
/// changes nothing leaves every count where it was; the gate asserts that.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Rebuilds {
    pub coverage: u64,
    pub rules: u64,
    pub predictions: u64,
    pub preview: u64,
    pub dead_spot_label: u64,
}

/// The coverage group: the rings, the report along the approaches, the approach names
/// and which gaps stand accepted (GAP-007, GAP-006, GAP-106).
#[derive(Debug)]
struct Coverage {
    key: CoverageKey,
    circles: Vec<CoverageCircle>,
    report: Option<CoverageReport>,
    names: Vec<String>,
    accepted: Vec<bool>,
}

/// What the coverage group reads: the baseline (the frame, the approaches, the analytics
/// parameters, the laydown in force), each active sensor's coverage region (its mode,
/// position, range and sector, which are what `coverage_from_registry` builds the volumes
/// from), the surface the report is masked against, by identity, and the acceptances that
/// stand.
#[derive(Debug)]
struct CoverageKey {
    config: u32,
    regions: Vec<CoverageRegion>,
    terrain: Option<Arc<HeightField>>,
    acceptances: (usize, u64),
}

impl CoverageKey {
    fn current(&self, state: &AppState, regions: &[CoverageRegion]) -> bool {
        let same_terrain = match (&self.terrain, masking_terrain(state)) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same_terrain
            && self.config == state.config.revision
            && self.acceptances == acceptances_key(state)
            && self.regions == regions
    }
}

/// The surface `sustainment::masked_report` masks against, when the terrain is loaded.
fn masking_terrain(state: &AppState) -> Option<&Arc<HeightField>> {
    state
        .height_field
        .as_ref()
        .filter(|_| state.terrain.is_masking())
}

/// The standing acceptances, by count and highest identifier: an acceptance is only ever
/// appended with a new highest identifier or removed, so the pair changes whenever the
/// list does.
fn acceptances_key(state: &AppState) -> (usize, u64) {
    (
        state.gap_acceptances.standing.len(),
        state.gap_acceptances.highest,
    )
}

/// The rule layers: the hazards (GAP-017) and the geofences (GAP-088), placed in the frame.
#[derive(Debug)]
struct Rules {
    key: RulesKey,
    hazards: Vec<hazards::PlacedHazard>,
    geofences: Vec<geofences::PlacedGeofence>,
}

/// What the rule layers read: the baseline (the frame and the fences), and the hazard layer
/// built from it, by the baseline version it carries and its count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RulesKey {
    config: u32,
    hazards_version: u32,
    hazards: usize,
}

impl RulesKey {
    fn of(state: &AppState) -> Self {
        Self {
            config: state.config.revision,
            hazards_version: state.hazards.baseline_version,
            hazards: state.hazards.hazards.len(),
        }
    }
}

/// The predicted lines (GAP-020): the filter predictor over every track at every horizon.
#[derive(Debug)]
struct Predictions {
    key: PredictionsKey,
    paths: Vec<prediction::PredictedPath>,
}

/// What the predictor reads: the baseline (the horizons and the asset anchors), the process
/// noise it runs with, and the tracks themselves, compared whole. A `NaN` anywhere in a
/// track never equals itself, so such a snapshot predicts on every frame; that errs towards
/// drawing, as `glyphs_need_rebuild` does for the same reason.
#[derive(Debug)]
struct PredictionsKey {
    config: u32,
    process_noise_psd: u64,
    tracks: Vec<TrackView>,
}

impl PredictionsKey {
    fn current(&self, state: &AppState) -> bool {
        self.config == state.config.revision
            && self.process_noise_psd == state.pipeline.process_noise_psd.to_bits()
            && self.tracks.as_slice() == state.tracking.tracks()
    }
}

/// The selected laydown's preview (GAP-087).
#[derive(Debug)]
struct Preview {
    key: PreviewKey,
    data: Option<sustainment::LaydownPreviewData>,
}

/// What the preview reads: the baseline's laydowns and the selection.
#[derive(Debug)]
struct PreviewKey {
    config: u32,
    laydown: Option<LaydownId>,
}

impl PreviewKey {
    fn current(&self, state: &AppState) -> bool {
        self.config == state.config.revision && self.laydown.as_ref() == state.selected_laydown()
    }
}

/// The shown sensor's label under the dead-spot field (GAP-246).
#[derive(Debug)]
struct DeadSpotLabel {
    key: DeadSpotLabelKey,
    label: String,
}

/// What the label reads: the chosen sensor, and the baseline the sensors' names come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeadSpotLabelKey {
    config: u32,
    sensor: Option<SensorId>,
}

impl DeadSpotLabelKey {
    fn of(state: &AppState) -> Self {
        Self {
            config: state.config.revision,
            sensor: state.dead_spots.for_sensor,
        }
    }
}

impl PictureCache {
    /// Bring every group up to date with `state`: each is recomputed when its key no
    /// longer matches, and left alone otherwise. Called once a frame, before
    /// [`frame`](Self::frame); a second call in the same frame, for a detached viewport,
    /// finds every key matching and does nothing.
    pub fn sync(&mut self, state: &AppState) {
        // The regions are read every frame to learn whether a sensor changed: the registry
        // keeps no revision, and the regions are a handful of records.
        let regions = state.sensors.coverage();
        if !self
            .coverage
            .as_ref()
            .is_some_and(|c| c.key.current(state, &regions))
        {
            let report = sustainment::coverage_report(state);
            let accepted = report
                .as_ref()
                .map(|r| sustainment::live_accepted(state, r))
                .unwrap_or_default();
            self.coverage = Some(Coverage {
                key: CoverageKey {
                    config: state.config.revision,
                    regions,
                    terrain: masking_terrain(state).cloned(),
                    acceptances: acceptances_key(state),
                },
                circles: sustainment::coverage_circles(state),
                report,
                names: sustainment::approach_names(state),
                accepted,
            });
            self.rebuilds.coverage += 1;
        }

        let rules = RulesKey::of(state);
        if self.rules.as_ref().is_none_or(|r| r.key != rules) {
            self.rules = Some(Rules {
                key: rules,
                hazards: hazards::placed(state),
                geofences: geofences::placed(state),
            });
            self.rebuilds.rules += 1;
        }

        if !self
            .predictions
            .as_ref()
            .is_some_and(|p| p.key.current(state))
        {
            self.predictions = Some(Predictions {
                key: PredictionsKey {
                    config: state.config.revision,
                    process_noise_psd: state.pipeline.process_noise_psd.to_bits(),
                    tracks: state.tracking.tracks().to_vec(),
                },
                paths: prediction::paths(&prediction::predict(state)),
            });
            self.rebuilds.predictions += 1;
        }

        if !self.preview.as_ref().is_some_and(|p| p.key.current(state)) {
            self.preview = Some(Preview {
                key: PreviewKey {
                    config: state.config.revision,
                    laydown: state.selected_laydown().cloned(),
                },
                data: sustainment::laydown_preview(state),
            });
            self.rebuilds.preview += 1;
        }

        let label = DeadSpotLabelKey::of(state);
        if self.dead_spot_label.as_ref().is_none_or(|l| l.key != label) {
            self.dead_spot_label = Some(DeadSpotLabel {
                key: label,
                label: dead_spots::shown_label(state),
            });
            self.rebuilds.dead_spot_label += 1;
        }
    }

    /// How many times each group has been computed.
    #[must_use]
    pub fn rebuilds(&self) -> Rebuilds {
        self.rebuilds
    }

    /// This frame's layers, borrowed from the cache and from `sources`. Before the first
    /// [`sync`](Self::sync) every cached layer is empty, which the coverage layer reports as
    /// no active sensor; the desktop and the gate sync first.
    #[must_use]
    pub fn frame<'a>(&'a self, sources: &Sources<'a>) -> PictureFrame<'a> {
        let (circles, gaps) = match &self.coverage {
            Some(c) => (
                c.circles.as_slice(),
                c.report.as_ref().map_or_else(Vec::new, |r| {
                    sustainment::gap_polylines(&c.names, r, &c.accepted)
                }),
            ),
            None => (&[][..], Vec::new()),
        };
        let (hazards, geofences) = match &self.rules {
            Some(r) => (
                hazards::outlines(&r.hazards),
                geofences::outlines(&r.geofences),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let predictions = self.predictions.as_ref().map_or_else(Vec::new, |p| {
            p.paths
                .iter()
                .map(|p| PredictedPath {
                    track: p.track,
                    points: &p.points,
                    uncertainty_drawable: p.uncertainty_drawable,
                })
                .collect()
        });
        let laydown_preview = self
            .preview
            .as_ref()
            .and_then(|p| p.data.as_ref())
            .map(|p| LaydownPreview {
                intent: &p.intent,
                sensors: &p.sensor_positions,
                resources: &p.resource_positions,
            });
        let label = self
            .dead_spot_label
            .as_ref()
            .map_or("", |l| l.label.as_str());
        PictureFrame {
            coverage: sustainment::coverage_layer_of(sources.has_origin, circles),
            gaps,
            hazards,
            geofences,
            predictions,
            terrain: terrain::layer(sources.terrain, sources.data),
            terrain_tiles: sources.terrain_tiles,
            fill_layers: sources.terrain_tiles.fill_layers(),
            point_clouds: pointcloud::layers(sources.point_cloud, sources.data),
            laydown_preview,
            dense_group: sources.tracking.dense_group(),
            dead_spots: dead_spots::layer(sources.dead_spots, label),
        }
    }
}

/// The parts of `AppState` a frame reads beside the cache, named one by one so the caller
/// can borrow the viewport mutably alongside them: `render` takes `&mut ViewportState`, and
/// a frame built from `&AppState` would hold the whole state while it ran.
#[derive(Clone, Copy)]
pub struct Sources<'a> {
    /// Whether the baseline declares a local frame origin, which decides whether an empty
    /// coverage layer is "nothing active" or "unplaceable" (GAP-007).
    pub has_origin: bool,
    pub palette: &'a Palette,
    pub plan: &'a PlanView,
    pub tracking: &'a dyn TrackingService,
    pub dead_spots: &'a DeadSpots,
    pub terrain: &'a TerrainStatus,
    /// A staged store's tiles, drawn by view (GAP-227).
    pub terrain_tiles: &'a crate::terrain_tiles::TileStreaming,
    pub point_cloud: &'a PointCloudStatus,
    pub data: &'a gungnir_data::DataStore,
}

impl<'a> Sources<'a> {
    /// The sources of a state whose viewport is held elsewhere, as the gate's is.
    #[must_use]
    pub fn of(state: &'a AppState) -> Self {
        Self {
            has_origin: state.config.origin.is_some(),
            palette: &state.palette,
            plan: &state.last_plan,
            tracking: state.tracking.as_ref(),
            dead_spots: &state.dead_spots,
            terrain: &state.terrain,
            terrain_tiles: &state.terrain_tiles,
            point_cloud: &state.point_cloud,
            data: &state.data,
        }
    }

    /// The sources and the viewport of one state, borrowed apart so the desktop can draw
    /// into its own viewport from its own state.
    #[must_use]
    pub fn split(state: &'a mut AppState) -> (Self, &'a mut ViewportState) {
        let AppState {
            config,
            palette,
            last_plan,
            tracking,
            dead_spots,
            terrain,
            terrain_tiles,
            point_cloud,
            data,
            viewport,
            ..
        } = state;
        (
            Self {
                has_origin: config.origin.is_some(),
                palette,
                plan: last_plan,
                tracking: &**tracking,
                dead_spots,
                terrain,
                terrain_tiles,
                point_cloud,
                data,
            },
            viewport,
        )
    }
}

/// One frame's layers: the views the viewport borrows, pointing into the cache and the
/// sources. Built by [`PictureCache::frame`], lent by [`inputs`](Self::inputs).
pub struct PictureFrame<'a> {
    coverage: CoverageLayer<'a>,
    gaps: Vec<GapPolyline<'a>>,
    hazards: Vec<HazardOutline<'a>>,
    geofences: Vec<GeofenceOutline<'a>>,
    predictions: Vec<PredictedPath<'a>>,
    terrain: Option<TerrainLayer<'a>>,
    /// A staged store's tiles and the fill sub-stores its layer borrows (GAP-227).
    terrain_tiles: &'a crate::terrain_tiles::TileStreaming,
    fill_layers: Vec<gungnir_viewport3d::streaming::terrain::FillLayer<'a>>,
    point_clouds: Vec<PointCloudLayer<'a>>,
    laydown_preview: Option<LaydownPreview<'a>>,
    dense_group: Option<&'a DenseGroupView>,
    dead_spots: DeadSpotLayer<'a>,
}

impl PictureFrame<'_> {
    /// The layers as the viewport takes them, borrowed from this frame.
    #[must_use]
    pub fn inputs(&self) -> LayerInputs<'_> {
        LayerInputs {
            coverage: self.coverage,
            gaps: &self.gaps,
            hazards: &self.hazards,
            geofences: &self.geofences,
            predictions: &self.predictions,
            terrain: self.terrain,
            terrain_tiles: self.terrain_tiles.layer(&self.fill_layers),
            point_clouds: &self.point_clouds,
            laydown_preview: self.laydown_preview,
            dense_group: self.dense_group,
            dead_spots: self.dead_spots,
        }
    }
}

/// Draw the 2D projection of the picture into `ui` from `cache` and `sources`: the one
/// call the desktop's docked and detached viewports and the egui gate share. The cache
/// must have been [`sync`](PictureCache::sync)ed this frame.
pub fn render_projection(
    ui: &mut egui::Ui,
    cache: &PictureCache,
    sources: &Sources<'_>,
    viewport: &mut ViewportState,
) {
    let frame = cache.frame(sources);
    gungnir_viewport3d::render(
        ui,
        sources.palette,
        viewport,
        sources.tracking.tracks(),
        sources.tracking.bearing_rays(),
        sources.plan,
        frame.inputs(),
    );
}
