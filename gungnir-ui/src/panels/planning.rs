// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-16, the planning panel: laydown options, compared (GAP-087,
//! `docs/design/DN-26-laydown-options.md`).
//!
//! **What this draws, and what it does not.** DN-26 gives a laydown a schema and an
//! identity so this panel has something to choose between; the note itself is explicit
//! about the boundary of what it engineers, and this panel does not cross it:
//!
//! * **No adoption.** §6 rule 4: moving a sensor is a physical act with an authority
//!   chain this system does not model, and a button that appeared to do it would be
//!   the most dangerous control on the display. This panel compares; a person acts.
//! * **No rehearsal.** §7: that is GAP-045, deliberately out of DN-26's scope. The
//!   rehearsal section [`WF-16-planning.puml`](../../../docs/ux/wireframes/WF-16-planning.puml)
//!   shows is drawn as [`crate::panels::unavailable::Unavailable`] rather than omitted,
//!   so an operator sees that a rehearsal belongs here and is not yet built, rather than
//!   concluding this panel is the whole of PN-16.
//! * **Ranking is advisory and must say so.** §5: coverage is one of several things a
//!   laydown is chosen on, and a plausible-looking order would read as a
//!   recommendation this system is not making. The difference against the current
//!   laydown is shown as a number, not as a rank.
//! * **A laydown that could not be evaluated is not one that scored zero.** §5's other
//!   rule: [`LaydownCoverage::NotComputed`] is a distinct state from a covered length of
//!   zero, and this panel never collapses the two.

use crate::panels::unavailable::{draw_unavailable, Section, Unavailable};
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::LaydownId;

/// One laydown's coverage answer, or the reason it has none (DN-26 §5).
#[derive(Debug, Clone, PartialEq)]
pub enum LaydownCoverage {
    /// Computed under the terrain model every laydown in this comparison shares.
    Computed {
        gap_segments: usize,
        uncovered_m: f64,
        /// This laydown's `uncovered_m` minus the current laydown's. Negative is less
        /// gap than today; positive is more. `None` for the current laydown itself,
        /// which is not a difference from itself.
        delta_uncovered_m: Option<f64>,
    },
    /// Could not be evaluated -- no terrain loaded for the model in force, no approach
    /// declared, no local frame -- and the reason travels rather than a zero standing
    /// in for it.
    NotComputed { reason: String },
}

/// One row of the options table.
#[derive(Debug, Clone, PartialEq)]
pub struct LaydownRow {
    pub id: LaydownId,
    pub intent: String,
    /// True for exactly one row: the placement actually in force.
    pub current: bool,
    pub coverage: LaydownCoverage,
}

/// Everything PN-16 draws.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanningView<'a> {
    /// One row per declared laydown, or the honest account of why there are none.
    pub laydowns: Section<'a, LaydownRow>,
    /// Which line-of-sight model produced every `Computed` row in this comparison
    /// (DN-26 §5's first rule: every laydown compared under the same one).
    pub terrain_model: &'a str,
    /// The rehearsal section: always unbuilt today (GAP-045).
    pub rehearsal: Unavailable<'a>,
    /// Which option, if any, PN-11 is drawing a before-and-after preview of
    /// (GAP-087's own remaining item). Clicking the selected option again clears it,
    /// the same toggle-by-reclick rule PN-03's track selection uses.
    pub selected: Option<&'a LaydownId>,
}

