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

/// Everything PN-11 draws.
// Four independent toggles are four bools; see `gungnir_viewport3d::CoverageLayers`.
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
}

/// What the operator toggled this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerAction {
    ShowRings(bool),
    ShowGaps(bool),
    ShowHazards(bool),
    ShowGeofences(bool),
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

    // **The sentence this panel exists to make impossible to miss.** A hidden layer and
    // an empty one look identical on the map, and only one of them is the map telling
    // you something about the sector.
    if !view.rings_visible || !view.gaps_visible || !view.hazards_visible || !view.geofences_visible
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
