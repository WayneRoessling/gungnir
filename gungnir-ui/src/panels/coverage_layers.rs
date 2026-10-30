// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-11, coverage layer controls (GAP-007).
//!
//! `docs/ux/information-architecture.md` §1: which layers the viewport draws, for the
//! sensor manager, the planner and the supervisor.
//!
//! # Hiding a layer is not the same as there being nothing to draw
//!
//! The whole risk of a visibility control on a coverage map is that turning a layer off
//! makes the map look like a sector with no gaps in it. So this panel and the viewport
//! both say when something is hidden, and the panel says what each layer *would* show if
//! it were on -- a count an operator can read without turning it back on.
//!
//! # Before-and-after comparison (GAP-087)
//!
//! The information architecture lists this against PN-11, and it means putting a
//! laydown option's sensors and resources on the map, which needs an option selected
//! on PN-16 first. So the status line here is not a toggle -- there is nothing to turn
//! on or off from this panel -- it is a report of what PN-16's own selection is doing,
//! the same way [`draw_hazard_currency`] reports the hazard layer's staleness rather
//! than controlling it.

use crate::theme;
use egui::{RichText, Ui};

/// What each layer would draw, so an operator can tell an empty layer from a hidden one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerCounts {
    /// Sensors contributing a ring right now.
    pub rings: usize,
    /// Gap segments found along the declared approaches.
    pub gaps: usize,
    /// Of those, the segments a standing acceptance names (GAP-106): counted in `gaps`,
    /// never taken out of it.
    pub accepted_gaps: usize,
    /// Hazards placed on the map (DN-14, GAP-017).
    pub hazards: usize,
    /// Geofences placed on the map (GAP-088).
    pub geofences: usize,
}

/// What the hazard layer is and how current it is (DN-14 §5).
///
/// A static layer that claims to be live is a layer that lies: a boom removed last week
/// is still in the survey. The baseline version is the honest limit, and the panel says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HazardCurrency {
    /// Hazards the baseline declares, placed or not.
    pub declared: usize,
    /// The baseline version the layer came from.
    pub baseline_version: u32,
}

/// Why there is no coverage to draw at all, when there is none.
///
/// Distinct from every layer being hidden: this one is the deployment's state and the
/// operator cannot fix it from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NothingToDraw<'a> {
    /// There is coverage; the counts say how much.
    Available,
    /// The reason, in the words `gungnir_viewport3d::layers::NoCoverage` already uses.
    Because { reason: &'a str },
}

/// What PN-16's own selection is doing to the map, for the status line here to report
/// (GAP-087). Not a toggle: there is nothing to turn on or off from this panel, only a
/// selection made on PN-16 to reflect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LaydownComparison<'a> {
    /// No option is selected on PN-16.
    NothingSelected,
    /// Previewing this option: its own words for what it is for, and how much it
    /// places, so an operator can tell a preview with nothing in it from one that has
    /// not loaded.
    Showing {
        intent: &'a str,
        sensors: usize,
        resources: usize,
    },
}

/// Where the dead-spot field stands (GAP-246, `docs/design/DN-34-sensor-dead-spots.md` §7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeadSpotStatus<'a> {
    /// Not computed, and why: no DEM, no local frame, a refused surface. The dead spots
    /// are unknown, not absent, and the panel says which.
    NotComputed { reason: &'a str },
    /// The first field is being computed.
    Computing,
    Computed {
        /// The measure in words (`gungnir_analytics::ShadowMeasure::sentence`).
        measure: &'a str,
        age_s: f64,
        cell_m: f64,
        /// The inputs changed and a newer field is on its way.
        stale: bool,
        /// The cell asked for, when the budget made it coarser.
        clamped_from_m: Option<f64>,
        /// The desktop is linked: the field is its own, from the node's sensors (D-148).
        linked: bool,
    },
}

