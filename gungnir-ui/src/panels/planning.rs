// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-16, the planning panel: laydown options, compared (GAP-087,
//! `docs/design/DN-26-laydown-options.md`), and rehearsed (GAP-045, GAP-105).
//!
//! **What this draws, and what it does not.** DN-26 gives a laydown a schema and an
//! identity so this panel has something to choose between; the note itself is explicit
//! about the boundary of what it engineers, and this panel does not cross it:
//!
//! * **No adoption.** §6 rule 4: moving a sensor is a physical act with an authority
//!   chain this system does not model, and a button that appeared to do it would be
//!   the most dangerous control on the display. This panel compares; a person acts.
//! * **A rehearsal is re-observed from a recording, and says so every time**
//!   (`docs/design/DN-32-re-observation-for-a-laydown.md` §6). The desktop re-observes
//!   one of the ten committed test-track recordings with the selected laydown's own
//!   sensors where it places them, each with the detection model the deployment names
//!   for it, and runs what they would have detected through a throwaway pipeline. Every
//!   result is labelled as re-observed, with the recording and each sensor's model named,
//!   because a count of detections is exactly the kind of number that could be mistaken
//!   for one a sensor produced. It reports what the run measured -- detections per
//!   sensor, tracks formed, decisions raised and expired -- and **not** a
//!   first-engagement range: DN-02 §7 says that is the aggregate of predictions over a
//!   rehearsal, and GAP-020 carries its rule.
//! * **The table compares the run, not only the arithmetic.** A laydown's coverage
//!   column is computed from its declared placements; its rehearsal columns are read from
//!   the last run of it, and the difference from the current laydown is drawn only when
//!   both were rehearsed against the same recording, naming the sensors the difference
//!   came from.
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

/// A laydown's rehearsal against the current laydown's, for the table (GAP-105).
#[derive(Debug, Clone, PartialEq)]
pub enum VersusCurrent {
    /// This row is the current laydown.
    IsCurrent,
    /// Both were rehearsed against the same recording: this laydown's detections of a
    /// recorded target minus the current laydown's, and the sensors whose own counts
    /// differ (DN-32 §10's round-1 row: the difference names where it came from).
    Difference { detections: i64, sensors: Vec<u32> },
    /// The current laydown has not been rehearsed, so there is nothing to compare with.
    CurrentNotRehearsed,
    /// The current laydown's last rehearsal was of another recording, and two
    /// recordings' counts are not a comparison.
    DifferentRecording(TestTrackNumber),
}

/// What the table shows of a laydown's last rehearsal.
#[derive(Debug, Clone, PartialEq)]
pub enum RowRehearsal {
    NotRehearsed,
    Rehearsed {
        scenario: TestTrackNumber,
        /// Detections of a recorded target, every sensor summed.
        detections: usize,
        tracks_formed: usize,
        versus_current: VersusCurrent,
    },
}

/// One row of the options table.
#[derive(Debug, Clone, PartialEq)]
pub struct LaydownRow {
    pub id: LaydownId,
    pub intent: String,
    /// True for exactly one row: the placement actually in force.
    pub current: bool,
    pub coverage: LaydownCoverage,
    /// Read from the last rehearsal of this laydown, not from its declared placements.
    pub rehearsal: RowRehearsal,
}

/// One sensor's line in a rehearsal's result: which model re-observed with, what it
/// produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RehearsedSensor {
    pub sensor: u32,
    /// The detection model it was re-observed with, or `None` when the laydown places it
    /// in a mode that does not observe and it was not run.
    pub detection_model: Option<String>,
    /// Detections of a recorded target.
    pub detections: usize,
    pub false_alarms: usize,
    /// This sensor's detections minus its detections under the current laydown's
    /// rehearsal of the same recording; `None` when there is no such rehearsal, or this
    /// is the current laydown.
    pub delta_from_current: Option<i64>,
}

/// What a rehearsal actually measured, for PN-16 to draw. A view type of this panel's
/// own rather than `gungnir-app`'s own rehearsal bookkeeping -- `gungnir-ui` depends on
/// `gungnir-model` alone, the same reason `gungnir-viewport3d`'s `LaydownPreview` is its
/// own type rather than a borrow of the app's.
#[derive(Debug, Clone, PartialEq)]
pub struct RehearsalSummary {
    /// The recording re-observed.
    pub scenario: TestTrackNumber,
    /// The seed every draw of the run derived from.
    pub seed: u64,
    pub tracks_formed: usize,
    pub decisions_raised: usize,
    pub decisions_expired: usize,
    /// One line per sensor the laydown places, in its order.
    pub sensors: Vec<RehearsedSensor>,
    /// The recording's losses and electronic-attack windows, which name its own sensors
    /// and so were not applied to this laydown's (D-73).
    pub recording_events_not_applied: usize,
}

