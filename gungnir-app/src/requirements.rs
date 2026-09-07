// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Collection requirements wired to the desktop (GAP-005, DN-11).
//!
//! MT-08 steps 2 and 3 -- an analyst's tasking request and the sensor manager's
//! concurrence -- happened outside the system. This is what brings them inside it.
//!
//! # One owner for task state
//!
//! `gungnir_workflow::TaskingCase` holds a requirement *and* the tasks serving it, but
//! the registry owns those tasks and changes them: an acknowledgement, a refusal, and
//! the tick's timeout sweep all move a task's state. So `AppState` stores only the
//! requirements, and a case is **assembled from the registry** whenever one is needed.
//! Storing the tasks alongside would give them two owners, and the copy would be stale
//! the first time a sweep ran -- the panel would report a requirement as being worked by
//! a task that had already timed out.
//!
//! # Concurring and tasking are one action here
//!
//! `RequirementState::Tasked` means a sensor task exists *and* somebody concurred, so
//! [`task`] issues the command first and records the concurrence only if a task actually
//! came out of it. With no adapter configured (GAP-001) the issue is refused, no task is
//! recorded, and the requirement stays stated -- which is the truth, and is what PN-15
//! shows.

use crate::state::AppState;
use gungnir_model::events::RequirementEvent;
use gungnir_model::{
    AssetPriority, CollectionRequirement, Concurrence, MissionTime, RequirementId,
    RequirementState, SensorId,
};
use gungnir_sensor_management::{SensorControl, SensorManagementError};
use gungnir_ui::panels::requirements::{Progress, RequirementRow, Standing};
use gungnir_workflow::{CollectionProgress, TaskingCase, TaskingError};

/// Why the requirement list is empty, which is never simply "it is".
///
/// A list that came back empty because nothing was ever asked for and one that came back
/// empty because the journal could not be read look identical on screen and mean opposite
/// things: the first says there is no outstanding collection, the second says nobody
/// knows. PN-15 has to tell them apart -- the same argument `EmptyBecause` settles for
/// the approval queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    /// The journal was read and held no requirements.
    NothingStated,
    /// The journal was read and these came back.
    FromJournal { sessions: usize },
    /// The journal could not be read, so **whether anything is outstanding is unknown**.
    Unreadable { reason: String },
}

/// Rebuild the requirement list from every session in the journal.
///
/// Requirements outlive a session: an analyst who states one on Monday expects it on
/// Tuesday, and until this existed the list lived in memory and died with the process.
///
/// **Folded across all sessions in order**, because a requirement stated in one session
/// is answered in another and only the last word counts. Sessions are read oldest first,
/// which is what `SessionId` ordering gives: the identifier is derived from the mission
/// time the session opened.
///
/// # Errors
///
/// Never. A journal that cannot be read yields [`Recovered::Unreadable`] rather than
/// stopping the desktop from starting -- the same choice DN-22 §5 makes for a missing
/// keystore. What must not happen is an empty list presented as though nothing had been
/// asked for.
#[must_use]
pub fn recover(
    journal: &dyn gungnir_store::EventJournal,
) -> (Vec<CollectionRequirement>, Recovered) {
    let sessions = match journal.sessions() {
        Ok(sessions) => sessions,
        Err(err) => {
            return (
                Vec::new(),
                Recovered::Unreadable {
                    reason: err.to_string(),
                },
            )
        }
    };
    let mut ordered = sessions;
    ordered.sort_unstable_by_key(|s| s.0);

    let mut requirements: Vec<CollectionRequirement> = Vec::new();
    let mut read = 0usize;
    for session in ordered {
        let envelopes = match journal.read_session(session) {
            Ok(envelopes) => envelopes,
            // One unreadable session does not hide the rest, but it does mean the list
            // is incomplete and the caller must be told rather than shown a short list
            // as though it were the whole one.
            Err(err) => {
                return (
                    requirements,
                    Recovered::Unreadable {
                        reason: format!("session {}: {err}", session.0),
                    },
                )
            }
        };
        read += 1;
        for envelope in envelopes {
            let gungnir_eventing::Event::Requirement(event) = &envelope.event else {
                continue;
            };
            apply_recovered(&mut requirements, event);
        }
    }

    let outcome = if requirements.is_empty() {
        Recovered::NothingStated
    } else {
        Recovered::FromJournal { sessions: read }
    };
    (requirements, outcome)
}

