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
//!   one of the eleven committed test-track recordings with the selected laydown's own
//!   sensors where it places them, each with the detection model the deployment names
//!   for it, and runs what they would have detected through a throwaway pipeline. Every
//!   result is labelled as re-observed, with the recording and each sensor's model named,
//!   because a count of detections is exactly the kind of number that could be mistaken
//!   for one a sensor produced. It reports what the run measured -- detections per
//!   sensor, tracks formed, decisions raised and expired -- and each declared
//!   approach's first-engagement range.
//! * **First-engagement range is the worst case over the run, and says so** (GAP-020,
//!   `docs/design/DN-02-prediction-and-approach.md` §9, D-45): the least ground range
//!   from an approach's inner end at which the planner predicted it would first engage a
//!   track coming down it, with the number of predictions it is over. An approach with
//!   no prediction on it is [`ApproachEngagement::NotComputable`] with the reason, never
//!   a zero, and every figure carries the rehearsal it came from: the recording, its
//!   seed and when it was run.
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
use gungnir_model::{LaydownId, MissionTime, ResourceId, TestTrackNumber, TrackId};

/// One laydown's coverage answer, or the reason it has none (DN-26 §5).
#[derive(Debug, Clone, PartialEq)]
pub enum LaydownCoverage {
    /// Computed under the terrain model every laydown in this comparison shares.
    Computed {
        gap_segments: usize,
        uncovered_m: f64,
        /// Of `gap_segments` and `uncovered_m`, what a standing acceptance names (GAP-106,
        /// DN-33 §7): counted in both, never taken out of them. Only the laydown in force
        /// can have any, because an acceptance holds for the laydown it was made under.
        accepted_segments: usize,
        accepted_uncovered_m: f64,
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
    /// Rehearsed only in an earlier session (GAP-107, D-120): the record says it was run,
    /// against which recording and in which session; the figures were that session's.
    RehearsedEarlier {
        scenario: TestTrackNumber,
        session: Option<u64>,
    },
    Rehearsed {
        scenario: TestTrackNumber,
        /// Detections of a recorded target, every sensor summed.
        detections: usize,
        tracks_formed: usize,
        versus_current: VersusCurrent,
        /// The desktop's mission time when the rehearsal was run.
        ran_at: MissionTime,
    },
}

/// One approach's first-engagement range from one laydown's last rehearsal (GAP-020,
/// DN-02 §9, D-45).
#[derive(Debug, Clone, PartialEq)]
pub enum ApproachEngagement {
    /// D-45: the worst case, the minimum over the run, and how many predictions it is
    /// over.
    WorstCase {
        /// Ground range from the approach's inner end to the worst predicted first
        /// engagement, metres.
        range_m: f64,
        /// Recorded targets first engaged on this approach in the run: the count the
        /// minimum is over. Never zero.
        predictions: usize,
        /// The recorded target the worst case was predicted for, its track, the
        /// effector, and when in the run.
        target: String,
        track: TrackId,
        resource: ResourceId,
        proposed_at: MissionTime,
        /// This worst case minus the current laydown's on the same approach, when both
        /// were rehearsed against the same recording and both have one. Positive is
        /// first engaged farther out than the current laydown.
        versus_current_m: Option<f64>,
    },
    /// No range, and why: never a zero standing in for one.
    NotComputable { reason: String },
}

/// A table row's first-engagement cells (GAP-020).
#[derive(Debug, Clone, PartialEq)]
pub enum RowFirstEngagement {
    NotRehearsed,
    /// Not computed on any approach -- no local frame to place the approaches in, or none
    /// declared -- and why.
    NotComputed {
        reason: String,
    },
    /// One per declared approach, in [`PlanningView::approaches`] order.
    PerApproach(Vec<ApproachEngagement>),
}

/// One approach's line in the rehearsal section.
#[derive(Debug, Clone, PartialEq)]
pub struct ApproachLine {
    pub approach: String,
    /// The corridor either side of the axis a track counted as on it, metres; `None`
    /// when the approach declares none (D-108).
    pub corridor_half_width_m: Option<f64>,
    pub engagement: ApproachEngagement,
}

/// The rehearsal section's first-engagement account (GAP-020).
#[derive(Debug, Clone, PartialEq)]
pub enum RehearsalFirstEngagement {
    NotComputed {
        reason: String,
    },
    PerApproach {
        lines: Vec<ApproachLine>,
        /// Paired targets in no declared corridor, which no approach's figure includes.
        on_no_corridor: usize,
        /// Tracks the planner paired that were none of the recording's targets --
        /// clutter -- which no figure includes (D-107).
        clutter_pairings: usize,
        /// Plans the run proposed, and how many of them the deployment's own policy did
        /// not offer for decision, with each reason the chain recorded (GAP-182, D-113).
        plans_proposed: usize,
        plans_not_offered: usize,
        not_offered_because: Vec<(String, usize)>,
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
    /// Each approach's first-engagement range from that same rehearsal.
    pub first_engagement: RowFirstEngagement,
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
    /// The desktop's mission time when the rehearsal was run.
    pub ran_at: MissionTime,
    /// Each declared approach's first-engagement range over the run (GAP-020).
    pub first_engagement: RehearsalFirstEngagement,
}

/// PN-16's rehearsal section (GAP-045).
#[derive(Debug, Clone, PartialEq)]
pub enum RehearsalSection {
    /// No laydown is selected this frame; there is nothing to rehearse yet.
    NothingSelected,
    /// A laydown is selected and no rehearsal has been run for it.
    NotYetRun,
    /// Rehearsed in an earlier session only (D-120): on the record, figures not held.
    RanInEarlierSession {
        scenario: TestTrackNumber,
        session: Option<u64>,
    },
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
    /// The declared approaches' names, in order: one first-engagement column each.
    pub approaches: &'a [String],
    /// The rehearsal section, for the selected laydown (GAP-045).
    pub rehearsal: RehearsalSection,
    /// Which scenario the rehearsal picker currently has chosen, so a "run" click
    /// carries the same one the operator is looking at.
    pub rehearsal_scenario: TestTrackNumber,
    /// Which option, if any, PN-11 is drawing a before-and-after preview of
    /// (GAP-087's own remaining item). Clicking the selected option again clears it,
    /// the same toggle-by-reclick rule PN-03's track selection uses.
    pub selected: Option<&'a LaydownId>,
    /// Where the laydown in force stands (GAP-107, `docs/design/DN-26-laydown-options.md`
    /// §11 item 5), and whether a decision on a plan must acknowledge it; `None` for a
    /// deployment that declares no laydown.
    pub in_force: Option<InForceLine<'a>>,
}

/// The rehearsal standing of the laydown in force, as PN-16 says it (GAP-107).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InForceLine<'a> {
    pub sentence: &'a str,
    /// True when a decision acting on a plan asks for it to be acknowledged: never
    /// rehearsed, rehearsed under something else, or not rehearsable (D-119).
    pub asks: bool,
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

