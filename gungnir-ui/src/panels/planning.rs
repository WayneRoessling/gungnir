// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-16, the planning panel: laydown options, compared (GAP-087,
//! `docs/design/DN-26-laydown-options.md`), and rehearsed (GAP-045).
//!
//! **What this draws, and what it does not.** DN-26 gives a laydown a schema and an
//! identity so this panel has something to choose between; the note itself is explicit
//! about the boundary of what it engineers, and this panel does not cross it:
//!
//! * **No adoption.** §6 rule 4: moving a sensor is a physical act with an authority
//!   chain this system does not model, and a button that appeared to do it would be
//!   the most dangerous control on the display. This panel compares; a person acts.
//! * **Rehearsal replays a fixture; it does not generate one.** GAP-045's harness
//!   reads one of the ten committed test-track scenarios and reports what a real tick
//!   loop measured -- tracks formed, decisions raised and expired. It does **not**
//!   report a first-engagement range: DN-02 §7 says that number is the aggregate of
//!   predictions over a rehearsal, and no note has yet specified how they aggregate.
//!   Drawing one anyway would be deciding that rule here rather than where it needs
//!   review, which is the same discipline this panel already applies to ranking.
//! * **Ranking is advisory and must say so.** §5: coverage is one of several things a
//!   laydown is chosen on, and a plausible-looking order would read as a
//!   recommendation this system is not making. The difference against the current
//!   laydown is shown as a number, not as a rank.
//! * **A laydown that could not be evaluated is not one that scored zero.** §5's other
//!   rule: [`LaydownCoverage::NotComputed`] is a distinct state from a covered length of
//!   zero, and this panel never collapses the two.

use crate::panels::unavailable::Section;
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::{LaydownId, TestTrackNumber};

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

/// What a rehearsal actually measured, for PN-16 to draw. A view type of this panel's
/// own rather than `gungnir-app`'s own rehearsal bookkeeping -- `gungnir-ui` depends on
/// `gungnir-model` alone, the same reason `gungnir-viewport3d`'s `LaydownPreview` is its
/// own type rather than a borrow of the app's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RehearsalSummary {
    pub scenario: TestTrackNumber,
    pub tracks_formed: usize,
    pub decisions_raised: usize,
    pub decisions_expired: usize,
}

/// PN-16's rehearsal section (GAP-045).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RehearsalSection {
    /// No laydown is selected this frame; there is nothing to rehearse yet.
    NothingSelected,
    /// A laydown is selected and no rehearsal has been run for it.
    NotYetRun,
    /// The last rehearsal recorded for the selected laydown.
    Ran(RehearsalSummary),
}

/// Everything PN-16 draws.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanningView<'a> {
    /// One row per declared laydown, or the honest account of why there are none.
    pub laydowns: Section<'a, LaydownRow>,
    /// Which line-of-sight model produced every `Computed` row in this comparison
    /// (DN-26 §5's first rule: every laydown compared under the same one).
    pub terrain_model: &'a str,
    /// The rehearsal section, for the selected laydown (GAP-045).
    pub rehearsal: RehearsalSection,
    /// Which scenario the rehearsal picker currently has chosen, so a "run" click
    /// carries the same one the operator is looking at.
    pub rehearsal_scenario: TestTrackNumber,
    /// Which option, if any, PN-11 is drawing a before-and-after preview of
    /// (GAP-087's own remaining item). Clicking the selected option again clears it,
    /// the same toggle-by-reclick rule PN-03's track selection uses.
    pub selected: Option<&'a LaydownId>,
}

/// What an operator did on this frame, for the caller to apply (the same shape PN-03's
/// track table returns a click as).
#[derive(Debug, Clone, PartialEq)]
pub enum PlanningAction {
    /// Preview this option on PN-11, or clear the preview if it is already selected.
    SelectLaydown(LaydownId),
    /// Change which scenario the rehearsal picker offers to run next. Not itself a
    /// rehearsal: nothing runs until [`PlanningAction::RunRehearsal`].
    PickScenario(TestTrackNumber),
    /// Rehearse the selected laydown against the picked scenario.
    RunRehearsal(TestTrackNumber),
}

