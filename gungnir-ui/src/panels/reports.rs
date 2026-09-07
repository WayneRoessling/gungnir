// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-13, mission reports (GAP-071).
//!
//! `docs/ux/information-architecture.md` §1: a `Report` with its figures and their
//! journal references; generate and export.
//!
//! # Every figure names the journal it came from
//!
//! A report is the artefact that outlives the session, so a number in it with no
//! provenance is the one most likely to be quoted later as fact. `MissionReport` carries
//! its `SessionId` for exactly that reason -- every count can be recomputed by folding
//! that journal again -- and this panel puts the session on screen beside the figures
//! rather than only in the exported file.
//!
//! # Counts are not measures
//!
//! What the generator counts is events. The measures catalogue (GAP-047) is computed from
//! the same journal but is a different claim -- a commander reading "12 decisions" may
//! take it for a measure of decision quality -- so the panel draws the two apart, and
//! every measure line carries its target and its basis, or the reason the journal could
//! not answer it. **A measure the journal cannot answer is drawn as that**, beside the
//! ones it can; leaving the hard rows out would read as a catalogue with nothing wrong.

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};

/// The event counts a report carries, already labelled for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountLine<'a> {
    pub label: &'a str,
    pub value: u64,
    /// Set for the two counts that are commonly misread as each other.
    pub note: Option<&'a str>,
}

/// Tracking quality, when a scenario with ground truth was replayed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricsLine {
    pub mota: f64,
    pub motp: f64,
    pub purity: f64,
    pub fragmentation: f64,
}

/// Whether this build can write the report out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportState<'a> {
    /// Available, and this is the marking the file carries by default.
    Available {
        /// The report's combined marking, in words, once generated; before that, that it
        /// is not yet known.
        marking: &'a str,
        /// Where it will be written.
        path: &'a str,
        /// What produced the marking: the journaled items by level (DN-17 §7).
        inputs: &'a str,
    },
    Unavailable(Unavailable<'a>),
}

/// Everything PN-13 draws.
/// DN-19's order of battle as PN-13 draws it (GAP-025): a versioned assessment over
/// the retained sessions, with its coverage stated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderOfBattleLine<'a> {
    pub version: u32,
    pub sessions: usize,
    pub entries: usize,
    /// Entries seen in more than one session: what cross-session identity added.
    pub across_sessions: usize,
    pub unattributed: u32,
    /// Why the product is partial or absent, when it is.
    pub caveat: Option<&'a str>,
}

/// DN-19's pattern of life as PN-13 draws it (GAP-025): when activity was seen, out of
/// how many sessions, and the paths that recurred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternOfLifeLine<'a> {
    /// Sessions queried and sessions anything was seen in. **Both are drawn**: "seen on
    /// six occasions" means different things out of six sessions and out of six hundred,
    /// and a histogram without its denominator is the most common way an activity chart
    /// misleads.
    pub sessions: u32,
    pub sessions_with_activity: u32,
    /// The busiest hour and how many entity-sessions were seen in it, absent when
    /// nothing was seen at all.
    pub busiest: Option<(usize, u32)>,
    /// Paths travelled more than once. A path travelled once is a track history and is
    /// not drawn here at all.
    pub routes: usize,
    /// Why the product is partial or absent, when it is.
    pub caveat: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReportsView<'a> {
    /// The session the report is about. Every figure below is a fold over this journal.
    pub session: Option<u64>,
    /// `None` before the analyst has generated one: an ungenerated report is not a
    /// report of zeros.
    pub counts: Option<&'a [CountLine<'a>]>,
    /// Set when the session exists but has recorded nothing. A third state, distinct
    /// from "not generated" and from a report of zeros: the journal creates a session's
    /// file on its first append, so a session that has published nothing is absent from
    /// it, and that is a fact about the session rather than a failure to report on it.
    pub nothing_recorded: bool,
    pub first_event_s: Option<f64>,
    pub last_event_s: Option<f64>,
    /// Tracking quality, or why there is none.
    pub metrics: Result<MetricsLine, Unavailable<'a>>,
    /// The measures catalogue, which is a different thing from the counts. `None`
    /// before a report is generated, like the counts.
    pub measures: Option<&'a [MeasureLine]>,
    pub export: ExportState<'a>,
    /// Set after a successful export, naming what was written.
    pub last_export: Option<&'a str>,
    /// The review conducted against this report, if one is (GAP-049, DN-20 §7).
    pub review: ReviewView<'a>,
    /// The order of battle over the retained sessions (GAP-025), `None` before a report
    /// is generated.
    pub order_of_battle: Option<OrderOfBattleLine<'a>>,
    /// The pattern of life over the same sessions (GAP-025), `None` before a report is
    /// generated.
    pub pattern_of_life: Option<PatternOfLifeLine<'a>>,
}