    // GAP-107, DN-26 §11: first, because it is what a decision on a plan is told.
    if let Some(line) = view.in_force {
        draw_in_force(ui, palette, line);
    }

    ui.label(
        RichText::new(format!("Coverage compared under: {}", view.terrain_model))
            .small()
            .color(palette.muted_text_color()),
    );
    ui.separator();

    let mut action = None;
    if view.laydowns.draw_header(ui, palette, "Laydown options") {
        let rows = view.laydowns.items().unwrap_or_default();
        // One first-engagement column per declared approach, or one that says why there
        // is none: a missing column would read as a comparison nobody thought to make.
        let engagement_columns = view.approaches.len().max(1);
        egui::Grid::new("planning_laydown_options")
            .num_columns(6 + engagement_columns)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Option");
                ui.strong("Intent");
                ui.strong("Coverage");
                ui.strong("Difference from current");
                if view.approaches.is_empty() {
                    ui.strong("First engagement (worst case)");
                } else {
                    for approach in view.approaches {
                        ui.strong(format!("{approach}: first engagement (worst case)"));
                    }
                }
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
                    draw_row_coverage(ui, palette, &row.coverage);
                    draw_row_first_engagement(
                        ui,
                        palette,
                        &row.first_engagement,
                        engagement_columns,
                    );
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
        ui.label(
            RichText::new(FIRST_ENGAGEMENT_CAPTION)
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

/// A row's two coverage cells: the answer or why there is none, and the difference from
/// the current laydown.
fn draw_row_coverage(ui: &mut Ui, palette: &theme::Palette, coverage: &LaydownCoverage) {
    match coverage {
        LaydownCoverage::Computed {
            gap_segments,
            uncovered_m,
            accepted_segments,
            accepted_uncovered_m,
            ..
        } => {
            // GAP-106: the accepted part is said inside the total, never taken out of it.
            if *accepted_segments > 0 {
                ui.label(format!(
                    "{gap_segments} gap segment(s) ({accepted_segments} accepted), \
                     {uncovered_m:.0} m uncovered ({accepted_uncovered_m:.0} m of it accepted)"
                ));
            } else {
                ui.label(format!(
                    "{gap_segments} gap segment(s), {uncovered_m:.0} m uncovered"
                ));
            }
        }
        LaydownCoverage::NotComputed { reason } => {
            ui.label(RichText::new(format!("Not computed: {reason}")).color(palette.warning_color));
        }
    }
    match coverage {
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
            ui.label(RichText::new("-- (current)").color(palette.muted_text_color()));
        }
        LaydownCoverage::NotComputed { .. } => {
            ui.label(RichText::new("--").color(palette.muted_text_color()));
        }
    }
}

fn draw_row_rehearsal(ui: &mut Ui, palette: &theme::Palette, rehearsal: &RowRehearsal) {
    match rehearsal {
        RowRehearsal::NotRehearsed => {
            ui.label(RichText::new("not rehearsed").color(palette.muted_text_color()));
            ui.label(RichText::new("--").color(palette.muted_text_color()));
        }
        RowRehearsal::RehearsedEarlier { scenario, session } => {
            ui.label(
                RichText::new(earlier_label(*scenario, *session)).color(palette.muted_text_color()),
            );
            ui.label(RichText::new("--").color(palette.muted_text_color()));
        }
        RowRehearsal::Rehearsed {
            scenario,
            detections,
            tracks_formed,
            versus_current,
            ran_at,
        } => {
            ui.label(format!(
                "{}: {detections} detection(s), {tracks_formed} track(s); run at T+{:.0} s",
                scenario.label(),
                ran_at.0
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

/// What the first-engagement column says, under the table: the rule (D-45) and where the
/// figure comes from, once, rather than in every cell.
pub const FIRST_ENGAGEMENT_CAPTION: &str =
    "First engagement is the worst case over each laydown's last rehearsal: the least \
     ground range from an approach's inner end at which a plan this deployment's own \
     policy offered for decision would first engage a recorded target coming down it, \
     over the n predictions shown. It is read from a re-observed recording, not from \
     sensor data.";

/// A worst case in words: the range, that it is the worst case, and its count (D-45).
#[must_use]
pub fn worst_case_label(range_m: f64, predictions: usize) -> String {
    format!("worst {:.1} km (n = {predictions})", range_m / 1000.0)
}

/// The difference from the current laydown's worst case, worded so the sign needs no
/// reading: farther out is engaged earlier.
#[must_use]
pub fn engagement_difference_label(delta_m: f64) -> String {
    if delta_m > 0.0 {
        format!("{:.1} km farther out than current", delta_m / 1000.0)
    } else if delta_m < 0.0 {
        format!("{:.1} km closer in than current", -delta_m / 1000.0)
    } else {
        "same as current".to_string()
    }
}

fn draw_approach_engagement(ui: &mut Ui, palette: &theme::Palette, e: &ApproachEngagement) {
    match e {
        ApproachEngagement::WorstCase {
            range_m,
            predictions,
            versus_current_m,
            ..
        } => {
            let mut text = worst_case_label(*range_m, *predictions);
            if let Some(delta) = versus_current_m {
                text.push_str("; ");
                text.push_str(&engagement_difference_label(*delta));
            }
            ui.label(text);
        }
        ApproachEngagement::NotComputable { reason } => {
            ui.label(
                RichText::new(format!("not computable: {reason}")).color(palette.warning_color),
            );
        }
    }
}

fn draw_row_first_engagement(
    ui: &mut Ui,
    palette: &theme::Palette,
    first_engagement: &RowFirstEngagement,
    columns: usize,
) {
    match first_engagement {
        RowFirstEngagement::NotRehearsed => {
            for _ in 0..columns {
                ui.label(RichText::new("not rehearsed").color(palette.muted_text_color()));
            }
        }
        RowFirstEngagement::NotComputed { reason } => {
            ui.label(RichText::new(format!("Not computed: {reason}")).color(palette.warning_color));
            for _ in 1..columns {
                ui.label(RichText::new("--").color(palette.muted_text_color()));
            }
        }
        RowFirstEngagement::PerApproach(cells) => {
            for i in 0..columns {
                match cells.get(i) {
                    Some(e) => draw_approach_engagement(ui, palette, e),
                    // A row built against other approaches than the header's: drawn, so
                    // a caller's mismatch is visible rather than shifting the columns.
                    None => {
                        ui.label(RichText::new("--").color(palette.muted_text_color()));
                    }
                }
            }
        }
    }
}

fn draw_first_engagement_account(
    ui: &mut Ui,
    palette: &theme::Palette,
    account: &RehearsalFirstEngagement,
) {
    match account {
        RehearsalFirstEngagement::NotComputed { reason } => {
            ui.label(
                RichText::new(format!("First engagement: not computed: {reason}"))
                    .color(palette.warning_color),
            );
        }
        RehearsalFirstEngagement::PerApproach {
            lines,
            on_no_corridor,
            clutter_pairings,
            plans_proposed,
            plans_not_offered,
            not_offered_because,
        } => {
            ui.label("First engagement, worst case over this run:");
            for line in lines {
                let corridor = line.corridor_half_width_m.map_or_else(
                    || "no corridor declared".to_string(),
                    |w| format!("corridor {:.1} km either side", w / 1000.0),
                );
                match &line.engagement {
                    ApproachEngagement::WorstCase {
                        range_m,
                        predictions,
                        target,
                        track,
                        resource,
                        proposed_at,
                        versus_current_m,
                    } => {
                        let mut text = format!(
                            "{} ({corridor}): {}, target {target} (track {}) against resource \
                             {} proposed at T+{:.0} s of the recording",
                            line.approach,
                            worst_case_label(*range_m, *predictions),
                            track.0,
                            resource.0,
                            proposed_at.0
                        );
                        if let Some(delta) = versus_current_m {
                            text.push_str("; ");
                            text.push_str(&engagement_difference_label(*delta));
                        }
                        ui.label(text);
                    }
                    ApproachEngagement::NotComputable { reason } => {
                        ui.label(
                            RichText::new(format!(
                                "{} ({corridor}): not computable: {reason}",
                                line.approach
                            ))
                            .color(palette.warning_color),
                        );
                    }
                }
            }
            if *on_no_corridor > 0 {
                ui.label(
                    RichText::new(format!(
                        "{on_no_corridor} paired target(s) were in no declared corridor and \
                         are in no approach's figure."
                    ))
                    .small()
                    .color(palette.muted_text_color()),
                );
            }
            if *plans_not_offered > 0 {
                let why = not_offered_because
                    .iter()
                    .map(|(why, n)| format!("{n} denied: {why}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                ui.label(
                    RichText::new(format!(
                        "{plans_not_offered} of the {plans_proposed} plan(s) the run proposed \
                         were not offered for decision under this deployment's policy \
                         ({why}), and engage nothing here."
                    ))
                    .small()
                    .color(palette.warning_color),
                );
            }
            if *clutter_pairings > 0 {
                ui.label(
                    RichText::new(format!(
                        "{clutter_pairings} paired track(s) were none of the recording's \
                         targets (clutter) and are in no figure."
                    ))
                    .small()
                    .color(palette.muted_text_color()),
                );
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
        RehearsalSection::RanInEarlierSession { scenario, session } => {
            ui.label(
                RichText::new(format!(
                    "{}. Its figures were that session's; run it again to see them here.",
                    earlier_label(*scenario, *session)
                ))
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
        "last: {}, run at T+{:.0} s -- {} track(s) formed, {} decision(s) raised ({} expired)",
        summary.scenario.label(),
        summary.ran_at.0,
        summary.tracks_formed,
        summary.decisions_raised,
        summary.decisions_expired
    ));
    draw_first_engagement_account(ui, palette, &summary.first_engagement);
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

/// Where the laydown in force stands (GAP-107, DN-26 §11 item 5): warning-coloured, with
/// what a decision will ask, when a decision will ask; muted when it will not.
fn draw_in_force(ui: &mut Ui, palette: &theme::Palette, line: InForceLine<'_>) {
    ui.label(RichText::new(line.sentence).color(if line.asks {
        palette.warning_color
    } else {
        palette.muted_text_color()
    }));
    if line.asks {
        ui.label(
            RichText::new(
                "Advisory: a decision on a plan asks for this to be acknowledged, and \
                 records that it was. Nothing refuses a plan for it.",
            )
            .small()
            .color(palette.muted_text_color()),
        );
    }
    ui.separator();
}

/// A rehearsal the record holds from an earlier session (D-120), in words.
#[must_use]
pub fn earlier_label(scenario: TestTrackNumber, session: Option<u64>) -> String {
    match session {
        Some(n) => format!("rehearsed against {} in session {n}", scenario.label()),
        None => format!(
            "rehearsed against {} in an earlier session",
            scenario.label()
        ),
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
            approaches: &[],
            rehearsal,
            rehearsal_scenario: TestTrackNumber(1),
            selected,
            in_force: None,
        }
    }

    #[test]
    fn a_computed_row_and_a_not_computed_row_are_different_states() {
        let computed = LaydownCoverage::Computed {
            gap_segments: 0,
            uncovered_m: 0.0,
            delta_uncovered_m: None,
            accepted_segments: 0,
            accepted_uncovered_m: 0.0,
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

    /// D-45: the figure says it is the worst case and carries its count, and a
    /// difference from the current laydown is worded rather than signed.
    #[test]
    fn a_first_engagement_says_worst_case_and_its_count() {
        assert_eq!(worst_case_label(9_240.0, 3), "worst 9.2 km (n = 3)");
        assert_eq!(
            engagement_difference_label(9_000.0),
            "9.0 km farther out than current"
        );
        assert_eq!(
            engagement_difference_label(-2_500.0),
            "2.5 km closer in than current"
        );
        assert_eq!(engagement_difference_label(0.0), "same as current");
        assert!(FIRST_ENGAGEMENT_CAPTION.contains("worst case"));
        assert!(FIRST_ENGAGEMENT_CAPTION.contains("not from sensor data"));
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

    /// The picker offers every committed recording once: the ten plan-07 scenarios and
    /// TT-11, round 1's own raid (GAP-147, D-112).
    #[test]
    fn all_eleven_scenarios_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for s in TestTrackNumber::ALL {
            assert!(seen.insert(s.0), "TT-{:02} listed twice", s.0);
        }
        assert_eq!(seen.len(), 11);
        assert_eq!(TestTrackNumber(11).label(), "TT-11");
    }
}
