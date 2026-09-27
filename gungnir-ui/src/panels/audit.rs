// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-20, audit and accounts (GAP-057, GAP-059; DN-23 §7; `WF-20-audit-accounts.puml`).
//!
//! Five things, in the order an administrator reads them: who is signed in -- or which
//! of the three reasons nobody is, which PN-01 and PN-07 have to tell apart -- the
//! accounts the store lists, the handoffs this desktop issued and what came back of
//! them, the audit record as a whole, and this run's audit log with every attempt in it,
//! failed ones included (DN-23 §5 rule 7).
//!
//! # The audit record (GAP-163, GAP-152; D-104, D-106)
//!
//! Whether the record verifies against the heads the event journal holds, said in the
//! warning colour when it does not, with each problem naming its file; every segment,
//! earlier runs' included, with its state; a "Verify now" control; and any earlier
//! segment opened read only. Nothing here changes a segment: the only act is verifying,
//! which reads.
//!
//! # The node's audit record (GAP-179, D-116)
//!
//! On a desktop linked to a node, the node's record beside this desktop's own, read only:
//! the node's verification against its journal's heads, its segments, and a page of any
//! one of them. **Read when a person asks, never polled**, because every read is an entry
//! on the node's record. Said plainly when the signed-in role does not hold `audit.read`,
//! when nothing has been read yet, and when the node could not be reached -- in which case
//! whatever an earlier read showed stays, labelled with when it was read.
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
    /// The audit record beyond this run's entries: whether it verifies against the heads
    /// the journal holds, each segment's state, and an earlier segment read back
    /// (GAP-163, GAP-152).
    pub record: AuditRecordView<'a>,
    /// The node's audit record, on a desktop linked to one (GAP-179, D-116); `None` on a
    /// desktop that has no node, which has no second record to show.
    pub node: Option<NodeAuditRecordView<'a>>,
}

/// The node's audit record as a linked desktop's PN-20 draws it (GAP-179, D-116).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeAuditRecordView<'a> {
    /// Which node, in words.
    pub node: &'a str,
    /// Where the read stands, in one sentence: the node's verification with when it ran
    /// and when it was read, or why nothing is shown.
    pub summary: &'a str,
    /// Whether the node's record verified. Anything else is drawn in the warning colour.
    pub sound: bool,
    /// A second sentence when there is one: why the last read failed, or that what is
    /// shown is an earlier read.
    pub note: &'a str,
    /// Whether the signed-in role may read it here: `audit.read`, and a session the link
    /// signed in with.
    pub can_read: bool,
    /// A read is on its way.
    pub reading: bool,
    /// Each problem the node's verification found, naming its file.
    pub problems: &'a [String],
    pub segments: &'a [RecordSegmentLine<'a>],
    pub shown: Option<NodeShownPage<'a>>,
}

/// A page of one of the node's segments, read only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeShownPage<'a> {
    pub segment: &'a str,
    pub lines: &'a [AuditLine<'a>],
    /// Which entries these are of how many, and the segment's chain as read.
    pub note: &'a str,
    pub sound: bool,
    pub has_older: bool,
    pub has_newer: bool,
}

/// One segment of the audit record as PN-20 lists it (GAP-152, D-106).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordSegmentLine<'a> {
    /// Its state in words, naming its file (`gungnir_security::SegmentReport::describe`).
    pub description: &'a str,
    /// No chain break and no finding.
    pub sound: bool,
    /// On the disk, so it can be shown.
    pub readable: bool,
}

/// An earlier segment PN-20 is showing, read only (GAP-152, D-106).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShownSegment<'a> {
    pub segment: &'a str,
    pub lines: &'a [AuditLine<'a>],
    /// Its chain as it was read, and anything in it that did not read.
    pub note: &'a str,
    pub sound: bool,
}

/// PN-20's account of the audit record as a whole (GAP-163, D-104; GAP-152, D-106).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuditRecordView<'a> {
    /// The last verification in one sentence, with when it ran.
    pub summary: &'a str,
    /// Whether it verified. Anything else is drawn in the warning colour, never muted.
    pub sound: bool,
    /// Each problem found, naming its file.
    pub problems: &'a [String],
    pub segments: &'a [RecordSegmentLine<'a>],
    pub shown: Option<ShownSegment<'a>>,
    /// Whether and how old audit segments are purged.
    pub retention: &'a str,
}