/// The change a dead-spot what-if will try, as the operator is setting it (GAP-249, D-149,
/// `docs/design/DN-34-sensor-dead-spots.md` amendment 4). Held by the desktop and edited
/// here; nothing is computed until it is run.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WhatIfDraft {
    /// The sensor to try, by identifier: any declared sensor, whatever its mode.
    pub sensor: Option<u32>,
    /// Metres added to its declared height; negative lowers it.
    pub mast_m: f64,
    /// Degrees its sector is turned clockwise; ignored for a sensor with no sector.
    pub turn_deg: f64,
    /// Metres it is moved east and north.
    pub east_m: f64,
    pub north_m: f64,
}

/// A sensor a what-if may be run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhatIfSensor<'a> {
    pub id: u32,
    /// Its name and its mode now, so a standby sensor reads as one.
    pub label: &'a str,
    /// Whether it declares a sector, so there is something to turn.
    pub sectored: bool,
}

/// Where a what-if stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WhatIfStatus<'a> {
    /// None has been run, or the last was cleared.
    NotRun,
    /// The last run was refused before anything was computed, and why.
    Refused { reason: &'a str },
    /// It cannot be computed -- no DEM, no frame -- and why.
    NotComputed { reason: &'a str },
    /// It is being computed.
    Computing { describes: &'a str },
    /// Its answer, on the map and here.
    Shown {
        describes: &'a str,
        /// Square kilometres of dead spots now and with the change, cleared by it and left
        /// newly dark by it, and watched now and with it.
        before_shadow_km2: f64,
        after_shadow_km2: f64,
        cleared_km2: f64,
        darkened_km2: f64,
        watched_before_km2: f64,
        watched_after_km2: f64,
        /// The live sensors, the terrain or the height changed and it is being recomputed.
        stale: bool,
    },
}

/// PN-11's what-if controls (GAP-249, D-152: on PN-11, beside the chooser).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhatIfView<'a> {
    pub sensors: &'a [WhatIfSensor<'a>],
    pub draft: WhatIfDraft,
    pub status: WhatIfStatus<'a>,
    /// A what-if is recorded with who ran it (D-151), so nobody signed in may run one.
    pub signed_in: bool,
}

/// PN-11's dead-spot controls (DN-34 §7 item 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeadSpotsView<'a> {
    pub visible: bool,
    /// The baseline's heights above ground, metres (`analytics.dead_spot_heights_agl_m`).
    pub heights_agl_m: &'a [f64],
    /// Which of them is shown.
    pub selected_height: usize,
    /// The active sensors by identifier and label, for showing one sensor's shadow alone.
    pub sensors: &'a [(u32, &'a str)],
    /// The sensor whose shadow alone is shown, or `None` for every active sensor's.
    pub for_sensor: Option<u32>,
    pub status: DeadSpotStatus<'a>,
    /// Trying a change to one sensor against the dead spots (GAP-249).
    pub what_if: WhatIfView<'a>,
}

/// Everything PN-11 draws.
// Five independent toggles are five bools; see `gungnir_viewport3d::CoverageLayers`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverageLayersView<'a> {
    pub rings_visible: bool,
    pub gaps_visible: bool,
    pub hazards_visible: bool,
    pub geofences_visible: bool,
    pub counts: LayerCounts,
    pub hazards: HazardCurrency,
    pub coverage: NothingToDraw<'a>,
    /// The before-and-after preview PN-16's selection is driving on the map.
    pub comparison: LaydownComparison<'a>,
    /// The terrain dead spots (GAP-246).
    pub dead_spots: DeadSpotsView<'a>,
}

/// What the operator toggled this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerAction {
    ShowRings(bool),
    ShowGaps(bool),
    ShowHazards(bool),
    ShowGeofences(bool),
    /// GAP-246: the dead-spot layer on or off.
    ShowDeadSpots(bool),
    /// Show the field at the baseline's height with this index.
    DeadSpotHeight(usize),
    /// Show one sensor's shadow alone, or every active sensor's.
    DeadSpotsFor(Option<u32>),
    /// Compute the field again now.
    RecomputeDeadSpots,
    /// GAP-249: the what-if being set changed.
    EditWhatIf(WhatIfDraft),
    /// Run the what-if as set.
    RunWhatIf,
    /// Take the what-if off the map.
    ClearWhatIf,
}