/// Fold one event into the recovered list.
///
/// A state change for a requirement nothing stated is ignored rather than invented: a
/// journal whose earlier session was pruned by retention would otherwise produce a
/// requirement with no title and no area.
fn apply_recovered(requirements: &mut Vec<CollectionRequirement>, event: &RequirementEvent) {
    match event {
        RequirementEvent::Stated { requirement, .. } => {
            match requirements.iter_mut().find(|r| r.id == requirement.id) {
                Some(existing) => *existing = requirement.clone(),
                None => requirements.push(requirement.clone()),
            }
        }
        RequirementEvent::Tasked {
            requirement, by, ..
        } => {
            set_state(
                requirements,
                *requirement,
                RequirementState::Tasked { by: by.clone() },
            );
        }
        RequirementEvent::Declined {
            requirement,
            by,
            reason,
            ..
        } => set_state(
            requirements,
            *requirement,
            RequirementState::Declined {
                by: by.clone(),
                reason: reason.clone(),
            },
        ),
        RequirementEvent::Satisfied {
            requirement,
            evidence,
            ..
        } => set_state(
            requirements,
            *requirement,
            RequirementState::Satisfied {
                evidence: evidence.clone(),
            },
        ),
        RequirementEvent::Lapsed { requirement, .. } => {
            set_state(requirements, *requirement, RequirementState::Lapsed);
        }
    }
}

fn set_state(
    requirements: &mut [CollectionRequirement],
    id: RequirementId,
    state: RequirementState,
) {
    if let Some(existing) = requirements.iter_mut().find(|r| r.id == id) {
        existing.state = state;
    }
}

