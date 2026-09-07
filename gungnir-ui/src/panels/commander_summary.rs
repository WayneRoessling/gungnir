//! PN-17, the commander summary (GAP-073).
//!
//! `docs/ux/ux-to-code-map.md` §1: queue statistics, delegations in force, accepted
//! coverage gaps, the plan in force, and outcomes; writes delegate and accept-gap.
//! It is the commander's *leading* panel -- first in that role's workspace -- so it is
//! the first thing that role reads, and what it does not know it has to say.
//!
//! Two of the five sections are real today: the plan in force comes from
//! `AppState::last_plan`, and the delegations from the authority rules of the
//! configuration baseline. Queue statistics, accepted coverage gaps and outcomes come
//! from crates the desktop does not wire, and are drawn as unavailable rather than as
//! zero. A commander reading "0 pending approvals" when the queue is simply not
//! connected would be the worst failure this panel could have.

use crate::panels::status_strip::DelegationLine;
use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::{MissionTime, Vocabulary};

/// Approval-queue statistics from `gungnir-command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStats {
    pub pending: usize,
    pub decided_this_session: usize,
    /// Windows that closed with nobody deciding. **Not rejections**: nobody chose.
    pub expired: usize,
}

/// A coverage gap the commander has accepted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptedGap<'a> {
    pub description: &'a str,
    pub accepted_at_s: f64,
}

/// The plan currently in force.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanInForce<'a> {
    pub summary: &'a str,
    pub assignments: usize,
}

/// What one watch hands the next (DN-21 §5, GAP-054).
///
/// Every field but the notes is assembled from the record. The notes are the outgoing
/// watch's judgement, and **the summary is not complete until somebody acknowledges it by
/// name** -- an unacknowledged handover is visible here rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandoverView<'a> {
    pub period: (MissionTime, MissionTime),
    pub open_alerts: usize,
    pub pending_approvals: usize,
    pub expired_approvals: usize,
    pub sensors_degraded: usize,
    pub maintenance_outstanding: usize,
    /// Who took over, and when. `None` means the handover is still open.
    pub acknowledged_by: Option<(&'a str, MissionTime)>,
    /// Whether anything in the period still needs somebody.
    pub outstanding_work: bool,
    /// Parts of the summary this build cannot assemble, named rather than left blank.
    ///
    /// **A blank line in a handover reads as "all clear"**, which is the one thing it must
    /// never do when the truth is that nothing looked.
    pub not_assembled: &'a [Unavailable<'a>],
}

/// One exposed asset, as the commander reads it: which track, which asset, how soon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExposureLine<'a> {
    pub track: u64,
    pub asset: &'a str,
    pub priority: &'a str,
    /// 0.0 to 1.0.
    pub score: f32,
    /// `None` when the track is not closing on the asset.
    pub time_to_impact_s: Option<f32>,
}

fn draw_exposure(ui: &mut Ui, exposure: Result<&[ExposureLine<'_>], &str>) {
    ui.strong("Most exposed assets");
    match exposure {
        Err(reason) => {
            ui.label(RichText::new(reason).color(theme::MUTED_TEXT_COLOR));
        }
        Ok([]) => {
            ui.label(
                RichText::new("No track threatens a listed asset.").color(theme::MUTED_TEXT_COLOR),
            );
        }
        Ok(lines) => {
            for line in lines.iter().take(5) {
                let when = line.time_to_impact_s.map_or_else(
                    || "not closing".to_string(),
                    |t| format!("{t:.0} s to impact"),
                );
                ui.label(format!(
                    "track {}: {} (priority {}), score {:.2}, {when}",
                    line.track, line.asset, line.priority, line.score
                ));
            }
        }
    }
    ui.separator();
}

/// Engagement outcomes with the evidence sources kept apart (DN-06 §5, §7).
///
/// A view struct rather than the facade's `EffectTally`, because this crate takes plain
/// views; the app maps one to the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutcomeCounts {
    pub open: u32,
    pub effective_corroborated: u32,
    pub effective_track_inferred: u32,
    pub ineffective_corroborated: u32,
    pub ineffective_track_inferred: u32,
    pub indeterminate: u32,
    pub aborted: u32,
}

impl OutcomeCounts {
    #[must_use]
    pub fn total(self) -> u32 {
        self.open
            + self.effective_corroborated
            + self.effective_track_inferred
            + self.ineffective_corroborated
            + self.ineffective_track_inferred
            + self.indeterminate
            + self.aborted
    }
}

/// **The two evidence columns are never added together on screen.** A commander reading
/// "3 effective" with two of them inferred from a track dropping out of the picture is
/// reading a number the evidence does not support.
fn draw_outcomes(ui: &mut Ui, counts: OutcomeCounts) {
    if counts.total() == 0 {
        ui.label("No engagements this session.");
        return;
    }
    ui.label(format!("Open: {}", counts.open));
    ui.label(format!(
        "Effective: {} corroborated, {} track-inferred",
        counts.effective_corroborated, counts.effective_track_inferred
    ));
    ui.label(format!(
        "Ineffective: {} corroborated, {} track-inferred",
        counts.ineffective_corroborated, counts.ineffective_track_inferred
    ));
    ui.label(format!(
        "Indeterminate: {}   Aborted: {}",
        counts.indeterminate, counts.aborted
    ));
    if counts.effective_track_inferred + counts.ineffective_track_inferred > 0 {
        ui.label(
            RichText::new(
                "Track-inferred outcomes rest on the track leaving or staying in the \
                 picture: destroyed, masked, or dropped by the tracker, and the evidence \
                 does not say which.",
            )
            .small()
            .color(theme::WARNING_COLOR),
        );
    }
}

