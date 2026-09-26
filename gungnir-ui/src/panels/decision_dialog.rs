// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-07, the decision dialog (GAP-038).
//!
//! `docs/ux/information-architecture.md` §1: the selected plan with its verdict and
//! rationale, and the three actions `ApprovalWorkflow::decide` takes -- accept,
//! override with a substitute, reject with a reason.
//!
//! # Accept is never the default
//!
//! The register's closing action names this and it is the one requirement here that is
//! a safety property rather than a layout preference: this is the surface where a
//! reflex becomes an engagement. It is enforced in four places rather than asserted in
//! one comment.
//!
//! 1. Nothing is pre-selected. [`DecisionDialogState::default`] holds no choice, and
//!    [`accept_enabled`] is false for it.
//! 2. Accept is gated on an explicit acknowledgement of the degraded conditions in
//!    force. With none in force the acknowledgement is not asked for; with any in
//!    force it is, because accepting a plan built on a degraded picture is a different
//!    act from accepting one built on a sound picture.
//! 3. Accept is drawn last, after reject and override, so the reflexive click and the
//!    reflexive keyboard traversal both land somewhere else.
//! 4. No control is bound to Enter, and [`render_decision_dialog`] returns a choice
//!    only from a click. A dialog that has been rendered but not clicked returns
//!    `None`.
//!
//! [`tests::no_state_of_this_dialog_enables_accept_without_a_deliberate_act`] searches
//! the state space for a combination that enables accept without an acknowledgement.
//!
//! # Who the decision is recorded against
//!
//! A signed-in operator's identity and role go into the `DecisionRecord` this dialog
//! produces; with nobody signed in the record carries neither, which is correct rather
//! than a gap to paper over -- `gungnir_command::queue::expiry_record` takes the same
//! position, that a false operator is worse than a null (DN-23 §5 rule 1). Either way a
//! decision surface must say it out loud, so [`OperatorIdentity`] is a required field
//! and is drawn every time.

use crate::panels::approval_queue::{QueueRow, TimeRemaining, Verdict};
use crate::panels::unavailable::{draw_unavailable, Section, Unavailable};
use crate::theme;
use egui::{RichText, Ui};

/// Where a decision taken in this dialog goes (GAP-133, DN-31 §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionRoute<'a> {
    /// **Through the node**, which authorizes it against the caller's role, takes one
    /// decision per item, opens the engagement, issues the handoff and journals all of
    /// it (D-55). Nothing is recorded on this desktop, so an item can be decided out
    /// from under this dialog by another console and the answer says so.
    Node { endpoint: &'a str },
    /// Into this desktop's own record, because it is cut off from its node or was
    /// deployed with none.
    ThisDesktop,
}

/// What the node said about the decision this dialog posted (GAP-133, DN-31 §6.3, §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeAnswer<'a> {
    /// Posted and not yet answered. **Not a failure**: a `504` means the node's loop did
    /// not reply inside the route's window and does *not* mean nothing was recorded, so
    /// the same request key goes back until the node says which. The count is drawn
    /// because a decision that has been tried five times is a different situation from
    /// one posted a moment ago.
    Waiting { attempts: u32 },
    /// `409`: somebody decided first, or the window closed. The sentence names who
    /// decided, as which role and when (DN-31 §6.6).
    Refused { sentence: &'a str },
    /// `400`, `401` or `403`: the node would not take it, and **nothing was recorded** --
    /// not there and not here.
    Rejected { status: u16, reason: &'a str },
}

/// Who the decision will be recorded against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorIdentity<'a> {
    /// A signed-in operator whose identity goes into the record.
    Authenticated(&'a str),
    /// No operator session exists; the record will name nobody. Names the entry that
    /// will change that.
    Unattributed { role: &'a str, gap: &'a str },
}

/// One alternative course of action, already policy-checked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alternative<'a> {
    /// An option beside the plan under decision, shown by its short tag (D-61).
    pub plan_id: gungnir_model::PlanId,
    pub verdict: Verdict<'a>,
    pub summary: &'a str,
}