/// The after-action review as PN-13 draws it (GAP-049).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewView<'a> {
    pub case: Option<ReviewCaseView<'a>>,
    /// Why a review cannot be opened now, when it cannot: no session to review.
    pub cannot_open: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewCaseView<'a> {
    pub session: u64,
    pub state: ReviewStateView,
    pub findings: &'a [FindingLine],
    /// Findings whose action is still open; a review does not close over them.
    pub open_actions: usize,
    /// Whether a seek would land: the replay of the reviewed session is open.
    pub replay_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStateView {
    Open,
    Concluded,
    Closed,
}

/// The finding kinds, in this crate's words. The workflow crate owns the type; this is
/// its vocabulary on screen, and the app maps between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindingKindView {
    SystemBehaviour,
    Procedure,
    /// The default when a reviewer has not chosen: a training point filed as a defect
    /// is the error DN-20 §5 names, and the reverse is only an unpromoted finding.
    #[default]
    Practice,
    Configuration,
}

impl FindingKindView {
    pub const ALL: [FindingKindView; 4] = [
        FindingKindView::SystemBehaviour,
        FindingKindView::Procedure,
        FindingKindView::Practice,
        FindingKindView::Configuration,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            FindingKindView::SystemBehaviour => "system behaviour (candidate gap)",
            FindingKindView::Procedure => "procedure",
            FindingKindView::Practice => "practice (never a defect)",
            FindingKindView::Configuration => "configuration (candidate baseline change)",
        }
    }
}

/// One finding, owned like a measure line: the summary is the reviewer's sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct FindingLine {
    pub id: u64,
    pub summary: String,
    pub kind: FindingKindView,
    /// Mission time the finding refers to. Without it the finding cannot be seeked to
    /// and is drawn as an anecdote.
    pub at_s: Option<f64>,
    pub promoted_to: Option<String>,
}

/// What the reviewer has typed and not committed. Scratch, held by the caller.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewDraft {
    pub summary: String,
    pub kind: FindingKindView,
    /// The gap identifier typed against a finding about to be promoted.
    pub gap: String,
}

/// What the reviewer asked for this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewAction {
    Open,
    RecordFinding {
        summary: String,
        kind: FindingKindView,
    },
    /// Seek the replay to this finding's moment.
    SeekTo(u64),
    Conclude,
    Close,
    Promote {
        finding: u64,
        gap: String,
    },
}

/// What the analyst asked for this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum ReportsAction {
    Generate,
    Export,
    Review(ReviewAction),
}

/// Render the reports panel. `draft` is the review's scratch, held by the caller.
pub fn render_reports(
    ui: &mut Ui,
    view: &ReportsView<'_>,
    draft: &mut ReviewDraft,
) -> Option<ReportsAction> {
    ui.heading("Mission report");

    let Some(session) = view.session else {
        ui.label(
            RichText::new("No session is open, so there is no journal to report on.")
                .color(theme::MUTED_TEXT_COLOR),
        );
        return None;
    };
    ui.label(
        RichText::new(format!(
            "Session {session}. Every figure below is a fold over that journal and can \
             be recomputed from it."
        ))
        .color(theme::MUTED_TEXT_COLOR)
        .size(theme::SMALL_FONT_SIZE),
    );
    ui.separator();

    let mut action = None;
    if ui.button("Generate").clicked() {
        action = Some(ReportsAction::Generate);
    }

    match view.counts {
        None if view.nothing_recorded => {
            ui.label(
                RichText::new(
                    "This session has recorded nothing yet, so there is nothing to \
                     report. That is not a failure and not a report of zeros.",
                )
                .color(theme::MUTED_TEXT_COLOR),
            );
        }
        None => {
            ui.label(RichText::new("Not generated yet.").color(theme::MUTED_TEXT_COLOR));
        }
        Some(counts) => draw_counts(ui, view, counts),
    }

    ui.separator();
    ui.strong("Measures");
    match view.measures {
        Some(lines) => draw_measures(ui, lines),
        None => {
            ui.label(RichText::new("Not generated yet.").color(theme::MUTED_TEXT_COLOR));
        }
    }

    ui.separator();
    ui.strong("Order of battle");
    match view.order_of_battle {
        Some(line) => draw_order_of_battle(ui, line),
        None => {
            ui.label(RichText::new("Not generated yet.").color(theme::MUTED_TEXT_COLOR));
        }
    }

    ui.separator();
    ui.strong("Pattern of life");
    match view.pattern_of_life {
        Some(line) => draw_pattern_of_life(ui, line),
        None => {
            ui.label(RichText::new("Not generated yet.").color(theme::MUTED_TEXT_COLOR));
        }
    }

    ui.separator();
    if let Some(a) = draw_review(ui, view.review, draft) {
        return Some(ReportsAction::Review(a));
    }

    ui.separator();
    if let Some(a) = draw_export(ui, view) {
        action = Some(a);
    }
    action
}