/// PN-16's rehearsal section (GAP-045).
#[derive(Debug, Clone, PartialEq)]
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

/// The label every rehearsal result carries (DN-32 §6): what it is, and what it is of.
#[must_use]
pub fn reobserved_label(scenario: TestTrackNumber, seed: u64) -> String {
    format!(
        "Re-observed from a recording: {} (seed {seed}). Simulated detections, not sensor data.",
        scenario.label()
    )
}

/// Render PN-16.
pub fn render_planning(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &PlanningView<'_>,
) -> Option<PlanningAction> {
    ui.heading("Planning: laydown options");

    ui.label(
        RichText::new(format!("Coverage compared under: {}", view.terrain_model))
            .small()
            .color(palette.muted_text_color()),
    );
    ui.separator();

    let mut action = None;
    if view.laydowns.draw_header(ui, palette, "Laydown options") {
        let rows = view.laydowns.items().unwrap_or_default();
        egui::Grid::new("planning_laydown_options")
            .num_columns(6)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Option");
                ui.strong("Intent");
                ui.strong("Coverage");
                ui.strong("Difference from current");
                ui.strong("Rehearsed (re-observed)");
                ui.strong("Rehearsal against current");
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
                                    .color(palette.warning_color),
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
                            ui.label(
                                RichText::new("-- (current)").color(palette.muted_text_color()),
                            );
                        }
                        LaydownCoverage::NotComputed { .. } => {
                            ui.label(RichText::new("--").color(palette.muted_text_color()));
                        }
                    }
                    draw_row_rehearsal(ui, palette, &row.rehearsal);
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
            .color(palette.muted_text_color()),
        );
    }

    ui.separator();
    ui.strong("Rehearsal");
    if let Some(a) = draw_rehearsal(ui, palette, view) {
        action = Some(a);
    }

    ui.separator();
    ui.label(
        RichText::new(
            "Adopting a laydown is not done from here: moving a sensor is a physical act \
             with its own authority chain.",
        )
        .small()
        .color(palette.muted_text_color()),
    );
    action
}

fn draw_row_rehearsal(ui: &mut Ui, palette: &theme::Palette, rehearsal: &RowRehearsal) {
    match rehearsal {
        RowRehearsal::NotRehearsed => {
            ui.label(RichText::new("not rehearsed").color(palette.muted_text_color()));
            ui.label(RichText::new("--").color(palette.muted_text_color()));
        }
        RowRehearsal::Rehearsed {
            scenario,
            detections,
            tracks_formed,
            versus_current,
        } => {
            ui.label(format!(
                "{}: {detections} detection(s), {tracks_formed} track(s)",
                scenario.label()
            ));
            match versus_current {
                VersusCurrent::IsCurrent => {
                    ui.label(RichText::new("-- (current)").color(palette.muted_text_color()));
                }
                VersusCurrent::Difference {
                    detections,
                    sensors,
                } => {
                    ui.label(detection_difference_label(*detections, sensors));
                }
                VersusCurrent::CurrentNotRehearsed => {
                    ui.label(
                        RichText::new("rehearse the current laydown to compare")
                            .color(palette.muted_text_color()),
                    );
                }
                VersusCurrent::DifferentRecording(other) => {
                    ui.label(
                        RichText::new(format!(
                            "not comparable: current last rehearsed against {}",
                            other.label()
                        ))
                        .color(palette.muted_text_color()),
                    );
                }
            }
        }
    }
}

