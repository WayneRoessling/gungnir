// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! FL-08's status dialog: a live, audited weapons control status change (GAP-203).
//!
//! `docs/ux/interaction-design.md` FL-08: a supervisor opens this from a PN-01 chip,
//! chooses the layer's new status, says why, and the change goes to policy, which
//! re-evaluates the pending plans and records the decision. The dialog knows none of
//! that: it draws what `docs/design/DN-09-authority-and-control-status.md` §7 and §10
//! ask for and returns what the person asked for, and the desktop does the rest.
//!
//! # Set is never the default
//!
//! PN-07's discipline (`decision_dialog`), applied to the one control that can make an
//! engagement proposable that was not: this is the surface where a layer goes from
//! hold to free.
//!
//! 1. Nothing is pre-selected. [`ControlStatusDialogState::open`] holds no target, and
//!    [`set_enabled`] is false for it.
//! 2. The status in force is shown but cannot be chosen, so "set" always means a change.
//! 3. A change needs its reason, because the record carries one and an empty reason is
//!    not a decision (DN-09 §6: setting the status *is itself a recorded decision*).
//! 4. Under the two-person rule (D-140) it needs the second person's id and
//!    passphrase as well, and says who that may be.
//! 5. Cancel is drawn first and set last, and no control is bound to Enter:
//!    [`render_control_status_dialog`] returns an action only from a click.
//! 6. The permissive choice is drawn last among the statuses, so the reflexive first
//!    click lands on hold.
//!
//! `tests::set_is_enabled_exactly_under_the_rule` searches the state space for a
//! combination that enables set outside the rule.
//!
//! # What the dialog never knows
//!
//! Whether the signed-in role holds `weapons.control_status`, whether the second person
//! is somebody else and holds it too, and what the change does to the queue are the
//! application's to establish; the view carries the answers as data (`may_set`,
//! `would_withdraw`) and the dialog draws them. A dialog that worked those out from its
//! own copy of the policy would be a second policy engine.

use crate::panels::status_strip::control_status_color;
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::{EffectorLayer, MissionTime, Vocabulary, WeaponsControlStatus};

/// What the person has entered so far. Owned by the desktop and carried across frames;
/// immediate mode has nowhere else to keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlStatusDialogState {
    pub layer: EffectorLayer,
    /// The status chosen, and `None` until one is: nothing is pre-selected.
    pub target: Option<WeaponsControlStatus>,
    /// Why, in the person's words. Recorded with the change.
    pub reason: String,
    /// The second person's operator id as typed, under the two-person rule (D-140).
    pub confirm_operator: String,
    /// The second person's passphrase as typed. Cleared the moment a set is returned,
    /// so it is held for exactly as long as the draft needs it.
    pub confirm_passphrase: String,
}

impl ControlStatusDialogState {
    /// A fresh dialog for `layer`, with nothing chosen and nothing typed.
    #[must_use]
    pub fn open(layer: EffectorLayer) -> Self {
        Self {
            layer,
            target: None,
            reason: String::new(),
            confirm_operator: String::new(),
            confirm_passphrase: String::new(),
        }
    }
}

/// Who last set the layer's status live, from the recorded change (DN-09 §10).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LastChange<'a> {
    /// The operator id in its decimal spelling, as every record names one.
    pub operator: &'a str,
    pub role: &'a str,
    pub at: MissionTime,
    pub reason: &'a str,
    /// The second person, as the record names them, when the two-person rule was in
    /// force for that change (D-140).
    pub confirmed_by: Option<&'a str>,
}

/// Where the change goes (GAP-203): the same two places a decision goes (DN-31 §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route<'a> {
    /// Into this desktop's own policy and record, because it is cut off from its node
    /// or was deployed with none.
    ThisDesktop,
    /// Through the node, which authorizes it, applies it and journals it, so every
    /// linked console sees the same status.
    Node { endpoint: &'a str },
}

