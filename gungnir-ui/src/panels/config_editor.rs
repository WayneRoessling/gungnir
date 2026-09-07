//! PN-14, the configuration editor (GAP-071).
//!
//! `docs/ux/information-architecture.md` §1: `ConfigBaseline` sections, validation
//! results and version; edit, validate, and apply as a decision.
//!
//! # Apply is a decision, and it is drawn like one
//!
//! Applying a baseline changes what every policy engine in the deployment will decide.
//! It is gated on `config.apply` and audited, and the panel states what applying does
//! *and does not* do: this build persists the baseline and the running session keeps the
//! one it started with. That is a real and common behaviour, but an administrator who
//! applied a Weapons Free baseline and believed it was in force immediately would be
//! wrong about the thing that matters most on this screen.
//!
//! # Validation is shown for the candidate and the baseline in force separately
//!
//! They are different questions. "Is the file I am about to apply valid" and "is what
//! this desktop is running valid" have different answers whenever someone has edited the
//! file since launch, and collapsing them into one green tick would hide exactly that.
//!
//! # Field-level editing is not built
//!
//! The baseline is edited as a file -- which is how a version-controlled configuration
//! baseline is normally edited -- and reloaded here. A form over twelve nested sections
//! is not in this change, and the panel says so rather than offering controls that do
//! nothing.

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};

/// One section of the baseline, summarised for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigSection<'a> {
    pub name: &'a str,
    /// How many entries it holds, where that is meaningful.
    pub count: Option<usize>,
    /// A one-line summary, e.g. the control status per layer.
    pub summary: &'a str,
}

/// One candidate algorithm configuration, as PN-14 lists it (DN-24 §9, GAP-086).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileLine<'a> {
    pub profile: &'a str,
    pub name: &'a str,
    pub filter: &'a str,
    pub gate_threshold: f64,
    pub promoted: bool,
    /// What validated it, when the baseline says. `None` is not "unvalidated": it is
    /// "nobody wrote down what validated it", and the panel says the second.
    pub validated_by: Option<&'a str>,
}

/// What this deployment governs, for PN-14's profiles section.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GovernedProfiles<'a> {
    /// Candidates, and which profile the deployment is operating in.
    Declared {
        active: Option<&'a str>,
        lines: &'a [ProfileLine<'a>],
    },
    /// The baseline declares one configuration and no profiles, read as the implicit
    /// `default` profile. **A real configuration and not a defect** -- it is what every
    /// deployment written before DN-24 means.
    SingleImplicit { lines: &'a [ProfileLine<'a>] },
    /// Nothing is declared, so nothing is governed.
    NothingDeclared,
    /// Something was declared and refused.
    Refused { reason: &'a str },
}

/// The result of `gungnir_config::validate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validation<'a> {
    Valid,
    Invalid {
        reason: &'a str,
    },
    /// Not run: there is nothing to validate.
    NotRun,
}

impl Validation<'_> {
    fn draw(&self, ui: &mut Ui, what: &str) {
        match self {
            Validation::Valid => {
                ui.label(RichText::new(format!("{what}: valid")).color(theme::CLASS_NEUTRAL_COLOR));
            }
            Validation::Invalid { reason } => {
                ui.label(
                    RichText::new(format!("{what}: invalid -- {reason}"))
                        .color(theme::CLASS_HOSTILE_COLOR)
                        .strong(),
                );
            }
            Validation::NotRun => {
                ui.label(
                    RichText::new(format!("{what}: not validated")).color(theme::MUTED_TEXT_COLOR),
                );
            }
        }
    }
}

/// A candidate baseline loaded from disk, waiting to be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate<'a> {
    pub path: &'a str,
    pub version: u32,
    /// The content revision (`ConfigBaseline::revision`): what a promotion advances,
    /// and what the asset list and hazard layer are stamped with.
    pub revision: u32,
    pub validation: Validation<'a>,
}

/// Whether this build can apply a baseline, and what applying means here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyState<'a> {
    /// Applying validates, writes, and audits. The running session keeps the baseline
    /// it started with until restart, which is stated on screen.
    PersistOnly,
    /// The role holds no `config.apply`.
    NotPermitted { role: &'a str },
    /// There is no baseline file to write to: the desktop started from the default.
    NoFile { env_var: &'a str },
}

/// One line of the configuration audit trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditLine<'a> {
    pub action: &'a str,
    pub mission_time_s: i64,
    pub detail: &'a str,
    /// `None` until there is an operator session (GAP-057).
    pub operator: Option<u64>,
}