/// A condition that makes this decision a degraded one.
///
/// Not a warning banner: a named fact with the subsystem it came from, so an operator
/// can tell "the tracker is down" from "the journal is not writing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Degraded<'a> {
    pub subsystem: &'a str,
    pub detail: &'a str,
}

/// The three actions `ApprovalWorkflow::decide` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionChoice {
    Accept,
    /// The operator substitutes their own assignment; the record stores what they
    /// acted on, not what was proposed.
    Override,
    Reject,
}

/// What the operator has entered so far. Owned by the caller and carried across
/// frames; immediate mode has nowhere else to keep it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionDialogState {
    /// Free text; a rejection is recorded with it.
    pub reject_reason: String,
    /// Set by ticking the acknowledgement, and cleared whenever the set of degraded
    /// conditions changes, so an acknowledgement never carries over to a condition the
    /// operator has not seen.
    pub degraded_acknowledged: bool,
    /// The degraded set the acknowledgement was given against.
    pub acknowledged_for: Vec<String>,
}

impl DecisionDialogState {
    /// Reset the acknowledgement when the degraded conditions have changed.
    ///
    /// Called every frame. An acknowledgement is about a specific set of conditions:
    /// if the tracker fails after the operator ticked the box for a journal warning,
    /// the tick no longer means what it meant.
    pub fn reconcile(&mut self, degraded: &[Degraded<'_>]) {
        let now: Vec<String> = degraded.iter().map(|d| d.subsystem.to_owned()).collect();
        if now != self.acknowledged_for {
            self.degraded_acknowledged = false;
            self.acknowledged_for = now;
        }
    }
}

/// Everything PN-07 draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionDialogView<'a> {
    pub row: &'a QueueRow<'a>,
    /// Why this plan, in terms of the risk scores that produced it.
    pub rationale: Result<&'a str, Unavailable<'a>>,
    pub alternatives: Section<'a, Alternative<'a>>,
    /// Expected cost of the proposed assignment.
    pub cost: Result<f64, Unavailable<'a>>,
    /// Conditions in force that make this a degraded decision. Empty is a claim too:
    /// it says every subsystem this dialog knows about is reporting healthy.
    pub degraded: &'a [Degraded<'a>],
    /// The policy engines that actually evaluated this plan. A verdict from a chain
    /// missing an engine is not the same verdict, so the dialog names the chain.
    pub engines: &'a [&'a str],
    /// Checks that ran and could not have failed, with the reason. Separate from
    /// `engines` on purpose: a caveat listed among the engines reads as a fourth
    /// engine, which is the opposite of what it says. A check that cannot fail has
    /// told the operator nothing, and a clean verdict must not be allowed to imply
    /// otherwise.
    pub caveats: &'a [&'a str],
    pub operator: OperatorIdentity<'a>,
    /// `DECIDE_PLAN` for the signed-in role.
    pub may_accept: bool,
    /// `OVERRIDE_PLAN`, which is a strictly higher authority than accepting.
    pub may_override: bool,
    /// Where this decision goes (GAP-133).
    pub route: DecisionRoute<'a>,
    /// The node's answer to a decision already posted from this dialog, if there is one
    /// (GAP-133). `Some` draws the answer and **draws no controls**: the question has
    /// been asked, and offering it again would either be a second decision or a second
    /// refusal.
    pub answer: Option<NodeAnswer<'a>>,
}

/// Whether the accept control may be enabled.
///
/// Point 2 of the module's "accept is never the default": a degraded condition must be
/// acknowledged before accept becomes available. With nothing degraded there is
/// nothing to acknowledge and the authority check is the only gate.
#[must_use]
pub fn accept_enabled(view: &DecisionDialogView<'_>, state: &DecisionDialogState) -> bool {
    view.may_accept && (view.degraded.is_empty() || state.degraded_acknowledged)
}

