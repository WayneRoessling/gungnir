//! PN-15, requirements and tasking (GAP-005).
//!
//! `docs/ux/information-architecture.md` §1: requirement objects with status, for the
//! intelligence analyst, the sensor manager, and the commander; the actions are state,
//! task, and decline.
//!
//! # Two roles, two authorities, one panel
//!
//! MT-08 splits the work between people who may not do each other's job, and the
//! authority matrix already says so: `Role::IntelligenceAnalyst` deliberately does **not**
//! hold `sensor.task`, because DN-11 §4 makes sensor tasking a *request* from the analyst
//! rather than authority they hold. So this panel draws two different permissions:
//!
//! - **Stating** a requirement is the analyst saying what they need to know. It is not an
//!   authorized action and needs no matrix row -- asking is not acting.
//! - **Tasking or declining** it is the sensor manager's concurrence, which is
//!   `sensor.task`.
//!
//! A panel that offered both to everybody would put the analyst in the position of
//! tasking their own requirement, which is the control MT-08 step 3 exists to be.
//!
//! # What "tasked" means here, and what it does not
//!
//! A requirement is tasked when a sensor task exists *and* somebody concurred. Neither
//! half alone. And a task the sensor acknowledged is **not** an answer: `Working` is as
//! far as the tasks can carry a requirement, and moving it to answered is a person's
//! judgement that always names its evidence.
//!
//! # Why the areas come from the defended assets
//!
//! A requirement needs an area, and there is no way to draw one: PN-16 is unbuilt and
//! nothing on any screen turns a click into a position. Rather than a form asking an
//! analyst to type latitude and longitude in radians, the areas offered are the ones the
//! baseline already declares. A deployment with no assets declared gets a stated reason
//! instead of an empty picker, because a requirement over nowhere is worse than none.

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::AssetPriority;

/// Where a requirement's collection stands, derived from the tasks serving it.
///
/// A view of `gungnir_workflow::CollectionProgress`, which this crate may not depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Untasked,
    Awaiting,
    /// At least one task was acknowledged. **Not the same as answered.**
    Working,
    /// Every task ended without acknowledgement.
    Stalled,
}

impl Progress {
    /// What the operator reads. Never a variant name, and `Working` is worded so it
    /// cannot be mistaken for an answer.
    #[must_use]
    pub fn phrase(self) -> &'static str {
        match self {
            Progress::Untasked => "no sensor tasked",
            Progress::Awaiting => "awaiting acknowledgement",
            Progress::Working => "a sensor is on it; not yet answered",
            Progress::Stalled => "every task ended without an answer",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Progress::Untasked => theme::MUTED_TEXT_COLOR,
            Progress::Awaiting => theme::WARNING_COLOR,
            Progress::Working => theme::CLASS_NEUTRAL_COLOR,
            Progress::Stalled => theme::CLASS_HOSTILE_COLOR,
        }
    }
}

/// A requirement's state as the panel words it.
///
/// A view of `gungnir_model::RequirementState`. The attribution arrives already resolved
/// to a phrase, because whether a concurrence carried an operator is a fact about the
/// deployment's operator session and not something a panel should infer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing<'a> {
    Stated,
    Tasked {
        by: &'a str,
    },
    Satisfied {
        evidence: &'a str,
    },
    Declined {
        by: &'a str,
        reason: &'a str,
    },
    /// The needed-by time passed with nobody deciding either way. **Not a decline.**
    Lapsed,
}

impl Standing<'_> {
    #[must_use]
    pub fn phrase(&self) -> String {
        match self {
            Standing::Stated => "stated".to_owned(),
            Standing::Tasked { by } => format!("tasked, concurred by {by}"),
            Standing::Satisfied { evidence } => format!("answered: {evidence}"),
            Standing::Declined { by, reason } => format!("declined by {by}: {reason}"),
            Standing::Lapsed => "lapsed: the time passed with nobody deciding".to_owned(),
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self, Standing::Stated | Standing::Tasked { .. })
    }

    fn color(&self) -> egui::Color32 {
        match self {
            Standing::Stated => theme::WARNING_COLOR,
            Standing::Tasked { .. } => theme::CLASS_NEUTRAL_COLOR,
            Standing::Satisfied { .. } => theme::CLASS_FRIENDLY_COLOR,
            Standing::Declined { .. } | Standing::Lapsed => theme::MUTED_TEXT_COLOR,
        }
    }
}