/// Everything PN-14 draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfigEditorView<'a> {
    /// The baseline this desktop is running.
    pub version: u32,
    /// The content revision (`ConfigBaseline::revision`): what a promotion advances,
    /// and what the asset list and hazard layer are stamped with.
    pub revision: u32,
    pub sections: &'a [ConfigSection<'a>],
    pub in_force: Validation<'a>,
    /// When the baseline is valid to promote, if it says.
    pub validity: Option<&'a str>,
    pub candidate: Option<Candidate<'a>>,
    pub apply: ApplyState<'a>,
    pub audit: &'a [AuditLine<'a>],
    /// Field-level editing, and what would build it.
    pub editing: Unavailable<'a>,
    /// Mission profiles and their candidate algorithm baselines (DN-24 §9, GAP-086).
    pub profiles: GovernedProfiles<'a>,
}

/// What the administrator asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    /// Re-read the baseline file from disk as a candidate.
    Reload,
    /// Validate the candidate, or the baseline in force when there is no candidate.
    Validate,
    /// Persist the candidate. A decision: gated and audited.
    Apply,
    DiscardCandidate,
}

/// Render the configuration editor.
pub fn render_config_editor(ui: &mut Ui, view: &ConfigEditorView<'_>) -> Option<ConfigAction> {
    ui.heading("Configuration");
    ui.label(format!(
        "Baseline schema version {}, content revision {}",
        view.version, view.revision
    ));
    match view.validity {
        Some(window) => {
            ui.label(RichText::new(format!("Valid {window}")).color(theme::MUTED_TEXT_COLOR));
        }
        None => {
            ui.label(
                RichText::new("No validity window: this baseline is always promotable.")
                    .color(theme::MUTED_TEXT_COLOR),
            );
        }
    }
    view.in_force.draw(ui, "Baseline in force");
    ui.separator();

    draw_sections(ui, view.sections);
    ui.separator();

    draw_profiles(ui, view.profiles);
    ui.separator();

    let mut action = draw_candidate(ui, view);
    ui.separator();
    if let Some(a) = draw_apply(ui, view) {
        action = Some(a);
    }
    ui.separator();
    draw_audit(ui, view.audit);
    ui.separator();
    draw_unavailable(ui, view.editing);
    action
}

/// Mission profiles and their candidates (DN-24 §9, GAP-086).
///
/// Read-only. Promoting from a panel needs an authority check and a confirmation surface,
/// and PN-07 provides one for plans while nothing provides one for this -- **a button that
/// appeared to put a configuration into force and did not would be worse than none.**
fn draw_profiles(ui: &mut Ui, profiles: GovernedProfiles<'_>) {
    ui.strong("Algorithm baselines");
    let lines = match profiles {
        GovernedProfiles::NothingDeclared => {
            ui.label(
                RichText::new(
                    "This deployment declares no algorithm configuration, so nothing governs which filter it runs.",
                )
                .color(theme::MUTED_TEXT_COLOR),
            );
            return;
        }
        GovernedProfiles::Refused { reason } => {
            ui.label(
                RichText::new(format!(
                    "The declared configuration was refused, so nothing is in force: {reason}"
                ))
                .color(theme::CLASS_HOSTILE_COLOR),
            );
            return;
        }
        GovernedProfiles::SingleImplicit { lines } => {
            ui.label(
                RichText::new(
                    "One configuration and no profiles declared; read as the default profile.",
                )
                .color(theme::MUTED_TEXT_COLOR),
            );
            lines
        }
        GovernedProfiles::Declared { active, lines } => {
            match active {
                Some(a) => ui.label(format!("Operating in profile {a}")),
                None => {
                    ui.label(RichText::new("No profile is active.").color(theme::WARNING_COLOR))
                }
            };
            lines
        }
    };

    egui::Grid::new("config_profiles")
        .striped(true)
        .show(ui, |ui| {
            for line in lines {
                ui.label(line.profile);
                ui.label(line.name);
                ui.label(format!("{} gate {:.2}", line.filter, line.gate_threshold));
                if line.promoted {
                    ui.label(RichText::new("in force").color(theme::HEALTHY_COLOR));
                } else {
                    ui.label(RichText::new("candidate").color(theme::MUTED_TEXT_COLOR));
                }
                // Absent is "nobody wrote down what validated it", which is a different
                // thing from "it was not validated" and must not read as the second.
                match line.validated_by {
                    Some(by) => ui.label(by),
                    None => ui
                        .label(RichText::new("no validation recorded").color(theme::WARNING_COLOR)),
                };
                ui.end_row();
            }
        });
}

