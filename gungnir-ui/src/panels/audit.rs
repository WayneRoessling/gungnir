// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-20, audit and accounts (GAP-057, GAP-059; DN-23 §7; `WF-20-audit-accounts.puml`).
//!
//! Four things, in the order an administrator reads them: who is signed in -- or which
//! of the three reasons nobody is, which PN-01 and PN-07 have to tell apart -- the
//! accounts the store lists, the handoffs this desktop issued and what came back of
//! them, and the audit log with every attempt in it, failed ones included (DN-23 §5
//! rule 7).
//!
//! The passphrase field is masked and is cleared by the caller on submit; the panel holds
//! it only in the draft the caller owns.
//!
//! # The handoff rows (GAP-040, DN-07 §7)
//!
//! PN-06 asks what is still owed and shows only that. PN-20 is the after-action account
//! and shows every handoff, delivered ones included: reconstructing an engagement means
//! knowing what was passed to whom, when, and what the receiving system said -- and a
//! delivered handoff is the row that reconstruction needs most. The wording, colour and
//! age arithmetic are `crate::panels::handoff`'s, so the administrator's account matches
//! the sentence the operator was reading at the time.

use crate::panels::config_editor::AuditLine;
use crate::panels::handoff::{self, HandoffRow};
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::MissionTime;

/// Who is signed in, or why nobody is (`gungnir_security::SessionState` in words).
#[derive(Debug, Clone, PartialEq)]
pub enum SessionLine {
    SignedIn {
        operator: u64,
        role: String,
        expires_s: Option<f64>,
    },
    NobodySignedIn,
    Expired {
        operator: u64,
        at_s: f64,
    },
    StoreUnavailable {
        reason: String,
    },
}

/// One account as PN-20 lists it: never the hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountLine<'a> {
    pub operator: u64,
    pub role: &'a str,
}

/// Everything PN-20 draws.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditView<'a> {
    pub session: SessionLine,
    /// The accounts, or why there is no list.
    pub accounts: Result<&'a [AccountLine<'a>], &'a str>,
    pub audit: &'a [AuditLine<'a>],
    /// Every handoff this desktop issued, oldest first (GAP-040). Unfiltered: this is
    /// the after-action view, so a delivered handoff is a row and not an omission.
    pub handoffs: &'a [HandoffRow<'a>],
    /// The mission clock, for the age of anything still waiting on an endpoint.
    pub now: MissionTime,
    /// False when the store is unavailable: the form is drawn disabled, with the reason
    /// above it, rather than accepting a credential it cannot check.
    pub can_sign_in: bool,
    /// Whether the signed-in role may assign roles (GAP-057). The form is drawn
    /// disabled otherwise, with the reason.
    pub can_assign_roles: bool,
}

/// What the administrator has typed. The caller clears the passphrase on submit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignInDraft {
    pub operator: String,
    pub passphrase: String,
    /// The assign-role form (GAP-057): the operator and the role name chosen.
    pub assign_operator: String,
    pub assign_role: String,
}

/// The role names PN-20 offers, in the order `gungnir_security::Role` declares them.
/// Names rather than the type: this crate depends on the model alone, and the host maps
/// the name back.
pub const ROLE_NAMES: [&str; 9] = [
    "Operator",
    "Supervisor",
    "Analyst",
    "SensorManager",
    "Administrator",
    "Commander",
    "Planner",
    "SecurityOfficer",
    "IntelligenceAnalyst",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    SignIn,
    SignOut,
    /// Assign `role` to `operator` in the account store (GAP-057). An act with
    /// authority behind it: the host refuses it for a role without `account.assign_role`.
    AssignRole {
        operator: u64,
        role: &'static str,
    },
}

