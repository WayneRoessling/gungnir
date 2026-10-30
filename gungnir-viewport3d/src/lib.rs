// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! viewport3d/: three-d scene setup, camera, render-loop glue. Combines
//! rust-3d-data-ecosystem-build-vs-adopt.md §2 (streaming/scientific bridges) with
//! track/intercept rendering (ARCHITECTURE.md §5) -- the one place this project
//! lets a rendering crate see service output types directly, since a 3D track
//! symbol is a rendering concern.
//!
//! Rendering context: three-d draws through OpenGL via `glow`, inside the context
//! eframe provides when the application runs with `Renderer::Glow`. This crate never
//! touches the `wgpu` compute device (ARCHITECTURE.md §9).
//!
//! **Status (GAP-022, 2026-09-05):** the scene attaches to eframe's GL context through
//! [`gl::SceneRenderer`], and [`prepare_3d`] is the entry the binary drives with an
//! `egui_glow` paint callback. The scene is the **default** (`ui.scene_3d`, on) by the
//! owner's decision of 2026-09-05, with [`render`]'s 2D projection as the opt-out and as
//! the fallback when no context could be attached. The owner looked at the GL draw on
//! 2026-10-30 and it passed (GAP-226, D-153), but there is no headless probe for one, so
//! between looks the status line and the runtime toggle are what stand in for a test:
//! see `ARCHITECTURE.md` §10 items 52 and 53. Each track's frame, labels and assignment
//! are drawn by egui over the scene's discs (GAP-251), which a test can see; the discs
//! under them it cannot. The terrain is drawn by a second GL pass under the layers, from
//! the surface the projection draws (GAP-252, `docs/design/DN-35-terrain-in-the-three-d-scene.md`),
//! and a test can see where each pass sits in the paint order but not what it draws.

pub mod gl;

/// What this crate cannot draw yet, named rather than panicked (GAP-082).
///
/// Its own error rather than `gl::AttachError`, which is about failing to reach an OpenGL
/// context: **a scene that could not be attached and a scene feature nobody has written
/// are different situations**, and one of them is a fault an administrator can act on.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ViewportError {
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
    /// A mesh with no triangle: refused rather than drawn as an object with no
    /// geometry (GAP-023).
    #[error("the mesh has no triangle to draw")]
    EmptyMesh,
    #[error("the mesh is malformed: {reason}")]
    MalformedMesh { reason: String },
}
pub mod interaction;
pub mod layers;
pub mod materials;
pub mod scene;
pub mod scientific;
pub mod streaming;
pub mod tracks;

use gungnir_model::{BearingRayView, PlanView, TrackView};
use gungnir_ui::theme;

/// Everything the viewport owns across frames: the 3D camera, the static scene,
/// the current track glyphs, and the 2D fallback view.
/// Which coverage layers the viewport draws (PN-11, GAP-007).
///
/// Session state, not baseline: hiding a layer to read the map underneath is a thing an
/// operator is doing now, and it must not persist into what the deployment is configured
/// to show. The next launch draws everything again.
///
/// **Both default to on.** A viewport that started with coverage hidden would look
/// exactly like one with no coverage to draw, and those are the two states DN-12 §7 is
/// most concerned to keep apart.
// Four independent toggles are four bools; a flag set would hide which one an operator
// turned off, which is the thing the viewport has to say.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverageLayers {
    /// Each sensor's coverage ring.
    pub rings: bool,
    /// The gaps found along the declared approaches.
    pub gaps: bool,
    /// Booms, nets, barriers, wrecks and shoals (DN-14, GAP-017).
    pub hazards: bool,
    /// Geofences (GAP-088): rules about where we may act, drawn apart from hazards.
    pub geofences: bool,
    /// Where the terrain blinds the sensors (GAP-246, DN-34 §7). On by default for DN-12
    /// §7's reason: a hidden shadow layer and a sector with no shadow look the same.
    pub dead_spots: bool,
}

impl Default for CoverageLayers {
    fn default() -> Self {
        Self {
            rings: true,
            gaps: true,
            hazards: true,
            geofences: true,
            dead_spots: true,
        }
    }
}

impl CoverageLayers {
    /// True when the operator has hidden something.
    ///
    /// The viewport says so, because a hidden layer and an empty one look identical and
    /// only one of them is the map telling you something.
    #[must_use]
    pub fn anything_hidden(self) -> bool {
        !self.rings || !self.gaps || !self.hazards || !self.geofences || !self.dead_spots
    }