impl AuditRecordView<'static> {
    /// Before any verification has run: said, not left blank.
    pub const NOT_VERIFIED: Self = Self {
        summary: "The audit record has not been verified yet.",
        sound: false,
        problems: &[],
        segments: &[],
        shown: None,
        retention: "",
    };
}

/// Most entries of an earlier segment PN-20 draws at once; the rest are named, not
/// dropped in silence.
pub const SHOWN_ENTRIES_LIMIT: usize = 2_000;

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
    /// Verify the audit record against the journal now (GAP-163, D-104). A read: it
    /// changes nothing but the record of having checked.
    VerifyAuditRecord,
    /// Show the segment at this position in [`AuditRecordView::segments`], read only.
    ShowAuditSegment(usize),
    /// Stop showing it.
    HideAuditSegment,
    /// Read the node's audit record: its last verification and its segments (GAP-179,
    /// D-116). One entry on the node's record.
    ReadNodeAuditRecord,
    /// Ask the node to verify its record again, and read it.
    VerifyNodeAuditRecord,
    /// Read the newest page of the node's segment at this position in
    /// [`NodeAuditRecordView::segments`].
    ShowNodeAuditSegment(usize),
    /// The page before (`older`) or after the one shown.
    PageNodeAuditSegment {
        older: bool,
    },
    /// Stop showing the node's segment.
    HideNodeAuditSegment,
}

/// PN-20's audit-record section: the verification, every segment with its state, and an
/// earlier segment's entries when one is open (GAP-163, GAP-152).
fn draw_record(
    ui: &mut Ui,
    palette: &theme::Palette,
    record: &AuditRecordView<'_>,
) -> Option<SessionAction> {
    let mut action = None;
    ui.separator();
    ui.strong("Audit record");
    let colour = if record.sound {
        palette.muted_text_color()
    } else {
        palette.warning_color
    };
    ui.label(RichText::new(record.summary).color(colour));
    for problem in record.problems {
        ui.label(RichText::new(problem.as_str()).color(palette.warning_color));
    }
    if !record.retention.is_empty() {
        ui.label(
            RichText::new(record.retention)
                .small()
                .color(palette.muted_text_color()),
        );
    }
    if ui.button("Verify now").clicked() {
        action = Some(SessionAction::VerifyAuditRecord);
    }
    for (index, segment) in record.segments.iter().enumerate() {
        ui.horizontal(|ui| {
            let text = RichText::new(segment.description);
            ui.label(if segment.sound {
                text
            } else {
                text.color(palette.warning_color)
            });
            if segment.readable && ui.small_button("Show").clicked() {
                action = Some(SessionAction::ShowAuditSegment(index));
            }
        });
    }
    if let Some(shown) = &record.shown {
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("{} (read only)", shown.segment));
            if ui.small_button("Close").clicked() {
                action = Some(SessionAction::HideAuditSegment);
            }
        });
        let colour = if shown.sound {
            palette.muted_text_color()
        } else {
            palette.warning_color
        };
        ui.label(RichText::new(shown.note).small().color(colour));
        if shown.lines.len() > SHOWN_ENTRIES_LIMIT {
            ui.label(
                RichText::new(format!(
                    "The newest {SHOWN_ENTRIES_LIMIT} of {} entries are drawn; the file holds \
                     them all.",
                    shown.lines.len()
                ))
                .small()
                .color(palette.muted_text_color()),
            );
        }
        egui::Grid::new("audit_record_segment")
            .striped(true)
            .show(ui, |ui| {
                for line in shown.lines.iter().rev().take(SHOWN_ENTRIES_LIMIT) {
                    draw_audit_line(ui, line);
                }
            });
    }
    action
}