/// One requirement and the tasks serving it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RequirementRow<'a> {
    pub id: u64,
    pub title: &'a str,
    pub priority: AssetPriority,
    pub standing: Standing<'a>,
    pub progress: Progress,
    /// How many sensor tasks serve it.
    pub tasks: usize,
    /// Time left before its needed-by, in seconds. `None` means none was set, which
    /// means it never lapses -- a decision, and different from an overdue one.
    pub time_remaining_s: Option<f64>,
}

/// An area a requirement can be stated over: a defended asset the baseline declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaChoice<'a> {
    pub name: &'a str,
}

/// A sensor a requirement can be tasked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SensorChoice<'a> {
    pub id: u32,
    pub modality: &'a str,
    /// Whether a command to it can leave this machine. `false` for every sensor until
    /// GAP-001, and the reason tasking it is refused rather than appearing to work.
    pub controllable: bool,
}

/// Where the list came from, so an empty one says which kind of empty it is.
///
/// **"Nothing has been asked for" and "we could not read the record" look identical on
/// screen and mean opposite things**: the first says there is no outstanding collection,
/// the second says nobody knows. A view of `gungnir_app::requirements::Recovered`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOrigin<'a> {
    /// Read from the record, which held nothing.
    NothingStated,
    /// Read from the record, which held these.
    FromRecord { sessions: usize },
    /// The record could not be read. **Whether anything is outstanding is unknown.**
    Unreadable { reason: &'a str },
}

impl ListOrigin<'_> {
    /// What an empty list says about itself.
    #[must_use]
    pub fn empty_sentence(&self) -> String {
        match self {
            ListOrigin::NothingStated | ListOrigin::FromRecord { .. } => {
                "Nothing has been asked for yet.".to_owned()
            }
            ListOrigin::Unreadable { reason } => format!(
                "This list is empty because the record could not be read ({reason}). \
                 That is not the same as nothing being outstanding: whether anything is \
                 outstanding is unknown."
            ),
        }
    }

    #[must_use]
    pub fn is_fault(&self) -> bool {
        matches!(self, ListOrigin::Unreadable { .. })
    }
}

/// Why the panel cannot offer to state a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannotState {
    /// The baseline declares no defended assets, so there is no area to state one over.
    NoAreas,
}

impl CannotState {
    #[must_use]
    pub fn sentence(self) -> &'static str {
        match self {
            CannotState::NoAreas => {
                "This baseline declares no defended assets, so there is no area to state \
                 a requirement over. Nothing on any screen turns a click into a position \
                 yet, so an area has to come from the configuration."
            }
        }
    }
}

/// Everything PN-15 draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RequirementsView<'a> {
    pub rows: &'a [RequirementRow<'a>],
    /// Areas a new requirement can be stated over, or why there are none.
    pub areas: Result<&'a [AreaChoice<'a>], CannotState>,
    pub sensors: &'a [SensorChoice<'a>],
    /// Whether the signed-in role may concur with tasking, or decline (`sensor.task`).
    ///
    /// Deliberately separate from stating: the intelligence analyst may state and may
    /// not concur, which is the whole shape of MT-08 steps 2 and 3.
    pub may_concur: bool,
    pub role: &'a str,
    /// Whether an operator session exists to attribute a concurrence to.
    ///
    /// `false` today (GAP-057). A concurrence is still recorded, and says that nobody
    /// was signed in rather than putting a role in a field that reads as a person.
    pub operator_session: bool,
    /// Where the requirement list is kept between sessions.
    pub persistence: Unavailable<'a>,
    pub last_error: Option<&'a str>,
    /// Where the list came from (GAP-005).
    pub origin: ListOrigin<'a>,
}

/// What the operator has typed but not yet committed.
///
/// Panel-local scratch, held by the caller across frames the way `DecisionDialogState`
/// is: a half-typed title that vanished when the panel lost focus would be worse than
/// no form at all. None of it is mission state.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Draft {
    pub title: String,
    /// Index into the view's `areas`.
    pub area: usize,
    /// Minutes until it is needed. Empty means no needed-by time, so it never lapses.
    pub within_minutes: String,
    pub priority: AssetPriority,
    /// The requirement the concurrence controls act on.
    pub selected: Option<u64>,
    pub reason: String,
    pub evidence: String,
}