    /// The hidden layers by name, for the line the viewport draws.
    #[must_use]
    pub fn hidden_names(self) -> Vec<&'static str> {
        [
            (self.rings, "rings"),
            (self.gaps, "gaps"),
            (self.hazards, "hazards"),
            (self.geofences, "geofences"),
            (self.dead_spots, "dead spots"),
        ]
        .into_iter()
        .filter_map(|(shown, name)| (!shown).then_some(name))
        .collect()
    }
}

pub struct ViewportState {
    pub camera: three_d::Camera,
    pub scene: scene::Scene,
    pub glyphs: Vec<tracks::TrackGlyph>,
    /// Retained bearings, drawn as rays rather than glyphs (DN-27 §7; GAP-096).
    pub bearing_rays: Vec<tracks::BearingRayGlyph>,
    pub view: interaction::TopDownView,
    /// True once a three-d context has been created from eframe's GL context.
    pub gl_ready: bool,
    /// Whether the operator is looking at the three-d scene or the 2D projection.
    ///
    /// Seeded from `ui.scene_3d` and flipped by the control in the corner of the
    /// viewport. It is here rather than in the baseline because it is a thing this
    /// session is doing: switching back does not change the deployment's configuration,
    /// and the next launch starts from the baseline again.
    pub use_3d: bool,
    /// Which coverage layers to draw (PN-11, GAP-007).
    pub layers: CoverageLayers,
    /// The dead-spot field's texture, built when the field changes and never per frame
    /// (GAP-246, `docs/design/DN-34-sensor-dead-spots.md` §8.1).
    pub dead_spot_texture: Option<layers::DeadSpotTexture>,
    /// The terrain surface both renderers draw, built once per load and never per frame
    /// (GAP-252, `docs/design/DN-35-terrain-in-the-three-d-scene.md` §6).
    pub terrain_surface: Option<layers::TerrainSurfaceSlot>,
    /// Who drew the ground on the last frame, for PN-09's terrain line (DN-35 §7 item 3).
    pub ground: Ground,
}

/// Who draws the terrain this frame (GAP-252, DN-35 §5.2 and §7). Decided once a frame,
/// before anything is drawn, and handed to both [`draw_layers`] and the ground pass, so the
/// ground is drawn exactly once: never twice, which would double its alpha, and never by
/// neither while a surface is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    /// egui draws it in `draw_layers`: the projection always, and the scene until the GL
    /// pass holds this revision's surface.
    Overlay,
    /// The scene's ground pass draws it; `draw_layers` skips `draw_terrain_2d`.
    Scene,
}

/// The scene draws its own ground only when the GL pass already holds the surface for the
/// revision this frame wants; otherwise egui draws it, as the projection does (DN-35 §7).
#[must_use]
pub fn ground_source(wanted: Option<u64>, resident: Option<u64>) -> Ground {
    match (wanted, resident) {
        (Some(wanted), Some(resident)) if wanted == resident => Ground::Scene,
        _ => Ground::Overlay,
    }
}

impl ViewportState {
    pub fn new() -> Self {
        Self {
            camera: three_d::Camera::new_perspective(
                three_d::Viewport::new_at_origo(1, 1),
                three_d::vec3(0.0, -300.0, 180.0),
                three_d::vec3(0.0, 0.0, 0.0),
                three_d::vec3(0.0, 0.0, 1.0),
                three_d::degrees(45.0),
                0.1,
                50_000.0,
            ),
            scene: scene::Scene::default(),
            glyphs: Vec::new(),
            bearing_rays: Vec::new(),
            view: interaction::TopDownView::default(),
            gl_ready: false,
            use_3d: false,
            layers: CoverageLayers::default(),
            dead_spot_texture: None,
            terrain_surface: None,
            ground: Ground::Overlay,
        }
    }

    /// The terrain surface, when one is placed and could be built.
    #[must_use]
    pub fn terrain_surface(&self) -> Option<&std::sync::Arc<layers::TerrainSurface>> {
        self.terrain_surface
            .as_ref()
            .and_then(|slot| slot.surface.as_ref().ok())
    }

    /// One line for PN-09 about who draws the ground, or why nobody does (DN-35 §7 item 3);
    /// `None` while no terrain is placed.
    #[must_use]
    pub fn ground_line(&self) -> Option<String> {
        let slot = self.terrain_surface.as_ref()?;
        Some(match (&slot.surface, self.ground) {
            (Err(err), _) => format!("ground not drawn: {err}"),
            (Ok(_), Ground::Scene) => "ground drawn by the scene".to_owned(),
            (Ok(_), Ground::Overlay) => "ground drawn by the overlay".to_owned(),
        })
    }
}

impl Default for ViewportState {
    fn default() -> Self {
        Self::new()
    }
}