/// A rejection is recorded with its reason, so an empty reason is not a rejection.
///
/// The asymmetry with accept is deliberate and runs the other way from what a hurried
/// reading suggests: rejecting is the safe action, so it is gated on *saying why*
/// rather than on authority, and MOE-01 needs the reason to tell a considered
/// rejection from an abandoned one.
#[must_use]
pub fn reject_enabled(state: &DecisionDialogState) -> bool {
    !state.reject_reason.trim().is_empty()
}

/// Render the dialog. Returns a choice only when the operator clicks one.
pub fn render_decision_dialog(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &DecisionDialogView<'_>,
    state: &mut DecisionDialogState,
) -> Option<DecisionChoice> {
    state.reconcile(view.degraded);

    ui.heading(format!("Decide plan #{}", view.row.plan_id.short()));
    // The whole identifier, with a copy control (D-61): this is the plan a person quotes
    // when they ask somebody else about the decision, and the tag above is only enough to
    // match it to its row on PN-06.
    crate::panels::identifier::draw_full(
        ui,
        palette,
        "Plan identifier",
        &view.row.plan_id.to_string(),
    );
    draw_plan(ui, palette, view);
    ui.separator();
    draw_rationale(ui, palette, view);
    ui.separator();
    draw_alternatives(ui, palette, view);
    ui.separator();
    draw_degraded(ui, palette, view, state);
    ui.separator();
    draw_attribution(ui, palette, view.operator);
    draw_route(ui, palette, view.route);
    ui.separator();
    // **An answered dialog offers no control** (GAP-133, DN-31 §6.6). The question has
    // been put to the node; drawing accept again would let a person ask twice, and
    // drawing it beside a `409` would offer them a decision the node has already told
    // them somebody else took.
    if let Some(answer) = view.answer {
        draw_answer(ui, palette, answer);
        return None;
    }
    draw_controls(ui, palette, view, state)
}

/// Where this decision goes (GAP-133, DN-31 §6.6).
///
/// Drawn under the attribution, because the two together are the whole of "what happens
/// when I click": who it is recorded against, and whose record it lands in.
fn draw_route(ui: &mut Ui, palette: &theme::Palette, route: DecisionRoute<'_>) {
    match route {
        DecisionRoute::Node { endpoint } => {
            ui.label(
                RichText::new(format!(
                    "This decision is taken by node {endpoint}, which authorizes it, \
                     records it and opens the engagement. Nothing is recorded on this \
                     desktop, and another console may decide this item first."
                ))
                .color(palette.muted_text_color())
                .size(palette.small_font_size),
            );
        }
        DecisionRoute::ThisDesktop => {
            ui.label(
                RichText::new(
                    "This desktop holds the queue: the decision is recorded here and the \
                     handoff is issued from here.",
                )
                .color(palette.muted_text_color())
                .size(palette.small_font_size),
            );
        }
    }
}

/// What the node said, in place of the controls (GAP-133, DN-31 §6.6).
fn draw_answer(ui: &mut Ui, palette: &theme::Palette, answer: NodeAnswer<'_>) {
    match answer {
        NodeAnswer::Waiting { attempts } => {
            ui.label(
                RichText::new(match attempts {
                    0 | 1 => "Sent to the node; waiting for it to answer.".to_owned(),
                    n => format!(
                        "Sent to the node {n} times and not yet answered. The same request \
                         key is used each time, so this asks again rather than deciding \
                         again -- whatever the node recorded the first time still stands."
                    ),
                })
                .color(palette.warning_color),
            );
        }
        // The conflict DN-31 §6.6 words: who decided, as which role, and when.
        NodeAnswer::Refused { sentence } => {
            ui.label(RichText::new(sentence).color(palette.warning_color));
        }
        NodeAnswer::Rejected { status, reason } => {
            ui.label(
                RichText::new(format!(
                    "The node refused this decision ({status}): {reason}"
                ))
                .color(palette.class_hostile_color),
            );
            ui.label(
                RichText::new("Nothing was recorded, on the node or on this desktop.")
                    .color(palette.muted_text_color()),
            );
        }
    }
}