/// The assign-role form (GAP-057, WF-20): an operator number and a role, submitted as an
/// act the host checks authority for and records.
fn draw_assign_role(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &AuditView<'_>,
    draft: &mut SignInDraft,
) -> Option<SessionAction> {
    let mut action = None;
    let store_ok = view.accounts.is_ok();
    if !view.can_assign_roles {
        ui.label(
            RichText::new(
                "Assigning a role needs the account.assign_role action, which the \
                 signed-in role does not hold.",
            )
            .small()
            .color(palette.muted_text_color()),
        );
    }
    ui.add_enabled_ui(view.can_assign_roles && store_ok, |ui| {
        ui.horizontal(|ui| {
            ui.label("Assign");
            ui.add(egui::TextEdit::singleline(&mut draft.assign_operator).desired_width(60.0));
            let current = if draft.assign_role.is_empty() {
                "role"
            } else {
                draft.assign_role.as_str()
            };
            egui::ComboBox::from_id_salt("assign_role")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for name in ROLE_NAMES {
                        ui.selectable_value(&mut draft.assign_role, name.to_string(), name);
                    }
                });
            let operator = draft.assign_operator.trim().parse::<u64>().ok();
            let role = ROLE_NAMES
                .iter()
                .copied()
                .find(|n| *n == draft.assign_role.as_str());
            let ready = operator.is_some() && role.is_some();
            if ui
                .add_enabled(ready, egui::Button::new("Assign role"))
                .clicked()
            {
                if let (Some(operator), Some(role)) = (operator, role) {
                    action = Some(SessionAction::AssignRole { operator, role });
                }
            }
        });
    });
    action
}

/// PN-20's handoff rows (GAP-040, DN-07 §7): what was handed off, to whom, when, and
/// what came back.
///
/// Every issued handoff, in issue order, delivered ones included -- see the module
/// documentation for why this list is unfiltered where PN-06's is not. The panel does not
/// reorder, so the sequence an administrator reads is the sequence the desktop recorded.
fn draw_handoffs(ui: &mut Ui, palette: &theme::Palette, view: &AuditView<'_>) {
    ui.separator();
    ui.strong("Handoffs");
    if view.handoffs.is_empty() {
        // Not a gap and not a fault: no decision has been actionable on this desktop.
        // Said rather than left blank, because a blank section reads as one that failed
        // to load.
        ui.label(
            RichText::new("Nothing has been handed off on this desktop.")
                .color(palette.muted_text_color()),
        );
        return;
    }
    if !view.handoffs.iter().any(|row| !row.reports.is_empty()) {
        ui.label(
            RichText::new(handoff::NO_REPORTS_YET)
                .small()
                .color(palette.muted_text_color()),
        );
    }
    for row in view.handoffs {
        draw_handoff_row(ui, palette, row, view.now);
    }
}

/// One handoff: to whom, by whose decision, when, where delivery stands, and what came
/// back.
///
/// A block of wrapping labels rather than a grid row. Two of the fields are sentences
/// rather than values, and a grid wide enough for them puts the columns after them
/// outside the pane, where egui stops painting them -- so the field an administrator
/// most needs, what came back, would be the one that disappeared.
fn draw_handoff_row(ui: &mut Ui, palette: &theme::Palette, row: &HandoffRow<'_>, now: MissionTime) {
    let destination = row
        .endpoint
        .map_or_else(|| "by radio call".to_owned(), |e| format!("to {e}"));
    // The one-word state on the headline and the sentence below it. An administrator
    // scanning a long log needs a word to sort on; the sentence is what they read once
    // they have stopped, and neither is enough on its own.
    ui.label(
        RichText::new(format!(
            "Decision {} (plan #{}) {destination}, issued T+{:.0} s -- {}",
            row.decision,
            row.plan,
            row.issued.0,
            handoff::delivery_label(row.delivery)
        ))
        .strong(),
    );
    // The attribution as the record holds it, which is "nobody signed in" when that is
    // what happened. An audit surface that tidied that into a role would be inventing an
    // operator (DN-23 §5 rule 1).
    ui.label(format!("Decided by {} ({}).", row.operator, row.role));
    ui.label(
        RichText::new(handoff::delivery_sentence(row, now))
            .color(handoff::delivery_color(palette, row.delivery)),
    );
    if row.reports.is_empty() {
        ui.label(
            RichText::new(handoff::nothing_reported_sentence(row))
                .color(palette.muted_text_color()),
        );
    } else {
        // Every report, not the latest: an acknowledgement followed by a refusal is a
        // different account from a refusal alone, and the middle of the sequence is
        // where an after-action review looks.
        for report in row.reports {
            ui.label(format!(
                "Reported back: {}",
                handoff::report_sentence(report)
            ));
        }
    }
    ui.add_space(palette.row_spacing);
}

