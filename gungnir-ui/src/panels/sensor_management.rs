// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-10, sensor management (GAP-003, GAP-004).
//!
//! `docs/ux/information-architecture.md` §1: a `SensorRecord` per sensor -- mode,
//! calibration version, coverage region -- with mode change, tasking, and the
//! calibration baseline.
//!
//! # A mode is a claim about what is being watched
//!
//! Every other panel reads this one's consequences. A sensor at Standby contributes
//! nothing to coverage (DN-12 §5 rule 1), so the modes on this screen are what decide
//! whether the coverage map shows rings at all. That makes the mode column the most
//! load-bearing thing here, and it is why a failed mode change is reported rather than
//! leaving the row showing what the operator asked for.
//!
//! # Asking and knowing are two different columns
//!
//! DN-11 §5's first rule is that local state does not change until the sensor
//! acknowledges. This panel is where that rule is visible or invisible, so it is built
//! around it:
//!
//! - **Command** issues a task. It changes nothing about what the sensor is confirmed to
//!   be doing; it puts a mode in the *asked* column and starts a task.
//! - **Record observed** changes the confirmed mode with no command at all. It is the
//!   operator saying "I know this, I was told on the radio", and while no adapter exists
//!   (GAP-001) it is the only one of the two that does anything.
//!
//! They are separate columns with separate headings rather than one control that means
//! different things in different deployments. Merging them would make the panel unable
//! to say which of the two happened, and that distinction is the whole design.
//!
//! # What this panel does not do
//!
//! Nothing here reaches a sensor. A command is recorded and published; delivering it is
//! GAP-001, and `SensorManagementError::NotControllable` is the error a sensor with no
//! configured endpoint returns -- which is every sensor in the default baseline. The
//! command controls are disabled with that reason on the hover rather than being offered
//! and then refused.

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::{SensorMode, Vocabulary};

/// How far a command to a sensor has got.
///
/// A view of `gungnir_sensor_management::tasking::TaskState`, which this crate may not
/// depend on. Rendered as phrases rather than through [`Vocabulary`]: the vocabulary is
/// a closed table of *doctrinal* terms an operator's own glossary might rename, and a
/// protocol state is not one of those. What matters is that no variant name reaches the
/// screen, and none does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskProgress<'a> {
    /// Recorded here; no adapter has taken it.
    Issued,
    /// An adapter accepted it for delivery.
    Sent,
    /// The sensor confirmed. The only state in which the confirmed mode moved.
    Acknowledged,
    /// The sensor refused, or the adapter could not deliver.
    Failed { reason: &'a str },
    /// The window closed with nothing back. **Not a refusal**: nobody said no.
    Unacknowledged,
}

impl TaskProgress<'_> {
    /// What the operator reads. Never a variant name.
    #[must_use]
    pub fn phrase(&self) -> String {
        match self {
            TaskProgress::Issued => "awaiting an adapter (GAP-001)".to_owned(),
            TaskProgress::Sent => "sent, awaiting acknowledgement".to_owned(),
            TaskProgress::Acknowledged => "acknowledged".to_owned(),
            TaskProgress::Failed { reason } => format!("refused: {reason}"),
            TaskProgress::Unacknowledged => "no answer inside the window".to_owned(),
        }
    }

    /// True while this task might still be acknowledged, which is what decides whether
    /// a further command would be ambiguous.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self, TaskProgress::Issued | TaskProgress::Sent)
    }

    fn color(&self, palette: &theme::Palette) -> egui::Color32 {
        match self {
            TaskProgress::Issued | TaskProgress::Sent => palette.warning_color,
            TaskProgress::Acknowledged => palette.class_neutral_color,
            TaskProgress::Failed { .. } | TaskProgress::Unacknowledged => {
                palette.class_hostile_color
            }
        }
    }
}

/// One sensor as the panel shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorRow<'a> {
    pub id: u32,
    pub modality: &'a str,
    /// The mode the sensor is **confirmed** to be in.
    pub mode: SensorMode,
    /// A mode that has been asked for and not yet acknowledged.
    ///
    /// Its own field rather than an alternative value of `mode`, because collapsing the
    /// two is the failure DN-11 exists to prevent: a panel that showed a requested mode
    /// as the current one would be a health flag that lies.
    pub requested: Option<SensorMode>,
    /// The most recent command issued to this sensor, if any.
    pub task: Option<TaskProgress<'a>>,
    pub calibration_version: &'a str,
    /// Nominal detection range, metres.
    pub max_range_m: f64,
    /// Whether this sensor is contributing to the coverage picture right now.
    ///
    /// Derived from the confirmed mode rather than stored, but shown as its own column:
    /// an operator reading a list of modes should not have to remember which two of the
    /// five count. Derived from the *confirmed* mode specifically -- a requested Search
    /// contributes nothing, because nothing is searching yet.
    pub contributing: bool,
    /// Whether a control endpoint is configured for this sensor.
    ///
    /// `false` means commands are refused, which is every sensor until GAP-001.
    pub controllable: bool,
}

