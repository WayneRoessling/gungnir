// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Renders the current intercept plan (resource -> track assignments, intercept
//! geometry where known). Read-only over `gungnir_model::PlanView`; the accept /
//! override / reject controls belong to the approval workflow in `gungnir-command`
//! and are added here once `gungnir-app` wires that crate in.

use crate::theme;
use egui::RichText;
use gungnir_model::PlanView;

/// A resource the planner declined to propose, as PN-05 lists it (DN-04 §5, GAP-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WithheldLine<'a> {
    pub resource: u32,
    pub reason: &'a str,
}

pub fn render_intercept_panel(
    ui: &mut egui::Ui,
    plan: &PlanView,
    withheld: &[WithheldLine<'_>],
    fires: &[FiresCheckLine<'_>],
    handoffs: &[HandoffLine<'_>],
    alternatives: &Alternatives<'_>,
) {
    ui.heading("Intercept plan");
    render_summary(ui, plan);
    render_solution_list(ui, plan);
    render_fires(ui, plan, fires);
    render_withheld(ui, withheld);
    render_handoffs(ui, handoffs);
    render_alternatives(ui, alternatives);
}

/// One course of action beside the recommendation, as PN-05 lists it (GAP-032).
///
/// `verdict` and `denied` are separate because they answer different questions: the first
/// is what the policy chain said, in the words the record uses, and the second is whether
/// any decision could make this option actionable. A panel that showed only the sentence
/// would leave the reader to parse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlternativeLine<'a> {
    /// The full rationale: what distinguishes this option, then one line per assignment
    /// highest risk first.
    pub rationale: &'a str,
    /// The verdict the policy chain returned for this option, as the record spells it.
    pub verdict: &'a str,
    /// True when policy has already refused it.
    pub denied: bool,
    pub assignments: usize,
}

/// The courses of action beside the plan in force (GAP-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Alternatives<'a> {
    /// Ranked alternatives to the recommendation, the ones that could still be acted on
    /// ahead of the ones policy has already refused.
    pub options: &'a [AlternativeLine<'a>],
    /// The rehearsal against a hypothetical picture, when the operator has posed one.
    /// `None` draws nothing: a what-if nobody asked for is a hypothesis nobody posed.
    pub what_if: Option<AlternativeLine<'a>>,
}

/// The options considered and refused alongside the one recommended.
///
/// **A refused alternative is drawn, not filtered out.** An operator who cannot see that
/// the obvious second option is barred by policy will ask for it on the radio, and the
/// answer will arrive later than this line would have.
fn render_alternatives(ui: &mut egui::Ui, alternatives: &Alternatives<'_>) {
    if alternatives.options.is_empty() && alternatives.what_if.is_none() {
        return;
    }
    if !alternatives.options.is_empty() {
        ui.separator();
        ui.label(RichText::new("Alternatives").strong());
        for option in alternatives.options {
            render_course(ui, option);
        }
    }
    if let Some(what_if) = &alternatives.what_if {
        ui.separator();
        ui.label(RichText::new("What if").strong());
        render_course(ui, what_if);
    }
}

/// One course of action: its verdict first, then the rationale that produced it.
fn render_course(ui: &mut egui::Ui, course: &AlternativeLine<'_>) {
    let headline = format!("{} assignment(s): {}", course.assignments, course.verdict);
    let text = RichText::new(headline);
    ui.label(if course.denied {
        text.color(theme::WARNING_COLOR)
    } else {
        text
    });
    ui.label(RichText::new(course.rationale).color(theme::MUTED_TEXT_COLOR));
}

/// One issued handoff and where its delivery stands (DN-07 §7, GAP-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffLine<'a> {
    pub decision: u64,
    /// `None` is manual delivery: a radio call the operator must make.
    pub endpoint: Option<&'a str>,
    pub delivered: bool,
    pub detail: &'a str,
}

