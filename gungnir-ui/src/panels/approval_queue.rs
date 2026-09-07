//! PN-06, the approval queue (GAP-038).
//!
//! `docs/ux/information-architecture.md` §1: pending approvals ordered by priority and
//! time remaining, with delegation marks; selection opens PN-07.
//!
//! # An empty queue is a claim, and this build's queue is always empty
//!
//! Every other panel's danger is showing a zero it did not compute. This one's is
//! subtler and worse: the queue *is* correct when it says nothing is pending, and it
//! will say that in every configuration this build can run. The tracking pipeline is
//! not implemented, so there are no tracks; with no tracks the allocator returns an
//! empty plan; an empty plan is denied by policy before it can be queued. "0 pending"
//! is therefore true and, on its own, deeply misleading -- an operator reads a calm
//! queue as *nothing needs deciding*, when what it means is *nothing upstream is
//! running*.
//!
//! So [`EmptyBecause`] is a required field rather than an optional note, and it is
//! rendered whenever the queue is empty. The three variants are three different
//! operational situations, and the panel refuses to let them look alike.
//!
//! # Time remaining, and the two ways there can be none
//!
//! The countdown is real since GAP-034: the workflow times each item from the layer it
//! engages and the baseline's `DecisionSettings`. What has to stay distinguishable is
//! *why* an item has no countdown. DN-10 makes "no expiry configured" mean the item is
//! **preserved indefinitely** -- silence about expiry preserves, because discarding a
//! decision nobody took loses information -- so a blank cell would read as a decision
//! the deployment had taken. [`TimeRemaining`] keeps that apart from a build that
//! simply does not track deadlines, which is what this column said before GAP-034.
//!
//! # Ordering
//!
//! The queue arrives ordered by time remaining, then priority: time pressure outranks
//! severity, because a high-priority item with two minutes left can wait behind a
//! lower-priority one with ten seconds left and the reverse loses both. The priority
//! tie-break needs risk scores from `gungnir-assessment`, which is not wired, so
//! [`QueueOrder`] says whether the ordering an operator is looking at includes it.
//!
//! # A decision stays visible until its handoff is delivered
//!
//! An item leaves the queue the instant the decision is recorded, and that is right: it
//! is no longer waiting on a human. But the *work* is not finished -- something still has
//! to reach an effector, and on most deployments that something is a person on a radio.
//! Until GAP-040 this panel let the item vanish at the moment of decision, which is the
//! one moment an operator is most likely to consider it dealt with.
//!
//! So the panel carries a second list below the queue: every handoff this desktop has
//! issued that `handoff::stays_visible` keeps, drawn whether or not the queue itself has
//! anything in it. The rule is delivery, not distress -- `crate::panels::handoff` sets
//! out why that is a different question from `DeliveryState::needs_attention`, and why
//! answering it with `needs_attention` would hide exactly the manual handoffs this
//! build produces.

use crate::panels::handoff::{self, HandoffRow};
use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::MissionTime;

/// The queue's identifier for one pending item, mirroring
/// `gungnir_command::PendingApprovalId` without depending on that crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PendingId(pub u64);

/// The policy verdict a queued plan carries.
///
/// A denied plan can never be queued -- `ApprovalWorkflow::submit_for_approval` refuses
/// it -- so `Denied` appears here only to carry the reason into the dialog and the
/// empty-queue explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// Every engine approved outright. The built-in engines never return this.
    Approved,
    RequiresHumanApproval,
    Denied {
        reason: &'a str,
    },
}

impl Verdict<'_> {
    fn label(&self) -> &str {
        match self {
            Verdict::Approved => "Pre-authorized",
            Verdict::RequiresHumanApproval => "Needs a decision",
            Verdict::Denied { reason } => reason,
        }
    }

    fn color(&self) -> egui::Color32 {
        match self {
            Verdict::Approved => theme::CLASS_NEUTRAL_COLOR,
            Verdict::RequiresHumanApproval => theme::WARNING_COLOR,
            Verdict::Denied { .. } => theme::CLASS_HOSTILE_COLOR,
        }
    }
}

/// How long is left to decide.
///
/// The three cases are deliberately distinct. "Not tracked" is this build's own
/// limitation; "no expiry configured" is a decision the deployment took, and DN-10
/// makes it mean the item is preserved indefinitely rather than dropped. Collapsing
/// either into a blank cell would let an operator read one as the other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeRemaining<'a> {
    Seconds(f64),
    /// The deployment configured no expiry for this layer, so the item never expires.
    NoExpiryConfigured,
    /// The build does not carry deadlines on a queued item. Names the register entry.
    ///
    /// Unreachable on the desktop since GAP-034; kept because it is the honest answer
    /// for any `ApprovalWorkflow` that does not time its queue, and deleting it would
    /// leave such an implementation with only the "no expiry configured" wording, which
    /// would be a false claim about the deployment's settings.
    NotTracked {
        gap: &'a str,
    },
}