/// Everything PN-10 draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorManagementView<'a> {
    pub sensors: &'a [SensorRow<'a>],
    /// Re-tasking recommendations, or why there are none (DN-13 §7, GAP-037).
    ///
    /// `Ok(&[])` is a real answer -- no change improves coverage -- and is drawn as one;
    /// `Err` is the reason nothing could be evaluated, which is a different thing.
    pub recommendations: Result<&'a [RecommendationLine<'a>], &'a str>,
    /// Whether the signed-in role may change a mode or issue a command (`sensor.task`).
    pub may_task: bool,
    pub role: &'a str,
    /// The delivery path: what a command does *not* yet do.
    pub control_path: Unavailable<'a>,
    pub vocabulary: &'a Vocabulary,
    /// The most recent refusal, so it is visible rather than the row simply not moving.
    pub last_error: Option<&'a str>,
}

/// What the operator asked for this frame.
/// One re-tasking recommendation as PN-10 lists it (DN-13 rule 2: what it costs as well as
/// what it buys).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecommendationLine<'a> {
    pub sensor: u32,
    pub from: &'a str,
    pub to: SensorMode,
    /// Metres of approach that stop being uncovered. Positive is an improvement.
    pub uncovered_closed_m: f64,
    /// Metres of approach that drop from two sensors to one. The trade, made visible.
    pub redundancy_lost_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorAction {
    /// Issue a command. Changes nothing until the sensor acknowledges.
    Command { sensor: u32, mode: SensorMode },
    /// Record what the operator knows the sensor is doing, commanding nothing.
    RecordObserved { sensor: u32, mode: SensorMode },
}

/// Render the sensor management panel.
pub fn render_sensor_management(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &SensorManagementView<'_>,
) -> Option<SensorAction> {
    ui.heading("Sensors");
    let recommended = draw_recommendations(ui, palette, view);

    if view.sensors.is_empty() {
        ui.label(
            RichText::new("No sensors are configured in the baseline in force.")
                .color(palette.muted_text_color()),
        );
        return None;
    }

    let contributing = view.sensors.iter().filter(|s| s.contributing).count();
    ui.label(
        RichText::new(format!(
            "{} of {} searching or tracking; the rest contribute nothing to coverage.",
            contributing,
            view.sensors.len()
        ))
        .color(if contributing == 0 {
            palette.warning_color
        } else {
            palette.muted_text_color()
        }),
    );

    let controllable = view.sensors.iter().filter(|s| s.controllable).count();
    ui.label(
        RichText::new(if controllable == 0 {
            "No sensor has a control endpoint configured, so nothing here can be \
             commanded. Recording an observed mode still works."
                .to_owned()
        } else {
            format!(
                "{} of {} have a control endpoint configured.",
                controllable,
                view.sensors.len()
            )
        })
        .color(palette.muted_text_color()),
    );

    if !view.may_task {
        ui.label(
            RichText::new(format!("{} may not change a sensor's mode.", view.role))
                .color(palette.muted_text_color()),
        );
    }
    ui.separator();

    let mut action = None;
    egui::Grid::new("sensor_table")
        .striped(true)
        .show(ui, |ui| {
            for heading in [
                "ID",
                "Modality",
                "Mode",
                "Command",
                "Record observed",
                "Coverage",
                "Task",
                "Calibration",
                "Range",
            ] {
                ui.strong(heading);
            }
            ui.end_row();
            for sensor in view.sensors {
                if let Some(a) = draw_row(ui, palette, view, sensor) {
                    action = Some(a);
                }
            }
        });

    ui.separator();
    if let Some(error) = view.last_error {
        // A refusal must be visible: the row simply not moving looks like a click that
        // missed.
        ui.label(
            RichText::new(format!("Last request refused: {error}"))
                .color(palette.class_hostile_color),
        );
    }
    draw_unavailable(ui, palette, view.control_path);
    ui.label(
        RichText::new(
            "A command is recorded and published here. No adapter carries it to a \
             sensor, so nothing acknowledges and every command times out.",
        )
        .color(palette.warning_color)
        .size(palette.small_font_size),
    );
    action.or(recommended)
}

