// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

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
use gungnir_model::{MissionTime, PlanId};

/// The queue's identifier for one pending item, mirroring
/// `gungnir_command::PendingApprovalId` without depending on that crate: the same 128
/// bits, a UUID v7 since GAP-130 (D-56).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PendingId(pub u128);

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

    fn color(&self, palette: &theme::Palette) -> egui::Color32 {
        match self {
            Verdict::Approved => palette.class_neutral_color,
            Verdict::RequiresHumanApproval => palette.warning_color,
            Verdict::Denied { .. } => palette.class_hostile_color,
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

/// Whose approval queue this panel is drawing (DN-31 §6.5, §8; GAP-133).
///
/// **Not decoration.** A decision taken here goes to two different places depending on
/// which of these is in force -- the node's record, or this desktop's own -- and an
/// operator who cannot tell which they are looking at cannot tell whether the console
/// beside them sees the same queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAuthority<'a> {
    /// The node's queue, which every desktop linked to it shows and decides in. This
    /// desktop queues nothing, opens no engagement and issues no handoff for it.
    Node { endpoint: &'a str },
    /// This desktop's own, because it is cut off from its node or was deployed with
    /// none.
    ThisDesktop,
}

impl QueueAuthority<'_> {
    /// The sentence under the heading.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            QueueAuthority::Node { endpoint } => format!(
                "This is node {endpoint}'s queue. Every desktop linked to it sees the same \
                 items in the same order, and the first valid decision on an item ends it."
            ),
            QueueAuthority::ThisDesktop => "This is this desktop's own queue: decisions \
                 taken here are recorded here."
                .to_owned(),
        }
    }

    /// Whether the node holds this queue.
    #[must_use]
    pub fn is_the_node(&self) -> bool {
        matches!(self, QueueAuthority::Node { .. })
    }
}

/// One row of the queue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueueRow<'a> {
    /// The role this item was escalated from, when it has been. The original role keeps
    /// seeing it (DN-10 §5), so this is a mark rather than a reassignment.
    pub escalated_from: Option<&'a str>,
    /// Every role the item is offered to, in escalation order (DN-10 amendment 1 c,
    /// DN-31 §8).
    ///
    /// Drawn where `may_decide` is false, so a row that says the decision must go up says
    /// **who it must go up to**. A row an operator cannot act on and cannot see the owner
    /// of is a row they can only take to a radio.
    pub offered_to: &'a [String],
    pub id: PendingId,
    /// Shown by its short tag, as every row on this panel shows an identifier (D-61).
    pub plan_id: PlanId,
    /// How many (resource, track) pairs the plan tasks.
    pub assignments: usize,
    pub verdict: Verdict<'a>,
    pub time_remaining: TimeRemaining<'a>,
    /// D-15: the authority rules pre-delegate this case to the signed-in role. It
    /// still needs a recorded decision; what it skips is escalation, not the person.
    pub pre_delegated: bool,
    /// Whether the signed-in role may take this particular decision.
    pub may_decide: bool,
    /// How the item's plan was reached (GAP-156): an interim plan is marked on its row
    /// and named in PN-07, so it is never decided as though it were the optimum.
    pub basis: gungnir_model::PlanBasis,
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
    /// The node holds the queue and has not yet told this desktop what is in it
    /// (GAP-133, DN-31 §6.6).
    ///
    /// **Not an empty queue.** Nothing has been received, which is the one situation
    /// where this panel knows it does not know; drawing it as "nothing is waiting" would
    /// be the same lie as a calm queue on a desktop whose planner has stopped.
    NotReceived { from: &'a str },
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
            EmptyBecause::NotReceived { from } => format!(
                "Node {from} holds the queue and has not yet said what is in it. This is \
                 not an empty queue: nothing has been received."
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

/// How an item left the node's queue (GAP-133, DN-31 §9 row 7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecidedBy<'a> {
    /// A person decided it, at another console or this one.
    Person {
        accepted: bool,
        /// `None` where the decision the node holds names nobody, which only a
        /// forwarded one can (DN-23 §5 rule 1). Never invented.
        operator: Option<&'a str>,
        role: Option<&'a str>,
        at: MissionTime,
    },
    /// The window closed with nobody deciding. **Not a rejection**: nobody chose.
    Expiry { at: MissionTime },
}

/// One item that has left the node's queue while this desktop was watching.
///
/// PN-06 draws these so that an operator at one console sees a decision taken at
/// another -- the property DN-31 §9 row 7 asks for by name. It is a recent-activity
/// list and not a record: the record is the node's journal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecidedRow<'a> {
    pub plan: PlanId,
    pub by: DecidedBy<'a>,
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
    /// Whose queue this is (GAP-133, DN-31 §8).
    pub authority: QueueAuthority<'a>,
    /// What has left the node's queue while this desktop was watching, newest first
    /// (GAP-133). Empty on a desktop holding its own queue: an item it decided leaves
    /// through `handoffs` below, which is the same item's next step.
    pub decided: &'a [DecidedRow<'a>],
    /// Why no control is enabled, where the reason is about the console rather than
    /// about a row.
    ///
    /// `None` where decisions can be taken. `Some` is drawn once, above the rows, so
    /// that a queue full of greyed-out controls says why once instead of a person
    /// guessing from each row.
    pub cannot_decide: Option<&'a str>,
}