/// One row of the queue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueueRow<'a> {
    /// The role this item was escalated from, when it has been. The original role keeps
    /// seeing it (DN-10 §5), so this is a mark rather than a reassignment.
    pub escalated_from: Option<&'a str>,
    pub id: PendingId,
    pub plan_id: u64,
    /// How many (resource, track) pairs the plan tasks.
    pub assignments: usize,
    pub verdict: Verdict<'a>,
    pub time_remaining: TimeRemaining<'a>,
    /// D-15: the authority rules pre-delegate this case to the signed-in role. It
    /// still needs a recorded decision; what it skips is escalation, not the person.
    pub pre_delegated: bool,
    /// Whether the signed-in role may take this particular decision.
    pub may_decide: bool,
}

/// Why the queue is empty. Required whenever it is: see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyBecause<'a> {
    /// Plans are being produced and policy is passing them; none is waiting. This is
    /// the only variant that means what a calm queue looks like it means.
    NothingPending,
    /// No plan has been produced to decide on, and this is what is not producing one.
    NoPlanProduced { because: &'a str },
    /// Plans were produced and policy denied every one. Nothing is waiting on a human
    /// because nothing was allowed to reach one.
    AllDenied { reason: &'a str, count: usize },
}

impl EmptyBecause<'_> {
    /// The sentence the panel shows under an empty queue.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            EmptyBecause::NothingPending => "Nothing is waiting on a decision.".to_owned(),
            EmptyBecause::NoPlanProduced { because } => format!(
                "Empty because no plan has been produced to decide on: {because}. \
                 This is not the same as nothing needing a decision."
            ),
            EmptyBecause::AllDenied { reason, count } => format!(
                "Empty because policy denied every plan ({count} so far); the most \
                 recent reason was {reason}. Nothing reached a human."
            ),
        }
    }

    /// Whether an empty queue here is evidence that nothing needs deciding.
    ///
    /// Only [`EmptyBecause::NothingPending`] is. The panel colours the other two as
    /// warnings for exactly this reason.
    #[must_use]
    pub fn means_nothing_to_decide(&self) -> bool {
        matches!(self, EmptyBecause::NothingPending)
    }
}

/// What the queue's order actually is.
///
/// An operator scanning a list top to bottom is reading an ordering claim. If the
/// priority tie-break is inactive because nothing computes risk scores, saying so is
/// cheaper than letting them believe the most urgent item is at the top for two reasons
/// when it is there for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOrder<'a> {
    /// Time remaining ascending, then priority descending.
    TimeThenPriority,
    /// Time remaining only; the priority tie-break needs a threat score.
    TimeOnly { priority: Unavailable<'a> },
}

/// Everything PN-06 draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApprovalQueueView<'a> {
    /// Ordered by the caller via `gungnir_command::queue::order_queue`. The panel does
    /// not re-order, so the queue an operator sees is the queue the workflow holds.
    pub rows: &'a [QueueRow<'a>],
    pub order: QueueOrder<'a>,
    pub empty_because: EmptyBecause<'a>,
    pub selected: Option<PendingId>,
    /// Whether the signed-in role holds `DECIDE_PLAN` at all.
    pub may_decide: bool,
    pub role: &'a str,
    /// Every handoff this desktop has issued (GAP-040).
    ///
    /// The whole list, not a pre-filtered one: the panel applies
    /// `handoff::stays_visible` itself, so PN-06's rule is enforced in one place rather
    /// than trusted to each caller. A caller that filtered on `needs_attention` would
    /// drop every manual handoff, which is the failure the rule exists to prevent.
    pub handoffs: &'a [HandoffRow<'a>],
    /// The mission clock, for the age of anything still waiting on an endpoint.
    pub now: MissionTime,
}

/// Render the queue. Returns the item the operator clicked this frame, if any.
pub fn render_approval_queue(ui: &mut Ui, view: &ApprovalQueueView<'_>) -> Option<PendingId> {
    ui.heading("Approval queue");

    if !view.may_decide {
        ui.label(
            RichText::new(format!(
                "{} holds no decision authority; this queue is read-only.",
                view.role
            ))
            .color(theme::MUTED_TEXT_COLOR),
        );
    }

    let mut clicked = None;
    if view.rows.is_empty() {
        draw_empty(ui, view.empty_because);
    } else {
        ui.label(RichText::new(format!("{} waiting", view.rows.len())).color(theme::WARNING_COLOR));
        draw_order(ui, view.order);
        egui::Grid::new("approval_queue")
            .striped(true)
            .show(ui, |ui| {
                draw_header(ui);
                for row in view.rows {
                    if draw_row(ui, row, view.selected == Some(row.id)) {
                        clicked = Some(row.id);
                    }
                }
            });
    }
    // Drawn after both branches on purpose: see `draw_undelivered`.
    draw_undelivered(ui, view);
    clicked
}