impl Draft {
    /// Forget the composed action, leaving the new-requirement form alone.
    ///
    /// Called after an action is applied: leaving a decline reason in the box after it
    /// has been recorded invites it being sent twice against the next requirement.
    pub fn clear_action(&mut self) {
        self.selected = None;
        self.reason.clear();
        self.evidence.clear();
    }

    /// Forget the new-requirement form.
    pub fn clear_statement(&mut self) {
        self.title.clear();
        self.within_minutes.clear();
        self.priority = AssetPriority::default();
    }
}

/// What the operator asked for this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum RequirementAction {
    /// State a new requirement over the area at `area`.
    State {
        title: String,
        area: usize,
        priority: AssetPriority,
        /// Minutes from now. `None` means no deadline, so it never lapses.
        within_minutes: Option<f64>,
    },
    /// Task a sensor against a requirement and record the concurrence. One action, not
    /// two: `RequirementState::Tasked` means a task exists *and* somebody concurred, so
    /// a control that recorded only the concurrence could produce a state the model
    /// says is impossible.
    Task {
        requirement: u64,
        sensor: u32,
    },
    Decline {
        requirement: u64,
        reason: String,
    },
    Satisfy {
        requirement: u64,
        evidence: String,
    },
}

/// Render the requirements panel.
pub fn render_requirements(
    ui: &mut Ui,
    view: &RequirementsView<'_>,
    draft: &mut Draft,
) -> Option<RequirementAction> {
    let mut action = None;
    ui.heading("Collection requirements");

    let open = view.rows.iter().filter(|r| r.standing.is_open()).count();
    ui.label(
        RichText::new(format!("{} of {} still open.", open, view.rows.len()))
            .color(theme::MUTED_TEXT_COLOR),
    );
    if !view.may_concur {
        ui.label(
            RichText::new(format!(
                "{} may state a requirement but not concur with tasking it. Sensor \
                 tasking is a request from the analyst, not authority they hold; the \
                 sensor manager concurs.",
                view.role
            ))
            .color(theme::MUTED_TEXT_COLOR),
        );
    }
    ui.separator();

    if view.rows.is_empty() {
        ui.label(
            RichText::new(view.origin.empty_sentence()).color(if view.origin.is_fault() {
                theme::CLASS_HOSTILE_COLOR
            } else {
                theme::MUTED_TEXT_COLOR
            }),
        );
    } else {
        draw_table(ui, view, draft);
        // A partial list is worse than an empty one if nobody says it is partial.
        if let ListOrigin::Unreadable { reason } = view.origin {
            ui.label(
                RichText::new(format!(
                    "This list may be incomplete: the record could not be read past a \
                     point ({reason})."
                ))
                .color(theme::CLASS_HOSTILE_COLOR),
            );
        }
    }

    ui.separator();
    if let Some(a) = draw_actions(ui, view, draft) {
        action = Some(a);
    }

    ui.separator();
    if let Some(a) = draw_state_form(ui, view, draft) {
        action = Some(a);
    }

    ui.separator();
    if let Some(error) = view.last_error {
        ui.label(RichText::new(format!("Refused: {error}")).color(theme::CLASS_HOSTILE_COLOR));
    }
    if !view.operator_session {
        ui.label(
            RichText::new(
                "No operator session exists, so a concurrence records the role that \
                 acted and states that nobody was signed in. It does not name a person.",
            )
            .color(theme::WARNING_COLOR)
            .size(theme::SMALL_FONT_SIZE),
        );
    }
    draw_unavailable(ui, view.persistence);
    action
}