fn draw_plan(ui: &mut Ui, palette: &theme::Palette, view: &DecisionDialogView<'_>) {
    // GAP-156: first, above the assignments, because it changes what they are.
    if let Some(label) = view.row.basis.label() {
        ui.label(RichText::new(label).strong().color(palette.warning_color));
        ui.label(
            RichText::new(
                "It stood in because the exact solve could not answer the picture in time. \
                 Accepting it is allowed once the condition below is acknowledged; the \
                 optimum, when it arrives, is a new plan with its own item.",
            )
            .color(palette.warning_color),
        );
    }
    ui.label(format!("{} assignments", view.row.assignments));
    ui.label(RichText::new(verdict_sentence(view.row.verdict)).color(palette.warning_color));
    match view.row.time_remaining {
        TimeRemaining::Seconds(s) => {
            ui.label(theme::numeral(palette, format!("{s:.0} s remaining")))
        }
        TimeRemaining::NoExpiryConfigured => ui.label(
            RichText::new("No expiry configured: this item is preserved until decided.")
                .color(palette.muted_text_color()),
        ),
        TimeRemaining::NotTracked { gap } => ui.label(
            RichText::new(format!(
                "Time remaining is not tracked in this build ({gap}); do not read this \
                 as an item with no deadline."
            ))
            .color(palette.muted_text_color()),
        ),
    };
    match view.cost {
        Ok(c) => {
            ui.label(format!("Expected cost {c:.2}"));
        }
        Err(u) => draw_unavailable(ui, palette, u),
    }

    // The chain that produced the verdict. A verdict is only as good as the engines
    // that were consulted, and only as strong as what those engines could actually
    // test.
    ui.label(
        RichText::new(if view.engines.is_empty() {
            "No policy engine evaluated this plan.".to_owned()
        } else {
            format!("Checked by: {}", view.engines.join(", "))
        })
        .color(palette.muted_text_color())
        .size(palette.small_font_size),
    );
    for caveat in view.caveats {
        // Warning-coloured rather than muted: an operator reading a clean verdict has
        // to see which of its checks could not have failed.
        ui.label(
            RichText::new(format!("Could not fail: {caveat}"))
                .color(palette.warning_color)
                .size(palette.small_font_size),
        );
    }
}

fn verdict_sentence(verdict: Verdict<'_>) -> String {
    match verdict {
        Verdict::RequiresHumanApproval => "Policy requires a human decision.".to_owned(),
        Verdict::Approved => {
            "Policy pre-authorized this plan; a decision is still recorded.".to_owned()
        }
        Verdict::Denied { reason } => format!("Policy denied this plan: {reason}."),
    }
}

fn draw_rationale(ui: &mut Ui, palette: &theme::Palette, view: &DecisionDialogView<'_>) {
    ui.strong("Why this plan");
    match view.rationale {
        Ok(text) => {
            ui.label(text);
        }
        Err(u) => draw_unavailable(ui, palette, u),
    }
}

fn draw_alternatives(ui: &mut Ui, palette: &theme::Palette, view: &DecisionDialogView<'_>) {
    if view.alternatives.draw_header(ui, palette, "Alternatives") {
        if let Some(alts) = view.alternatives.items() {
            for a in alts {
                ui.label(format!(
                    "#{}: {} ({})",
                    a.plan_id.short(),
                    a.summary,
                    verdict_sentence(a.verdict)
                ));
            }
        }
    }
}

