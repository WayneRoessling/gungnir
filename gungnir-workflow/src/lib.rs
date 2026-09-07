//! Human factors & operator workflow design, per docs/gungnir-capabilities.md
//! §5.6. `gungnir-ui` names panels; this crate defines who uses which view, how an
//! alert is acknowledged, escalated, and closed, and how annotations and cases are
//! attached to tracks -- so operator workload is managed by design rather than by
//! whichever panel happens to be open.

pub mod review;
pub mod tasking_case;
pub mod warning;

pub use review::{
    Finding, FindingAction, FindingId, FindingKind, FindingSubject, ReviewCase, ReviewError,
    ReviewState,
};
pub use tasking_case::{CollectionProgress, TaskingCase, TaskingError};
pub use warning::{
    mark_overdue, needs_attention, raise_due, state_label, Warning, WarningChange, WarningDelivery,
    WarningLedger, WarningState,
};

use gungnir_model::{MissionTime, TrackId};
use gungnir_observability::Alert;
use gungnir_security::Role;

/// Every panel the desktop can show: the twenty of `docs/ux/ux-to-code-map.md` §1,
/// where each variant's PN number and its owning file are recorded.
///
/// The identifiers are `PN-01` to `PN-20` in the UX documents. They are named here
/// rather than numbered so that a layout reads as a workspace instead of as a list of
/// indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PanelId {
    /// PN-01. In every layout; see [`WorkspaceLayout::ALWAYS`].
    StatusStrip,
    /// PN-02. In every layout; the centre of the screen.
    Viewport3d,
    /// PN-03.
    TrackTable,
    /// PN-04, the evidence card.
    TrackDetail,
    /// PN-05, the recommendation.
    InterceptPanel,
    /// PN-06.
    ApprovalQueue,
    /// PN-07.
    DecisionDialog,
    /// PN-08.
    Alerts,
    /// PN-09.
    SystemHealth,
    /// PN-10.
    SensorManagement,
    /// PN-11.
    CoverageLayers,
    /// PN-12.
    Replay,
    /// PN-13.
    Reports,
    /// PN-14.
    ConfigEditor,
    /// PN-15.
    Requirements,
    /// PN-16.
    Planning,
    /// PN-17.
    CommanderSummary,
    /// PN-18.
    Reconciliation,
    /// PN-19.
    Assistant,
    /// PN-20.
    Audit,
}

impl PanelId {
    /// Every panel, for exhaustive tests and for a layout editor to enumerate.
    pub const ALL: &'static [PanelId] = &[
        PanelId::StatusStrip,
        PanelId::Viewport3d,
        PanelId::TrackTable,
        PanelId::TrackDetail,
        PanelId::InterceptPanel,
        PanelId::ApprovalQueue,
        PanelId::DecisionDialog,
        PanelId::Alerts,
        PanelId::SystemHealth,
        PanelId::SensorManagement,
        PanelId::CoverageLayers,
        PanelId::Replay,
        PanelId::Reports,
        PanelId::ConfigEditor,
        PanelId::Requirements,
        PanelId::Planning,
        PanelId::CommanderSummary,
        PanelId::Reconciliation,
        PanelId::Assistant,
        PanelId::Audit,
    ];

    /// The `PN-nn` identifier the UX documents use, so a panel can be traced from the
    /// running application back to the wireframe it came from.
    #[must_use]
    pub fn pn(self) -> &'static str {
        match self {
            PanelId::StatusStrip => "PN-01",
            PanelId::Viewport3d => "PN-02",
            PanelId::TrackTable => "PN-03",
            PanelId::TrackDetail => "PN-04",
            PanelId::InterceptPanel => "PN-05",
            PanelId::ApprovalQueue => "PN-06",
            PanelId::DecisionDialog => "PN-07",
            PanelId::Alerts => "PN-08",
            PanelId::SystemHealth => "PN-09",
            PanelId::SensorManagement => "PN-10",
            PanelId::CoverageLayers => "PN-11",
            PanelId::Replay => "PN-12",
            PanelId::Reports => "PN-13",
            PanelId::ConfigEditor => "PN-14",
            PanelId::Requirements => "PN-15",
            PanelId::Planning => "PN-16",
            PanelId::CommanderSummary => "PN-17",
            PanelId::Reconciliation => "PN-18",
            PanelId::Assistant => "PN-19",
            PanelId::Audit => "PN-20",
        }
    }

    /// A short human title for the panel's frame.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            PanelId::StatusStrip => "Status",
            PanelId::Viewport3d => "Viewport",
            PanelId::TrackTable => "Tracks",
            PanelId::TrackDetail => "Evidence",
            PanelId::InterceptPanel => "Recommendation",
            PanelId::ApprovalQueue => "Approval queue",
            PanelId::DecisionDialog => "Decision",
            PanelId::Alerts => "Alerts",
            PanelId::SystemHealth => "System health",
            PanelId::SensorManagement => "Sensors",
            PanelId::CoverageLayers => "Coverage",
            PanelId::Replay => "Replay",
            PanelId::Reports => "Reports",
            PanelId::ConfigEditor => "Configuration",
            PanelId::Requirements => "Requirements",
            PanelId::Planning => "Planning",
            PanelId::CommanderSummary => "Commander summary",
            PanelId::Reconciliation => "Reconciliation",
            PanelId::Assistant => "Assistant",
            PanelId::Audit => "Audit",
        }
    }
}