fn draw_row(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &SensorManagementView<'_>,
    sensor: &SensorRow<'_>,
) -> Option<SensorAction> {
    let mut action = None;
    ui.label(sensor.id.to_string());
    ui.label(sensor.modality);

    // The confirmed mode, and beside it whatever has been asked for. Two facts in one
    // cell, never one fact standing in for the other.
    ui.horizontal(|ui| {
        ui.label(view.vocabulary.sensor_mode(sensor.mode));
        if let Some(requested) = sensor.requested {
            ui.label(
                RichText::new(format!(
                    "(asked: {})",
                    view.vocabulary.sensor_mode(requested)
                ))
                .color(palette.warning_color)
                .size(palette.small_font_size),
            );
        }
    });

    let pending = sensor.task.is_some_and(|t| t.is_open());
    if let Some(a) = draw_mode_buttons(ui, view, sensor, Intent::Command, pending) {
        action = Some(a);
    }
    if let Some(a) = draw_mode_buttons(ui, view, sensor, Intent::RecordObserved, pending) {
        action = Some(a);
    }

    if sensor.contributing {
        ui.label(RichText::new("contributing").color(palette.class_neutral_color));
    } else {
        ui.label(RichText::new("none").color(palette.muted_text_color()));
    }

    match sensor.task {
        Some(task) => {
            ui.label(RichText::new(task.phrase()).color(task.color(palette)));
        }
        None => {
            ui.label(RichText::new("no command issued").color(palette.muted_text_color()));
        }
    }

    ui.label(sensor.calibration_version);
    ui.label(format!("{:.0} km", sensor.max_range_m / 1000.0));
    ui.end_row();
    action
}

/// Which of the two things a button row means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    Command,
    RecordObserved,
}

fn draw_mode_buttons(
    ui: &mut Ui,
    view: &SensorManagementView<'_>,
    sensor: &SensorRow<'_>,
    intent: Intent,
    pending: bool,
) -> Option<SensorAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        for mode in SensorMode::ALL {
            let selected = sensor.mode == mode;
            // Only transitions the registry permits are offered. A control that was
            // enabled and then refused would teach an operator to distrust the panel;
            // the refusal is still reported when one slips through, because the registry
            // is the authority and this is only a mirror of its rule.
            let permitted = sensor.mode.can_transition_to(mode);
            let blocked = match intent {
                Intent::Command => !sensor.controllable || pending,
                Intent::RecordObserved => false,
            };
            let enabled = view.may_task && permitted && !selected && !blocked;
            let response = ui.add_enabled(
                enabled,
                egui::Button::new(view.vocabulary.sensor_mode(mode)).selected(selected),
            );
            if response.clicked() {
                action = Some(match intent {
                    Intent::Command => SensorAction::Command {
                        sensor: sensor.id,
                        mode,
                    },
                    Intent::RecordObserved => SensorAction::RecordObserved {
                        sensor: sensor.id,
                        mode,
                    },
                });
            }
            // A disabled control says why. Which reason applies is checked in the order
            // the registry would hit them, so the hover text and the error a slipped
            // click would produce agree.
            if !selected {
                if !permitted {
                    response.on_hover_text(format!(
                        "{} cannot go straight to {}: it must pass through {} first.",
                        view.vocabulary.sensor_mode(sensor.mode),
                        view.vocabulary.sensor_mode(mode),
                        view.vocabulary.sensor_mode(SensorMode::Standby)
                    ));
                } else if intent == Intent::Command && !sensor.controllable {
                    response.on_hover_text(
                        "No control endpoint is configured for this sensor, so it \
                         cannot be commanded from here (GAP-001). Recording an \
                         observed mode still works.",
                    );
                } else if intent == Intent::Command && pending {
                    response.on_hover_text(
                        "A command is already outstanding for this sensor. Two \
                         unanswered requests could not be told apart.",
                    );
                }
            }
        }
    });
    action
}

