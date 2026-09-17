// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The tasking case: a collection requirement and the sensor tasks serving it.
//!
//! Design: docs/design/DN-11-sensor-control-and-tasking.md. Capability CAP-2.12;
//! mission thread MT-08 steps 2 and 3, which the desktop brings inside the system
//! (`gungnir-app/src/requirements.rs`, GAP-005).
//!
//! This crate gained an edge to `gungnir-sensor-management` for it, accepted by the
//! engineering reviewer on 2026-09-05 and drawn in ARCHITECTURE.md §7.1. It is
//! **narrow by construction**: this module reads task state and never issues a
//! command, so exactly one crate can talk to a sensor.
//!
//! Concurrence is an authorized action, `sensor.task`, which already exists in
//! `gungnir_security::actions`, so the authority matrix governs it without a new
//! action name.
//!
//! # A tasking concurrence names an operator
//!
//! The CAP-2.12 criterion moves a requirement from stated to tasked "only with a
//! concurrence carrying an operator". DN-11 amendment 1 b kept that criterion unchanged
//! while no build had an operator session to name one, and `concur` accepted
//! `Concurrence::UnattributedRole` in the meantime. Desktop sign-in exists now (GAP-057),
//! and the GAP-067 walk (2026-09-16) made the criterion the rule here: [`TaskingCase::concur`]
//! refuses a concurrence that names nobody. **Declining is not covered by the criterion**
//! and still accepts either, because a decline recorded as "the role on watch, nobody
//! signed in" is a true record of a refusal, where a tasking recorded that way is work
//! nobody can be asked about.

use gungnir_model::{
    CollectionRequirement, Concurrence, MissionTime, RequirementId, RequirementState,
};
use gungnir_sensor_management::{SensorTask, SensorTaskId, TaskState};

/// A requirement with the tasks serving it, as the requirements panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskingCase {
    pub requirement: CollectionRequirement,
    /// Tasks issued against this requirement, in issue order.
    pub tasks: Vec<SensorTask>,
}

/// How a requirement's collection is going, derived from its tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionProgress {
    /// No task has been issued yet.
    Untasked,
    /// At least one task is issued or sent and might still be acknowledged.
    Awaiting,
    /// At least one task was acknowledged. **Not the same as answered.**
    Working,
    /// Every task ended without acknowledgement.
    Stalled,
}

impl TaskingCase {
    pub fn new(requirement: CollectionRequirement) -> Self {
        Self {
            requirement,
            tasks: Vec::new(),
        }
    }

    /// Tasks that are still open.
    pub fn open_tasks(&self) -> impl Iterator<Item = &SensorTask> {
        self.tasks.iter().filter(|t| t.state.is_open())
    }

    /// Where the collection stands.
    ///
    /// **`Working` is not `Satisfied`.** A task the sensor acknowledged means it
    /// took the command, not that it answered the question. Satisfaction is a
    /// person's judgement and always names its evidence.
    pub fn progress(&self) -> CollectionProgress {
        if self.tasks.is_empty() {
            return CollectionProgress::Untasked;
        }
        if self.tasks.iter().any(|t| t.state.is_confirmed()) {
            return CollectionProgress::Working;
        }
        if self.tasks.iter().any(|t| t.state.is_open()) {
            return CollectionProgress::Awaiting;
        }
        CollectionProgress::Stalled
    }

    /// Tasks that ended without the sensor taking them, for the alerts panel.
    pub fn unacknowledged(&self) -> Vec<SensorTaskId> {
        self.tasks
            .iter()
            .filter(|t| {
                matches!(
                    t.state,
                    TaskState::Unacknowledged | TaskState::Failed { .. }
                )
            })
            .map(|t| t.id)
            .collect()
    }