/// What the watch does on this panel.
#[derive(Debug, Clone, PartialEq)]
pub enum HandoverAction {
    /// The outgoing watch's notes, as typed.
    Notes(String),
    /// The incoming watch takes over.
    Acknowledge,
}

/// Everything the summary draws.
/// Warnings owed in the period (DN-03 §7, GAP-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WarningCounts {
    pub open: usize,
    pub late: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommanderSummaryView<'a> {
    /// The open handover, when one has come due (GAP-054). `None` outside a shift change.
    pub handover: Option<HandoverView<'a>>,
    pub queue: Result<QueueStats, Unavailable<'a>>,
    /// The pre-delegated authorities in force (D-15). The same lines PN-01 shows,
    /// from the same rules: a commander and an operator must not be able to read two
    /// different answers to "what is delegated right now".
    pub delegations: &'a [DelegationLine<'a>],
    pub accepted_gaps: Result<&'a [AcceptedGap<'a>], Unavailable<'a>>,
    pub plan: Option<PlanInForce<'a>>,
    /// Engagement outcomes for the session (GAP-043, DN-06 §7).
    pub outcomes: Result<OutcomeCounts, Unavailable<'a>>,
    /// The most exposed assets: the highest-scoring tracks and what each threatens
    /// (GAP-026, DN-01). `Err` carries why nothing is scored -- no origin, no assets --
    /// which is the deployment's state, not a gap.
    pub exposure: Result<&'a [ExposureLine<'a>], &'a str>,
    /// Whether this build can delegate or accept a gap. `false` draws no control:
    /// a button that appears to commit an authority and does not is worse than none.
    pub controls_available: bool,
    /// Warnings owed, late and failed (GAP-042).
    pub warnings: WarningCounts,
    /// The words this deployment uses (D-12).
    pub vocabulary: &'a Vocabulary,
}

/// Render the commander summary.
///
/// Returns what the watch did, if anything. `notes` is the scratch buffer the outgoing
/// watch types into; it outlives a frame and is not mission state, which is why it is
/// passed in rather than held here.
/// GAP-042: warnings owed in the period; late and failed are the two a commander has to
/// act on.
fn draw_warnings(ui: &mut Ui, warnings: WarningCounts) {
    ui.strong("Warnings");
    if warnings.open == 0 {
        ui.label(egui::RichText::new("none owed").color(theme::MUTED_TEXT_COLOR));
    } else {
        ui.label(
            egui::RichText::new(format!(
                "{} owed, {} late, {} failed",
                warnings.open, warnings.late, warnings.failed
            ))
            .color(if warnings.late + warnings.failed > 0 {
                theme::ALERT_COLOR
            } else {
                theme::WARNING_COLOR
            }),
        );
    }
    ui.separator();
}

pub fn render_commander_summary(
    ui: &mut Ui,
    view: &CommanderSummaryView<'_>,
    notes: &mut String,
) -> Option<HandoverAction> {
    ui.heading("Commander summary");

    // Drawn first when it is open: at a shift change this is what the panel is for, and
    // burying it under the queue statistics would make the one time-critical thing on the
    // screen the thing you scroll to.
    let action = view.handover.and_then(|h| draw_handover(ui, &h, notes));

    ui.strong("Approval queue");
    match view.queue {
        Ok(q) => {
            ui.label(format!(
                "{} pending, {} decided this session, {} expired",
                q.pending, q.decided_this_session, q.expired
            ));
            if q.expired > 0 {
                ui.label(
                    RichText::new("Expired windows are not rejections: nobody decided.")
                        .color(theme::WARNING_COLOR),
                );
            }
        }
        Err(u) => draw_unavailable(ui, u),
    }
    ui.separator();

    ui.strong("Plan in force");
    match view.plan {
        Some(p) => {
            ui.label(p.summary);
            ui.label(format!("{} assignments", p.assignments));
        }
        None => {
            ui.label(RichText::new("No plan proposed.").color(theme::MUTED_TEXT_COLOR));
        }
    }
    ui.separator();

    ui.strong("Delegations in force");
    if view.delegations.is_empty() {
        ui.label(RichText::new("None.").color(theme::MUTED_TEXT_COLOR));
    } else {
        for d in view.delegations {
            let text = match (d.layer, d.class) {
                (Some(l), Some(c)) => {
                    format!(
                        "{} delegated to {} ({}) [{c}]",
                        d.action,
                        d.role,
                        view.vocabulary.layer(l)
                    )
                }
                (Some(l), None) => format!(
                    "{} delegated to {} ({})",
                    d.action,
                    d.role,
                    view.vocabulary.layer(l)
                ),
                (None, Some(c)) => format!("{} delegated to {} [{c}]", d.action, d.role),
                (None, None) => format!("{} delegated to {}", d.action, d.role),
            };
            ui.label(RichText::new(text).color(theme::WARNING_COLOR));
        }
    }
    ui.separator();

    ui.strong("Accepted coverage gaps");
    draw_warnings(ui, view.warnings);

    match view.accepted_gaps {
        Ok([]) => {
            ui.label(RichText::new("None accepted.").color(theme::MUTED_TEXT_COLOR));
        }
        Ok(gaps) => {
            for g in gaps {
                ui.label(format!(
                    "{} (accepted at {:.0} s)",
                    g.description, g.accepted_at_s
                ));
            }
        }
        Err(u) => draw_unavailable(ui, u),
    }
    ui.separator();

    draw_exposure(ui, view.exposure);

    ui.strong("Outcomes");
    match view.outcomes {
        Ok(counts) => draw_outcomes(ui, counts),
        Err(u) => draw_unavailable(ui, u),
    }
    ui.separator();

    if !view.controls_available {
        ui.label(
            RichText::new(
                "Delegate and accept-gap controls need gungnir-command wired to the \
                 desktop; none is drawn.",
            )
            .color(theme::MUTED_TEXT_COLOR),
        );
    }

    action
}