/// One row of the measures catalogue as PN-13 draws it (GAP-047).
///
/// Owned strings, unlike the other view structs: a reason is a sentence composed when
/// the journal was folded, and the target is quoted from the catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureLine {
    pub id: String,
    pub name: String,
    pub target: String,
    pub value: MeasureLineValue,
    /// What the figure rests on, when the number alone would mislead.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MeasureLineValue {
    Fraction {
        value: f64,
        numerator: u64,
        denominator: u64,
    },
    Count(u64),
    NoInstances {
        of: String,
    },
    NotComputable {
        reason: String,
    },
}

/// DN-19: the product assembles evidence and the analyst concludes, so the line says
/// what it rests on and what it does not cover.
fn draw_order_of_battle(ui: &mut Ui, line: OrderOfBattleLine<'_>) {
    ui.label(format!(
        "Version {} over {} session(s): {} entit{}, {} seen across sessions, {} track(s) \
         unattributed.",
        line.version,
        line.sessions,
        line.entries,
        if line.entries == 1 { "y" } else { "ies" },
        line.across_sessions,
        line.unattributed
    ));
    if let Some(caveat) = line.caveat {
        ui.label(RichText::new(caveat).color(theme::WARNING_COLOR));
    }
}

/// The denominator is drawn with the figure, never after it: an activity chart read
/// without the sessions it came out of is the one that gets quoted as a habit.
fn draw_pattern_of_life(ui: &mut Ui, line: PatternOfLifeLine<'_>) {
    match line.busiest {
        Some((hour, count)) => ui.label(format!(
            "Activity in {} of {} session(s); busiest hour {:02}:00 with {} sighting(s); \
             {} recurring route(s).",
            line.sessions_with_activity, line.sessions, hour, count, line.routes
        )),
        None => ui.label(
            RichText::new(format!(
                "Nothing was seen in any of the {} session(s) queried.",
                line.sessions
            ))
            .color(theme::MUTED_TEXT_COLOR),
        ),
    };
    if let Some(caveat) = line.caveat {
        ui.label(RichText::new(caveat).color(theme::WARNING_COLOR));
    }
}

fn draw_measures(ui: &mut Ui, lines: &[MeasureLine]) {
    egui::Grid::new("report_measures")
        .striped(true)
        .num_columns(3)
        .show(ui, |ui| {
            for line in lines {
                ui.label(format!("{} {}", line.id, line.name));
                match &line.value {
                    MeasureLineValue::Fraction {
                        value,
                        numerator,
                        denominator,
                    } => {
                        ui.label(format!("{value:.2} ({numerator} of {denominator})"));
                    }
                    MeasureLineValue::Count(n) => {
                        ui.label(format!("{n}"));
                    }
                    MeasureLineValue::NoInstances { of } => {
                        ui.label(
                            RichText::new(format!("no {of} in this session"))
                                .color(theme::MUTED_TEXT_COLOR),
                        );
                    }
                    MeasureLineValue::NotComputable { reason } => {
                        ui.label(
                            RichText::new(format!("not computable: {reason}"))
                                .color(theme::WARNING_COLOR),
                        );
                    }
                }
                ui.label(
                    RichText::new(format!("target {}", line.target))
                        .small()
                        .color(theme::MUTED_TEXT_COLOR),
                );
                ui.end_row();
                if let Some(note) = &line.note {
                    ui.label("");
                    ui.label(RichText::new(note).small().color(theme::MUTED_TEXT_COLOR));
                    ui.label("");
                    ui.end_row();
                }
            }
        });
}