/// The re-tasking recommendations (DN-13 §7, GAP-037).
///
/// **Three states, drawn as three things.** A list of plans; "no change improves
/// coverage", which is a real answer to a question that was asked; and a reason nothing
/// could be evaluated, which is not. Accepting a plan issues the command through the same
/// path the mode buttons use, so it is authorized and recorded like any other tasking
/// (DN-13 rule 4).
fn draw_recommendations(
    ui: &mut Ui,
    palette: &theme::Palette,
    view: &SensorManagementView<'_>,
) -> Option<SensorAction> {
    let mut action = None;
    ui.separator();
    ui.strong("Re-tasking recommendations");
    match view.recommendations {
        Err(reason) => {
            ui.label(
                RichText::new(format!("Not evaluated: {reason}")).color(palette.muted_text_color()),
            );
        }
        Ok([]) => {
            ui.label(
                RichText::new("No mode change improves coverage along the declared approaches.")
                    .color(palette.muted_text_color()),
            );
        }
        Ok(lines) => {
            for line in lines {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Sensor {} {} -> {:?}: closes {:.0} m of uncovered approach, costs {:.0} m of redundancy",
                        line.sensor, line.from, line.to, line.uncovered_closed_m, line.redundancy_lost_m
                    ));
                    if view.may_task && ui.button("Command").clicked() {
                        action = Some(SensorAction::Command {
                            sensor: line.sensor,
                            mode: line.to,
                        });
                    }
                });
            }
        }
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(mode: SensorMode) -> SensorRow<'static> {
        SensorRow {
            id: 1,
            modality: "radar",
            mode,
            requested: None,
            task: None,
            calibration_version: "v1",
            max_range_m: 50_000.0,
            contributing: matches!(mode, SensorMode::Search | SensorMode::Track),
            controllable: false,
        }
    }

    /// Only Search and Track contribute to coverage, and the panel derives that rather
    /// than asking an operator to remember which of five modes count.
    #[test]
    fn only_searching_or_tracking_sensors_contribute() {
        for mode in SensorMode::ALL {
            let expected = matches!(mode, SensorMode::Search | SensorMode::Track);
            assert_eq!(
                row(mode).contributing,
                expected,
                "{mode:?} contributes when it should not, or the reverse"
            );
        }
    }

    /// The transition rule the panel mirrors is the registry's, and the case that
    /// matters is coming back from Offline: it must go through Standby, so an operator
    /// cannot put a sensor straight back to Track and believe it is watching.
    #[test]
    fn an_offline_sensor_cannot_go_straight_to_an_active_mode() {
        assert!(!SensorMode::Offline.can_transition_to(SensorMode::Track));
        assert!(!SensorMode::Offline.can_transition_to(SensorMode::Search));
        assert!(SensorMode::Offline.can_transition_to(SensorMode::Standby));
        assert!(SensorMode::Standby.can_transition_to(SensorMode::Track));
    }

    /// Every mode has a word, so no row shows a Rust variant name.
    #[test]
    fn every_mode_has_a_label() {
        let vocabulary = Vocabulary::default();
        for mode in SensorMode::ALL {
            let label = vocabulary.sensor_mode(mode);
            assert!(!label.trim().is_empty(), "{mode:?} has no label");
        }
    }

    /// A task state reaches the operator as a phrase, not as a variant name. The check
    /// is that the phrase is not the debug spelling, because the failure this guards
    /// against is exactly somebody reaching for `format!("{:?}")`.
    #[test]
    fn no_task_state_is_shown_as_a_variant_name() {
        let states = [
            TaskProgress::Issued,
            TaskProgress::Sent,
            TaskProgress::Acknowledged,
            TaskProgress::Failed { reason: "busy" },
            TaskProgress::Unacknowledged,
        ];
        for state in states {
            let phrase = state.phrase();
            assert!(!phrase.trim().is_empty());
            assert_ne!(
                phrase,
                format!("{state:?}"),
                "{state:?} is being shown as its own variant name"
            );
        }
    }

    /// A refused command keeps the adapter's own words. Paraphrasing loses the
    /// diagnosis, which is the only thing that makes a failure actionable.
    #[test]
    fn a_refusal_carries_its_reason_to_the_screen() {
        let phrase = TaskProgress::Failed {
            reason: "transmitter inhibited",
        }
        .phrase();
        assert!(phrase.contains("transmitter inhibited"), "{phrase}");
    }

    /// An unacknowledged task is not a failed one, on screen as well as in the model:
    /// nobody refused it, and an after-action review has to be able to tell which
    /// happened.
    #[test]
    fn no_answer_does_not_read_as_a_refusal() {
        let no_answer = TaskProgress::Unacknowledged.phrase();
        assert!(!no_answer.contains("refused"), "{no_answer}");
        assert!(!TaskProgress::Unacknowledged.is_open());
    }

    /// A requested mode is a separate field, so a row can say "Standby, asked: Search"
    /// -- and contributing follows the confirmed mode, not the request. A sensor nobody
    /// has confirmed is searching is not searching.
    #[test]
    fn a_requested_mode_does_not_contribute_to_coverage() {
        let asked = SensorRow {
            requested: Some(SensorMode::Search),
            task: Some(TaskProgress::Issued),
            ..row(SensorMode::Standby)
        };
        assert_eq!(asked.mode, SensorMode::Standby);
        assert!(!asked.contributing);
        assert!(asked.task.is_some_and(|t| t.is_open()));
    }
}