/// Render the coverage layer controls.
pub fn render_coverage_layers(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &CoverageLayersView<'_>,
) -> Option<LayerAction> {
    let mut action = None;
    ui.heading("Coverage layers");

    if let NothingToDraw::Because { reason } = view.coverage {
        // The controls still draw, because an operator should be able to see what they
        // would toggle; but the reason comes first, since toggling will change nothing.
        ui.label(RichText::new(reason).color(palette.warning_color));
        ui.separator();
    }

    let mut rings = view.rings_visible;
    if ui
        .checkbox(
            &mut rings,
            layer_label("Sensor coverage rings", view.counts.rings),
        )
        .changed()
    {
        action = Some(LayerAction::ShowRings(rings));
    }

    let mut gaps = view.gaps_visible;
    let gap_label = if view.counts.accepted_gaps > 0 {
        format!(
            "Gaps along approaches ({}, {} accepted)",
            view.counts.gaps, view.counts.accepted_gaps
        )
    } else {
        layer_label("Gaps along approaches", view.counts.gaps)
    };
    if ui.checkbox(&mut gaps, gap_label).changed() {
        action = Some(LayerAction::ShowGaps(gaps));
    }

    let mut hazards = view.hazards_visible;
    if ui
        .checkbox(
            &mut hazards,
            layer_label("Hazards and barriers", view.counts.hazards),
        )
        .changed()
    {
        action = Some(LayerAction::ShowHazards(hazards));
    }
    draw_hazard_currency(ui, palette, view.hazards, view.counts.hazards);

    let mut geofences = view.geofences_visible;
    if ui
        .checkbox(
            &mut geofences,
            layer_label("Geofences (rules)", view.counts.geofences),
        )
        .changed()
    {
        action = Some(LayerAction::ShowGeofences(geofences));
    }

    ui.separator();
    if let Some(a) = draw_dead_spots(ui, palette, &view.dead_spots) {
        action = Some(a);
    }

    // **The sentence this panel exists to make impossible to miss.** A hidden layer and
    // an empty one look identical on the map, and only one of them is the map telling
    // you something about the sector.
    if !view.rings_visible
        || !view.gaps_visible
        || !view.hazards_visible
        || !view.geofences_visible
        || !view.dead_spots.visible
    {
        ui.separator();
        ui.label(
            RichText::new(
                "A hidden layer is not an empty one: the map is not showing everything \
                 it has.",
            )
            .color(palette.warning_color),
        );
    }

    ui.separator();
    draw_laydown_comparison(ui, palette, view.comparison);
    action
}

/// The dead-spot controls (GAP-246, DN-34 §7 item 1): the toggle, the height, whose shadow,
/// what the field measures, how old it is, and the control to compute it again.
fn draw_dead_spots(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &DeadSpotsView<'_>,
) -> Option<LayerAction> {
    let mut action = None;
    let mut visible = view.visible;
    let label = match view.status {
        DeadSpotStatus::NotComputed { .. } => "Terrain dead spots (not computed)",
        DeadSpotStatus::Computing => "Terrain dead spots (computing)",
        DeadSpotStatus::Computed { .. } => "Terrain dead spots",
    };
    if ui.checkbox(&mut visible, label).changed() {
        action = Some(LayerAction::ShowDeadSpots(visible));
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Height above ground:").small());
        if view.heights_agl_m.len() > 1 {
            for (i, h) in view.heights_agl_m.iter().enumerate() {
                if ui
                    .selectable_label(i == view.selected_height, format!("{h:.0} m"))
                    .clicked()
                    && i != view.selected_height
                {
                    action = Some(LayerAction::DeadSpotHeight(i));
                }
            }
        } else if let Some(h) = view.heights_agl_m.first() {
            ui.label(RichText::new(format!("{h:.0} m")).small());
        }
    });
    if !view.sensors.is_empty() {
        let shown = view
            .for_sensor
            .and_then(|id| view.sensors.iter().find(|(s, _)| *s == id))
            .map_or("every active sensor", |(_, name)| *name);
        egui::ComboBox::from_id_salt("dead_spots_for")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(view.for_sensor.is_none(), "every active sensor")
                    .clicked()
                {
                    action = Some(LayerAction::DeadSpotsFor(None));
                }
                for (id, name) in view.sensors {
                    if ui
                        .selectable_label(view.for_sensor == Some(*id), *name)
                        .clicked()
                    {
                        action = Some(LayerAction::DeadSpotsFor(Some(*id)));
                    }
                }
            });
    }
    draw_dead_spot_status(ui, palette, view.status);
    if ui.small_button("Recompute dead spots").clicked() {
        action = Some(LayerAction::RecomputeDeadSpots);
    }
    ui.separator();
    if let Some(a) = draw_what_if(ui, palette, &view.what_if) {
        action = Some(a);
    }
    action
}