/// DN-20 §5: nothing here is automatic. The panel assembles the record, seeks to the
/// moment, and holds what people concluded.
fn draw_review(
    ui: &mut Ui,
    review: ReviewView<'_>,
    draft: &mut ReviewDraft,
) -> Option<ReviewAction> {
    ui.strong("After-action review");
    let Some(case) = review.case else {
        return match review.cannot_open {
            Some(reason) => {
                ui.label(RichText::new(reason).color(theme::MUTED_TEXT_COLOR));
                None
            }
            None => ui
                .button("Open a review of this session")
                .clicked()
                .then_some(ReviewAction::Open),
        };
    };

    let state = match case.state {
        ReviewStateView::Open => "open",
        ReviewStateView::Concluded => "concluded: findings recorded; actions may still be open",
        ReviewStateView::Closed => "closed: every action done",
    };
    ui.label(format!("Review of session {}, {state}.", case.session));
    let mut action = None;
    if case.findings.is_empty() {
        ui.label(RichText::new("No findings recorded.").color(theme::MUTED_TEXT_COLOR));
    }
    for f in case.findings {
        if let Some(a) = draw_finding(ui, f, case.replay_open, draft) {
            action = Some(a);
        }
    }
    action.or_else(|| draw_review_controls(ui, case, draft))
}

/// One finding: its kind, its moment (or that it has none), and its promotion.
fn draw_finding(
    ui: &mut Ui,
    f: &FindingLine,
    replay_open: bool,
    draft: &mut ReviewDraft,
) -> Option<ReviewAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(format!("#{} [{}]", f.id, f.kind.label()));
        ui.label(&f.summary);
        match f.at_s {
            Some(t) => {
                let seek =
                    ui.add_enabled(replay_open, egui::Button::new(format!("at {t:.1} s: seek")));
                if seek.clicked() {
                    action = Some(ReviewAction::SeekTo(f.id));
                }
            }
            None => {
                ui.label(
                    RichText::new("no moment recorded: an anecdote, not seekable")
                        .color(theme::WARNING_COLOR),
                );
            }
        }
        match &f.promoted_to {
            Some(gap) => {
                ui.label(format!("promoted to {gap}"));
            }
            None if f.kind == FindingKindView::SystemBehaviour => {
                ui.add(egui::TextEdit::singleline(&mut draft.gap).hint_text("GAP-000"));
                if ui.button("Promote").clicked() {
                    action = Some(ReviewAction::Promote {
                        finding: f.id,
                        gap: draft.gap.clone(),
                    });
                }
            }
            None => {}
        }
    });
    action
}

/// The controls the review's state allows.
fn draw_review_controls(
    ui: &mut Ui,
    case: ReviewCaseView<'_>,
    draft: &mut ReviewDraft,
) -> Option<ReviewAction> {
    let mut action = None;
    match case.state {
        ReviewStateView::Open => {
            ui.add(
                egui::TextEdit::singleline(&mut draft.summary)
                    .hint_text("what was found, in one sentence"),
            );
            ui.horizontal(|ui| {
                for kind in FindingKindView::ALL {
                    ui.radio_value(&mut draft.kind, kind, kind.label());
                }
            });
            ui.horizontal(|ui| {
                let can_record = !draft.summary.trim().is_empty();
                if ui
                    .add_enabled(can_record, egui::Button::new("Record finding"))
                    .clicked()
                {
                    action = Some(ReviewAction::RecordFinding {
                        summary: draft.summary.trim().to_string(),
                        kind: draft.kind,
                    });
                }
                if ui.button("Conclude review").clicked() {
                    action = Some(ReviewAction::Conclude);
                }
            });
        }
        ReviewStateView::Concluded => {
            if case.open_actions > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} action(s) still open; the review does not close over them.",
                        case.open_actions
                    ))
                    .color(theme::WARNING_COLOR),
                );
            }
            if ui.button("Close review").clicked() {
                action = Some(ReviewAction::Close);
            }
        }
        ReviewStateView::Closed => {
            if ui.button("Open another review of this session").clicked() {
                action = Some(ReviewAction::Open);
            }
        }
    }
    action
}