/// Everything the dialog draws, borrowed from the desktop each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlStatusDialogView<'a> {
    pub layer: EffectorLayer,
    pub current: WeaponsControlStatus,
    /// Whether the baseline sets this layer. `false` means hold by default, which is
    /// the safe default and not a decision (`ControlStatusSettings::for_layer`).
    pub configured: bool,
    /// The live change that set the current status, when one did.
    pub last_change: Option<LastChange<'a>>,
    /// For each status other than the current, how many queued items that status would
    /// take off the queue. The application computes it; a status with no entry is drawn
    /// as not computed rather than as zero.
    pub would_withdraw: &'a [(WeaponsControlStatus, usize)],
    /// How many queued items task this layer now.
    pub queued_on_layer: usize,
    /// D-140: the baseline's two-person rule is on, so a second signed-in person must
    /// confirm the change.
    pub two_person: bool,
    /// `Ok` when this console may set the status; `Err` carries why not, in words: not
    /// signed in, the role does not hold `weapons.control_status`, cut off from the
    /// node with the node's status in force.
    pub may_set: Result<(), &'a str>,
    pub route: Route<'a>,
    /// The words this deployment uses (D-12).
    pub vocabulary: &'a Vocabulary,
}

/// The second person's credential, under the two-person rule (D-140). The desktop holds
/// it for one frame, long enough to send it; `Debug` never shows the passphrase, so
/// neither does a log line that prints the draft.
#[derive(Clone, PartialEq, Eq)]
pub struct Confirmation {
    pub operator: u64,
    pub passphrase: String,
}

impl std::fmt::Debug for Confirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confirmation")
            .field("operator", &self.operator)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// The change a person asked for: what the desktop sends to policy or to the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlStatusDraft {
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
    /// Trimmed, and never empty.
    pub reason: String,
    /// Present exactly when the view said the two-person rule was in force.
    pub confirmation: Option<Confirmation>,
}

/// What the person did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlStatusDialogAction {
    Set(ControlStatusDraft),
    Cancel,
}

/// Whether `to` is a more permissive status than `from`: hold to tight, tight to free,
/// hold to free. `WeaponsControlStatus` is declared most to least permissive, so a
/// loosening is a move towards the first variant.
#[must_use]
pub fn is_loosening(from: WeaponsControlStatus, to: WeaponsControlStatus) -> bool {
    to < from
}

/// Whether the second person's fields hold a credential that can be sent: an id that
/// parses as an operator id, and a passphrase that is not empty. Whether the id is
/// somebody else's, and holds the authority, is the node's or the policy's to say.
#[must_use]
pub fn confirmation_complete(state: &ControlStatusDialogState) -> bool {
    state.confirm_operator.trim().parse::<u64>().is_ok() && !state.confirm_passphrase.is_empty()
}

/// Whether the set control may be enabled: the console may set, a status other than the
/// one in force is chosen, a reason is given, and under the two-person rule the second
/// person's credential is complete.
#[must_use]
pub fn set_enabled(view: &ControlStatusDialogView<'_>, state: &ControlStatusDialogState) -> bool {
    view.may_set.is_ok()
        && state.target.is_some_and(|t| t != view.current)
        && !state.reason.trim().is_empty()
        && (!view.two_person || confirmation_complete(state))
}

/// What still stands between the person and set, when something does: the line drawn
/// under a disabled set control. `None` when set is enabled, or when `may_set` is `Err`,
/// which is drawn by its own sentence.
fn what_is_missing(
    view: &ControlStatusDialogView<'_>,
    state: &ControlStatusDialogState,
) -> Option<&'static str> {
    if view.may_set.is_err() || set_enabled(view, state) {
        None
    } else if state.target.is_none_or(|t| t == view.current) {
        Some("Choose a status other than the one in force.")
    } else if state.reason.trim().is_empty() {
        Some("A change is recorded with its reason; enter one.")
    } else {
        Some("The second person's operator id and passphrase are needed.")
    }
}