/// The panels a role sees, in layout order.
///
/// Two things are separated here that the scaffold ran together, because
/// `docs/ux/information-architecture.md` §1 separates them: the panels that are
/// **docked** for a role, and the ones it **may open** on selection or from the strip.
/// A layout that listed both as one set would put the decision dialog permanently on an
/// operator's screen.
///
/// [`WorkspaceLayout::ALWAYS`] is neither: the status strip and the viewport are in
/// every layout by construction, which is why no role lists them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceLayout {
    pub role: Role,
    /// Docked panels, in layout order.
    pub panels: Vec<PanelId>,
    /// Panels this role may open but that are not docked by default.
    pub on_demand: Vec<PanelId>,
}

impl WorkspaceLayout {
    /// In every layout, whatever the role: the strip that says which backend, session
    /// and health are in force, and the viewport at the centre of the screen
    /// (`docs/ux/information-architecture.md` §1).
    pub const ALWAYS: &'static [PanelId] = &[PanelId::StatusStrip, PanelId::Viewport3d];

    /// The workspace for a role, transcribed from the layout table in
    /// `docs/ux/information-architecture.md` §1.
    #[must_use]
    pub fn for_role(role: Role) -> Self {
        use PanelId::{
            Alerts, ApprovalQueue, Assistant, Audit, CommanderSummary, ConfigEditor,
            CoverageLayers, DecisionDialog, InterceptPanel, Planning, Reconciliation, Replay,
            Reports, Requirements, SensorManagement, SystemHealth, TrackDetail, TrackTable,
        };
        let (panels, on_demand) = match role {
            Role::Operator => (
                vec![
                    ApprovalQueue,
                    InterceptPanel,
                    TrackTable,
                    Alerts,
                    SystemHealth,
                ],
                vec![TrackDetail, DecisionDialog, Assistant],
            ),
            Role::Supervisor => (
                vec![
                    ApprovalQueue,
                    InterceptPanel,
                    Alerts,
                    SystemHealth,
                    SensorManagement,
                    TrackTable,
                ],
                vec![
                    TrackDetail,
                    DecisionDialog,
                    ConfigEditor,
                    CommanderSummary,
                    Reconciliation,
                    Assistant,
                ],
            ),
            Role::Analyst => (
                vec![Replay, TrackTable, Reports],
                vec![TrackDetail, Assistant],
            ),
            Role::SensorManager => (
                vec![SensorManagement, CoverageLayers, SystemHealth, Alerts],
                vec![ConfigEditor, Requirements, Assistant],
            ),
            // "Every other panel read-only for administration; no decision dialogs for
            // engagements" -- so the administrator may open everything except the
            // decision dialog, which is the one surface that commits an engagement.
            Role::Administrator => (
                vec![ConfigEditor, Audit, SystemHealth],
                PanelId::ALL
                    .iter()
                    .copied()
                    .filter(|p| {
                        !matches!(
                            p,
                            PanelId::ConfigEditor
                                | PanelId::Audit
                                | PanelId::SystemHealth
                                | PanelId::DecisionDialog
                                | PanelId::StatusStrip
                                | PanelId::Viewport3d
                        )
                    })
                    .collect(),
            ),
            Role::IntelligenceAnalyst => (
                vec![Requirements, TrackDetail, TrackTable, Reports],
                vec![Replay, Assistant],
            ),
            Role::Planner => (
                vec![Planning, CoverageLayers, ConfigEditor, Replay],
                vec![SensorManagement, Assistant],
            ),
            Role::Commander => (
                vec![CommanderSummary, ApprovalQueue, Alerts, SystemHealth],
                vec![TrackDetail, InterceptPanel, DecisionDialog, Planning],
            ),
            // The security officer operates nothing (DN-22 §11, D-30): the audit record
            // and the health summary, and no panel that decides, tasks, or configures.
            Role::SecurityOfficer => (vec![Audit, SystemHealth], vec![]),
        };
        Self {
            role,
            panels,
            on_demand,
        }
    }

