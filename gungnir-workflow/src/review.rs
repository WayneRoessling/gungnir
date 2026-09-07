//! After-action review: sessions, findings, and the actions that come out of them.
//!
//! Design: docs/design/DN-20-after-action-review.md. Capability CAP-5.3; measure
//! MOE-12 (rehearsal effect). Reports are generated and exported today, and nothing
//! holds what the review concluded, so a lesson lives in whoever attended.
//!
//! Three rules shape this module:
//!
//! 1. **A finding points at the record, not at a memory.** A finding carries the
//!    mission time and the subjects it refers to, so the panel can seek the replay to
//!    the moment. A finding that cannot be seeked to is an anecdote.
//! 2. **Findings are typed, and the type matters.** A system-behaviour finding is a
//!    candidate gap-register entry; a configuration finding is a candidate baseline
//!    change; a practice finding is neither and must not be filed as a defect.
//! 3. **A review concludes; its actions close separately.** Conflating them would let
//!    a review be declared finished with its actions open, which is how lessons stop
//!    being learned.
//!
//! Nothing here is automatic. The system does not propose findings, score the
//! session, or judge the operators.

use gungnir_model::{MissionTime, SessionId, TrackId};

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct FindingId(pub u64);

/// What a finding is about, so the panel can select it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingSubject {
    Track(TrackId),
    Decision(gungnir_model::DecisionId),
    Sensor(gungnir_model::SensorId),
}

/// The kind of finding, which decides where it goes next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    /// Something the system did wrong or failed to do. A candidate gap.
    SystemBehaviour,
    /// Something a procedure did not cover.
    Procedure,
    /// Something a person did that is worth repeating, or not. **Never a defect.**
    Practice,
    /// A configuration that turned out wrong. A candidate baseline change.
    Configuration,
}