/// The handover section (DN-21 §5 and §7, GAP-054).
///
/// **The acknowledgement is the point.** MOE-13 counts handovers taken inside the window,
/// so the state that matters here is not what happened on the watch -- that is assembled
/// and shown -- but whether somebody has said, by name, that they now have it.
fn draw_handover(ui: &mut Ui, h: &HandoverView<'_>, notes: &mut String) -> Option<HandoverAction> {
    let mut action = None;
    ui.separator();
    ui.strong("Watch handover");
    ui.label(format!(
        "Period {} to {}",
        crate::panels::status_strip::format_clock(h.period.0),
        crate::panels::status_strip::format_clock(h.period.1)
    ));
    ui.label(format!(
        "{} open alerts, {} approvals pending, {} expired, {} sensors degraded, {} \
         maintenance windows outstanding",
        h.open_alerts,
        h.pending_approvals,
        h.expired_approvals,
        h.sensors_degraded,
        h.maintenance_outstanding
    ));

    // Named, never blank. A section this build cannot assemble must not look like a
    // section that was assembled and found nothing.
    for u in h.not_assembled {
        draw_unavailable(ui, *u);
    }

    if let Some((who, at)) = h.acknowledged_by {
        ui.label(
            RichText::new(format!(
                "Taken by {who} at {}",
                crate::panels::status_strip::format_clock(at)
            ))
            .color(theme::HEALTHY_COLOR),
        );
    } else {
        {
            // The one part the system does not assemble, and the part that matters most.
            ui.label("Notes for the incoming watch:");
            if ui.text_edit_multiline(notes).changed() {
                action = Some(HandoverAction::Notes(notes.clone()));
            }
            let warning = if h.outstanding_work {
                "This watch is handing over unfinished work."
            } else {
                "Nothing on this watch is outstanding."
            };
            ui.label(RichText::new(warning).color(if h.outstanding_work {
                theme::WARNING_COLOR
            } else {
                theme::MUTED_TEXT_COLOR
            }));
            if ui.button("Acknowledge and take the watch").clicked() {
                action = Some(HandoverAction::Acknowledge);
            }
            ui.label(
                RichText::new("Not acknowledged: this handover is incomplete.")
                    .color(theme::WARNING_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
        }
    }
    ui.separator();
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction this panel exists to preserve: an unavailable queue is not a
    /// queue with nothing in it. A commander reading "0 pending" from a disconnected
    /// queue would believe there is nothing waiting on them. This is the one the panel
    /// would get wrong most easily, because `QueueStats::default()` is all zeros and
    /// reads as a calm screen.
    #[test]
    fn an_unavailable_queue_is_not_an_empty_queue() {
        let empty: Result<QueueStats, Unavailable<'_>> = Ok(QueueStats {
            pending: 0,
            decided_this_session: 0,
            expired: 0,
        });
        let unavailable: Result<QueueStats, Unavailable<'_>> = Err(Unavailable {
            owner: "gungnir-command",
            gap: "GAP-038",
        });
        assert_ne!(empty, unavailable);
        assert!(unavailable.is_err());
    }

    /// Likewise for accepted gaps and outcomes: three sections, three chances to
    /// mistake "not connected" for "nothing to report".
    #[test]
    fn every_unavailable_section_names_an_owner_and_a_gap() {
        for u in [
            Unavailable {
                owner: "gungnir-command",
                gap: "GAP-038",
            },
            Unavailable {
                owner: "gungnir-analytics",
                gap: "GAP-006",
            },
            Unavailable {
                owner: "gungnir-intercept-service",
                gap: "GAP-043",
            },
        ] {
            assert!(u.owner.starts_with("gungnir-"));
            assert!(u.gap.starts_with("GAP-"));
        }
    }
}