fn draw_table(ui: &mut Ui, view: &RequirementsView<'_>, draft: &mut Draft) {
    egui::Grid::new("requirements_table")
        .striped(true)
        .show(ui, |ui| {
            for heading in [
                "",
                "Priority",
                "Need",
                "Standing",
                "Collection",
                "Tasks",
                "Due",
            ] {
                ui.strong(heading);
            }
            ui.end_row();

            for row in view.rows {
                let selected = draft.selected == Some(row.id);
                if ui
                    .selectable_label(selected, format!("#{}", row.id))
                    .clicked()
                {
                    // Changing which requirement is selected drops the text composed for
                    // the previous one: a reason typed against #1 must not be applied
                    // to #2 by a stray click.
                    draft.clear_action();
                    if !selected {
                        draft.selected = Some(row.id);
                    }
                }
                ui.label(priority_word(row.priority));
                ui.label(row.title);
                ui.label(RichText::new(row.standing.phrase()).color(row.standing.color()));
                // The collection column is only meaningful while the requirement is
                // open: an answered one says how it was answered, not how it was worked.
                if row.standing.is_open() {
                    ui.label(RichText::new(row.progress.phrase()).color(row.progress.color()));
                } else {
                    ui.label(RichText::new("--").color(theme::MUTED_TEXT_COLOR));
                }
                ui.label(row.tasks.to_string());
                draw_due(ui, row);
                ui.end_row();
            }
        });
}

/// Priority as a word rather than a variant name.
fn priority_word(priority: AssetPriority) -> &'static str {
    match priority {
        AssetPriority::Low => "low",
        AssetPriority::Medium => "medium",
        AssetPriority::High => "high",
        AssetPriority::Critical => "critical",
    }
}

fn draw_due(ui: &mut Ui, row: &RequirementRow<'_>) {
    match row.time_remaining_s {
        // No needed-by time is a decision, not a blank: it means the requirement stands
        // until somebody closes it. Saying "none set" rather than leaving the cell empty
        // is the same rule PN-06 applies to an expiry.
        None => {
            ui.label(RichText::new("no time set").color(theme::MUTED_TEXT_COLOR));
        }
        Some(s) if s <= 0.0 => {
            ui.label(RichText::new("overdue").color(theme::CLASS_HOSTILE_COLOR));
        }
        Some(s) => {
            ui.label(
                RichText::new(format!("{:.0} min", s / 60.0)).color(if s < 300.0 {
                    theme::WARNING_COLOR
                } else {
                    theme::MUTED_TEXT_COLOR
                }),
            );
        }
    }
}