    /// Whether `by` could move this requirement to tasked, asked before any command is
    /// issued for it.
    ///
    /// The half of [`TaskingCase::concur`]'s checks that does not depend on a task
    /// existing: the requirement is open, and the concurrence names an operator. A caller
    /// that issues the command first and concurs only if a task came out of it (PN-15's
    /// control, DN-11 amendment 1 c) asks this **before** issuing, because a sensor task
    /// cannot be withdrawn once issued, and a refused concurrence must not leave one behind
    /// serving a requirement nobody tasked.
    ///
    /// # Errors
    ///
    /// [`TaskingError::NotOpen`] for a closed requirement, and
    /// [`TaskingError::Unattributed`] for a concurrence that names no operator.
    pub fn may_concur(&self, by: &Concurrence) -> Result<(), TaskingError> {
        if !self.requirement.is_open() {
            return Err(TaskingError::NotOpen(self.requirement.id));
        }
        if by.operator().is_none() {
            return Err(TaskingError::Unattributed(self.requirement.id));
        }
        Ok(())
    }

    /// Records the sensor manager's concurrence, moving the requirement to tasked.
    ///
    /// Refused when the requirement is closed: a lapsed or declined requirement is
    /// not silently reopened by somebody tasking against it.
    ///
    /// **Refused when the concurrence names no operator** (GAP-067 walk, 2026-09-16). The
    /// CAP-2.12 criterion asks for "a concurrence carrying an operator", and until the
    /// walk this accepted `Concurrence::UnattributedRole` because no build could produce
    /// anything else (DN-11 amendment 1 b). Sign-in exists now, so the criterion is
    /// enforced here rather than only described.
    ///
    /// **Also refused when no task serves it.** `RequirementState::Tasked` is defined as
    /// a task existing *and* the sensor manager concurring, and until 2026-09-05 this
    /// checked only the second half -- a requirement could sit in `Tasked` with nothing
    /// serving it, which the analyst who stated it would read as work in hand. Nothing
    /// could hit that before, because no caller could link a task to a requirement at
    /// all; GAP-005 made the link possible and the check necessary in the same change.
    ///
    /// # Errors
    ///
    /// What [`TaskingCase::may_concur`] refuses, then [`TaskingError::NoTask`].
    pub fn concur(&mut self, by: Concurrence) -> Result<(), TaskingError> {
        self.may_concur(&by)?;
        if self.tasks.is_empty() {
            return Err(TaskingError::NoTask(self.requirement.id));
        }
        self.requirement.state = RequirementState::Tasked { by };
        Ok(())
    }