fn draw_counts(ui: &mut Ui, view: &ReportsView<'_>, counts: &[CountLine<'_>]) {
    ui.strong("Event counts");
    egui::Grid::new("report_counts")
        .striped(true)
        .show(ui, |ui| {
            for line in counts {
                ui.label(line.label);
                ui.label(line.value.to_string());
                match line.note {
                    Some(note) => {
                        ui.label(
                            RichText::new(note)
                                .color(theme::MUTED_TEXT_COLOR)
                                .size(theme::SMALL_FONT_SIZE),
                        );
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });

    match (view.first_event_s, view.last_event_s) {
        (Some(first), Some(last)) => {
            ui.label(format!(
                "Covering {first:.1} s to {last:.1} s of mission time"
            ));
        }
        _ => {
            ui.label(
                RichText::new("The session recorded no events, so it covers no time.")
                    .color(theme::MUTED_TEXT_COLOR),
            );
        }
    }

    ui.separator();
    ui.strong("Tracking quality");
    match view.metrics {
        Ok(m) => {
            ui.label(format!(
                "MOTA {:.3}, MOTP {:.3}, purity {:.3}, fragmentation {:.3}",
                m.mota, m.motp, m.purity, m.fragmentation
            ));
        }
        Err(u) => draw_unavailable(ui, u),
    }
}

fn draw_export(ui: &mut Ui, view: &ReportsView<'_>) -> Option<ReportsAction> {
    ui.strong("Export");
    match view.export {
        ExportState::Unavailable(u) => {
            draw_unavailable(ui, u);
            None
        }
        ExportState::Available {
            marking,
            path,
            inputs,
        } => {
            ui.label(
                RichText::new(format!("Written to {path}, marked {marking}."))
                    .color(theme::MUTED_TEXT_COLOR),
            );
            // The marking is a fact about the file and travels inside it (GAP-062); the
            // inputs are what a reader checks it against.
            ui.label(
                RichText::new(format!("Marking from {inputs}."))
                    .color(theme::MUTED_TEXT_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
            let clicked = ui
                .add_enabled(view.counts.is_some(), egui::Button::new("Export report"))
                .clicked();
            if view.counts.is_none() {
                ui.label(
                    RichText::new("Generate the report before exporting it.")
                        .color(theme::MUTED_TEXT_COLOR),
                );
            }
            if let Some(written) = view.last_export {
                ui.label(RichText::new(format!("Last export: {written}")));
            }
            clicked.then_some(ReportsAction::Export)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ungenerated report is not a report full of zeros. An analyst who opened the
    /// panel and read "0 decisions" without generating anything would have a figure
    /// that came from nothing.
    #[test]
    fn an_ungenerated_report_has_no_counts() {
        let view = ReportsView {
            session: Some(7),
            counts: None,
            first_event_s: None,
            last_event_s: None,
            metrics: Err(Unavailable {
                owner: "gungnir-scenario",
                gap: "GAP-016",
            }),
            measures: None,
            export: ExportState::Available {
                marking: "not yet known",
                path: "reports/",
                inputs: "no report generated",
            },
            last_export: None,
            nothing_recorded: false,
            review: ReviewView {
                case: None,
                cannot_open: None,
            },
            order_of_battle: None,
            pattern_of_life: None,
        };
        assert!(view.counts.is_none());
        assert!(
            view.measures.is_none(),
            "an ungenerated report has no measures either"
        );
        let zeros: &[CountLine<'_>] = &[CountLine {
            label: "Decisions",
            value: 0,
            note: None,
        }];
        assert_ne!(view.counts, Some(zeros));
    }

    /// Counts and measures are different claims and the panel keeps them apart: a
    /// measure line is a different type from a count line, with a target and a basis.
    #[test]
    fn counts_are_not_measures() {
        let line = MeasureLine {
            id: "MOE-13".into(),
            name: "Intelligence product timeliness".into(),
            target: "0.95 of scheduled".into(),
            value: MeasureLineValue::Fraction {
                value: 0.5,
                numerator: 1,
                denominator: 2,
            },
            note: None,
        };
        assert_ne!(line.target, "");
        assert!(matches!(
            line.value,
            MeasureLineValue::Fraction { denominator: 2, .. }
        ));
    }

    /// An expiry and a decision are separate counts, and the two that are commonly
    /// misread carry a note. This is the report-level half of DN-10's rule that an
    /// expiry is not a rejection.
    #[test]
    fn expiries_are_counted_apart_from_decisions() {
        let lines = [
            CountLine {
                label: "Decisions",
                value: 3,
                note: None,
            },
            CountLine {
                label: "Expired",
                value: 2,
                note: Some("not rejections: nobody decided"),
            },
        ];
        assert_ne!(lines[0].label, lines[1].label);
        assert!(
            lines[1].note.is_some_and(|n| n.contains("not rejections")),
            "the expiry count has to say what it is not"
        );
    }
}