/// The control that switches between the three-d scene and the 2D projection.
///
/// Drawn in the corner of the viewport by whichever renderer is running, so there is
/// always a way back. The three-d path has been looked at once (GAP-226) but no test can
/// see it, and a deployment that finds it drawing badly must not have to edit a baseline
/// and restart during whatever is happening at the time.
///
/// Returns true when the operator switched.
/// Draw the coverage layers the operator has left on.
///
/// One function for both the 2D projection and the three-d scene, so a layer cannot be
/// hidden in one and drawn in the other -- which would make the toggle mean different
/// things depending on which renderer was in front. The terrain is the one thing that
/// changes renderer, and it has no toggle: `ground` says whether this call draws it
/// (GAP-252, DN-35 §5.2).
fn draw_layers(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    state: &ViewportState,
    layers: &layers::LayerInputs<'_>,
    ground: Ground,
) {
    // Terrain goes under everything: it is the ground the rest is drawn on (GAP-023).
    if ground == Ground::Overlay {
        layers::draw_terrain_2d(
            painter,
            rect,
            &state.view,
            state.terrain_surface().map(AsRef::as_ref),
        );
    }
    // Point clouds have no toggle either, the same reason predictions and the laydown
    // preview below do not: an empty slice already draws nothing, and there is no
    // separate "hidden" state to distinguish from that (GAP-098).
    layers::draw_point_clouds_2d(painter, palette, rect, &state.view, layers.point_clouds);
    // GAP-246: the dead spots are about the ground, so they lie on it and under every rule,
    // ring and gap drawn over them (DN-34 §7 item 2).
    if state.layers.dead_spots {
        layers::draw_dead_spots_2d(
            painter,
            palette,
            rect,
            &state.view,
            state.dead_spot_texture.as_ref(),
            layers.dead_spots,
        );
    }
    if state.layers.geofences {
        layers::draw_geofences_2d(painter, palette, rect, &state.view, layers.geofences);
    }
    if state.layers.rings {
        layers::draw_coverage_2d(painter, palette, rect, &state.view, layers.coverage);
    }
    if state.layers.gaps {
        layers::draw_gaps_2d(painter, palette, rect, &state.view, layers.gaps);
    }
    if state.layers.hazards {
        layers::draw_hazards_2d(painter, palette, rect, &state.view, layers.hazards);
    }
    // Predicted lines have no toggle: a prediction the operator cannot see is the case
    // DN-02 §5 warns about, and an empty list draws nothing.
    layers::draw_predictions_2d(painter, palette, rect, &state.view, layers.predictions);
    // A dense group has no toggle either: a raid the operator cannot see is the failure
    // GAP-015 exists about, and `None` draws nothing.
    layers::draw_dense_group_2d(painter, palette, rect, &state.view, layers.dense_group);
    // The laydown preview has no toggle either, for the same reason: selecting an
    // option on PN-16 is what turns it on, and there is nothing to hide when nothing
    // is selected (GAP-087).
    layers::draw_laydown_preview_2d(painter, palette, rect, &state.view, layers.laydown_preview);
    // **A hidden layer and an empty one look identical.** Saying which is the whole
    // reason PN-11's toggles are safe to have: without this, turning coverage off would
    // make the map claim a sector nobody had measured.
    if state.layers.anything_hidden() {
        painter.text(
            rect.left_bottom() + egui::vec2(palette.panel_spacing, -palette.panel_spacing),
            egui::Align2::LEFT_BOTTOM,
            format!("layers hidden: {}", state.layers.hidden_names().join(", ")),
            egui::FontId::proportional(palette.small_font_size),
            palette.warning_color,
        );
    }
}

/// Draw each track's glyph and the plan's assignments: the id label, the heading vector,
/// the one-sigma outline, the classification frame and the resource each track is
/// assigned to (GAP-251).
///
/// One function for both renderers, for `draw_layers`'s reason: the three-d scene drew
/// each track as a bare disc while the projection drew all of this, so the scene's status
/// line counted assignments the operator could not see. In the scene these sit over the
/// disc, which is the lifecycle fill the projection draws small inside each frame.
fn draw_tracks(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    state: &ViewportState,
    plan: &PlanView,
) {
    tracks::draw_glyphs_2d(painter, palette, rect, &state.view, &state.glyphs);
    tracks::draw_plan_2d(painter, palette, rect, &state.view, &state.glyphs, plan);
}