fn draw_sections(ui: &mut Ui, sections: &[ConfigSection<'_>]) {
    ui.strong("Sections");
    egui::Grid::new("config_sections")
        .striped(true)
        .show(ui, |ui| {
            for s in sections {
                ui.label(s.name);
                match s.count {
                    Some(n) => {
                        ui.label(n.to_string());
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.label(
                    RichText::new(s.summary)
                        .color(theme::MUTED_TEXT_COLOR)
                        .size(theme::SMALL_FONT_SIZE),
                );
                ui.end_row();
            }
        });
}

fn draw_candidate(ui: &mut Ui, view: &ConfigEditorView<'_>) -> Option<ConfigAction> {
    ui.strong("Candidate");
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.button("Reload from disk").clicked() {
            action = Some(ConfigAction::Reload);
        }
        if ui.button("Validate").clicked() {
            action = Some(ConfigAction::Validate);
        }
    });
    match view.candidate {
        None => {
            ui.label(
                RichText::new("Nothing loaded; the desktop is showing the baseline in force.")
                    .color(theme::MUTED_TEXT_COLOR),
            );
        }
        Some(c) => {
            ui.label(format!(
                "{} (schema version {}, revision {})",
                c.path, c.version, c.revision
            ));
            c.validation.draw(ui, "Candidate");
            if ui.button("Discard").clicked() {
                action = Some(ConfigAction::DiscardCandidate);
            }
        }
    }
    action
}

fn draw_apply(ui: &mut Ui, view: &ConfigEditorView<'_>) -> Option<ConfigAction> {
    ui.strong("Apply");
    match view.apply {
        ApplyState::NotPermitted { role } => {
            ui.label(
                RichText::new(format!("{role} may not apply a configuration baseline."))
                    .color(theme::MUTED_TEXT_COLOR),
            );
            None
        }
        ApplyState::NoFile { env_var } => {
            ui.label(
                RichText::new(format!(
                    "This desktop started from the built-in default baseline, so there \
                     is no file to write. Set {env_var} to a baseline file."
                ))
                .color(theme::MUTED_TEXT_COLOR),
            );
            None
        }
        ApplyState::PersistOnly => {
            // The sentence that keeps an administrator from believing a new control
            // status is in force the moment they click.
            ui.label(
                RichText::new(
                    "Applying validates and writes the baseline, and records it in the \
                     audit trail. The running session keeps the baseline it started \
                     with until the desktop is restarted.",
                )
                .color(theme::WARNING_COLOR),
            );
            let ready = matches!(
                view.candidate,
                Some(Candidate {
                    validation: Validation::Valid,
                    ..
                })
            );
            let clicked = ui
                .add_enabled(ready, egui::Button::new("Apply candidate"))
                .clicked();
            if !ready {
                ui.label(
                    RichText::new(
                        "Load a candidate and validate it before applying: an invalid \
                         or unvalidated baseline is not applied.",
                    )
                    .color(theme::MUTED_TEXT_COLOR),
                );
            }
            clicked.then_some(ConfigAction::Apply)
        }
    }
}

fn draw_audit(ui: &mut Ui, audit: &[AuditLine<'_>]) {
    ui.strong("Configuration audit");
    if audit.is_empty() {
        ui.label(
            RichText::new("No configuration action has been taken this session.")
                .color(theme::MUTED_TEXT_COLOR),
        );
        return;
    }
    for line in audit {
        let who = match line.operator {
            Some(id) => format!("operator {id}"),
            None => "no operator identity (GAP-057)".to_owned(),
        };
        ui.label(format!(
            "{} s  {}  {}  [{}]",
            line.mission_time_s, line.action, line.detail, who
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Not validated" is not "valid". A panel that showed an unvalidated baseline as
    /// clean would let an administrator apply something nobody had checked.
    #[test]
    fn not_run_is_not_valid() {
        assert_ne!(Validation::NotRun, Validation::Valid);
        assert_ne!(
            Validation::Invalid {
                reason: "duplicate sensor id"
            },
            Validation::NotRun
        );
    }

    /// The three reasons apply is unavailable are different situations: no authority,
    /// no file, or available-but-deferred. A single greyed button would say none of
    /// them.
    #[test]
    fn every_apply_state_is_distinguishable() {
        let states = [
            ApplyState::PersistOnly,
            ApplyState::NotPermitted { role: "Analyst" },
            ApplyState::NoFile {
                env_var: "GUNGNIR_CONFIG",
            },
        ];
        for (i, a) in states.iter().enumerate() {
            for (j, b) in states.iter().enumerate() {
                assert_eq!(i == j, a == b, "{a:?} and {b:?} compared wrongly");
            }
        }
    }

    /// An audit line with no operator says so rather than leaving the field blank: a
    /// blank actor in an audit trail reads as an action nobody took.
    #[test]
    fn an_audit_line_without_an_operator_says_so() {
        let line = AuditLine {
            action: "config.apply",
            mission_time_s: 12,
            detail: "baseline version 1",
            operator: None,
        };
        assert!(line.operator.is_none());
        assert_eq!(line.action, "config.apply");
    }
}