/// The what-if controls (GAP-249, DN-34 amendment 4): which sensor, what change, run and
/// clear, and what the last run found.
fn draw_what_if(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &WhatIfView<'_>,
) -> Option<LayerAction> {
    let mut action = None;
    ui.label(RichText::new("What if").small().strong());
    ui.label(
        RichText::new(
            "Try one sensor switched on, its mast raised or lowered, its sector turned, or \
             moved. Each run is recorded with who ran it; nothing is commanded.",
        )
        .small()
        .color(palette.muted_text_color()),
    );
    let mut draft = view.draft;
    let mut changed = false;
    let chosen = draft
        .sensor
        .and_then(|id| view.sensors.iter().find(|s| s.id == id));
    egui::ComboBox::from_id_salt("dead_spot_what_if_sensor")
        .selected_text(chosen.map_or("choose a sensor", |s| s.label))
        .show_ui(ui, |ui| {
            for s in view.sensors {
                if ui
                    .selectable_label(draft.sensor == Some(s.id), s.label)
                    .clicked()
                {
                    draft.sensor = Some(s.id);
                    changed = true;
                }
            }
        });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Mast").small());
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.mast_m)
                    .range(-200.0..=500.0)
                    .suffix(" m"),
            )
            .changed();
        if chosen.is_some_and(|s| s.sectored) {
            ui.label(RichText::new("Turn").small());
            changed |= ui
                .add(
                    egui::DragValue::new(&mut draft.turn_deg)
                        .range(-180.0..=180.0)
                        .suffix(" deg"),
                )
                .changed();
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Move east").small());
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.east_m)
                    .range(-50_000.0..=50_000.0)
                    .suffix(" m"),
            )
            .changed();
        ui.label(RichText::new("north").small());
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.north_m)
                    .range(-50_000.0..=50_000.0)
                    .suffix(" m"),
            )
            .changed();
    });
    if changed {
        action = Some(LayerAction::EditWhatIf(draft));
    }
    ui.horizontal(|ui| {
        let may_run = view.signed_in && draft.sensor.is_some();
        if ui
            .add_enabled(may_run, egui::Button::new("Run what-if"))
            .clicked()
        {
            action = Some(LayerAction::RunWhatIf);
        }
        let shown = !matches!(view.status, WhatIfStatus::NotRun);
        if ui
            .add_enabled(shown, egui::Button::new("Clear what-if"))
            .clicked()
        {
            action = Some(LayerAction::ClearWhatIf);
        }
    });
    if !view.signed_in {
        ui.label(
            RichText::new("Sign in to run a what-if: each is recorded with who ran it.")
                .small()
                .color(palette.muted_text_color()),
        );
    }
    draw_what_if_status(ui, palette, view.status);
    action
}

