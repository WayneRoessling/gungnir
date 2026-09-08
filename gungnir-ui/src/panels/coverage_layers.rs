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
    if ui
        .checkbox(
            &mut gaps,
            layer_label("Gaps along approaches", view.counts.gaps),
        )
        .changed()
    {
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