pub fn draw_renderer_toggle(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    rect: egui::Rect,
    state: &mut ViewportState,
) -> bool {
    if !state.gl_ready {
        return false;
    }
    let label = if state.use_3d { "3D" } else { "2D" };
    let size = egui::vec2(36.0, 20.0);
    let corner = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - size.x - palette.panel_spacing,
            rect.top() + palette.panel_spacing,
        ),
        size,
    );
    let response = ui
        .put(corner, egui::Button::new(label).small())
        .on_hover_text(
            "Switch between the three-d scene and the 2D projection. The projection is \
             the path the automated tests can see.",
        );
    if response.clicked() {
        state.use_3d = !state.use_3d;
        return true;
    }
    false
}

/// Where [`prepare_3d`] put the three-d scene's two GL passes.
#[derive(Debug, Clone, Copy)]
pub struct Prepared3d {
    /// The region the paint callbacks draw into.
    pub rect: egui::Rect,
    /// Who draws the ground this frame: the ground pass draws it only for
    /// [`Ground::Scene`], and uploads the surface whatever it is told (DN-35 §7 item 2).
    pub ground: Ground,
    /// The place in the paint order the ground pass goes, with `egui::Painter::set`: after
    /// the background and the metre grid, before every layer, so the layers lie on the
    /// ground rather than under it (GAP-252, DN-35 §5.1).
    pub ground_slot: egui::layers::ShapeIdx,
    /// The place in the paint order the glyph pass goes: after the map and before the
    /// glyphs, so each track's frame and labels draw over its disc rather than under it
    /// (GAP-251). A slot nobody fills draws nothing.
    pub scene_slot: egui::layers::ShapeIdx,
}

/// Prepare the viewport region for a three-d frame, and return where the caller's two
/// paint callbacks go (GAP-022, GAP-252).
///
/// Everything except the OpenGL draw calls lives here: input, the glyph rebuild, the
/// surface's build, which renderer draws the ground, the background, the layers between
/// the two GL passes, the per-track glyphs over the scene, and the status line saying
/// which renderer is in use. The caller sets the paint callbacks into
/// [`Prepared3d::ground_slot`] and [`Prepared3d::scene_slot`], because the callback type
/// belongs to `egui_glow` and only the binary depends on eframe. `ground_resident` is the
/// surface revision the GL pass holds, read by the caller under the renderer's lock; a lock
/// it could not take reads as `None`, which leaves egui drawing the ground.
///
/// Returns `None` when there is nothing to draw into.
#[allow(clippy::too_many_arguments)]
pub fn prepare_3d(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    state: &mut ViewportState,
    tracks: &[TrackView],
    bearing_rays: &[BearingRayView],
    plan: &PlanView,
    layers: layers::LayerInputs<'_>,
    ground_resident: Option<u64>,
) -> Option<Prepared3d> {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return None;
    }
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    state.view.apply_input(&response, scroll);

    if tracks::glyphs_need_rebuild(&state.glyphs, tracks) {
        state.glyphs = tracks::update_track_symbols(tracks);
    }
    if tracks::bearing_glyphs_need_rebuild(&state.bearing_rays, bearing_rays) {
        state.bearing_rays = tracks::update_bearing_ray_glyphs(bearing_rays);
    }

    // Coverage is drawn by egui rather than in the GL pass: it is a flat overlay on
    // the ground plane, and drawing it here means it survives whatever the callback
    // does -- the same reason the status line is here (GAP-022 item 53). Bearing rays
    // join it for the same reason: the scene's only GL geometry is the track disc, so
    // this overlay is where a ray is drawn. The dead spots join it too (GAP-246, DN-34
    // §8.2): the same texture, the same toggle, on whichever renderer is in front.
    layers::sync_dead_spot_texture(
        ui.ctx(),
        palette,
        &state.view,
        &mut state.dead_spot_texture,
        layers.dead_spots,
    );
    layers::sync_terrain_surface(palette, &mut state.terrain_surface, layers.terrain);
    // Decided once, before anything is drawn: the ground is drawn by exactly one renderer
    // this frame (DN-35 §7 item 2).
    let ground = ground_source(state.terrain_surface().map(|s| s.revision), ground_resident);
    state.ground = ground;
    let painter = ui.painter_at(rect);
    // The projection's background and metre grid, so the two renderers differ only in
    // how a track is drawn, and the grid is a fixed reference for checking that the
    // scene puts each track where the overlay's rings and rays expect it.
    painter.rect_filled(rect, 0.0, palette.viewport_background);
    interaction::draw_grid(&painter, palette, rect, &state.view);
    // The ground pass goes here, translucent over the grid as the projection's ground is,
    // and every layer after it lies on it (GAP-252, DN-35 §5.1).
    let ground_slot = painter.add(egui::Shape::Noop);
    draw_layers(&painter, palette, rect, state, &layers, ground);
    tracks::draw_bearing_rays_2d(&painter, palette, rect, &state.view, &state.bearing_rays);

    // The scene's discs go here, and everything each track carries beyond its lifecycle
    // colour is drawn over them by the function the projection uses (GAP-251).
    let scene_slot = painter.add(egui::Shape::Noop);
    draw_tracks(&painter, palette, rect, state, plan);

    // The status line is drawn by egui after both GL passes, so nothing either draws can
    // cover it, and it says what the operator is looking at whether or not the GL draw
    // produced anything (ARCHITECTURE.md §10 items 52 and 53; DN-35 §5.1).
    painter.text(
        rect.left_top() + egui::vec2(palette.panel_spacing, palette.panel_spacing),
        egui::Align2::LEFT_TOP,
        format!(
            "three-d scene: {} tracks, {} bearing rays, {} assignments. Drag to pan, scroll to zoom.",
            state.glyphs.len(),
            state.bearing_rays.len(),
            plan.solutions().len()
        ),
        egui::FontId::proportional(palette.small_font_size),
        palette.viewport_text_color,
    );
    Some(Prepared3d {
        rect,
        ground,
        ground_slot,
        scene_slot,
    })
}