/// What the order an operator is scanning actually is.
fn draw_order(ui: &mut Ui, order: QueueOrder<'_>) {
    match order {
        QueueOrder::TimeThenPriority => {
            ui.label(
                RichText::new("Ordered by time remaining, then priority.")
                    .color(theme::MUTED_TEXT_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
        }
        QueueOrder::TimeOnly { priority } => {
            ui.label(
                RichText::new("Ordered by time remaining only.")
                    .color(theme::MUTED_TEXT_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
            draw_unavailable(ui, priority);
        }
    }
}

/// PN-06's *stays visible until delivered* rule (GAP-040, DN-07 §5).
///
/// Drawn outside the empty/non-empty branch, and that placement is the point. The queue
/// empties at the instant a decision is recorded, so a shift that has decided everything
/// in front of it sees an empty queue -- and if this section were inside the non-empty
/// branch, the handoff that most needs carrying would be the one nobody could see.
///
/// The section is silent when nothing is waiting. A standing "all handoffs delivered"
/// line on a build where nothing can be delivered would be read past within a shift, and
/// the day it disappeared nobody would notice.
fn draw_undelivered(ui: &mut Ui, view: &ApprovalQueueView<'_>) {
    let mut waiting = view
        .handoffs
        .iter()
        .filter(|row| handoff::stays_visible(row.delivery))
        .peekable();
    if waiting.peek().is_none() {
        return;
    }
    ui.separator();
    ui.strong("Decided, not yet delivered");
    ui.label(
        RichText::new(
            "A decision leaves the queue when it is recorded. It leaves this list only \
             when an effector has it -- including the ones that travel by voice, which \
             need no chasing and are not finished either.",
        )
        .color(theme::MUTED_TEXT_COLOR)
        .size(theme::SMALL_FONT_SIZE),
    );
    for row in waiting {
        draw_undelivered_row(ui, row, view.now);
    }
}

/// One handoff still owed to somebody.
///
/// A block of wrapping labels rather than a grid row. The delivery sentence is a sentence
/// -- it has to be, because "manual" on its own is the word an operator reads as done --
/// and a column wide enough for it pushes the columns after it out of a docked pane,
/// where egui stops painting them and the panel silently loses its last field.
fn draw_undelivered_row(ui: &mut Ui, row: &HandoffRow<'_>, now: MissionTime) {
    // The endpoint's own name where there is one, and the act where there is not: an
    // operator reading this is looking for what they have to do next.
    let destination = row
        .endpoint
        .map_or_else(|| "by radio call".to_owned(), |e| format!("to {e}"));
    ui.label(
        RichText::new(format!(
            "Decision {} (plan #{}) {destination}, issued T+{:.0} s",
            row.decision, row.plan, row.issued.0
        ))
        .strong(),
    );
    ui.label(
        RichText::new(handoff::delivery_sentence(row, now))
            .color(handoff::delivery_color(row.delivery)),
    );
    ui.add_space(theme::ROW_SPACING);
}

/// The empty case, which is the one this panel exists to get right.
fn draw_empty(ui: &mut Ui, because: EmptyBecause<'_>) {
    let text = RichText::new(because.sentence());
    if because.means_nothing_to_decide() {
        ui.label(text.color(theme::MUTED_TEXT_COLOR));
    } else {
        // Not muted: an operator scanning a quiet screen has to see that the quiet is
        // upstream silence rather than an absence of work.
        ui.label(text.color(theme::WARNING_COLOR));
    }
}

fn draw_header(ui: &mut Ui) {
    for heading in [
        "Plan",
        "Tasks",
        "Verdict",
        "Time left",
        "Authority",
        "Escalation",
    ] {
        ui.strong(heading);
    }
    ui.end_row();
}

fn draw_row(ui: &mut Ui, row: &QueueRow<'_>, selected: bool) -> bool {
    let clicked = ui
        .selectable_label(selected, format!("#{}", row.plan_id))
        .clicked();
    ui.label(row.assignments.to_string());
    ui.label(RichText::new(row.verdict.label()).color(row.verdict.color()));
    draw_time_remaining(ui, row.time_remaining);
    draw_authority(ui, row);
    match row.escalated_from {
        Some(from) => {
            ui.label(RichText::new(format!("escalated from {from}")).color(theme::WARNING_COLOR));
        }
        None => {
            ui.label(RichText::new("--").color(theme::MUTED_TEXT_COLOR));
        }
    }
    ui.end_row();
    clicked
}

fn draw_time_remaining(ui: &mut Ui, remaining: TimeRemaining<'_>) {
    match remaining {
        TimeRemaining::Seconds(s) => {
            #[allow(clippy::cast_possible_truncation)]
            let s32 = s as f32;
            let color = if s32 <= theme::TIME_REMAINING_CRITICAL_S {
                theme::CLASS_HOSTILE_COLOR
            } else if s32 <= theme::TIME_REMAINING_WARN_S {
                theme::WARNING_COLOR
            } else {
                theme::MUTED_TEXT_COLOR
            };
            ui.label(theme::numeral(format!("{s:.0} s")).color(color).strong());
        }
        TimeRemaining::NoExpiryConfigured => {
            // Not a blank and not a warning: this is a setting the deployment chose,
            // and DN-10 makes it mean the item is preserved until somebody decides it.
            ui.label(RichText::new("no expiry (preserved)").color(theme::MUTED_TEXT_COLOR));
        }
        TimeRemaining::NotTracked { gap } => {
            ui.label(RichText::new(format!("not tracked ({gap})")).color(theme::MUTED_TEXT_COLOR));
        }
    }
}

fn draw_authority(ui: &mut Ui, row: &QueueRow<'_>) {
    if !row.may_decide {
        ui.label(RichText::new("must go up").color(theme::WARNING_COLOR));
    } else if row.pre_delegated {
        // Named rather than hidden: a pre-delegated case still produces a record, and
        // an operator should know which of their decisions were pre-authorized.
        ui.label(RichText::new("pre-delegated").color(theme::CLASS_NEUTRAL_COLOR));
    } else {
        ui.label(RichText::new("yours to decide").color(theme::MUTED_TEXT_COLOR));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the panel exists to preserve. Only one of the three ways to be
    /// empty means what a calm queue looks like it means.
    #[test]
    fn only_one_kind_of_empty_means_nothing_needs_deciding() {
        assert!(EmptyBecause::NothingPending.means_nothing_to_decide());
        assert!(!EmptyBecause::NoPlanProduced {
            because: "the allocator has produced no assignment"
        }
        .means_nothing_to_decide());
        assert!(!EmptyBecause::AllDenied {
            reason: "EmptyPlan",
            count: 12
        }
        .means_nothing_to_decide());
    }

    /// Each explanation has to carry the fact that distinguishes it, or the three
    /// collapse back into one sentence on screen.
    #[test]
    fn each_empty_explanation_names_its_cause() {
        let no_plan = EmptyBecause::NoPlanProduced {
            because: "the allocator has produced no assignment",
        };
        assert!(no_plan.sentence().contains("allocator"));
        assert!(
            no_plan.sentence().contains("not the same"),
            "the sentence has to say what it is not, or it reads as reassurance"
        );

        let denied = EmptyBecause::AllDenied {
            reason: "EmptyPlan",
            count: 7,
        };
        assert!(denied.sentence().contains("EmptyPlan"));
        assert!(denied.sentence().contains('7'));
    }

    /// "Not tracked" and "no expiry configured" are different claims: the first is
    /// this build's limitation, the second is a decision the deployment took that
    /// means the item is preserved indefinitely (DN-10).
    #[test]
    fn an_untracked_countdown_is_not_an_absent_deadline() {
        let untracked: TimeRemaining<'_> = TimeRemaining::NotTracked { gap: "GAP-034" };
        assert_ne!(untracked, TimeRemaining::NoExpiryConfigured);
        match untracked {
            TimeRemaining::NotTracked { gap } => assert!(gap.starts_with("GAP-")),
            _ => panic!("variant changed"),
        }
    }

    /// A denied plan never reaches the queue, so a row can never carry a denial.
    /// This is `ApprovalWorkflow::submit_for_approval`'s invariant restated where the
    /// panel would be the place to violate it.
    #[test]
    fn a_queued_row_is_never_a_denied_one() {
        let row = QueueRow {
            id: PendingId(1),
            plan_id: 4,
            assignments: 2,
            verdict: Verdict::RequiresHumanApproval,
            time_remaining: TimeRemaining::Seconds(12.0),
            pre_delegated: false,
            may_decide: true,
            escalated_from: None,
        };
        assert!(
            !matches!(row.verdict, Verdict::Denied { .. }),
            "the workflow refuses a denied plan before it can be queued"
        );
    }
}