    /// Whether the panel is on this role's screen without them opening it: docked, or
    /// one of the two that are always present.
    #[must_use]
    pub fn shows(&self, panel: PanelId) -> bool {
        Self::ALWAYS.contains(&panel) || self.panels.contains(&panel)
    }

    /// Whether the role may have this panel at all, docked or opened.
    ///
    /// This is a layout question, not an authorisation one. A panel being openable does
    /// not mean its actions are permitted: `gungnir_security::role_permits` decides
    /// that, and the decision dialog in particular is reachable for roles that may not
    /// decide, because it is also how a delegated decision is *viewed*.
    #[must_use]
    pub fn may_open(&self, panel: PanelId) -> bool {
        self.shows(panel) || self.on_demand.contains(&panel)
    }

    /// Docked panels in order, with the always-present two first.
    pub fn docked(&self) -> impl Iterator<Item = PanelId> + '_ {
        Self::ALWAYS
            .iter()
            .copied()
            .chain(self.panels.iter().copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AlertState {
    New,
    Acknowledged,
    Escalated,
    Closed,
}

impl AlertState {
    pub fn can_transition_to(self, to: AlertState) -> bool {
        use AlertState::{Acknowledged, Closed, Escalated, New};
        matches!(
            (self, to),
            (New, Acknowledged | Escalated)
                | (Acknowledged, Escalated | Closed)
                | (Escalated, Acknowledged | Closed)
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("alert cannot move from {from:?} to {to:?}")]
    InvalidAlertTransition { from: AlertState, to: AlertState },
}

/// One alert's journey through the operator workflow, with every step recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct AlertLifecycle {
    pub alert: Alert,
    pub state: AlertState,
    pub history: Vec<AlertTransition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertTransition {
    pub to: AlertState,
    pub mission_time: MissionTime,
    pub operator: Option<String>,
}

impl AlertLifecycle {
    pub fn new(alert: Alert, now: MissionTime) -> Self {
        Self {
            alert,
            state: AlertState::New,
            history: vec![AlertTransition {
                to: AlertState::New,
                mission_time: now,
                operator: None,
            }],
        }
    }

    pub fn transition(
        &mut self,
        to: AlertState,
        now: MissionTime,
        operator: Option<String>,
    ) -> Result<(), WorkflowError> {
        if !self.state.can_transition_to(to) {
            return Err(WorkflowError::InvalidAlertTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        self.history.push(AlertTransition {
            to,
            mission_time: now,
            operator,
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Annotation {
    pub author: Option<String>,
    pub mission_time: MissionTime,
    pub text: String,
    pub track: Option<TrackId>,
}

/// A named collection of annotations an analyst or supervisor works through.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Case {
    pub id: u64,
    pub title: String,
    pub annotations: Vec<Annotation>,
    pub open: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_observability::AlertSeverity;

    #[test]
    fn analysts_do_not_get_the_approval_queue() {
        assert!(!WorkspaceLayout::for_role(Role::Analyst).shows(PanelId::ApprovalQueue));
        assert!(WorkspaceLayout::for_role(Role::Operator).shows(PanelId::ApprovalQueue));
        assert!(WorkspaceLayout::for_role(Role::Administrator).shows(PanelId::ConfigEditor));
    }

    #[test]
    fn alert_lifecycle_enforces_transitions_and_records_history() {
        let alert = Alert {
            severity: AlertSeverity::Warning,
            summary: "sensor 3 dropout".into(),
            related_track_ids: vec![],
        };
        let mut life = AlertLifecycle::new(alert, MissionTime(0.0));
        assert!(matches!(
            life.transition(AlertState::Closed, MissionTime(1.0), None),
            Err(WorkflowError::InvalidAlertTransition { .. })
        ));
        life.transition(
            AlertState::Acknowledged,
            MissionTime(1.0),
            Some("op-1".into()),
        )
        .expect("ack");
        life.transition(AlertState::Escalated, MissionTime(2.0), Some("op-1".into()))
            .expect("escalate");
        life.transition(AlertState::Closed, MissionTime(3.0), Some("sup-1".into()))
            .expect("close");
        assert_eq!(life.state, AlertState::Closed);
        assert_eq!(life.history.len(), 4);
        assert!(matches!(
            life.transition(AlertState::Acknowledged, MissionTime(4.0), None),
            Err(WorkflowError::InvalidAlertTransition { .. })
        ));
    }
}

#[cfg(test)]
mod security_officer_tests {
    use super::*;

    /// DN-22 §11, D-30: the officer's screen holds nothing that decides, tasks, or
    /// configures, and nothing it can open does either.
    #[test]
    fn the_security_officer_sees_the_record_and_the_health_and_nothing_operating() {
        let layout = WorkspaceLayout::for_role(Role::SecurityOfficer);
        assert!(layout.shows(PanelId::Audit));
        assert!(layout.shows(PanelId::SystemHealth));
        for operating in [
            PanelId::ApprovalQueue,
            PanelId::DecisionDialog,
            PanelId::SensorManagement,
            PanelId::ConfigEditor,
            PanelId::Planning,
            PanelId::InterceptPanel,
        ] {
            assert!(!layout.shows(operating), "{operating:?}");
            assert!(
                !layout.may_open(operating),
                "{operating:?} must not even open"
            );
        }
    }
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    /// Every adopted role has a workspace. D-05 adopted three roles on 2026-09-04 and
    /// they entered the code under GAP-068; a role without a layout would show an
    /// operator an empty screen.
    #[test]
    fn every_role_has_a_workspace() {
        for role in Role::ALL {
            let layout = WorkspaceLayout::for_role(*role);
            assert_eq!(layout.role, *role);
            assert!(
                !layout.panels.is_empty(),
                "{role:?} has no docked panels beyond the always-present two"
            );
        }
    }

    /// The strip and the viewport are in every layout without any role listing them.
    #[test]
    fn the_strip_and_viewport_are_always_present() {
        for role in Role::ALL {
            let layout = WorkspaceLayout::for_role(*role);
            assert!(
                layout.shows(PanelId::StatusStrip),
                "{role:?} has no status strip"
            );
            assert!(
                layout.shows(PanelId::Viewport3d),
                "{role:?} has no viewport"
            );
            assert!(
                !layout.panels.contains(&PanelId::StatusStrip),
                "{role:?} lists the strip explicitly; it is implicit"
            );
        }
    }

    /// A panel is docked or on demand, never both: the two lists are what separates
    /// "on screen" from "may open".
    #[test]
    fn docked_and_on_demand_do_not_overlap() {
        for role in Role::ALL {
            let layout = WorkspaceLayout::for_role(*role);
            for panel in &layout.on_demand {
                assert!(
                    !layout.panels.contains(panel),
                    "{role:?}: {panel:?} is both docked and on demand"
                );
            }
        }
    }

    /// The layouts transcribed from `docs/ux/information-architecture.md` §1. Spot
    /// checks on the rows that carry a design decision rather than on every cell.
    #[test]
    fn layouts_match_the_ux_table() {
        use PanelId::{
            Alerts, ApprovalQueue, CommanderSummary, ConfigEditor, CoverageLayers, DecisionDialog,
            InterceptPanel, Planning, Replay, Reports, Requirements, SensorManagement,
            SystemHealth, TrackDetail, TrackTable,
        };
        // The operator gains the queue as the first panel.
        let operator = WorkspaceLayout::for_role(Role::Operator);
        assert_eq!(operator.panels.first(), Some(&ApprovalQueue));
        assert_eq!(
            operator.panels,
            vec![
                ApprovalQueue,
                InterceptPanel,
                TrackTable,
                Alerts,
                SystemHealth
            ]
        );
        // The evidence card and the decision dialog open, they are not docked.
        assert!(!operator.shows(TrackDetail) && operator.may_open(TrackDetail));
        assert!(!operator.shows(DecisionDialog) && operator.may_open(DecisionDialog));

        // The commander leads with the summary, not the track table.
        let commander = WorkspaceLayout::for_role(Role::Commander);
        assert_eq!(commander.panels.first(), Some(&CommanderSummary));
        assert!(
            !commander.shows(TrackTable),
            "the commander does not dock the table"
        );
        assert!(commander.may_open(Planning));

        // The planner works in planning, coverage, configuration and replay.
        let planner = WorkspaceLayout::for_role(Role::Planner);
        assert_eq!(
            planner.panels,
            vec![Planning, CoverageLayers, ConfigEditor, Replay]
        );
        assert!(!planner.may_open(ApprovalQueue), "the planner has no queue");
        assert!(
            !planner.may_open(DecisionDialog),
            "the planner decides nothing"
        );

        // The intelligence analyst leads with requirements and docks the evidence card.
        let intel = WorkspaceLayout::for_role(Role::IntelligenceAnalyst);
        assert_eq!(
            intel.panels,
            vec![Requirements, TrackDetail, TrackTable, Reports]
        );

        let sensor = WorkspaceLayout::for_role(Role::SensorManager);
        assert_eq!(
            sensor.panels,
            vec![SensorManagement, CoverageLayers, SystemHealth, Alerts]
        );
    }

    /// The administrator may open everything for administration **except** the decision
    /// dialog, which is the one surface that commits an engagement. The UX table says
    /// "no decision dialogs for engagements" and this is what enforces it in the layout.
    #[test]
    fn the_administrator_cannot_reach_the_decision_dialog() {
        let admin = WorkspaceLayout::for_role(Role::Administrator);
        assert!(
            !admin.may_open(PanelId::DecisionDialog),
            "the administrator must not have a decision surface"
        );
        // But may reach everything else.
        for panel in PanelId::ALL {
            if *panel == PanelId::DecisionDialog {
                continue;
            }
            assert!(
                admin.may_open(*panel),
                "administrator cannot open {panel:?}"
            );
        }
    }

    /// Only roles that hold decision authority get a decision surface. This is a layout
    /// check, not an authorisation one, but a role that cannot decide should not be
    /// shown the dialog either.
    #[test]
    fn only_deciding_roles_can_open_the_decision_dialog() {
        use gungnir_security::actions::DECIDE_PLAN;
        use gungnir_security::authz::role_permits;
        for role in Role::ALL {
            let layout = WorkspaceLayout::for_role(*role);
            if layout.may_open(PanelId::DecisionDialog) {
                assert!(
                    role_permits(*role, DECIDE_PLAN),
                    "{role:?} can open the decision dialog but may not decide"
                );
            }
        }
    }

    /// Every panel identifier maps to a distinct PN number, so a panel in the running
    /// application can be traced back to the wireframe it came from.
    #[test]
    fn panel_identifiers_are_unique_and_traceable() {
        assert_eq!(PanelId::ALL.len(), 20, "the UX set is twenty panels");
        let mut seen = std::collections::HashSet::new();
        for panel in PanelId::ALL {
            assert!(seen.insert(panel.pn()), "{panel:?} duplicates a PN number");
            assert!(panel.pn().starts_with("PN-"));
            assert!(!panel.title().is_empty());
        }
    }
}