/// Per-frame entry point, called once from gungnir-app's `eframe::App::update()`
/// inside the central panel. Handles pan/zoom input, rebuilds glyphs only when the
/// track set changed, and draws.
pub fn render(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    state: &mut ViewportState,
    tracks: &[TrackView],
    bearing_rays: &[BearingRayView],
    plan: &PlanView,
    layers: layers::LayerInputs<'_>,
) {
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    state.view.apply_input(&response, scroll);

    if tracks::glyphs_need_rebuild(&state.glyphs, tracks) {
        state.glyphs = tracks::update_track_symbols(tracks);
    }
    if tracks::bearing_glyphs_need_rebuild(&state.bearing_rays, bearing_rays) {
        state.bearing_rays = tracks::update_bearing_ray_glyphs(bearing_rays);
    }

    layers::sync_dead_spot_texture(
        ui.ctx(),
        palette,
        &state.view,
        &mut state.dead_spot_texture,
        layers.dead_spots,
    );
    layers::sync_terrain_surface(palette, &mut state.terrain_surface, layers.terrain);
    // The projection always draws its own ground (DN-35 §7 item 1).
    state.ground = Ground::Overlay;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.viewport_background);
    interaction::draw_grid(&painter, palette, rect, &state.view);
    // Coverage before the glyphs: it is context, and a ring must never sit on top of
    // the symbol an operator is looking at (GAP-007). A bearing ray is context in the
    // same sense -- a direction with no measured position -- so it is drawn here too,
    // under the tracks it has not (yet) been folded into (DN-27 §7).
    draw_layers(&painter, palette, rect, state, &layers, Ground::Overlay);
    tracks::draw_bearing_rays_2d(&painter, palette, rect, &state.view, &state.bearing_rays);
    draw_tracks(&painter, palette, rect, state, plan);

    // Says which renderer is running and why, because those are different situations:
    // a deployment that opted out of the scene, and a desktop that could not attach one.
    let status = if state.gl_ready {
        format!(
            "Top-down projection ({} tracks, {} bearing rays, {} assignments). The three-d scene is attached; use the control in the corner to switch. Drag to pan, scroll to zoom.",
            state.glyphs.len(),
            state.bearing_rays.len(),
            plan.solutions().len()
        )
    } else {
        format!(
            "Top-down projection ({} tracks, {} bearing rays, {} assignments). No three-d scene is attached, so there is nothing to switch to. Drag to pan, scroll to zoom.",
            state.glyphs.len(),
            state.bearing_rays.len(),
            plan.solutions().len()
        )
    };
    painter.text(
        rect.left_top() + egui::vec2(palette.panel_spacing, palette.panel_spacing),
        egui::Align2::LEFT_TOP,
        status,
        egui::FontId::proportional(palette.small_font_size),
        palette.viewport_text_color,
    );

    // Drawn from here too, or switching to the projection would be one-way.
    draw_renderer_toggle(ui, palette, rect, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{
        Classification, InterceptSolutionView, MissionTime, PlanId, Provenance, Quality,
        ResourceId, TrackId, TrackStatus,
    };
    use nalgebra::{SMatrix, SVector};
    use std::sync::Arc;

    fn track(id: u64, e: f64, classification: Classification) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: SVector::<f64, 6>::new(e, 0.0, 0.0, 3.0, 4.0, 0.0),
            covariance: SMatrix::<f64, 6, 6>::identity() * 400.0,
            classification,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn no_layers() -> layers::LayerInputs<'static> {
        layers::LayerInputs {
            coverage: layers::CoverageLayer::Circles {
                circles: &[],
                nominal: None,
            },
            gaps: &[],
            hazards: &[],
            geofences: &[],
            predictions: &[],
            terrain: None,
            point_clouds: &[],
            laydown_preview: None,
            dense_group: None,
            dead_spots: layers::DeadSpotLayer::NotComputed { reason: "test" },
        }
    }

    fn screen() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..egui::RawInput::default()
        }
    }

    /// The shapes `prepare_3d` painted after the scene's callback, with the callback set
    /// into its slot as the binary sets it, and the state it left.
    fn painted_over_the_scene(
        tracks: &[TrackView],
        plan: &PlanView,
    ) -> (Vec<egui::Shape>, ViewportState, egui::Rect) {
        let palette = theme::Palette::day();
        let mut state = ViewportState::new();
        let mut rect = egui::Rect::NOTHING;
        let output = egui::Context::default().run_ui(screen(), |ui| {
            let prepared = prepare_3d(
                ui,
                &palette,
                &mut state,
                tracks,
                &[],
                plan,
                no_layers(),
                None,
            )
            .expect("an 800 by 600 screen has room for the scene");
            rect = prepared.rect;
            ui.painter().set(
                prepared.scene_slot,
                egui::PaintCallback {
                    rect: prepared.rect,
                    callback: Arc::new(()),
                },
            );
        });
        let shapes: Vec<egui::Shape> = output.shapes.into_iter().map(|c| c.shape).collect();
        let scene = shapes
            .iter()
            .position(|s| matches!(s, egui::Shape::Callback(_)))
            .expect("the scene's callback is in the paint order");
        (shapes[scene + 1..].to_vec(), state, rect)
    }

    fn texts(shapes: &[egui::Shape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    fn plan_assigning(resource: u32, track: u64) -> PlanView {
        PlanView::intercept(
            PlanId(1),
            MissionTime(0.0),
            vec![InterceptSolutionView {
                resource: ResourceId(resource),
                track: TrackId(track),
                intercept_point: None,
                time_to_intercept_s: None,
            }],
            1.0,
        )
    }

    /// GAP-251: the scene drew each track as a bare disc, so its status line counted an
    /// assignment the operator could not see. Each track's id and each assignment must be
    /// painted, and painted after the scene's callback, or the disc would draw over them.
    #[test]
    fn the_scene_labels_each_track_and_assignment_over_its_disc() {
        let tracks = [
            track(41, 0.0, Classification::Hostile),
            track(42, 300.0, Classification::Unknown),
        ];
        let (over, _, _) = painted_over_the_scene(&tracks, &plan_assigning(7, 41));
        let over = texts(&over);
        for expected in ["41", "42", "R7"] {
            assert!(
                over.iter().any(|t| t == expected),
                "{expected:?} is not painted over the scene; over it are {over:?}"
            );
        }
    }

    /// GAP-251: what the scene draws over its discs is exactly what the projection draws
    /// for each track -- frame, sigma outline, heading vector, id and assignment -- so the
    /// two renderers cannot show different details for the same track.
    #[test]
    fn the_scene_draws_every_track_detail_the_projection_does() {
        let tracks = [
            track(41, 0.0, Classification::Hostile),
            track(42, 300.0, Classification::Friendly),
        ];
        let plan = plan_assigning(7, 42);
        let (mut over, state, rect) = painted_over_the_scene(&tracks, &plan);
        // The status line is painted last, over both GL passes (GAP-252); it is not a
        // track detail.
        assert!(
            matches!(over.pop(), Some(egui::Shape::Text(t)) if t.galley.text().starts_with("three-d scene: "))
        );

        let palette = theme::Palette::day();
        let projection: Vec<egui::Shape> = egui::Context::default()
            .run_ui(screen(), |ui| {
                draw_tracks(&ui.painter_at(rect), &palette, rect, &state, &plan);
            })
            .shapes
            .into_iter()
            .map(|c| c.shape)
            .collect();
        assert!(!projection.is_empty());
        assert_eq!(described(&over), described(&projection));
    }

    /// GAP-252, DN-35 §8.1 row d: the scene draws its own ground only when the GL pass
    /// holds the revision the frame wants, so no pairing draws it twice or never.
    #[test]
    fn the_ground_is_drawn_by_exactly_one_renderer() {
        let revisions = [None, Some(1), Some(2)];
        for wanted in revisions {
            for resident in revisions {
                let expected = if wanted.is_some() && wanted == resident {
                    Ground::Scene
                } else {
                    Ground::Overlay
                };
                assert_eq!(
                    ground_source(wanted, resident),
                    expected,
                    "wanted {wanted:?}, resident {resident:?}"
                );
            }
        }
    }

    /// A rolling 20 by 24 grid of 30 m cells around the origin.
    #[allow(clippy::cast_precision_loss)]
    fn hills() -> Vec<[f32; 3]> {
        let mut positions = Vec::new();
        for r in 0..20 {
            for c in 0..24 {
                let (e, n) = (c as f32 * 30.0 - 360.0, (19 - r) as f32 * 30.0 - 300.0);
                positions.push([e, n, 80.0 * (e / 200.0).sin() * (n / 260.0).cos() + 100.0]);
            }
        }
        positions
    }

    fn no_plan() -> PlanView {
        PlanView::intercept(PlanId(1), MissionTime(0.0), Vec::new(), 1.0)
    }

    const GROUND: &str = "ground pass";
    const GLYPHS: &str = "glyph pass";

    /// One frame of the scene over `hills`, a dead-spot field and a ring, with stand-in
    /// callbacks set into both slots as the binary sets them; the shapes in paint order.
    fn scene_frame(
        state: &mut ViewportState,
        terrain: &[[f32; 3]],
        resident: Option<u64>,
    ) -> (Vec<egui::Shape>, Ground) {
        let palette = theme::Palette::day();
        let cells = [layers::DeadSpotCell::Shadow; 4];
        let circles = [layers::CoverageCircle {
            sensor: 1,
            center: [0.0, 0.0, 0.0],
            radius_m: 2_000.0,
            confidence: 1.0,
            sector: None,
        }];
        let inputs = layers::LayerInputs {
            coverage: layers::CoverageLayer::Circles {
                circles: &circles,
                nominal: None,
            },
            terrain: Some(layers::TerrainLayer {
                positions: terrain,
                rows: 20,
                columns: 24,
                revision: 7,
            }),
            dead_spots: layers::DeadSpotLayer::Field(layers::DeadSpotField {
                revision: 1,
                origin_enu: [-300.0, -300.0],
                cell_m: 300.0,
                rows: 2,
                columns: 2,
                cells: &cells,
                height_agl_m: 100.0,
                age_s: 1.0,
                stale: false,
                sensor: None,
                linked: false,
                what_if: None,
            }),
            ..no_layers()
        };
        let tracks = [track(41, 0.0, Classification::Hostile)];
        let mut ground = Ground::Overlay;
        let output = egui::Context::default().run_ui(screen(), |ui| {
            let prepared = prepare_3d(
                ui,
                &palette,
                state,
                &tracks,
                &[],
                &no_plan(),
                inputs,
                resident,
            )
            .expect("an 800 by 600 screen has room for the scene");
            ground = prepared.ground;
            for (slot, name) in [
                (prepared.ground_slot, GROUND),
                (prepared.scene_slot, GLYPHS),
            ] {
                ui.painter().set(
                    slot,
                    egui::PaintCallback {
                        rect: prepared.rect,
                        callback: Arc::new(name),
                    },
                );
            }
        });
        (output.shapes.into_iter().map(|c| c.shape).collect(), ground)
    }

    /// Where the first shape `is` holds is in the paint order.
    fn first(shapes: &[egui::Shape], what: &str, is: impl Fn(&egui::Shape) -> bool) -> usize {
        shapes
            .iter()
            .position(is)
            .unwrap_or_else(|| panic!("no {what} was painted"))
    }

    fn callback(name: &'static str) -> impl Fn(&egui::Shape) -> bool {
        move |s| {
            matches!(s, egui::Shape::Callback(c)
                if c.callback.downcast_ref::<&str>() == Some(&name))
        }
    }

    /// The terrain's egui mesh: untextured, one vertex per kept grid vertex.
    fn is_terrain(s: &egui::Shape) -> bool {
        matches!(s, egui::Shape::Mesh(m)
            if m.texture_id == egui::TextureId::default() && m.vertices.len() == 20 * 24)
    }

    fn is_dead_spot_image(s: &egui::Shape) -> bool {
        matches!(s, egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default())
    }

    fn says(prefix: &'static str) -> impl Fn(&egui::Shape) -> bool {
        move |s| matches!(s, egui::Shape::Text(t) if t.galley.text().starts_with(prefix))
    }

    /// GAP-252, DN-35 §8.1 row e: the order on screen is the projection's, with two of its
    /// steps drawn by GL. In the scene: background, grid, the ground pass, the dead-spot
    /// image, the layers, the glyph pass, each track's labels, the status line. With the
    /// overlay drawing the ground, its mesh comes after the ground slot and before the
    /// dead-spot image, so whatever the slot holds is under it.
    #[test]
    fn the_scene_paints_the_ground_under_the_layers_and_the_status_line_last() {
        let terrain = hills();
        let mut state = ViewportState::new();
        let (shapes, ground) = scene_frame(&mut state, &terrain, Some(7));
        assert_eq!(ground, Ground::Scene);
        assert_eq!(
            state.ground_line().as_deref(),
            Some("ground drawn by the scene")
        );
        let background = first(
            &shapes,
            "background",
            |s| matches!(s, egui::Shape::Rect(r) if r.fill == theme::Palette::day().viewport_background),
        );
        let grid = first(&shapes, "grid line", |s| {
            matches!(s, egui::Shape::LineSegment { .. })
        });
        let ground = first(&shapes, "ground pass", callback(GROUND));
        let dead_spots = first(&shapes, "dead-spot image", is_dead_spot_image);
        let ring_radius = layers::radius_in_pixels(2_000.0, &state.view);
        let ring = first(
            &shapes,
            "coverage ring",
            |s| matches!(s, egui::Shape::Circle(c) if (c.radius - ring_radius).abs() < 0.5),
        );
        let glyphs = first(&shapes, "glyph pass", callback(GLYPHS));
        let label = first(
            &shapes,
            "track label",
            |s| matches!(s, egui::Shape::Text(t) if t.galley.text() == "41"),
        );
        let status = first(&shapes, "status line", says("three-d scene: "));
        let order = [
            background, grid, ground, dead_spots, ring, glyphs, label, status,
        ];
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "background, grid, ground, dead spots, ring, glyphs, label, status: {order:?}"
        );
        assert_eq!(
            status,
            shapes.len() - 1,
            "nothing is painted over the status line"
        );
        assert!(
            !shapes.iter().any(is_terrain),
            "the scene draws its own ground, so egui must not draw it as well"
        );

        let mut state = ViewportState::new();
        let (shapes, ground) = scene_frame(&mut state, &terrain, None);
        assert_eq!(ground, Ground::Overlay);
        let slot = first(&shapes, "ground pass", callback(GROUND));
        let terrain_mesh = first(&shapes, "terrain mesh", is_terrain);
        let dead_spots = first(&shapes, "dead-spot image", is_dead_spot_image);
        assert!(
            slot < terrain_mesh && terrain_mesh < dead_spots,
            "slot {slot}, terrain {terrain_mesh}, dead spots {dead_spots}"
        );
    }

    /// GAP-252, DN-35 §8.1 row d through the frame: the frame that sees the surface first
    /// has egui draw the ground while the ground pass uploads it; once the pass holds it,
    /// the scene draws it and egui does not. Dropping the terrain drops the surface.
    #[test]
    fn the_hand_over_draws_the_ground_once_on_every_frame() {
        let terrain = hills();
        let mut state = ViewportState::new();
        let (load, ground) = scene_frame(&mut state, &terrain, None);
        assert_eq!(ground, Ground::Overlay);
        assert_eq!(
            load.iter().filter(|s| is_terrain(s)).count(),
            1,
            "the load frame"
        );
        assert_eq!(
            state.ground_line().as_deref(),
            Some("ground drawn by the overlay")
        );
        let uploaded = state.terrain_surface().map(|s| s.revision);
        assert_eq!(uploaded, Some(7));
        for _ in 0..3 {
            let (after, ground) = scene_frame(&mut state, &terrain, uploaded);
            assert_eq!(ground, Ground::Scene);
            assert_eq!(after.iter().filter(|s| is_terrain(s)).count(), 0);
        }

        let palette = theme::Palette::day();
        let _ = egui::Context::default().run_ui(screen(), |ui| {
            prepare_3d(
                ui,
                &palette,
                &mut state,
                &[],
                &[],
                &no_plan(),
                no_layers(),
                Some(7),
            );
        });
        assert!(
            state.terrain_surface.is_none(),
            "no terrain drops the surface"
        );
        assert_eq!(state.ground, Ground::Overlay);
        assert_eq!(state.ground_line(), None);
    }

    /// A shape as a comparable string. Text is its position, words and colour: a galley
    /// also carries where its glyphs sit in the context's font atlas, which differs
    /// between two contexts drawing the same words.
    fn described(shapes: &[egui::Shape]) -> Vec<String> {
        shapes
            .iter()
            .map(|s| match s {
                egui::Shape::Text(t) => {
                    format!(
                        "text {:?} {:?} {:?}",
                        t.pos,
                        t.galley.text(),
                        t.fallback_color
                    )
                }
                other => format!("{other:?}"),
            })
            .collect()
    }
}