/// The node's audit record, on a linked desktop (GAP-179, D-116): read only, and read only
/// when a person asks.
fn draw_node_record(
    ui: &mut Ui,
    palette: &theme::Palette,
    node: &NodeAuditRecordView<'_>,
) -> Option<SessionAction> {
    let mut action = None;
    ui.separator();
    ui.strong(format!("The node's audit record ({})", node.node));
    let colour = if node.sound {
        palette.muted_text_color()
    } else {
        palette.warning_color
    };
    ui.label(RichText::new(node.summary).color(colour));
    if !node.note.is_empty() {
        ui.label(
            RichText::new(node.note)
                .small()
                .color(palette.warning_color),
        );
    }
    for problem in node.problems {
        ui.label(RichText::new(problem.as_str()).color(palette.warning_color));
    }
    ui.horizontal(|ui| {
        let ready = node.can_read && !node.reading;
        if ui
            .add_enabled(ready, egui::Button::new("Read the node's record"))
            .clicked()
        {
            action = Some(SessionAction::ReadNodeAuditRecord);
        }
        if ui
            .add_enabled(ready, egui::Button::new("Verify the node's record now"))
            .clicked()
        {
            action = Some(SessionAction::VerifyNodeAuditRecord);
        }
    });
    ui.label(
        RichText::new("Each read is recorded on the node's own audit record.")
            .small()
            .color(palette.muted_text_color()),
    );
    for (index, segment) in node.segments.iter().enumerate() {
        ui.horizontal(|ui| {
            let text = RichText::new(segment.description);
            ui.label(if segment.sound {
                text
            } else {
                text.color(palette.warning_color)
            });
            if segment.readable
                && ui
                    .add_enabled(
                        node.can_read && !node.reading,
                        egui::Button::new("Show").small(),
                    )
                    .clicked()
            {
                action = Some(SessionAction::ShowNodeAuditSegment(index));
            }
        });
    }
    if let Some(shown) = &node.shown {
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("{} on the node (read only)", shown.segment));
            let ready = node.can_read && !node.reading;
            if shown.has_older
                && ui
                    .add_enabled(ready, egui::Button::new("Older").small())
                    .clicked()
            {
                action = Some(SessionAction::PageNodeAuditSegment { older: true });
            }
            if shown.has_newer
                && ui
                    .add_enabled(ready, egui::Button::new("Newer").small())
                    .clicked()
            {
                action = Some(SessionAction::PageNodeAuditSegment { older: false });
            }
            if ui.small_button("Close").clicked() {
                action = Some(SessionAction::HideNodeAuditSegment);
            }
        });
        let colour = if shown.sound {
            palette.muted_text_color()
        } else {
            palette.warning_color
        };
        ui.label(RichText::new(shown.note).small().color(colour));
        egui::Grid::new("node_audit_record_segment")
            .striped(true)
            .show(ui, |ui| {
                for line in shown.lines.iter().rev() {
                    draw_audit_line(ui, line);
                }
            });
    }
    action
}

fn draw_audit_line(ui: &mut Ui, line: &AuditLine<'_>) {
    ui.label(format!("{} s", line.mission_time_s));
    ui.label(
        line.operator
            .map_or_else(|| "nobody".to_string(), |o| format!("operator {o}")),
    );
    ui.label(line.action);
    ui.label(line.detail);
    ui.end_row();
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
            row.decision.short(),
            row.plan.short(),
            row.issued.0,
            handoff::delivery_label(row.delivery)
        ))
        .strong(),
    );
    // The record's detail carries the whole identifiers with copy controls (D-61): this is
    // the after-action account, and the decision is what an effector's report and the
    // journal name.
    crate::panels::identifier::draw_full(ui, palette, "Decision", &row.decision.to_string());
    crate::panels::identifier::draw_full(ui, palette, "Plan", &row.plan.to_string());
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

    if let Some(a) = draw_record(ui, palette, &view.record) {
        action = Some(a);
    }
    if let Some(node) = &view.node {
        if let Some(a) = draw_node_record(ui, palette, node) {
            action = Some(a);
        }
    }

    ui.separator();
    ui.strong("Audit log: this run");
    if view.audit.is_empty() {
        ui.label(RichText::new("Nothing recorded yet.").color(palette.muted_text_color()));
    }
    egui::Grid::new("audit_log").striped(true).show(ui, |ui| {
        for line in view.audit.iter().rev().take(200) {
            draw_audit_line(ui, line);
        }
    });
    action
}