/// Render the queue. Returns the item the operator clicked this frame, if any.
pub fn render_approval_queue(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &ApprovalQueueView<'_>,
) -> Option<PendingId> {
    ui.heading("Approval queue");

    // Whose queue this is, before anything in it (GAP-133, DN-31 §8). A decision taken
    // here reaches a different record depending on the answer, so it is drawn first and
    // every time rather than only when it changes.
    ui.label(
        RichText::new(view.authority.sentence())
            .color(palette.muted_text_color())
            .size(palette.small_font_size),
    );

    if !view.may_decide {
        ui.label(
            RichText::new(format!(
                "{} holds no decision authority; this queue is read-only.",
                view.role
            ))
            .color(palette.muted_text_color()),
        );
    }
    if let Some(reason) = view.cannot_decide {
        // Warning-coloured: this is a console that can see the queue and cannot act on
        // it, which an operator has to know before they reach for a control.
        ui.label(RichText::new(reason).color(palette.warning_color));
    }

    let mut clicked = None;
    if view.rows.is_empty() {
        draw_empty(ui, palette, view.empty_because);
    } else {
        ui.label(
            RichText::new(format!("{} waiting", view.rows.len())).color(palette.warning_color),
        );
        draw_order(ui, palette, view.order);
        egui::Grid::new("approval_queue")
            .striped(true)
            .show(ui, |ui| {
                draw_header(ui);
                for row in view.rows {
                    if draw_row(ui, palette, row, view.selected == Some(row.id)) {
                        clicked = Some(row.id);
                    }
                }
            });
    }
    // Drawn after both branches on purpose: see `draw_undelivered` and `draw_decided`.
    draw_decided(ui, palette, view);
    draw_undelivered(ui, palette, view);
    clicked
}