/// What the last what-if found, or why it has nothing.
fn draw_what_if_status(ui: &mut Ui, palette: &theme::Palette, status: WhatIfStatus<'_>) {
    match status {
        WhatIfStatus::NotRun => {}
        WhatIfStatus::Refused { reason } => {
            ui.label(
                RichText::new(format!("Not run: {reason}"))
                    .small()
                    .color(palette.warning_color),
            );
        }
        WhatIfStatus::NotComputed { reason } => {
            ui.label(
                RichText::new(format!("Not computed: {reason}"))
                    .small()
                    .color(palette.warning_color),
            );
        }
        WhatIfStatus::Computing { describes } => {
            ui.label(
                RichText::new(format!("Computing what-if: {describes}"))
                    .small()
                    .color(palette.muted_text_color()),
            );
        }
        WhatIfStatus::Shown {
            describes,
            before_shadow_km2,
            after_shadow_km2,
            cleared_km2,
            darkened_km2,
            watched_before_km2,
            watched_after_km2,
            stale,
        } => {
            ui.label(
                RichText::new(format!("What-if: {describes}"))
                    .small()
                    .strong(),
            );
            ui.label(
                RichText::new(format!(
                    "Dead spots from {before_shadow_km2:.1} to {after_shadow_km2:.1} km\u{b2}: \
                 {cleared_km2:.1} km\u{b2} cleared, {darkened_km2:.1} km\u{b2} newly dark. \
                 Watched from {watched_before_km2:.1} to {watched_after_km2:.1} km\u{b2}."
                ))
                .small(),
            );
            ui.label(
                RichText::new(
                    "On the map: the live dead spots, with what the change would clear and \
                     darken marked over them. Not the live picture until the change is made.",
                )
                .small()
                .color(palette.muted_text_color()),
            );
            if stale {
                ui.label(
                    RichText::new(
                        "Recomputing: the live sensors, the terrain or the height changed.",
                    )
                    .small()
                    .color(palette.warning_color),
                );
            }
        }
    }
}

/// What the dead-spot field is, or why there is none.
fn draw_dead_spot_status(ui: &mut Ui, palette: &theme::Palette, status: DeadSpotStatus<'_>) {
    match status {
        DeadSpotStatus::NotComputed { reason } => {
            ui.label(
                RichText::new(format!(
                    "Not computed: {reason}. The dead spots are not absent, they are unknown."
                ))
                .small()
                .color(palette.warning_color),
            );
        }
        DeadSpotStatus::Computing => {
            ui.label(
                RichText::new("Computing the first field off the picture's thread.")
                    .small()
                    .color(palette.muted_text_color()),
            );
        }
        DeadSpotStatus::Computed {
            measure,
            age_s,
            cell_m,
            stale,
            clamped_from_m,
            linked,
        } => {
            ui.label(RichText::new(measure).small());
            ui.label(
                RichText::new(format!(
                    "Computed {age_s:.0} s ago on {cell_m:.0} m cells; geometric line of sight \
                     over the DEM, no refraction, so a radar sees a little more than this."
                ))
                .small()
                .color(palette.muted_text_color()),
            );
            if let Some(asked) = clamped_from_m {
                ui.label(
                    RichText::new(format!(
                        "The cell was made coarser to stay within the field's budget: \
                         {asked:.0} m asked, {cell_m:.0} m used."
                    ))
                    .small()
                    .color(palette.warning_color),
                );
            }
            if linked {
                ui.label(
                    RichText::new(
                        "This desktop's own field, from the node's sensors and this desktop's \
                         DEM: the node computes none.",
                    )
                    .small()
                    .color(palette.muted_text_color()),
                );
            }
            if stale {
                ui.label(
                    RichText::new("Recomputing: the sensors, the terrain or the height changed.")
                        .small()
                        .color(palette.warning_color),
                );
            }
        }
    }
}

/// The before-and-after preview's status: what PN-16 has selected, or that nothing is.
fn draw_laydown_comparison(
    ui: &mut Ui,
    palette: &theme::Palette,
    comparison: LaydownComparison<'_>,
) {
    ui.label(RichText::new("Laydown preview").small().strong());
    match comparison {
        LaydownComparison::NothingSelected => {
            ui.label(
                RichText::new("No option selected on PN-16.")
                    .small()
                    .color(palette.muted_text_color()),
            );
        }
        LaydownComparison::Showing {
            intent,
            sensors,
            resources,
        } => {
            ui.label(
                RichText::new(format!(
                    "Previewing \"{intent}\": {sensors} sensor(s), {resources} resource(s)."
                ))
                .small()
                .color(palette.muted_text_color()),
            );
        }
    }
}