/// **Manual delivery is stated plainly**, and an undelivered handoff is never shown as
/// delivered.
fn render_handoffs(ui: &mut egui::Ui, handoffs: &[HandoffLine<'_>]) {
    if handoffs.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Handoffs").strong());
    for h in handoffs {
        let line = match (h.endpoint, h.delivered) {
            (None, _) => format!(
                "decision {}: MANUAL delivery, no endpoint configured; make the call. {}",
                h.decision, h.detail
            ),
            (Some(e), true) => format!("decision {}: delivered to {e}. {}", h.decision, h.detail),
            (Some(e), false) => {
                format!("decision {}: UNDELIVERED to {e}. {}", h.decision, h.detail)
            }
        };
        let text = RichText::new(line);
        ui.label(if h.delivered {
            text
        } else {
            text.color(theme::WARNING_COLOR)
        });
    }
}

/// One deconfliction check as PN-05 lists it (DN-05 §7, GAP-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiresCheckLine<'a> {
    pub check: &'a str,
    pub passed: bool,
    pub detail: &'a str,
}

/// A fires task: the target, its location error, the firing unit, and **every check with
/// its result, failed ones as text and not colour alone** (DN-05 §7).
fn render_fires(ui: &mut egui::Ui, plan: &PlanView, checks: &[FiresCheckLine<'_>]) {
    let Some(fires) = plan.fires() else {
        return;
    };
    ui.separator();
    ui.label(RichText::new("Fires task").strong());
    ui.label(format!(
        "Target track {}, location error {:.0} m, firing unit {}",
        fires.target.0, fires.location_error_m, fires.firing_unit.0
    ));
    if checks.is_empty() {
        ui.label(RichText::new("Deconfliction checks not evaluated.").color(theme::WARNING_COLOR));
        return;
    }
    for c in checks {
        if c.passed {
            ui.label(format!("PASSED {}: {}", c.check, c.detail));
        } else {
            ui.label(
                RichText::new(format!("FAILED {}: {}", c.check, c.detail))
                    .color(theme::WARNING_COLOR),
            );
        }
    }
}

/// Resources not proposed, and why. **Drawn only when there are any**: a standing
/// "nothing withheld" line would be read past, and the case that matters is the one where
/// a resource with rounds in it is being held back on purpose.
fn render_withheld(ui: &mut egui::Ui, withheld: &[WithheldLine<'_>]) {
    if withheld.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Not proposed").strong());
    for w in withheld {
        ui.label(
            RichText::new(format!("Resource {}: {}", w.resource, w.reason))
                .color(theme::WARNING_COLOR),
        );
    }
}

fn render_summary(ui: &mut egui::Ui, plan: &PlanView) {
    ui.label(
        RichText::new(format!(
            "Plan #{} at t = {:.1} s, policy value {:.2}",
            plan.id.0, plan.mission_time.0, plan.policy_value
        ))
        .color(theme::MUTED_TEXT_COLOR),
    );
    if plan.is_empty() {
        ui.label(RichText::new("No assignments").italics());
    }
}

fn render_solution_list(ui: &mut egui::Ui, plan: &PlanView) {
    if plan.is_empty() {
        return;
    }
    egui::Grid::new("intercept_solutions")
        .striped(true)
        .show(ui, |ui| {
            for heading in ["Resource", "Track", "Intercept point", "Time to intercept"] {
                ui.strong(heading);
            }
            ui.end_row();
            for s in plan.solutions() {
                ui.label(s.resource.0.to_string());
                ui.label(s.track.0.to_string());
                ui.label(s.intercept_point.map_or_else(
                    || "n/a".to_string(),
                    |g| {
                        format!(
                            "{:.4} deg, {:.4} deg, {:.0} m",
                            g.lat_rad.to_degrees(),
                            g.lon_rad.to_degrees(),
                            g.alt_m
                        )
                    },
                ));
                ui.label(
                    s.time_to_intercept_s
                        .map_or_else(|| "n/a".to_string(), |t| format!("{t:.0} s")),
                );
                ui.end_row();
            }
        });
}