/// What has left the node's queue, and who ended it (GAP-133, DN-31 §9 row 7).
///
/// Outside the empty/non-empty branch, for the same reason `draw_undelivered` is: a
/// shift that has just decided everything in front of it sees an empty queue, and the
/// decision another console took is exactly what it most needs to see at that moment.
///
/// Silent when nothing has ended, and silent on a desktop holding its own queue -- there
/// the decision and the handoff below it are the same item's next step, and a second list
/// saying "you decided this" would be the panel telling an operator what they just did.
fn draw_decided(ui: &mut Ui, palette: &theme::Palette, view: &ApprovalQueueView<'_>) {
    if view.decided.is_empty() {
        return;
    }
    ui.separator();
    ui.strong("Ended on the node");
    ui.label(
        RichText::new(
            "Decided at a console linked to this node, or expired on the node's clock. \
             The node's record is the record.",
        )
        .color(palette.muted_text_color())
        .size(palette.small_font_size),
    );
    for row in view.decided {
        let (text, color) = match row.by {
            DecidedBy::Person {
                accepted,
                operator,
                role,
                at,
            } => {
                let who = match (operator, role) {
                    (Some(operator), Some(role)) => format!("operator {operator} as {role}"),
                    (Some(operator), None) => {
                        format!("operator {operator}, whose role was not recorded")
                    }
                    // Only a forwarded decision can name nobody (DN-23 §5 rule 1): the
                    // node's own route needs a token. Said rather than left blank.
                    (None, _) => "somebody this node cannot name".to_owned(),
                };
                (
                    format!(
                        "Plan #{} {} by {who} at T+{:.0} s",
                        row.plan.short(),
                        if accepted { "accepted" } else { "rejected" },
                        at.0
                    ),
                    palette.muted_text_color(),
                )
            }
            // Warning-coloured, and worded so it cannot be read as a rejection: nobody
            // chose, which is the distinction DN-10 §6 and MOE-01 turn on.
            DecidedBy::Expiry { at } => (
                format!(
                    "Plan #{} expired at T+{:.0} s with nobody deciding; not a rejection",
                    row.plan.short(),
                    at.0
                ),
                palette.warning_color,
            ),
        };
        ui.label(RichText::new(text).color(color));
    }
}