fn draw_rehearsal(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &PlanningView<'_>,
) -> Option<PlanningAction> {
    let Some(selected) = view.selected else {
        ui.label(
            RichText::new("Select a laydown option above to rehearse it.")
                .small()
                .color(palette.muted_text_color()),
        );
        return None;
    };

    let mut action = None;
    ui.horizontal(|ui| {
        ui.label("Recording:");
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
        RichText::new(format!(
            "under {}: its own sensors where it places them, re-observing the recording",
            selected.0
        ))
        .small()
        .color(palette.muted_text_color()),
    );

    match &view.rehearsal {
        RehearsalSection::NothingSelected => {
            // Reached only if `selected` became `Some` and `view.rehearsal` was built
            // from a different state; drawn rather than treated as unreachable, so a
            // caller's bug is visible instead of silently mismatched.
            ui.label(
                RichText::new("No laydown selected.")
                    .small()
                    .color(palette.muted_text_color()),
            );
        }
        RehearsalSection::NotYetRun => {
            ui.label(
                RichText::new("Not yet rehearsed.")
                    .small()
                    .color(palette.muted_text_color()),
            );
        }
        RehearsalSection::Ran(summary) => draw_summary(ui, palette, summary),
    }
    action
}

fn draw_summary(ui: &mut Ui, palette: &theme::Palette, summary: &RehearsalSummary) {
    // The label first and in the warning colour: a count of detections is exactly the
    // number that could be taken for one a sensor produced (DN-32 §6).
    ui.label(
        RichText::new(reobserved_label(summary.scenario, summary.seed))
            .color(palette.warning_color),
    );
    ui.label(format!(
        "last: {} -- {} track(s) formed, {} decision(s) raised ({} expired)",
        summary.scenario.label(),
        summary.tracks_formed,
        summary.decisions_raised,
        summary.decisions_expired
    ));
    egui::Grid::new("planning_rehearsal_sensors")
        .num_columns(5)
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Sensor");
            ui.strong("Detection model");
            ui.strong("Detections");
            ui.strong("False alarms");
            ui.strong("Against current");
            ui.end_row();
            for s in &summary.sensors {
                ui.label(format!("S{}", s.sensor));
                if let Some(model) = &s.detection_model {
                    ui.label(model);
                    ui.label(s.detections.to_string());
                    ui.label(s.false_alarms.to_string());
                } else {
                    ui.label(
                        RichText::new("not observing in this laydown")
                            .color(palette.muted_text_color()),
                    );
                    ui.label("--");
                    ui.label("--");
                }
                match s.delta_from_current {
                    Some(0) => {
                        ui.label("same");
                    }
                    Some(d) => {
                        ui.label(format!("{d:+}"));
                    }
                    None => {
                        ui.label(RichText::new("--").color(palette.muted_text_color()));
                    }
                }
                ui.end_row();
            }
        });
    // A rehearsal that re-observed nothing says why, rather than leaving a column of
    // zeros to read as a fault in the rehearsal or as a laydown that sees nothing at all.
    if summary
        .sensors
        .iter()
        .filter(|s| s.detection_model.is_some())
        .all(|s| s.detections == 0)
    {
        ui.label(
            RichText::new(format!(
                "None of this laydown's sensors detected a target of {}: where it places \
                 them, the recording's targets never come within their range bands.",
                summary.scenario.label()
            ))
            .small()
            .color(palette.warning_color),
        );
    }
    if summary.recording_events_not_applied > 0 {
        ui.label(
            RichText::new(format!(
                "{} loss or electronic-attack event(s) in the recording name its own \
                 sensors and were not applied to this laydown's.",
                summary.recording_events_not_applied
            ))
            .small()
            .color(palette.muted_text_color()),
        );
    }
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

/// The difference in detections from the current laydown, worded, with the sensors it
/// came from named.
fn detection_difference_label(delta: i64, sensors: &[u32]) -> String {
    let from = if sensors.is_empty() {
        String::new()
    } else {
        format!(
            ", from {}",
            sensors
                .iter()
                .map(|s| format!("S{s}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    match delta.cmp(&0) {
        std::cmp::Ordering::Greater => format!("{delta} more detection(s){from}"),
        std::cmp::Ordering::Less => format!("{} fewer detection(s){from}", -delta),
        std::cmp::Ordering::Equal if sensors.is_empty() => "same detections".to_string(),
        std::cmp::Ordering::Equal => format!("same total, redistributed{from}"),
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
    fn a_detection_difference_names_the_sensors_it_came_from() {
        assert_eq!(
            detection_difference_label(48, &[2]),
            "48 more detection(s), from S2"
        );
        assert_eq!(
            detection_difference_label(-3, &[1, 2]),
            "3 fewer detection(s), from S1, S2"
        );
        assert_eq!(detection_difference_label(0, &[]), "same detections");
        assert_eq!(
            detection_difference_label(0, &[2]),
            "same total, redistributed, from S2"
        );
    }

    #[test]
    fn every_rehearsal_result_says_it_was_re_observed_and_from_what() {
        let label = reobserved_label(TestTrackNumber(1), 1701);
        assert!(label.contains("Re-observed from a recording"), "{label}");
        assert!(label.contains("TT-01"), "{label}");
        assert!(label.contains("not sensor data"), "{label}");
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