    /// Records the sensor manager declining, with a reason.
    ///
    /// A reason is required for the same reason PN-07 requires one to reject a plan: the
    /// analyst who stated the requirement has to know whether to restate it differently
    /// or give up on it, and "declined" alone answers neither question.
    ///
    /// **Either concurrence is accepted.** The CAP-2.12 criterion governs moving a
    /// requirement to tasked and says nothing of declining, so the GAP-067 walk
    /// (2026-09-16) left this as it was: a decline with nobody signed in records the role
    /// and says nobody was, which is true, and refusing it would leave the analyst waiting
    /// on a requirement the sensor manager has already turned down.
    pub fn decline(
        &mut self,
        by: Concurrence,
        reason: impl Into<String>,
    ) -> Result<(), TaskingError> {
        if !self.requirement.is_open() {
            return Err(TaskingError::NotOpen(self.requirement.id));
        }
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(TaskingError::ReasonRequired(self.requirement.id));
        }
        self.requirement.state = RequirementState::Declined { by, reason };
        Ok(())
    }

    /// Records satisfaction, which always names the evidence that answered it.
    ///
    /// Never called automatically: a task acknowledged is not an answer.
    pub fn satisfy(&mut self, evidence: impl Into<String>) -> Result<(), TaskingError> {
        if !self.requirement.is_open() {
            return Err(TaskingError::NotOpen(self.requirement.id));
        }
        let evidence = evidence.into();
        if evidence.trim().is_empty() {
            return Err(TaskingError::EvidenceRequired(self.requirement.id));
        }
        self.requirement.state = RequirementState::Satisfied { evidence };
        Ok(())
    }

    /// Lapses the requirement if its needed-by time has passed.
    pub fn lapse_if_overdue(&mut self, now: MissionTime) -> bool {
        if self.requirement.has_lapsed(now) {
            self.requirement.state = RequirementState::Lapsed;
            return true;
        }
        false
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskingError {
    #[error("requirement {0:?} is closed and cannot be changed")]
    NotOpen(RequirementId),
    #[error("requirement {0:?} cannot be satisfied without naming its evidence")]
    EvidenceRequired(RequirementId),
    /// Concurrence with nothing serving the requirement.
    #[error("requirement {0:?} has no sensor task, so there is nothing to concur with")]
    NoTask(RequirementId),
    #[error("requirement {0:?} cannot be declined without a reason")]
    ReasonRequired(RequirementId),
    /// A tasking concurrence that names no operator (`Concurrence::UnattributedRole`),
    /// which the CAP-2.12 criterion does not accept (GAP-067 walk, 2026-09-16).
    ///
    /// This crate cannot say *why* nobody was named -- nobody signed in, a session that
    /// expired, or no account store to sign in against -- because it holds no session.
    /// The caller that does says which.
    #[error("requirement {0:?} cannot be tasked on a concurrence that names no operator")]
    Unattributed(RequirementId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{AssetExtent, Geodetic, SensorId};
    use gungnir_sensor_management::{SensorCommand, SensorMode};

    fn requirement() -> CollectionRequirement {
        CollectionRequirement {
            id: RequirementId(1),
            title: "identify the contact".into(),
            priority: gungnir_model::AssetPriority::High,
            area: AssetExtent::Point {
                position: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
            },
            needed_by: Some(MissionTime(500.0)),
            state: RequirementState::Stated,
        }
    }

    fn task(id: u64, state: TaskState) -> SensorTask {
        SensorTask {
            id: SensorTaskId(id),
            sensor: SensorId(1),
            command: SensorCommand::SetMode {
                mode: SensorMode::Track,
            },
            requirement: Some(RequirementId(1)),
            issued: MissionTime(100.0),
            state,
        }
    }

    #[test]
    fn an_untasked_requirement_reports_itself_untasked() {
        let case = TaskingCase::new(requirement());
        assert_eq!(case.progress(), CollectionProgress::Untasked);
        assert_eq!(case.open_tasks().count(), 0);
    }

    fn signed_in() -> Concurrence {
        Concurrence::Operator {
            id: "j.okonkwo".into(),
            role: "SensorManager".into(),
        }
    }

    fn nobody_signed_in() -> Concurrence {
        Concurrence::UnattributedRole {
            role: "SensorManager".into(),
        }
    }

    #[test]
    fn a_requirement_moves_to_tasked_only_with_a_concurrence_naming_someone() {
        let mut case = TaskingCase::new(requirement());
        case.tasks.push(task(1, TaskState::Issued));
        case.concur(signed_in()).expect("concurs");
        match &case.requirement.state {
            RequirementState::Tasked { by } => assert_eq!(by.operator(), Some("j.okonkwo")),
            other => panic!("expected tasked, got {other:?}"),
        }
    }

    /// The `gungnir-workflow` Collection requirements and tasking concurrence (CAP-2.12)
    /// row of `docs/verification-capability-table.md` §2: "a requirement moves from stated
    /// to tasked only with a concurrence carrying an operator" (GAP-067 walk, 2026-09-16).
    /// Refused **with a task serving it**, so the refusal is for the missing operator and
    /// not for the missing task, and the requirement is left stated.
    #[test]
    fn a_concurrence_naming_nobody_cannot_task() {
        let mut case = TaskingCase::new(requirement());
        case.tasks.push(task(1, TaskState::Issued));
        assert_eq!(
            case.concur(nobody_signed_in()),
            Err(TaskingError::Unattributed(RequirementId(1)))
        );
        assert_eq!(case.requirement.state, RequirementState::Stated);
    }

    /// The same row, asked before any task exists. A caller that issues the command and
    /// then concurs has to know first whether the concurrence could stand, because a task
    /// once issued cannot be withdrawn: an operator passes with nothing issued yet, and a
    /// concurrence naming nobody does not.
    #[test]
    fn whether_a_concurrence_could_task_is_known_before_any_command_is_issued() {
        let case = TaskingCase::new(requirement());
        assert!(case.tasks.is_empty());
        assert_eq!(case.may_concur(&signed_in()), Ok(()));
        assert_eq!(
            case.may_concur(&nobody_signed_in()),
            Err(TaskingError::Unattributed(RequirementId(1)))
        );
        assert_eq!(case.requirement.state, RequirementState::Stated);
    }

    /// Declining is outside the criterion, and the walk left it accepting either
    /// concurrence: a decline made with nobody signed in is recorded as one, and says so.
    #[test]
    fn a_decline_with_nobody_signed_in_is_still_recorded() {
        let mut case = TaskingCase::new(requirement());
        case.decline(nobody_signed_in(), "no sensor can reach that area")
            .expect("declines");
        match &case.requirement.state {
            RequirementState::Declined { by, reason } => {
                assert_eq!(by.operator(), None);
                assert_eq!(reason, "no sensor can reach that area");
            }
            other => panic!("expected declined, got {other:?}"),
        }
    }

    /// `RequirementState::Tasked` means a task exists **and** somebody concurred.
    /// Concurring with nothing serving the requirement would leave the analyst who
    /// stated it reading "tasked" as work in hand.
    #[test]
    fn concurring_with_no_task_is_refused() {
        let mut case = TaskingCase::new(requirement());
        assert_eq!(
            case.concur(signed_in()),
            Err(TaskingError::NoTask(RequirementId(1)))
        );
        assert_eq!(case.requirement.state, RequirementState::Stated);
    }

    /// A decline without a reason is refused, for the reason PN-07 will not reject a
    /// plan without one: the analyst has to know whether to restate it or give up.
    #[test]
    fn declining_without_a_reason_is_refused() {
        let mut case = TaskingCase::new(requirement());
        assert_eq!(
            case.decline(signed_in(), "  "),
            Err(TaskingError::ReasonRequired(RequirementId(1)))
        );
        assert!(case.requirement.is_open());
    }

    #[test]
    fn an_acknowledged_task_is_working_and_not_satisfied() {
        let mut case = TaskingCase::new(requirement());
        case.tasks.push(task(
            1,
            TaskState::Acknowledged {
                at: MissionTime(110.0),
            },
        ));
        assert_eq!(case.progress(), CollectionProgress::Working);
        assert!(
            case.requirement.is_open(),
            "the sensor took the command; it did not answer the question"
        );
    }

    #[test]
    fn satisfaction_always_names_its_evidence() {
        let mut case = TaskingCase::new(requirement());
        assert_eq!(
            case.satisfy("   "),
            Err(TaskingError::EvidenceRequired(RequirementId(1)))
        );
        case.satisfy("imagery at 01:42 shows the hull number")
            .expect("satisfies");
        assert!(!case.requirement.is_open());
    }

    #[test]
    fn a_closed_requirement_is_not_silently_reopened() {
        let mut case = TaskingCase::new(requirement());
        case.decline(signed_in(), "no sensor can reach that area")
            .expect("declines");
        case.tasks.push(task(1, TaskState::Issued));
        assert_eq!(
            case.concur(signed_in()),
            Err(TaskingError::NotOpen(RequirementId(1)))
        );
        assert_eq!(
            case.satisfy("something"),
            Err(TaskingError::NotOpen(RequirementId(1)))
        );
    }

    #[test]
    fn awaiting_becomes_stalled_when_every_task_ends_unacknowledged() {
        let mut case = TaskingCase::new(requirement());
        case.tasks.push(task(1, TaskState::Sent));
        assert_eq!(case.progress(), CollectionProgress::Awaiting);

        case.tasks[0].state = TaskState::Unacknowledged;
        assert_eq!(case.progress(), CollectionProgress::Stalled);
        assert_eq!(case.unacknowledged(), vec![SensorTaskId(1)]);
    }

    #[test]
    fn a_failed_task_is_surfaced_for_the_alerts_panel() {
        let mut case = TaskingCase::new(requirement());
        case.tasks.push(task(
            1,
            TaskState::Failed {
                reason: "sensor refused".into(),
            },
        ));
        assert_eq!(case.unacknowledged(), vec![SensorTaskId(1)]);
        assert_eq!(case.progress(), CollectionProgress::Stalled);
    }

    #[test]
    fn an_overdue_requirement_lapses_and_a_closed_one_does_not() {
        let mut case = TaskingCase::new(requirement());
        assert!(!case.lapse_if_overdue(MissionTime(400.0)));
        assert!(case.lapse_if_overdue(MissionTime(600.0)));
        assert_eq!(case.requirement.state, RequirementState::Lapsed);
        assert!(!case.lapse_if_overdue(MissionTime(700.0)), "it lapses once");
    }
}