/// Who accepted a gap, and why (GAP-106, `docs/design/DN-33-accepting-a-coverage-gap.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptanceLine<'a> {
    pub id: gungnir_model::GapAcceptanceId,
    pub operator: &'a str,
    pub role: &'a str,
    pub at: gungnir_model::MissionTime,
    pub reason: &'a str,
    /// What it holds for: the baseline revision and the laydown in force when accepted.
    pub revision: u32,
    pub laydown: Option<&'a str>,
}

/// One gap of the live report, and the acceptance that stands for it, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapLine<'a> {
    pub gap: &'a gungnir_model::AcceptedGap,
    pub acceptance: Option<AcceptanceLine<'a>>,
}

/// An acceptance that re-opened by itself this session, and why (DN-33 §5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReopenedLine<'a> {
    pub id: gungnir_model::GapAcceptanceId,
    pub gap: &'a gungnir_model::AcceptedGap,
    pub because: &'a str,
    pub at: gungnir_model::MissionTime,
}

/// DN-12's measure with the accepted part beside each total, never taken out of it
/// (DN-33 §7). The panel's own copy of `gungnir_analytics::CoverageMeasure`: this crate
/// depends on the model alone.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MeasureLine {
    pub segments: usize,
    pub accepted_segments: usize,
    pub uncovered_m: f64,
    pub uncovered_accepted_m: f64,
    pub single_sensor_m: f64,
    pub single_sensor_accepted_m: f64,
}

impl MeasureLine {
    /// The measure in words: each total, with the accepted part of it.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} gap segment(s), {} accepted; {:.0} m uncovered ({:.0} m of it accepted), \
             {:.0} m single-sensor ({:.0} m of it accepted). An accepted gap still counts.",
            self.segments,
            self.accepted_segments,
            self.uncovered_m,
            self.uncovered_accepted_m,
            self.single_sensor_m,
            self.single_sensor_accepted_m
        )
    }
}

/// PN-11's gap list and its accept control (GAP-106).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapAcceptanceView<'a> {
    /// The live report's gaps, in its order; empty with a reason when there is no report.
    pub gaps: &'a [GapLine<'a>],
    pub measure: Option<MeasureLine>,
    /// Acceptances that re-opened this session.
    pub reopened: &'a [ReopenedLine<'a>],
    /// `Ok` when the signed-in role may accept a gap; otherwise who may, in words.
    pub may_accept: Result<(), &'a str>,
}

/// What the commander has typed and not recorded: the gap being accepted, and the reason
/// so far. Held by the window, like PN-15's draft, because a half-typed reason is nobody's
/// record until it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AcceptanceDraft {
    pub gap: Option<gungnir_model::AcceptedGap>,
    pub reason: String,
}

/// An acceptance to record: the gap as it was drawn, and the reason.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptGap {
    pub gap: gungnir_model::AcceptedGap,
    pub reason: String,
}