/// Render PN-16.
pub fn render_planning(ui: &mut Ui, view: &PlanningView<'_>) -> Option<PlanningAction> {
    ui.heading("Planning: laydown options");

    ui.label(
        RichText::new(format!("Coverage compared under: {}", view.terrain_model))
            .small()
            .color(theme::MUTED_TEXT_COLOR),
    );
    ui.separator();

    let mut action = None;
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
                        action = Some(PlanningAction::SelectLaydown(row.id.clone()));
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
    if let Some(a) = draw_rehearsal(ui, view) {
        action = Some(a);
    }

    ui.separator();
    ui.label(
        RichText::new(
            "Adopting a laydown is not done from here: moving a sensor is a physical act \
             with its own authority chain.",
        )
        .small()
        .color(theme::MUTED_TEXT_COLOR),
    );
    action
}

fn draw_rehearsal(ui: &mut Ui, view: &PlanningView<'_>) -> Option<PlanningAction> {
    let Some(selected) = view.selected else {
        ui.label(
            RichText::new("Select a laydown option above to rehearse it.")
                .small()
                .color(theme::MUTED_TEXT_COLOR),
        );
        return None;
    };

    let mut action = None;
    ui.horizontal(|ui| {
        ui.label("Scenario:");
        egui::ComboBox::from_id_salt("planning_rehearsal_scenario")
            .selected_text(view.rehearsal_scenario.label())
            .show_ui(ui, |ui| {
                for scenario in TestTrackNumber::ALL {
                    if ui
                        .selectable_label(scenario == view.rehearsal_scenario, scenario.label())
                        .clicked()
                    {
                        action = Some(PlanningAction::PickScenario(scenario));
                    }
                }
            });
        if ui.button("Run rehearsal").clicked() {
            action = Some(PlanningAction::RunRehearsal(view.rehearsal_scenario));
        }
    });
    ui.label(
        RichText::new(format!("under {}", selected.0))
            .small()
            .color(theme::MUTED_TEXT_COLOR),
    );

    match view.rehearsal {
        RehearsalSection::NothingSelected => {
            // Reached only if `selected` became `Some` and `view.rehearsal` was built
            // from a different state; drawn rather than treated as unreachable, so a
            // caller's bug is visible instead of silently mismatched.
            ui.label(
                RichText::new("No laydown selected.")
                    .small()
                    .color(theme::MUTED_TEXT_COLOR),
            );
        }
        RehearsalSection::NotYetRun => {
            ui.label(
                RichText::new("Not yet rehearsed.")
                    .small()
                    .color(theme::MUTED_TEXT_COLOR),
            );
        }
        RehearsalSection::Ran(summary) => {
            ui.label(format!(
                "last: {} -- {} track(s) formed, {} decision(s) raised ({} expired)",
                summary.scenario.label(),
                summary.tracks_formed,
                summary.decisions_raised,
                summary.decisions_expired
            ));
        }
    }
    action
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

    fn view(selected: Option<&LaydownId>, rehearsal: RehearsalSection) -> PlanningView<'_> {
        PlanningView {
            laydowns: Section::Empty {
                reason: "This deployment has declared no laydown alternatives.",
            },
            terrain_model: "flat terrain",
            rehearsal,
            rehearsal_scenario: TestTrackNumber(1),
            selected,
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
    fn no_laydowns_declared_is_drawn_as_a_stated_reason_not_an_empty_table() {
        let v = view(None, RehearsalSection::NothingSelected);
        // `draw_header` is exercised through the widget test harness elsewhere
        // (`gungnir-ui --features harness`); this pins the state the view carries.
        assert!(v.laydowns.items().is_none());
    }

    #[test]
    fn a_test_track_scenario_labels_itself_with_two_digits() {
        assert_eq!(TestTrackNumber(1).label(), "TT-01");
        assert_eq!(TestTrackNumber(10).label(), "TT-10");
    }

    #[test]
    fn all_ten_scenarios_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for s in TestTrackNumber::ALL {
            assert!(seen.insert(s.0), "TT-{:02} listed twice", s.0);
        }
        assert_eq!(seen.len(), 10);
    }
}