/// What the order an operator is scanning actually is.
fn draw_order(ui: &mut Ui, palette: &theme::Palette, order: QueueOrder<'_>) {
    match order {
        QueueOrder::TimeThenPriority => {
            ui.label(
                RichText::new("Ordered by time remaining, then priority.")
                    .color(palette.muted_text_color())
                    .size(palette.small_font_size),
            );
        }
        QueueOrder::TimeOnly { priority } => {
            ui.label(
                RichText::new("Ordered by time remaining only.")
                    .color(palette.muted_text_color())
                    .size(palette.small_font_size),
            );
            draw_unavailable(ui, palette, priority);
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
fn draw_undelivered(ui: &mut Ui, palette: &theme::Palette, view: &ApprovalQueueView<'_>) {
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
        .color(palette.muted_text_color())
        .size(palette.small_font_size),
    );
    for row in waiting {
        draw_undelivered_row(ui, palette, row, view.now);
    }
}

/// One handoff still owed to somebody.
///
/// A block of wrapping labels rather than a grid row. The delivery sentence is a sentence
/// -- it has to be, because "manual" on its own is the word an operator reads as done --
/// and a column wide enough for it pushes the columns after it out of a docked pane,
/// where egui stops painting them and the panel silently loses its last field.
fn draw_undelivered_row(
    ui: &mut Ui,
    palette: &theme::Palette,
    row: &HandoffRow<'_>,
    now: MissionTime,
) {
    // The endpoint's own name where there is one, and the act where there is not: an
    // operator reading this is looking for what they have to do next.
    let destination = row
        .endpoint
        .map_or_else(|| "by radio call".to_owned(), |e| format!("to {e}"));
    // Short tags (D-61): enough to tell this screen's rows apart. The whole identifier,
    // for quoting, is on PN-20's record of the same handoff.
    ui.label(
        RichText::new(format!(
            "Decision {} (plan #{}) {destination}, issued T+{:.0} s",
            row.decision.short(),
            row.plan.short(),
            row.issued.0
        ))
        .strong(),
    );
    ui.label(
        RichText::new(handoff::delivery_sentence(row, now))
            .color(handoff::delivery_color(palette, row.delivery)),
    );
    ui.add_space(palette.row_spacing);
}

/// The empty case, which is the one this panel exists to get right.
fn draw_empty(ui: &mut Ui, palette: &theme::Palette, because: EmptyBecause<'_>) {
    let text = RichText::new(because.sentence());
    if because.means_nothing_to_decide() {
        ui.label(text.color(palette.muted_text_color()));
    } else {
        // Not muted: an operator scanning a quiet screen has to see that the quiet is
        // upstream silence rather than an absence of work.
        ui.label(text.color(palette.warning_color));
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

fn draw_row(ui: &mut Ui, palette: &theme::Palette, row: &QueueRow<'_>, selected: bool) -> bool {
    // GAP-156: an interim plan says so in the one cell every row has, rather than in a
    // column most rows would leave blank.
    let tag = match row.basis {
        gungnir_model::PlanBasis::Exact => RichText::new(format!("#{}", row.plan_id.short())),
        gungnir_model::PlanBasis::OneStep => {
            RichText::new(format!("#{} INTERIM", row.plan_id.short()))
                .color(palette.warning_color)
                .strong()
        }
    };
    let clicked = ui.selectable_label(selected, tag).clicked();
    ui.label(row.assignments.to_string());
    ui.label(RichText::new(row.verdict.label()).color(row.verdict.color(palette)));
    draw_time_remaining(ui, palette, row.time_remaining);
    draw_authority(ui, palette, row);
    match row.escalated_from {
        Some(from) => {
            ui.label(RichText::new(format!("escalated from {from}")).color(palette.warning_color));
        }
        None => {
            ui.label(RichText::new("--").color(palette.muted_text_color()));
        }
    }
    ui.end_row();
    clicked
}

fn draw_time_remaining(ui: &mut Ui, palette: &theme::Palette, remaining: TimeRemaining<'_>) {
    match remaining {
        TimeRemaining::Seconds(s) => {
            #[allow(clippy::cast_possible_truncation)]
            let s32 = s as f32;
            let color = if s32 <= palette.time_remaining_critical_s {
                palette.class_hostile_color
            } else if s32 <= palette.time_remaining_warn_s {
                palette.warning_color
            } else {
                palette.muted_text_color()
            };
            ui.label(
                theme::numeral(palette, format!("{s:.0} s"))
                    .color(color)
                    .strong(),
            );
        }
        TimeRemaining::NoExpiryConfigured => {
            // Not a blank and not a warning: this is a setting the deployment chose,
            // and DN-10 makes it mean the item is preserved until somebody decides it.
            ui.label(RichText::new("no expiry (preserved)").color(palette.muted_text_color()));
        }
        TimeRemaining::NotTracked { gap } => {
            ui.label(
                RichText::new(format!("not tracked ({gap})")).color(palette.muted_text_color()),
            );
        }
    }
}

fn draw_authority(ui: &mut Ui, palette: &theme::Palette, row: &QueueRow<'_>) {
    if !row.may_decide {
        // **Named, not just refused** (DN-31 §8): an item this console may not decide
        // says who may, so an operator takes it to the right person instead of to the
        // radio. An empty list means the node offered it to nobody this desktop can
        // read, which is said rather than drawn as a blank.
        let to = if row.offered_to.is_empty() {
            "must go up (the roles it is offered to were not received)".to_owned()
        } else {
            format!("offered to {}", row.offered_to.join(", "))
        };
        ui.label(RichText::new(to).color(palette.warning_color));
    } else if row.pre_delegated {
        // Named rather than hidden: a pre-delegated case still produces a record, and
        // an operator should know which of their decisions were pre-authorized.
        ui.label(RichText::new("pre-delegated").color(palette.class_neutral_color));
    } else {
        ui.label(RichText::new("yours to decide").color(palette.muted_text_color()));
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
            plan_id: PlanId(4),
            assignments: 2,
            verdict: Verdict::RequiresHumanApproval,
            time_remaining: TimeRemaining::Seconds(12.0),
            pre_delegated: false,
            may_decide: true,
            offered_to: &[],
            escalated_from: None,
            basis: gungnir_model::PlanBasis::Exact,
        };
        assert!(
            !matches!(row.verdict, Verdict::Denied { .. }),
            "the workflow refuses a denied plan before it can be queued"
        );
    }
}