/// Anything that can refuse a requirement action.
#[derive(Debug, thiserror::Error)]
pub enum RequirementError {
    #[error("no requirement with id {0:?}")]
    Unknown(RequirementId),
    /// The area index came from a picker built over the same list, so this means the
    /// baseline changed under the panel rather than a bad click.
    #[error("no defended asset at index {0}")]
    UnknownArea(usize),
    #[error(transparent)]
    Tasking(#[from] TaskingError),
    /// The command could not be issued, so there is no task to concur with.
    #[error(transparent)]
    Sensor(#[from] SensorManagementError),
}

/// The requirement and the tasks serving it, assembled from the registry.
#[must_use]
pub fn case(state: &AppState, id: RequirementId) -> Option<TaskingCase> {
    let requirement = state.requirements.iter().find(|r| r.id == id)?;
    Some(assemble(state, requirement))
}

fn assemble(state: &AppState, requirement: &CollectionRequirement) -> TaskingCase {
    TaskingCase {
        requirement: requirement.clone(),
        tasks: state
            .sensors
            .tasks_for(requirement.id)
            .into_iter()
            .cloned()
            .collect(),
    }
}

/// How this deployment attributes an act.
///
/// A verified operator when one is signed in, and the role with an explicit statement
/// that nobody was, when not (GAP-057, DN-23). `Concurrence::UnattributedRole` is not
/// dead code now that authentication exists: a deployment with no account store, or one
/// whose operator's session has expired, still acts and still has to record that it
/// could not say who -- which is a different fact from an operator who declined to give
/// a name, and the reason the two are separate variants.
fn attribution(state: &AppState) -> Concurrence {
    let role = format!("{:?}", state.role());
    match state.attributed_operator() {
        Some(operator) => Concurrence::Operator {
            id: operator.0.to_string(),
            role,
        },
        None => Concurrence::UnattributedRole { role },
    }
}

/// State a requirement over the defended asset at `area`.
///
/// The area comes from the baseline because nothing on any screen turns a click into a
/// position yet. A requirement over nowhere would be worse than none, so a deployment
/// with no assets declared gets a reason from PN-15 instead of a form.
pub fn state_requirement(
    state: &mut AppState,
    title: String,
    area: usize,
    priority: AssetPriority,
    within_minutes: Option<f64>,
) -> Result<RequirementId, RequirementError> {
    let asset = state
        .config
        .assets
        .get(area)
        .ok_or(RequirementError::UnknownArea(area))?
        .to_asset();
    let now = state.clock.now();
    let id = state.next_requirement_id();
    let requirement = CollectionRequirement {
        id,
        title,
        priority,
        area: asset.extent,
        // Absent means it never lapses, which is a decision rather than a gap: some
        // requirements stand until answered.
        needed_by: within_minutes.map(|m| MissionTime(now.0 + m * 60.0)),
        state: RequirementState::Stated,
    };
    state.requirements.push(requirement.clone());
    publish(
        state,
        now,
        RequirementEvent::Stated {
            requirement,
            at: now,
        },
    );
    crate::audit::record(
        state,
        gungnir_security::actions::REQUIREMENT,
        format!("stated requirement {}", id.0),
    );
    Ok(id)
}

/// Task a sensor against a requirement, and record the concurrence.
///
/// **The order matters.** The command is issued first; only if a task came out of it is
/// the concurrence recorded. Concurring first and then discovering the sensor cannot be
/// commanded would leave a requirement reading as tasked with nothing serving it, which
/// is exactly what `TaskingCase::concur` now refuses.
///
/// The command is a `Search` over the requirement's own area: that is what a collection
/// requirement asks a sensor to do, and taking the area from the requirement is what
/// keeps the task and the question about the same piece of ground.
pub fn task(
    state: &mut AppState,
    id: RequirementId,
    sensor: u32,
    now: MissionTime,
) -> Result<(), RequirementError> {
    let requirement = state
        .requirements
        .iter()
        .find(|r| r.id == id)
        .ok_or(RequirementError::Unknown(id))?;
    // Refuse early on a closed requirement, before issuing a command against one that
    // nobody is waiting on any more.
    if !requirement.is_open() {
        return Err(TaskingError::NotOpen(id).into());
    }
    let area = requirement.area;

    let task = state.sensors.issue(
        SensorId(sensor),
        gungnir_sensor_management::tasking::SensorCommand::Search { area },
        Some(id),
        now,
    )?;

    let by = attribution(state);
    let mut case = case(state, id).ok_or(RequirementError::Unknown(id))?;
    case.concur(by.clone())?;
    store(state, case.requirement);

    publish(
        state,
        now,
        RequirementEvent::Tasked {
            requirement: id,
            task,
            by,
            at: now,
        },
    );
    crate::audit::record(
        state,
        gungnir_security::actions::TASK_SENSOR,
        format!("tasked a sensor for requirement {}", id.0),
    );
    Ok(())
}

/// Record the sensor manager declining, with a reason.
pub fn decline(
    state: &mut AppState,
    id: RequirementId,
    reason: String,
) -> Result<(), RequirementError> {
    let by = attribution(state);
    let mut case = case(state, id).ok_or(RequirementError::Unknown(id))?;
    case.decline(by.clone(), reason.clone())?;
    store(state, case.requirement);

    let now = state.clock.now();
    publish(
        state,
        now,
        RequirementEvent::Declined {
            requirement: id,
            by,
            reason,
            at: now,
        },
    );
    crate::audit::record(
        state,
        gungnir_security::actions::REQUIREMENT,
        format!("declined requirement {}", id.0),
    );
    Ok(())
}

/// Record that the requirement was answered, naming the evidence.
///
/// Never called from anywhere but an operator's click. A task the sensor acknowledged is
/// not an answer, and inferring one would be the same error as inferring effect from a
/// track deletion (DN-06).
pub fn satisfy(
    state: &mut AppState,
    id: RequirementId,
    evidence: String,
) -> Result<(), RequirementError> {
    let mut case = case(state, id).ok_or(RequirementError::Unknown(id))?;
    case.satisfy(evidence.clone())?;
    store(state, case.requirement);

    let now = state.clock.now();
    publish(
        state,
        now,
        RequirementEvent::Satisfied {
            requirement: id,
            evidence,
            at: now,
        },
    );
    crate::audit::record(
        state,
        gungnir_security::actions::REQUIREMENT,
        format!("satisfied requirement {}", id.0),
    );
    Ok(())
}

/// Lapse every open requirement past its needed-by time.
///
/// Runs every frame for the reason the approval and sensor-task sweeps do: a deadline
/// passes on the clock, not on new input. A lapse is **not** a decline -- nobody refused
/// it -- so it is published as its own event and PN-15 words it apart.
pub fn sweep(state: &mut AppState) {
    let now = state.clock.now();
    let lapsed = gungnir_model::lapse_overdue(&mut state.requirements, now);
    for requirement in lapsed {
        state.alerts.push(format!(
            "Collection requirement {} lapsed: its needed-by time passed with nobody \
             deciding either way.",
            requirement.0
        ));
        publish(
            state,
            now,
            RequirementEvent::Lapsed {
                requirement,
                at: now,
            },
        );
    }
}

fn store(state: &mut AppState, requirement: CollectionRequirement) {
    if let Some(slot) = state
        .requirements
        .iter_mut()
        .find(|r| r.id == requirement.id)
    {
        *slot = requirement;
    }
}

fn publish(state: &mut AppState, now: MissionTime, event: RequirementEvent) {
    if let Err(err) = state
        .events
        .publish(now, gungnir_eventing::Event::Requirement(event))
    {
        tracing::error!(%err, "requirement event publish failed");
    }
}

/// The rows PN-15 draws.
///
/// Ordered by priority then by deadline, which is the order somebody deciding what to
/// task next reads them in. Closed requirements sink to the bottom rather than being
/// hidden: a declined one is the answer to "why is nobody working this".
#[must_use]
pub fn rows(state: &AppState) -> Vec<RequirementRow<'_>> {
    let now = state.clock.now();
    let mut rows: Vec<RequirementRow<'_>> = state
        .requirements
        .iter()
        .map(|r| {
            let case = assemble(state, r);
            RequirementRow {
                id: r.id.0,
                title: &r.title,
                priority: r.priority,
                standing: standing(&r.state),
                progress: progress(case.progress()),
                tasks: case.tasks.len(),
                time_remaining_s: r.needed_by.map(|by| by.0 - now.0),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.standing
            .is_open()
            .cmp(&a.standing.is_open())
            .then(priority_rank(b.priority).cmp(&priority_rank(a.priority)))
            .then_with(|| match (a.time_remaining_s, b.time_remaining_s) {
                // A requirement with no deadline is not more urgent than one with a
                // distant deadline; it sorts after every dated one.
                (Some(x), Some(y)) => x.total_cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    rows
}

fn priority_rank(priority: AssetPriority) -> u8 {
    match priority {
        AssetPriority::Low => 0,
        AssetPriority::Medium => 1,
        AssetPriority::High => 2,
        AssetPriority::Critical => 3,
    }
}

/// The attribution as a phrase, resolving the operator-session question here rather
/// than leaving the panel to infer it.
fn standing(state: &RequirementState) -> Standing<'_> {
    match state {
        RequirementState::Stated => Standing::Stated,
        RequirementState::Tasked { by } => Standing::Tasked { by: who(by) },
        RequirementState::Satisfied { evidence } => Standing::Satisfied { evidence },
        RequirementState::Declined { by, reason } => Standing::Declined {
            by: who(by),
            reason,
        },
        RequirementState::Lapsed => Standing::Lapsed,
    }
}

/// Who acted, said so that an unattributed role cannot be read as a person.
fn who(by: &Concurrence) -> &str {
    match by {
        Concurrence::Operator { id, .. } => id,
        // The role alone would read as a name. It is not one, and this build has no
        // operator session to produce one (GAP-057).
        Concurrence::UnattributedRole { .. } => "the role on watch; nobody was signed in",
    }
}

fn progress(progress: CollectionProgress) -> Progress {
    match progress {
        CollectionProgress::Untasked => Progress::Untasked,
        CollectionProgress::Awaiting => Progress::Awaiting,
        CollectionProgress::Working => Progress::Working,
        CollectionProgress::Stalled => Progress::Stalled,
    }
}