impl FindingKind {
    /// True for the one kind that may be promoted into the gap register.
    pub fn is_promotable(self) -> bool {
        self == FindingKind::SystemBehaviour
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FindingAction {
    pub owner: String,
    pub due: Option<MissionTime>,
    pub closed: Option<MissionTime>,
    pub outcome: Option<String>,
}

impl FindingAction {
    pub fn is_open(&self) -> bool {
        self.closed.is_none()
    }

    pub fn is_overdue(&self, now: MissionTime) -> bool {
        self.is_open() && self.due.is_some_and(|d| now > d)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub summary: String,
    pub kind: FindingKind,
    /// The moment in the session it refers to, so the reviewer can replay to it.
    pub at: Option<MissionTime>,
    pub refers_to: Vec<FindingSubject>,
    pub action: Option<FindingAction>,
    /// The gap this finding was promoted into, when it was.
    ///
    /// Recorded rather than filed automatically: a register entry needs scoring and
    /// an owner, so promotion is a person's act and this is the trace of it.
    pub promoted_to_gap: Option<String>,
}

impl Finding {
    /// True when the panel can seek the replay to this finding.
    pub fn is_seekable(&self) -> bool {
        self.at.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewState {
    Open,
    /// Findings recorded, actions assigned.
    Concluded,
    /// Every action closed.
    Closed,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReviewError {
    #[error("a review cannot be closed with {0} action(s) still open")]
    ActionsOpen(usize),
    #[error("a review must be concluded before it can be closed")]
    NotConcluded,
    #[error("only a system-behaviour finding may be promoted to a gap")]
    NotPromotable,
}

/// A review of one session.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReviewCase {
    pub session: SessionId,
    /// The report the review was conducted against, so a finding can be checked.
    pub report: Option<String>,
    pub findings: Vec<Finding>,
    pub state: ReviewState,
}

impl ReviewCase {
    pub fn open(session: SessionId) -> Self {
        Self {
            session,
            report: None,
            findings: Vec::new(),
            state: ReviewState::Open,
        }
    }

    /// Findings whose action is still open.
    pub fn open_actions(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.action.as_ref().is_some_and(FindingAction::is_open))
            .count()
    }

    /// Actions past their due date, for the alerts panel.
    pub fn overdue_actions(&self, now: MissionTime) -> Vec<FindingId> {
        self.findings
            .iter()
            .filter(|f| f.action.as_ref().is_some_and(|a| a.is_overdue(now)))
            .map(|f| f.id)
            .collect()
    }

    /// How many findings of each kind, so a review does not produce a list of
    /// engineering tickets for what were training points.
    pub fn count_by_kind(&self, kind: FindingKind) -> usize {
        self.findings.iter().filter(|f| f.kind == kind).count()
    }

    /// Records the findings and moves to concluded.
    pub fn conclude(&mut self) {
        self.state = ReviewState::Concluded;
    }

    /// Closes the review, which requires every action closed first.
    pub fn close(&mut self) -> Result<(), ReviewError> {
        if self.state == ReviewState::Open {
            return Err(ReviewError::NotConcluded);
        }
        let open = self.open_actions();
        if open > 0 {
            return Err(ReviewError::ActionsOpen(open));
        }
        self.state = ReviewState::Closed;
        Ok(())
    }

    /// Records that a finding was promoted into the gap register.
    pub fn promote(&mut self, id: FindingId, gap: impl Into<String>) -> Result<(), ReviewError> {
        let Some(finding) = self.findings.iter_mut().find(|f| f.id == id) else {
            return Err(ReviewError::NotPromotable);
        };
        if !finding.kind.is_promotable() {
            return Err(ReviewError::NotPromotable);
        }
        finding.promoted_to_gap = Some(gap.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: u64, kind: FindingKind, at: Option<f64>) -> Finding {
        Finding {
            id: FindingId(id),
            summary: "the queue got behind".into(),
            kind,
            at: at.map(MissionTime),
            refers_to: vec![FindingSubject::Track(TrackId(42))],
            action: None,
            promoted_to_gap: None,
        }
    }

    fn action(due: Option<f64>, closed: Option<f64>) -> FindingAction {
        FindingAction {
            owner: "engineer".into(),
            due: due.map(MissionTime),
            closed: closed.map(MissionTime),
            outcome: None,
        }
    }

    #[test]
    fn a_review_cannot_be_closed_with_an_open_action() {
        let mut review = ReviewCase::open(SessionId(1));
        let mut f = finding(1, FindingKind::SystemBehaviour, Some(100.0));
        f.action = Some(action(Some(500.0), None));
        review.findings.push(f);
        review.conclude();
        assert_eq!(review.close(), Err(ReviewError::ActionsOpen(1)));

        review.findings[0].action = Some(action(Some(500.0), Some(400.0)));
        review.close().expect("closes once every action is closed");
        assert_eq!(review.state, ReviewState::Closed);
    }

    #[test]
    fn a_review_must_be_concluded_before_it_is_closed() {
        let mut review = ReviewCase::open(SessionId(1));
        assert_eq!(review.close(), Err(ReviewError::NotConcluded));
    }

    #[test]
    fn a_finding_with_a_time_can_be_seeked_to() {
        assert!(finding(1, FindingKind::Practice, Some(100.0)).is_seekable());
        assert!(
            !finding(2, FindingKind::Practice, None).is_seekable(),
            "a finding that cannot be seeked to is an anecdote"
        );
    }

    #[test]
    fn findings_are_counted_separately_by_kind() {
        let mut review = ReviewCase::open(SessionId(1));
        review
            .findings
            .push(finding(1, FindingKind::SystemBehaviour, None));
        review
            .findings
            .push(finding(2, FindingKind::Practice, None));
        review
            .findings
            .push(finding(3, FindingKind::Practice, None));
        assert_eq!(review.count_by_kind(FindingKind::SystemBehaviour), 1);
        assert_eq!(review.count_by_kind(FindingKind::Practice), 2);
        assert_eq!(review.count_by_kind(FindingKind::Configuration), 0);
    }

    #[test]
    fn only_a_system_behaviour_finding_may_be_promoted_to_a_gap() {
        // A training point must not be filed as a defect.
        let mut review = ReviewCase::open(SessionId(1));
        review
            .findings
            .push(finding(1, FindingKind::Practice, None));
        review
            .findings
            .push(finding(2, FindingKind::SystemBehaviour, None));

        assert_eq!(
            review.promote(FindingId(1), "GAP-999"),
            Err(ReviewError::NotPromotable)
        );
        review.promote(FindingId(2), "GAP-999").expect("promotes");
        assert_eq!(
            review.findings[1].promoted_to_gap.as_deref(),
            Some("GAP-999")
        );
        assert!(
            review.findings[0].promoted_to_gap.is_none(),
            "and the practice finding is untouched"
        );
    }

    #[test]
    fn an_unknown_finding_cannot_be_promoted() {
        let mut review = ReviewCase::open(SessionId(1));
        assert_eq!(
            review.promote(FindingId(9), "GAP-999"),
            Err(ReviewError::NotPromotable)
        );
    }

    #[test]
    fn overdue_actions_are_surfaced_for_the_alerts_panel() {
        let mut review = ReviewCase::open(SessionId(1));
        let mut f = finding(1, FindingKind::Configuration, None);
        f.action = Some(action(Some(500.0), None));
        review.findings.push(f);

        assert!(review.overdue_actions(MissionTime(400.0)).is_empty());
        assert_eq!(
            review.overdue_actions(MissionTime(600.0)),
            vec![FindingId(1)]
        );

        review.findings[0].action = Some(action(Some(500.0), Some(450.0)));
        assert!(
            review.overdue_actions(MissionTime(600.0)).is_empty(),
            "a closed action is not overdue"
        );
    }

    #[test]
    fn an_action_with_no_due_date_is_open_but_never_overdue() {
        let a = action(None, None);
        assert!(a.is_open());
        assert!(!a.is_overdue(MissionTime(1_000_000.0)));
    }
}