/// Render PN-16. Returns the option an operator clicked this frame, for the caller to
/// toggle into (or out of) `AppState`'s selection -- the same shape PN-03's track table
/// returns a click as.
pub fn render_planning(ui: &mut Ui, view: &PlanningView<'_>) -> Option<LaydownId> {
    ui.heading("Planning: laydown options");

    ui.label(
        RichText::new(format!("Coverage compared under: {}", view.terrain_model))
            .small()
            .color(theme::MUTED_TEXT_COLOR),
    );
    ui.separator();

    let mut clicked = None;
    if view.laydowns.draw_header(ui, "Laydown options") {
        let rows = view.laydowns.items().unwrap_or_default();
        egui::Grid::new("planning_laydown_options")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Option");
                ui.strong("Intent");
                ui.strong("Coverage");
                ui.strong("Difference from current");
                ui.end_row();

                for row in rows {
                    let label = if row.current {
                        format!("{} (current)", row.id.0)
                    } else {
                        row.id.0.clone()
                    };
                    if ui
                        .selectable_label(view.selected == Some(&row.id), label)
                        .clicked()
                    {
                        clicked = Some(row.id.clone());
                    }
                    ui.label(&row.intent);
                    match &row.coverage {
                        LaydownCoverage::Computed {
                            gap_segments,
                            uncovered_m,
                            ..
                        } => {
                            ui.label(format!(
                                "{gap_segments} gap segment(s), {uncovered_m:.0} m uncovered"
                            ));
                        }
                        LaydownCoverage::NotComputed { reason } => {
                            ui.label(
                                RichText::new(format!("Not computed: {reason}"))
                                    .color(theme::WARNING_COLOR),
                            );
                        }
                    }
                    match &row.coverage {
                        LaydownCoverage::Computed {
                            delta_uncovered_m: Some(delta),
                            ..
                        } => {
                            ui.label(difference_label(*delta));
                        }
                        LaydownCoverage::Computed {
                            delta_uncovered_m: None,
                            ..
                        } => {
                            ui.label(RichText::new("-- (current)").color(theme::MUTED_TEXT_COLOR));
                        }
                        LaydownCoverage::NotComputed { .. } => {
                            ui.label(RichText::new("--").color(theme::MUTED_TEXT_COLOR));
                        }
                    }
                    ui.end_row();
                }
            });

        ui.separator();
        ui.label(
            RichText::new(
                "The difference is a measurement, not a recommendation: coverage is one \
                 of several things a laydown is chosen on, and this panel does not rank \
                 the options.",
            )
            .small()
            .color(theme::MUTED_TEXT_COLOR),
        );
    }

    ui.separator();
    ui.strong("Rehearsal");
    draw_unavailable(ui, view.rehearsal);

    ui.separator();
    ui.label(
        RichText::new(
            "Adopting a laydown is not done from here: moving a sensor is a physical act \
             with its own authority chain.",
        )
        .small()
        .color(theme::MUTED_TEXT_COLOR),
    );
    clicked
}

/// The signed difference, in metres, worded so a reader does not have to interpret the
/// sign: "less" is unambiguous where "-120 m" is not.
fn difference_label(delta_m: f64) -> String {
    if delta_m < 0.0 {
        format!("{:.0} m less gap than today", -delta_m)
    } else if delta_m > 0.0 {
        format!("{delta_m:.0} m more gap than today")
    } else {
        "same as today".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rehearsal() -> Unavailable<'static> {
        Unavailable {
            owner: "gungnir-tracking-service",
            gap: "GAP-045",
        }
    }

    #[test]
    fn a_computed_row_and_a_not_computed_row_are_different_states() {
        let computed = LaydownCoverage::Computed {
            gap_segments: 0,
            uncovered_m: 0.0,
            delta_uncovered_m: None,
        };
        let not_computed = LaydownCoverage::NotComputed {
            reason: "no terrain loaded".into(),
        };
        assert_ne!(
            computed, not_computed,
            "a zero is not the same claim as no answer"
        );
    }

    #[test]
    fn the_difference_label_says_less_more_or_same_rather_than_a_bare_signed_number() {
        assert_eq!(difference_label(-500.0), "500 m less gap than today");
        assert_eq!(difference_label(500.0), "500 m more gap than today");
        assert_eq!(difference_label(0.0), "same as today");
    }

    #[test]
    fn the_rehearsal_section_names_a_crate_and_a_gap_rather_than_being_silently_omitted() {
        let r = rehearsal();
        assert!(r.owner.starts_with("gungnir-"));
        assert_eq!(r.gap, "GAP-045");
    }

    #[test]
    fn no_laydowns_declared_is_drawn_as_a_stated_reason_not_an_empty_table() {
        let view = PlanningView {
            laydowns: Section::Empty {
                reason: "This deployment has declared no laydown alternatives.",
            },
            terrain_model: "flat terrain",
            rehearsal: rehearsal(),
            selected: None,
        };
        // `draw_header` is exercised through the widget test harness elsewhere
        // (`gungnir-ui --features harness`); this pins the state the view carries.
        assert!(view.laydowns.items().is_none());
    }
}