/// Render the dialog. Returns an action only when the person clicks one.
pub fn render_control_status_dialog(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &ControlStatusDialogView<'_>,
    state: &mut ControlStatusDialogState,
) -> Option<ControlStatusDialogAction> {
    draw_header(ui, palette, view);
    ui.separator();
    if let Err(why) = view.may_set {
        ui.label(RichText::new(why).color(palette.warning_color));
    }
    // A control that does nothing is worse than none, so the controls are disabled as
    // a whole when the console may not set; the status above stays readable because a
    // status you cannot change is still worth reading.
    ui.add_enabled_ui(view.may_set.is_ok(), |ui| {
        draw_choices(ui, palette, view, state);
        ui.separator();
        draw_route(ui, palette, view.route);
        ui.strong("Reason");
        ui.add(
            egui::TextEdit::multiline(&mut state.reason)
                .hint_text("Why, in your words")
                .desired_rows(2),
        );
        if view.two_person {
            ui.separator();
            draw_two_person(ui, palette, state);
        }
    });
    ui.separator();
    draw_controls(ui, palette, view, state)
}

/// The layer, its status in force in its colour, and where that status came from.
fn draw_header(ui: &mut Ui, palette: &theme::Palette, view: &ControlStatusDialogView<'_>) {
    let v = view.vocabulary;
    ui.heading(format!("Control status: {}", v.layer(view.layer)));
    ui.label(
        RichText::new(format!("{} in force", v.control_status(view.current)))
            .color(control_status_color(palette, view.current))
            .strong(),
    );
    match view.last_change {
        Some(c) => {
            ui.label(format!(
                "Set to {} by operator {} ({}) at T+{:.0} s: {}",
                v.control_status(view.current),
                c.operator,
                c.role,
                c.at.0,
                c.reason
            ));
            if let Some(who) = c.confirmed_by {
                ui.label(format!("Confirmed by {who}."));
            }
        }
        None if view.configured => {
            ui.label("Set by the configuration baseline.");
        }
        None => {
            ui.label(
                RichText::new("Hold by default: no baseline sets this layer.")
                    .color(palette.muted_text_color()),
            );
        }
    }
    ui.label(
        RichText::new(match view.queued_on_layer {
            0 => "Nothing queued tasks this layer.".to_owned(),
            1 => "1 queued item tasks this layer.".to_owned(),
            n => format!("{n} queued items task this layer."),
        })
        .color(palette.muted_text_color())
        .size(palette.small_font_size),
    );
}

/// The three statuses, hold first and free last, with what each would do to the queue.
fn draw_choices(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &ControlStatusDialogView<'_>,
    state: &mut ControlStatusDialogState,
) {
    ui.strong("Set to");
    for status in [
        WeaponsControlStatus::Hold,
        WeaponsControlStatus::Tight,
        WeaponsControlStatus::Free,
    ] {
        let term = view.vocabulary.control_status(status);
        if status == view.current {
            ui.add_enabled(
                false,
                egui::RadioButton::new(false, format!("{term}: in force")),
            );
            continue;
        }
        ui.horizontal(|ui| {
            if ui.radio(state.target == Some(status), term).clicked() {
                state.target = Some(status);
            }
            let withdraws = view
                .would_withdraw
                .iter()
                .find(|(s, _)| *s == status)
                .map(|(_, n)| *n);
            ui.label(
                RichText::new(match withdraws {
                    Some(0) => "withdraws nothing queued".to_owned(),
                    Some(1) => "withdraws 1 queued item".to_owned(),
                    Some(n) => format!("withdraws {n} queued items"),
                    None => "effect on the queue not computed".to_owned(),
                })
                .color(palette.muted_text_color())
                .size(palette.small_font_size),
            );
        });
        if is_loosening(view.current, status) {
            ui.label(
                RichText::new(
                    "Loosening: engagements this layer could not propose become proposable.",
                )
                .color(palette.alert_color),
            );
        }
    }
}