/// The concurrence actions, for whichever requirement is selected.
///
/// Long because each of the three controls carries its own precondition and its own
/// sentence saying what it needs; splitting them would put the reason a control is
/// disabled further from the control.
#[allow(clippy::too_many_lines)]
fn draw_actions(
    ui: &mut Ui,
    view: &RequirementsView<'_>,
    draft: &mut Draft,
) -> Option<RequirementAction> {
    let mut action = None;
    let Some(id) = draft.selected else {
        ui.label(
            RichText::new("Select a requirement to task, decline, or answer it.")
                .color(theme::MUTED_TEXT_COLOR),
        );
        return None;
    };
    let row = view.rows.iter().find(|r| r.id == id)?;
    if !row.standing.is_open() {
        ui.label(
            RichText::new(
                "This one is closed. A closed requirement is not reopened by tasking \
                 against it; state a new one.",
            )
            .color(theme::MUTED_TEXT_COLOR),
        );
        return None;
    }

    ui.horizontal(|ui| {
        ui.strong("Task a sensor:");
        if view.sensors.is_empty() {
            ui.label(RichText::new("No sensors are configured.").color(theme::MUTED_TEXT_COLOR));
        }
        for sensor in view.sensors {
            // The same rule PN-10 applies: a sensor with no control endpoint cannot be
            // commanded, so tasking it would record a concurrence against a task that
            // was never created.
            let enabled = view.may_concur && sensor.controllable;
            let response = ui.add_enabled(
                enabled,
                egui::Button::new(format!("{} #{}", sensor.modality, sensor.id)),
            );
            if response.clicked() {
                action = Some(RequirementAction::Task {
                    requirement: id,
                    sensor: sensor.id,
                });
            }
            if !sensor.controllable {
                response.on_hover_text(
                    "No control endpoint is configured for this sensor, so no task can \
                     be issued to it. Concurring against a task that does not exist \
                     would report work in hand that nobody is doing.",
                );
            } else if !view.may_concur {
                response.on_hover_text(
                    "Concurrence is the sensor manager's, under the sensor.task \
                     authority.",
                );
            }
        }
    });

    ui.horizontal(|ui| {
        ui.strong("Decline:");
        ui.add(
            egui::TextEdit::singleline(&mut draft.reason)
                .hint_text("why no sensor will serve this")
                .desired_width(260.0),
        );
        // A reason is required, so the control stays disabled without one rather than
        // being offered and then refused.
        let enabled = view.may_concur && !draft.reason.trim().is_empty();
        let response = ui.add_enabled(enabled, egui::Button::new("Decline"));
        if response.clicked() {
            action = Some(RequirementAction::Decline {
                requirement: id,
                reason: draft.reason.clone(),
            });
        }
        if draft.reason.trim().is_empty() {
            ui.label(
                RichText::new("a reason is required")
                    .color(theme::MUTED_TEXT_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
        }
    });

    ui.horizontal(|ui| {
        ui.strong("Answer:");
        ui.add(
            egui::TextEdit::singleline(&mut draft.evidence)
                .hint_text("the evidence that answered it")
                .desired_width(260.0),
        );
        // Answering is the analyst's judgement rather than an authorized action, so it
        // is not gated on sensor.task. It is gated on naming evidence: a requirement
        // marked answered with nothing behind it is the failure DN-11 names.
        let enabled = !draft.evidence.trim().is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new("Mark answered"))
            .clicked()
        {
            action = Some(RequirementAction::Satisfy {
                requirement: id,
                evidence: draft.evidence.clone(),
            });
        }
    });
    ui.label(
        RichText::new(
            "A sensor acknowledging a task is not an answer; answering is a person's \
             judgement and always names its evidence.",
        )
        .color(theme::MUTED_TEXT_COLOR)
        .size(theme::SMALL_FONT_SIZE),
    );

    action
}

/// The form for stating a new requirement.
fn draw_state_form(
    ui: &mut Ui,
    view: &RequirementsView<'_>,
    draft: &mut Draft,
) -> Option<RequirementAction> {
    let mut action = None;
    ui.strong("State a requirement");

    let areas = match view.areas {
        Err(reason) => {
            ui.label(RichText::new(reason.sentence()).color(theme::WARNING_COLOR));
            return None;
        }
        Ok(areas) => areas,
    };

    ui.add(
        egui::TextEdit::singleline(&mut draft.title)
            .hint_text("what do you need to know?")
            .desired_width(320.0),
    );
    ui.horizontal(|ui| {
        ui.label("Over:");
        for (index, area) in areas.iter().enumerate() {
            if ui
                .add(egui::Button::new(area.name).selected(draft.area == index))
                .clicked()
            {
                draft.area = index;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Priority:");
        for priority in [
            AssetPriority::Low,
            AssetPriority::Medium,
            AssetPriority::High,
            AssetPriority::Critical,
        ] {
            if ui
                .add(
                    egui::Button::new(priority_word(priority)).selected(draft.priority == priority),
                )
                .clicked()
            {
                draft.priority = priority;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Needed within:");
        ui.add(
            egui::TextEdit::singleline(&mut draft.within_minutes)
                .hint_text("minutes; blank for no deadline")
                .desired_width(160.0),
        );
        if draft.within_minutes.trim().is_empty() {
            ui.label(
                RichText::new("no deadline: it stands until somebody closes it")
                    .color(theme::MUTED_TEXT_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
        }
    });

    // A deadline that was typed but does not parse must not be silently dropped: the
    // analyst would get a requirement that never lapses when they asked for one that
    // does.
    let typed = draft.within_minutes.trim();
    let minutes: Option<f64> = typed.parse().ok();
    let deadline_broken = !typed.is_empty() && minutes.is_none_or(|m| m <= 0.0);
    if deadline_broken {
        ui.label(
            RichText::new(
                "That is not a number of minutes. Leave it blank for no deadline rather \
                 than stating one that would never lapse.",
            )
            .color(theme::CLASS_HOSTILE_COLOR)
            .size(theme::SMALL_FONT_SIZE),
        );
    }

    let enabled = !draft.title.trim().is_empty() && draft.area < areas.len() && !deadline_broken;
    if ui
        .add_enabled(enabled, egui::Button::new("State it"))
        .clicked()
    {
        action = Some(RequirementAction::State {
            title: draft.title.clone(),
            area: draft.area,
            priority: draft.priority,
            within_minutes: minutes,
        });
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the whole panel is built around. A sensor taking a command is
    /// not an answer, and the words on screen must not let the two be read as one.
    #[test]
    fn working_does_not_read_as_answered() {
        let working = Progress::Working.phrase();
        assert!(
            working.contains("not yet answered"),
            "a sensor being on it reads as an answer: {working}"
        );
        assert!(!Progress::Untasked.phrase().is_empty());
    }

    /// A lapse is not a decline: nobody refused it. The same distinction DN-10 draws
    /// between an expiry and a rejection.
    #[test]
    fn a_lapse_does_not_read_as_a_decline() {
        let lapsed = Standing::Lapsed.phrase();
        assert!(!lapsed.contains("declined"), "{lapsed}");
        assert!(lapsed.contains("nobody"), "{lapsed}");
        assert!(!Standing::Lapsed.is_open());
    }

    /// Answered always names its evidence, and declined always names its reason -- on
    /// screen, not merely in the model.
    #[test]
    fn a_closed_requirement_shows_what_closed_it() {
        let answered = Standing::Satisfied {
            evidence: "imagery at 01:42 shows the hull number",
        }
        .phrase();
        assert!(answered.contains("hull number"), "{answered}");

        let declined = Standing::Declined {
            by: "the sensor manager (nobody signed in)",
            reason: "no sensor can reach that area",
        }
        .phrase();
        assert!(declined.contains("no sensor can reach"), "{declined}");
        assert!(declined.contains("sensor manager"), "{declined}");
    }

    /// Stated and tasked are both still open; the other three are not.
    #[test]
    fn only_stated_and_tasked_are_open() {
        assert!(Standing::Stated.is_open());
        assert!(Standing::Tasked { by: "someone" }.is_open());
        assert!(!Standing::Satisfied { evidence: "e" }.is_open());
        assert!(!Standing::Declined {
            by: "someone",
            reason: "r"
        }
        .is_open());
        assert!(!Standing::Lapsed.is_open());
    }

    /// An empty list must say **which** kind of empty. Nothing asked for and a record
    /// that could not be read look identical and mean opposite things.
    #[test]
    fn an_empty_list_says_which_kind_of_empty_it_is() {
        let nothing = ListOrigin::NothingStated.empty_sentence();
        let unknown = ListOrigin::Unreadable {
            reason: "the journal is encrypted and no key is configured",
        }
        .empty_sentence();

        assert_ne!(nothing, unknown);
        assert!(unknown.contains("is unknown"), "{unknown}");
        assert!(unknown.contains("not the same as nothing"), "{unknown}");
        assert!(ListOrigin::Unreadable { reason: "x" }.is_fault());
        assert!(!ListOrigin::NothingStated.is_fault());
        // A list read successfully says the ordinary thing, however many sessions it
        // took: the session count is not the operator's problem.
        assert_eq!(
            ListOrigin::FromRecord { sessions: 3 }.empty_sentence(),
            nothing
        );
    }

    /// No area to state one over is a stated reason, not an empty picker.
    #[test]
    fn no_areas_gives_a_reason() {
        let sentence = CannotState::NoAreas.sentence();
        assert!(sentence.contains("no defended assets"), "{sentence}");
    }

    /// Every priority has a word, so no row shows a Rust variant name.
    #[test]
    fn every_priority_has_a_word() {
        for priority in [
            AssetPriority::Low,
            AssetPriority::Medium,
            AssetPriority::High,
            AssetPriority::Critical,
        ] {
            let word = priority_word(priority);
            assert!(!word.is_empty());
            assert_ne!(
                word,
                format!("{priority:?}"),
                "{priority:?} shows its variant name"
            );
        }
    }

    /// Changing selection drops text composed against the previous requirement: a
    /// decline reason typed for one must not be applied to another.
    #[test]
    fn changing_selection_drops_the_composed_action() {
        let mut draft = Draft {
            selected: Some(1),
            reason: "no sensor can reach that".into(),
            evidence: "some imagery".into(),
            title: "identify the contact".into(),
            ..Draft::default()
        };
        draft.clear_action();
        assert_eq!(draft.selected, None);
        assert!(draft.reason.is_empty() && draft.evidence.is_empty());
        // The new-requirement form is untouched: it is not part of the composed action.
        assert_eq!(draft.title, "identify the contact");
    }
}