/// Who is signed in, or which of the three reasons nobody is; sign-out when somebody is.
fn draw_session(
    ui: &mut Ui,
    palette: &theme::Palette,
    session: &SessionLine,
) -> Option<SessionAction> {
    let mut action = None;
    ui.strong("Session");
    match session {
        SessionLine::SignedIn {
            operator,
            role,
            expires_s,
        } => {
            let until = expires_s.map_or_else(
                || "until sign-out or shutdown".to_string(),
                |t| format!("until {t:.0} s"),
            );
            ui.label(format!("Signed in: operator {operator} ({role}), {until}."));
            if ui.button("Sign out").clicked() {
                action = Some(SessionAction::SignOut);
            }
        }
        SessionLine::NobodySignedIn => {
            ui.label(
                RichText::new("Nobody is signed in. Decisions are recorded as unattributed.")
                    .color(palette.warning_color),
            );
        }
        SessionLine::Expired { operator, at_s } => {
            ui.label(
                RichText::new(format!(
                    "Operator {operator}'s session expired at {at_s:.0} s; sign in again."
                ))
                .color(palette.warning_color),
            );
        }
        SessionLine::StoreUnavailable { reason } => {
            ui.label(
                RichText::new(format!(
                    "The account store is unavailable: {reason}. Nobody can sign in; the \
                     desktop runs unattributed."
                ))
                .color(palette.warning_color),
            );
        }
    }

    action
}

/// Render the panel.
pub fn render_audit(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &AuditView<'_>,
    draft: &mut SignInDraft,
) -> Option<SessionAction> {
    ui.heading("Audit and accounts");
    let mut action = None;

    if let Some(a) = draw_session(ui, palette, &view.session) {
        action = Some(a);
    }

    if !matches!(view.session, SessionLine::SignedIn { .. }) {
        ui.add_enabled_ui(view.can_sign_in, |ui| {
            ui.horizontal(|ui| {
                ui.label("Operator");
                ui.add(egui::TextEdit::singleline(&mut draft.operator).desired_width(80.0));
                ui.label("Passphrase");
                ui.add(
                    egui::TextEdit::singleline(&mut draft.passphrase)
                        .password(true)
                        .desired_width(160.0),
                );
                let ready = !draft.operator.trim().is_empty() && !draft.passphrase.is_empty();
                if ui
                    .add_enabled(ready, egui::Button::new("Sign in"))
                    .clicked()
                {
                    action = Some(SessionAction::SignIn);
                }
            });
        });
    }

    ui.separator();
    ui.strong("Accounts");
    match view.accounts {
        Err(reason) => {
            ui.label(RichText::new(reason).color(palette.muted_text_color()));
        }
        Ok([]) => {
            ui.label(
                RichText::new("The store lists no accounts.").color(palette.muted_text_color()),
            );
        }
        Ok(accounts) => {
            for a in accounts {
                ui.label(format!("operator {}: {}", a.operator, a.role));
            }
        }
    }
    if let Some(a) = draw_assign_role(ui, palette, view, draft) {
        action = Some(a);
    }
    // DN-17 §7's PN-20 row (GAP-062): drawn as what it is. A marking is fixed by where
    // the data came from and changes only with its provenance; nothing here rewrites
    // one, so there is no act to list, and saying so is the row.
    ui.separator();
    ui.strong("Marking changes");
    ui.label(
        RichText::new(
            "None can be made here: a marking is fixed by the data's origin (DN-17 §5) and \
             follows its provenance. A change would appear in the audit log as an act.",
        )
        .small()
        .color(palette.muted_text_color()),
    );

    draw_handoffs(ui, palette, view);

    ui.separator();
    ui.strong("Audit log");
    if view.audit.is_empty() {
        ui.label(RichText::new("Nothing recorded yet.").color(palette.muted_text_color()));
    }
    egui::Grid::new("audit_log").striped(true).show(ui, |ui| {
        for line in view.audit.iter().rev().take(200) {
            ui.label(format!("{} s", line.mission_time_s));
            ui.label(
                line.operator
                    .map_or_else(|| "nobody".to_string(), |o| format!("operator {o}")),
            );
            ui.label(line.action);
            ui.label(line.detail);
            ui.end_row();
        }
    });
    action
}