/// Where the change goes, in one sentence, under the choices and above the reason.
fn draw_route(ui: &mut Ui, palette: &theme::Palette, route: Route<'_>) {
    let sentence = match route {
        Route::ThisDesktop => "Applied on this desktop.".to_owned(),
        Route::Node { endpoint } => format!(
            "Sent to the node at {endpoint}; the node applies it and every linked console \
             sees it."
        ),
    };
    ui.label(
        RichText::new(sentence)
            .color(palette.muted_text_color())
            .size(palette.small_font_size),
    );
}

/// The second person's credential (D-140), boxed so it reads as the separate act it is.
fn draw_two_person(ui: &mut Ui, palette: &theme::Palette, state: &mut ControlStatusDialogState) {
    ui.group(|ui| {
        ui.strong("Two-person rule in force (D-140)");
        ui.label(
            RichText::new(
                "A second signed-in person confirms this change. They must hold \
                 weapons.control_status and be someone other than you; the record \
                 carries both identities.",
            )
            .color(palette.muted_text_color())
            .size(palette.small_font_size),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.confirm_operator)
                .hint_text("Second operator's id"),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.confirm_passphrase)
                .password(true)
                .hint_text("Their passphrase"),
        );
    });
}

/// Cancel, then set. The order is the safety property: point 5 of the module
/// documentation.
fn draw_controls(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &ControlStatusDialogView<'_>,
    state: &mut ControlStatusDialogState,
) -> Option<ControlStatusDialogAction> {
    let mut action = None;
    if let Some(missing) = what_is_missing(view, state) {
        ui.label(RichText::new(missing).color(palette.muted_text_color()));
    }
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            action = Some(ControlStatusDialogAction::Cancel);
        }
        let label = state.target.map_or_else(
            || "Set control status".to_owned(),
            |t| {
                format!(
                    "Set {} to {}",
                    view.vocabulary.layer(view.layer),
                    view.vocabulary.control_status(t)
                )
            },
        );
        let enabled = set_enabled(view, state);
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            if let Some(draft) = draft(view, state) {
                action = Some(ControlStatusDialogAction::Set(draft));
            }
        }
    });
    action
}