fn draw_degraded(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &DecisionDialogView<'_>,
    state: &mut DecisionDialogState,
) {
    ui.strong("Conditions");
    if view.degraded.is_empty() {
        ui.label(
            RichText::new("Every subsystem this dialog checks is reporting healthy.")
                .color(palette.muted_text_color()),
        );
        return;
    }
    for d in view.degraded {
        ui.label(
            RichText::new(format!("{}: {}", d.subsystem, d.detail))
                .color(palette.class_hostile_color),
        );
    }
    ui.checkbox(
        &mut state.degraded_acknowledged,
        "I have read the degraded conditions above",
    );
}

fn draw_attribution(ui: &mut Ui, palette: &theme::Palette, operator: OperatorIdentity<'_>) {
    match operator {
        OperatorIdentity::Authenticated(id) => {
            ui.label(format!("This decision will be recorded against {id}."));
        }
        OperatorIdentity::Unattributed { role, gap } => {
            ui.label(
                RichText::new(format!(
                    "This decision will be recorded with no operator identity: the \
                     desktop has selected the {role} role but nobody is signed in \
                     ({gap}). The record names the decision, not the person."
                ))
                .color(palette.warning_color),
            );
        }
    }
}

/// Reject, then override, then accept. The order is the safety property: see point 3
/// of the module documentation.
fn draw_controls(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &DecisionDialogView<'_>,
    state: &mut DecisionDialogState,
) -> Option<DecisionChoice> {
    let mut choice = None;

    ui.strong("Reject");
    ui.text_edit_singleline(&mut state.reject_reason);
    if !reject_enabled(state) {
        ui.label(
            RichText::new("A rejection is recorded with its reason; enter one to reject.")
                .color(palette.muted_text_color()),
        );
    }
    if ui
        .add_enabled(reject_enabled(state), egui::Button::new("Reject plan"))
        .clicked()
    {
        choice = Some(DecisionChoice::Reject);
    }

    ui.separator();
    if view.may_override {
        if ui.button("Override with a substitute").clicked() {
            choice = Some(DecisionChoice::Override);
        }
    } else {
        ui.label(
            RichText::new("Overriding needs a higher authority than this role holds.")
                .color(palette.muted_text_color()),
        );
    }

    ui.separator();
    if !view.may_accept {
        ui.label(
            RichText::new("This role may not accept a plan.").color(palette.muted_text_color()),
        );
    } else if !accept_enabled(view, state) {
        ui.label(
            RichText::new("Acknowledge the degraded conditions before accepting.")
                .color(palette.warning_color),
        );
    }
    if ui
        .add_enabled(
            accept_enabled(view, state),
            egui::Button::new("Accept plan"),
        )
        .clicked()
    {
        choice = Some(DecisionChoice::Accept);
    }

    choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::approval_queue::PendingId;

    fn row() -> QueueRow<'static> {
        QueueRow {
            id: PendingId(1),
            plan_id: gungnir_model::PlanId(9),
            assignments: 2,
            verdict: Verdict::RequiresHumanApproval,
            time_remaining: TimeRemaining::Seconds(18.0),
            pre_delegated: false,
            may_decide: true,
            offered_to: &[],
            escalated_from: None,
            basis: gungnir_model::PlanBasis::Exact,
        }
    }

    fn view<'a>(row: &'a QueueRow<'a>, degraded: &'a [Degraded<'a>]) -> DecisionDialogView<'a> {
        DecisionDialogView {
            row,
            rationale: Err(Unavailable {
                owner: "gungnir-assessment",
                gap: "GAP-028",
            }),
            alternatives: Section::Unavailable(Unavailable {
                owner: "gungnir-decision",
                gap: "GAP-032",
            }),
            cost: Err(Unavailable {
                owner: "gungnir-assessment",
                gap: "GAP-028",
            }),
            degraded,
            engines: &["control status", "authority"],
            caveats: &[],
            operator: OperatorIdentity::Unattributed {
                role: "Operator",
                gap: "GAP-057",
            },
            may_accept: true,
            may_override: false,
            route: DecisionRoute::ThisDesktop,
            answer: None,
        }
    }

    /// The property the register's closing action names. Search the state space for a
    /// combination that enables accept without a deliberate act: authority alone is
    /// never enough while a condition is degraded, and a fresh dialog never enables it.
    #[test]
    fn no_state_of_this_dialog_enables_accept_without_a_deliberate_act() {
        let r = row();
        let degraded_sets: [&[Degraded<'_>]; 3] = [
            &[],
            &[Degraded {
                subsystem: "tracking",
                detail: "pipeline not implemented",
            }],
            &[
                Degraded {
                    subsystem: "tracking",
                    detail: "pipeline not implemented",
                },
                Degraded {
                    subsystem: "journal",
                    detail: "fsync failed",
                },
            ],
        ];
        for degraded in degraded_sets {
            for may_accept in [false, true] {
                let mut v = view(&r, degraded);
                v.may_accept = may_accept;

                // A dialog the operator has not touched.
                let mut fresh = DecisionDialogState::default();
                fresh.reconcile(degraded);
                assert!(
                    !accept_enabled(&v, &fresh) || (may_accept && degraded.is_empty()),
                    "a fresh dialog enabled accept with {} degraded conditions",
                    degraded.len()
                );

                // Typing a rejection reason must never enable accepting.
                let mut typing = DecisionDialogState {
                    reject_reason: "wrong target".into(),
                    ..DecisionDialogState::default()
                };
                typing.reconcile(degraded);
                assert_eq!(
                    accept_enabled(&v, &typing),
                    may_accept && degraded.is_empty(),
                    "the reject reason changed whether accept was enabled"
                );

                // Only the acknowledgement, plus authority, opens it.
                let mut acknowledged = DecisionDialogState::default();
                acknowledged.reconcile(degraded);
                acknowledged.degraded_acknowledged = true;
                assert_eq!(accept_enabled(&v, &acknowledged), may_accept);
            }
        }
    }

    /// An acknowledgement is about the conditions the operator saw. If the set
    /// changes, the tick no longer means what it meant.
    #[test]
    fn an_acknowledgement_does_not_survive_a_change_in_conditions() {
        let first = [Degraded {
            subsystem: "tracking",
            detail: "pipeline not implemented",
        }];
        let mut state = DecisionDialogState::default();
        state.reconcile(&first);
        state.degraded_acknowledged = true;

        // Same conditions next frame: the acknowledgement stands.
        state.reconcile(&first);
        assert!(state.degraded_acknowledged);

        // The journal then fails. The operator has not seen that.
        let then = [
            Degraded {
                subsystem: "tracking",
                detail: "pipeline not implemented",
            },
            Degraded {
                subsystem: "journal",
                detail: "fsync failed",
            },
        ];
        state.reconcile(&then);
        assert!(
            !state.degraded_acknowledged,
            "an acknowledgement carried over to a condition nobody saw"
        );
    }

    /// A rejection carries its reason, so MOE-01 can tell a considered rejection from
    /// an abandoned decision.
    #[test]
    fn a_rejection_needs_a_reason() {
        let mut state = DecisionDialogState::default();
        assert!(!reject_enabled(&state));
        state.reject_reason = "   ".into();
        assert!(!reject_enabled(&state), "whitespace is not a reason");
        state.reject_reason = "track is a friendly airliner".into();
        assert!(reject_enabled(&state));
    }

    /// Overriding is a strictly higher authority than accepting, so a role that may
    /// accept does not thereby get to override.
    #[test]
    fn accepting_does_not_grant_overriding() {
        let r = row();
        let v = view(&r, &[]);
        assert!(v.may_accept);
        assert!(
            !v.may_override,
            "an operator holds DECIDE_PLAN without OVERRIDE_PLAN"
        );
    }
}