/// Render PN-11's gap list (GAP-106, DN-33 §8 rule 1): each gap with the acceptance that
/// stands for it or an accept control, the measure, and what re-opened. **Accepted gaps
/// are listed and drawn like any other**, marked accepted; nothing here hides one.
pub fn render_gap_acceptance(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &GapAcceptanceView<'_>,
    draft: &mut AcceptanceDraft,
) -> Option<AcceptGap> {
    let mut action = None;
    ui.label(RichText::new("Gaps and their acceptance").small().strong());
    if let Some(measure) = view.measure {
        ui.label(
            RichText::new(measure.sentence())
                .small()
                .color(palette.muted_text_color()),
        );
    }
    if view.gaps.is_empty() {
        ui.label(
            RichText::new("No gap on the declared approaches to accept.")
                .small()
                .color(palette.muted_text_color()),
        );
    }
    for (i, line) in view.gaps.iter().enumerate() {
        if let Some(a) = line.acceptance {
            ui.label(format!("{} -- ACCEPTED {}", line.gap.describe(), a.id));
            ui.label(
                RichText::new(format!(
                    "by operator {} as {} at T+{:.0} s, under baseline revision {}{}: {}",
                    a.operator,
                    a.role,
                    a.at.0,
                    a.revision,
                    a.laydown
                        .map_or_else(String::new, |l| format!(" and laydown {l}")),
                    a.reason
                ))
                .small()
                .color(palette.muted_text_color()),
            );
        } else {
            ui.horizontal(|ui| {
                ui.label(line.gap.describe());
                let drafting = draft.gap.as_ref() == Some(line.gap);
                if view.may_accept.is_ok()
                    && !drafting
                    && ui
                        .push_id(("accept_gap", i), |ui| ui.small_button("Accept..."))
                        .inner
                        .clicked()
                {
                    draft.gap = Some(line.gap.clone());
                    draft.reason.clear();
                }
            });
            if draft.gap.as_ref() == Some(line.gap) {
                ui.label("Why is this gap accepted? (recorded with your name)");
                ui.text_edit_singleline(&mut draft.reason);
                ui.horizontal(|ui| {
                    let has_reason = !draft.reason.trim().is_empty();
                    if ui
                        .add_enabled(has_reason, egui::Button::new("Record acceptance"))
                        .clicked()
                    {
                        action = Some(AcceptGap {
                            gap: line.gap.clone(),
                            reason: draft.reason.trim().to_owned(),
                        });
                    }
                    if ui.small_button("Cancel").clicked() {
                        *draft = AcceptanceDraft::default();
                    }
                });
            }
        }
    }
    if let Err(who) = view.may_accept {
        ui.label(RichText::new(who).small().color(palette.muted_text_color()));
    }
    if !view.reopened.is_empty() {
        ui.label(RichText::new("Re-opened this session").small().strong());
        for r in view.reopened {
            ui.label(
                RichText::new(format!(
                    "{} of {} re-opened at T+{:.0} s: {}",
                    r.id,
                    r.gap.describe(),
                    r.at.0,
                    r.because
                ))
                .small()
                .color(palette.warning_color),
            );
        }
    }
    action
}

/// DN-14 §5: the layer is static and says so, with the baseline version it came from.
fn draw_hazard_currency(
    ui: &mut Ui,
    palette: &theme::Palette,
    currency: HazardCurrency,
    placed: usize,
) {
    let line = match currency.declared {
        0 => "No hazards are declared in this baseline. A clear harbour on the map is a \
              baseline that lists nothing, not a survey."
            .to_string(),
        n if placed < n => format!(
            "{n} declared in baseline revision {}, {placed} placed: the rest cannot be put \
             on the map without a local frame origin.",
            currency.baseline_version
        ),
        n => format!(
            "{n} from baseline revision {}. A static layer: a boom removed since then is \
             still drawn until the baseline is promoted again.",
            currency.baseline_version
        ),
    };
    ui.label(
        RichText::new(line)
            .small()
            .color(palette.muted_text_color()),
    );
}

/// A layer's label, carrying what it would draw.
///
/// The count is on the control rather than beside it, so an operator deciding whether to
/// turn a layer back on can see whether it would show anything.
fn layer_label(name: &str, count: usize) -> String {
    match count {
        0 => format!("{name} (nothing to draw)"),
        1 => format!("{name} (1)"),
        n => format!("{name} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer with nothing in it says so on its own control, so turning it on to find
    /// out is not the only way to know.
    #[test]
    fn a_layer_with_nothing_to_draw_says_so_on_its_control() {
        assert!(layer_label("Gaps", 0).contains("nothing to draw"));
        assert!(layer_label("Gaps", 1).contains("(1)"));
        assert!(layer_label("Gaps", 7).contains("(7)"));
    }

    /// No coverage at all and every layer hidden are different states. The first is the
    /// deployment's, and the operator cannot fix it from this panel.
    #[test]
    fn no_coverage_and_hidden_layers_are_different_states() {
        let none = NothingToDraw::Because {
            reason: "this deployment has declared no local frame origin",
        };
        assert_ne!(none, NothingToDraw::Available);
    }
}