/// The draft a set returns, or `None` when the rule is not met: the button is disabled
/// then, so this is the same gate checked twice rather than once.
fn draft(
    view: &ControlStatusDialogView<'_>,
    state: &mut ControlStatusDialogState,
) -> Option<ControlStatusDraft> {
    if !set_enabled(view, state) {
        return None;
    }
    let status = state.target?;
    let confirmation = if view.two_person {
        let operator = state.confirm_operator.trim().parse::<u64>().ok()?;
        // Moved out, not copied: the state no longer holds the passphrase once the
        // draft does.
        let passphrase = std::mem::take(&mut state.confirm_passphrase);
        Some(Confirmation {
            operator,
            passphrase,
        })
    } else {
        None
    };
    Some(ControlStatusDraft {
        layer: view.layer,
        status,
        reason: state.reason.trim().to_owned(),
        confirmation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(
        vocabulary: &Vocabulary,
        current: WeaponsControlStatus,
        two_person: bool,
    ) -> ControlStatusDialogView<'_> {
        ControlStatusDialogView {
            layer: EffectorLayer::Area,
            current,
            configured: true,
            last_change: None,
            would_withdraw: &[],
            queued_on_layer: 0,
            two_person,
            may_set: Ok(()),
            route: Route::ThisDesktop,
            vocabulary,
        }
    }

    const STATUSES: [WeaponsControlStatus; 3] = [
        WeaponsControlStatus::Free,
        WeaponsControlStatus::Tight,
        WeaponsControlStatus::Hold,
    ];

    /// The property the module documentation names, searched rather than asserted: for
    /// every status in force, every target or none, a reason or none, the two-person
    /// rule on or off, and the second person's fields filled or not, set is enabled
    /// exactly when the rule says.
    #[test]
    fn set_is_enabled_exactly_under_the_rule() {
        let vocabulary = Vocabulary::default();
        for current in STATUSES {
            for target in STATUSES.iter().copied().map(Some).chain([None]) {
                for reason in ["", "   ", "civil traffic"] {
                    for two_person in [false, true] {
                        for (operator, passphrase) in
                            [("", ""), ("9", ""), ("", "pw"), ("nine", "pw"), ("9", "pw")]
                        {
                            for may_set in [Ok(()), Err("not signed in")] {
                                let mut v = view(&vocabulary, current, two_person);
                                v.may_set = may_set;
                                let state = ControlStatusDialogState {
                                    target,
                                    reason: reason.to_owned(),
                                    confirm_operator: operator.to_owned(),
                                    confirm_passphrase: passphrase.to_owned(),
                                    ..ControlStatusDialogState::open(EffectorLayer::Area)
                                };
                                let expected = may_set.is_ok()
                                    && target.is_some_and(|t| t != current)
                                    && !reason.trim().is_empty()
                                    && (!two_person || (operator == "9" && passphrase == "pw"));
                                assert_eq!(
                                    set_enabled(&v, &state),
                                    expected,
                                    "current {current:?}, target {target:?}, reason \
                                     {reason:?}, two-person {two_person}, operator \
                                     {operator:?}, passphrase {passphrase:?}, may_set \
                                     {may_set:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// A fresh dialog chooses nothing, so set is shut until a person acts.
    #[test]
    fn a_fresh_dialog_enables_nothing() {
        let vocabulary = Vocabulary::default();
        let state = ControlStatusDialogState::open(EffectorLayer::Point);
        assert_eq!(state.target, None);
        for current in STATUSES {
            assert!(!set_enabled(&view(&vocabulary, current, false), &state));
        }
    }

    /// Loosening is a move towards free, and only that.
    #[test]
    fn loosening_is_towards_free() {
        use WeaponsControlStatus::{Free, Hold, Tight};
        assert!(is_loosening(Hold, Tight));
        assert!(is_loosening(Tight, Free));
        assert!(is_loosening(Hold, Free));
        assert!(!is_loosening(Free, Tight));
        assert!(!is_loosening(Tight, Hold));
        assert!(!is_loosening(Free, Hold));
        for s in STATUSES {
            assert!(!is_loosening(s, s));
        }
    }

    /// The passphrase never reaches a log line through the draft's `Debug`.
    #[test]
    fn the_confirmation_redacts_its_passphrase_in_debug() {
        let c = Confirmation {
            operator: 9,
            passphrase: "correct horse".to_owned(),
        };
        let shown = format!("{c:?}");
        assert!(shown.contains("operator: 9"), "{shown}");
        assert!(!shown.contains("correct horse"), "{shown}");
        assert!(shown.contains("<redacted>"), "{shown}");
    }

    /// A returned draft carries the trimmed reason and, under the two-person rule, the
    /// credential moved out of the state, so the state no longer holds the passphrase.
    #[test]
    fn a_draft_takes_the_passphrase_out_of_the_state() {
        let vocabulary = Vocabulary::default();
        let v = view(&vocabulary, WeaponsControlStatus::Tight, true);
        let mut state = ControlStatusDialogState {
            target: Some(WeaponsControlStatus::Hold),
            reason: "  civil traffic  ".to_owned(),
            confirm_operator: " 9 ".to_owned(),
            confirm_passphrase: "pw".to_owned(),
            ..ControlStatusDialogState::open(EffectorLayer::Area)
        };
        let d = draft(&v, &mut state).expect("the rule is met");
        assert_eq!(d.reason, "civil traffic");
        assert_eq!(d.status, WeaponsControlStatus::Hold);
        assert_eq!(d.layer, EffectorLayer::Area);
        let c = d.confirmation.expect("two-person was on");
        assert_eq!(c.operator, 9);
        assert_eq!(c.passphrase, "pw");
        assert!(state.confirm_passphrase.is_empty());

        // Without the rule the draft carries no credential, whatever was typed.
        let v = view(&vocabulary, WeaponsControlStatus::Tight, false);
        state.confirm_passphrase = "pw".to_owned();
        let d = draft(&v, &mut state).expect("the rule is met");
        assert_eq!(d.confirmation, None);
    }
}
