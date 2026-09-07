//! Binding the role workspace to the screen (GAP-055, GAP-072, GAP-073).
//!
//! `gungnir_workflow::WorkspaceLayout::for_role` says which panels a role has and in
//! what order; this maps each [`PanelId`] to the `gungnir-ui` function that draws it.
//! Until GAP-055 `main.rs` drew a fixed side panel with the same four panels for
//! everyone, which is the gap that names: *every role sees the same screen*.
//!
//! # Wiring, not logic
//!
//! Architecture principle AP-13 and `rust-ui-architecture-coding-standards.md` §1 keep
//! the binaries to construction and connection. The decision about which panels a role
//! sees lives in `gungnir-workflow`, which is tested against the UX table; the decision
//! about what a panel draws lives in `gungnir-ui`. What lives here is only the map
//! between them, and it is exhaustive over `PanelId` on purpose: adding a panel to the
//! UX set will fail to compile here until someone says how it is drawn.
//!
//! # Honest empty slots
//!
//! Seven of the twenty panels are designed and unbuilt. Their slots render
//! [`gungnir_ui::panels::not_implemented`], which states the panel identifier and the
//! gap that will build it. Drawing an empty frame instead would tell an operator there
//! were no alerts, no requirements or no coverage gaps, which is the claim AP-02 and
//! `CONTRIBUTING.md`'s "No fake wiring" rule forbid.
//!
//! The same rule applies *inside* a built panel. PN-04 and PN-17 each draw several
//! sections, and most of those sections come from crates neither binary constructs. A
//! section is a `Result` or a `Section` rather than a `Vec` that happens to be empty,
//! and the map below is where the unavailability is stated -- once, next to the wiring
//! that would remove it.

use crate::state::AppState;
use gungnir_command::ApprovalWorkflow;
use gungnir_ui::panels::commander_summary::{CommanderSummaryView, HandoverView, PlanInForce};
use gungnir_ui::panels::config_editor::ConfigAction;
use gungnir_ui::panels::decision_dialog::{
    DecisionChoice, DecisionDialogView, Degraded, OperatorIdentity,
};
use gungnir_ui::panels::not_implemented::render_not_implemented;
use gungnir_ui::panels::replay::ReplayAction;
use gungnir_ui::panels::reports::ReportsAction;
use gungnir_ui::panels::status_strip::DelegationLine;
use gungnir_ui::panels::track_detail::{EvidenceCardView, FactorLine};
use gungnir_ui::panels::track_table::TrackTableView;
use gungnir_ui::panels::unavailable::{Section, Unavailable};
use gungnir_workflow::PanelId;

/// `gungnir-assessment` is implemented and tested; neither binary constructs it
/// (GAP-028). It owns the threat score in PN-03 and the factor breakdown in PN-04.
const ASSESSMENT: Unavailable<'static> = Unavailable {
    owner: "gungnir-assessment",
    gap: "GAP-028",
};

/// Coverage gaps are not computed, so none can have been accepted (GAP-006).
const COVERAGE: Unavailable<'static> = Unavailable {
    owner: "gungnir-analytics",
    gap: "GAP-006",
};

/// Requirements live for the session and are not written anywhere.
const REQUIREMENT_PERSISTENCE: Unavailable<'static> = Unavailable {
    owner: "gungnir-store",
    gap: "GAP-005",
};

/// A command is recorded and published here; no adapter carries it (GAP-001).
const CONTROL_PATH: Unavailable<'static> = Unavailable {
    owner: "gungnir-sensor-management",
    gap: "GAP-001",
};

/// The register entry that will build each unbuilt panel, from
/// `docs/ux/ux-to-code-map.md` §1's Gap column.
/// PN-18 (GAP-050): what the outage was and what its reconciliation waits on. Never a
/// merged record it did not compute.
fn render_reconciliation(ui: &mut egui::Ui, state: &AppState) -> Option<PanelAction> {
    use crate::failover::ReconciliationView;
    ui.heading("Reconciliation");
    match crate::failover::reconciliation_view(state) {
        ReconciliationView::NothingDue => {
            ui.label(
                egui::RichText::new("No outage this session; nothing to reconcile.")
                    .color(gungnir_ui::theme::MUTED_TEXT_COLOR),
            );
        }
        ReconciliationView::OutageOngoing { endpoint, since } => {
            ui.label(
                egui::RichText::new(format!(
                    "Running embedded since T+{:.0} s: node {endpoint} is silent. Decisions \
                     taken now are this desktop's and will need reconciling when it answers.",
                    since.0
                ))
                .color(gungnir_ui::theme::ALERT_COLOR),
            );
        }
        ReconciliationView::Due {
            endpoint,
            from,
            to,
            local_decisions,
            reconciliation,
            can_switch_back,
        } => {
            return render_reconciliation_due(
                ui,
                &endpoint,
                (from, to),
                local_decisions,
                reconciliation.as_ref(),
                can_switch_back,
                crate::failover::outbox_view(state),
            );
        }
    }
    None
}

/// What the desktop queued for the node during the outage (§8.4).
fn render_outbox(ui: &mut egui::Ui, outbox: Option<crate::failover::OutboxView>) {
    if let Some(o) = outbox {
        ui.label(format!(
            "Store-and-forward: {} detection(s) queued for the node, {} delivered, {} dropped.",
            o.queued, o.forwarded, o.dropped
        ));
    }
}

/// The outage has ended: the merge, its conflicts, and the switch (GAP-050, D-15).
fn render_reconciliation_due(
    ui: &mut egui::Ui,
    endpoint: &str,
    (from, to): (gungnir_model::MissionTime, gungnir_model::MissionTime),
    local_decisions: usize,
    reconciliation: Option<&Result<crate::failover::Reconciliation, String>>,
    can_switch_back: bool,
    outbox: Option<crate::failover::OutboxView>,
) -> Option<PanelAction> {
    let mut resolution = None;
    render_outbox(ui, outbox);
    ui.label(
        egui::RichText::new(format!(
            "Node {endpoint} answers again. Outage T+{:.0} s to T+{:.0} s; {local_decisions} \
                 decision(s) recorded here meanwhile.",
            from.0, to.0
        ))
        .color(gungnir_ui::theme::WARNING_COLOR),
    );
    match reconciliation {
        None => {
            ui.label("Fetching the node's journal for the outage.");
        }
        Some(Ok(r)) => {
            ui.label(format!(
                "Merged: {} envelope(s) from this desktop and {} from the node into {}, \
                     {} duplicate(s) dropped.",
                r.local, r.remote, r.merged, r.duplicates_dropped
            ));
            if r.conflicts.is_empty() {
                ui.label(
                    egui::RichText::new("No conflicting decision.")
                        .color(gungnir_ui::theme::HEALTHY_COLOR),
                );
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{} conflicting decision(s); each is on the record and is resolved \
                             through the workflow, not here:",
                        r.conflicts.len()
                    ))
                    .color(gungnir_ui::theme::ALERT_COLOR),
                );
                for c in &r.conflicts {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "  plan {}: this desktop {}, the node {}",
                            c.plan.0,
                            if c.local_accepted {
                                "accepted"
                            } else {
                                "rejected"
                            },
                            if c.remote_accepted {
                                "accepted"
                            } else {
                                "rejected"
                            }
                        ));
                        if ui.button("keep this desktop's").clicked() {
                            resolution = Some(PanelAction::ResolveConflict(c.plan, true));
                        }
                        if ui.button("keep the node's").clicked() {
                            resolution = Some(PanelAction::ResolveConflict(c.plan, false));
                        }
                    });
                }
            }
            for (plan, kept_local) in &r.resolved {
                ui.label(
                    egui::RichText::new(format!(
                        "  plan {}: resolved, {} kept (on the record)",
                        plan.0,
                        if *kept_local {
                            "this desktop's"
                        } else {
                            "the node's"
                        }
                    ))
                    .color(gungnir_ui::theme::MUTED_TEXT_COLOR),
                );
            }
        }
        Some(Err(reason)) => {
            ui.label(
                egui::RichText::new(format!("The node's half is unavailable: {reason}"))
                    .color(gungnir_ui::theme::ALERT_COLOR),
            );
        }
    }
    ui.label(
        egui::RichText::new(
            "The desktop stays embedded until somebody has seen this and asks for the \
                 node back (D-15).",
        )
        .color(gungnir_ui::theme::MUTED_TEXT_COLOR),
    );
    if ui
        .add_enabled(
            can_switch_back,
            egui::Button::new("Switch back to the node"),
        )
        .clicked()
    {
        return Some(PanelAction::SwitchBack);
    }
    resolution
}

fn owning_gap(panel: PanelId) -> &'static str {
    match panel {
        // Three entries were named for this panel in turn and none of them builds one:
        // GAP-006 is the coverage-gap analytics function, GAP-026 the defended-asset
        // list that supplies its content, GAP-045 its rehearsal records. It pointed at
        // GAP-055 -- the entry that *tracks* unbuilt panels -- because that was the only
        // honest answer until one was filed. GAP-087 is that entry (2026-09-05).
        PanelId::Planning => "GAP-087",
        PanelId::Reconciliation => "GAP-050",
        PanelId::Assistant => "GAP-044",
        // Everything built, plus the viewport, never reaches this function.
        PanelId::StatusStrip
        | PanelId::Audit
        | PanelId::Viewport3d
        | PanelId::TrackTable
        | PanelId::TrackDetail
        | PanelId::CommanderSummary
        | PanelId::ApprovalQueue
        | PanelId::DecisionDialog
        | PanelId::Replay
        | PanelId::Reports
        | PanelId::ConfigEditor
        | PanelId::SensorManagement
        | PanelId::Requirements
        | PanelId::CoverageLayers
        | PanelId::InterceptPanel
        | PanelId::Alerts
        | PanelId::SystemHealth => "built",
    }
}

/// What the operator asked for by clicking inside a panel this frame.
///
/// Panels borrow their data, so they cannot write to `AppState`; they return what the
/// click meant and `main.rs` applies it. That is the one-way flow
/// `rust-ui-architecture-coding-standards.md` §2 requires, and it keeps the panel
/// functions testable without an `AppState`.
// Not `Eq`: `ReplayAction::SeekFraction` carries the scrubber's position, which is a
// float. A float is the right type for a fraction of a session and `Eq` is the wrong
// trait for a float, so the enum drops it rather than the fraction being made discrete
// to satisfy a derive.
/// Not `Copy`: PN-15's actions carry the operator's own words -- a decline reason, the
/// evidence that answered a requirement -- and those are `String`s. Every consumer moves
/// the action once, so nothing wanted a copy.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    /// PN-03: show this track in PN-04, or close the card if it is already shown.
    SelectTrack(gungnir_model::TrackId),
    /// PN-06: open PN-07 on this queued item, or close it if it is already open.
    SelectApproval(gungnir_ui::panels::approval_queue::PendingId),
    /// PN-07: the operator decided. Applied by `main.rs` after the frame is drawn, so
    /// the decision that removes an item from the queue never runs mid-draw.
    Decide(
        gungnir_ui::panels::approval_queue::PendingId,
        DecisionChoice,
    ),
    /// PN-10: command a sensor into a mode. Confirms nothing (GAP-004).
    CommandSensorMode(u32, gungnir_model::SensorMode),
    /// PN-10: record the mode an operator knows a sensor to be in, commanding nothing.
    RecordSensorMode(u32, gungnir_model::SensorMode),
    /// PN-11: show or hide a coverage layer.
    CoverageLayer(gungnir_ui::panels::coverage_layers::LayerAction),
    /// PN-15: state, task, decline or answer a collection requirement.
    Requirement(gungnir_ui::panels::requirements::RequirementAction),
    /// PN-12: open, close, step, seek or change rate.
    Replay(ReplayAction),
    /// PN-20 (GAP-057).
    Session(gungnir_ui::panels::audit::SessionAction),
    /// PN-17: the outgoing watch's notes, or the incoming watch taking over (GAP-054).
    Handover(gungnir_ui::panels::commander_summary::HandoverAction),
    /// PN-13: generate or export a mission report.
    Reports(ReportsAction),
    /// PN-14: reload, validate, apply or discard a configuration candidate.
    Config(ConfigAction),
    /// PN-18: a person has seen the reconciliation and asks for the node back (GAP-050).
    SwitchBack,
    /// PN-18: a person keeps one side of a conflicting decision (GAP-050, D-03).
    ResolveConflict(gungnir_model::PlanId, bool),
}

/// Draw one panel of the role's workspace.
///
/// The viewport is not drawn here: it is the centre of the screen and `main.rs` gives
/// it the central area rather than a docked slot.
pub fn render_panel(ui: &mut egui::Ui, panel: PanelId, state: &AppState) -> Option<PanelAction> {
    match panel {
        PanelId::Reconciliation => return render_reconciliation(ui, state),
        PanelId::TrackTable => return render_track_table(ui, state),
        PanelId::ApprovalQueue => return render_approval_queue(ui, state),
        PanelId::SensorManagement => return render_sensor_management(ui, state),
        PanelId::CoverageLayers => return render_coverage_layers(ui, state),
        // The three sustainment panels are drawn by `main.rs`, which owns the state
        // they read: a replay cursor, the last report, and a candidate baseline all
        // outlive a frame, and `AppState` is not the place for a half-scrubbed replay.
        // PN-15 joins them for the same reason: a half-typed requirement and an
        // uncommitted decline reason outlive a frame, and neither is mission state.
        PanelId::Replay
        | PanelId::Reports
        | PanelId::ConfigEditor
        | PanelId::Requirements
        // PN-17 joined these with GAP-054: a half-typed handover note outlives a frame
        // and is not mission state, so `main.rs` owns the buffer and draws it.
        | PanelId::CommanderSummary => {
            ui.label("Drawn by the desktop with its own session state.");
        }
        // PN-07 is a modal over the queue, not a docked slot: it is opened by choosing
        // an item and it takes a decision, so it must not compete for attention with
        // the panel that opened it. `main.rs` draws it.
        PanelId::DecisionDialog => {
            ui.label("The decision dialog opens on a queued item.");
        }
        PanelId::TrackDetail => render_track_detail(ui, state),
        PanelId::InterceptPanel => {
            // GAP-030: the resources the planner would not propose, and why. The
            // embedded planner is the only one that knows; a remote one reports none,
            // which is honest -- the node decided, not this desktop.
            let handoffs = handoff_lines(state);
            let fires: Vec<gungnir_ui::panels::intercept_panel::FiresCheckLine> = state
                .fires_checks
                .iter()
                .map(|c| gungnir_ui::panels::intercept_panel::FiresCheckLine {
                    check: crate::decisions::check_name(c.kind),
                    passed: c.passed,
                    detail: &c.detail,
                })
                .collect();
            let withheld: Vec<gungnir_ui::panels::intercept_panel::WithheldLine<'_>> = state
                .withheld_resources()
                .iter()
                .map(|w| gungnir_ui::panels::intercept_panel::WithheldLine {
                    resource: w.resource.0,
                    reason: w.reason.sentence(),
                })
                .collect();
            // GAP-032: the options considered beside the one recommended, each with the
            // verdict the same four-engine chain returned for it, and the rehearsal for
            // the selected track being lost. Both are held on the state, refreshed by
            // the tick, so the panel cannot draw alternatives to a plan it is not
            // showing.
            // The verdict sentences are owned for the frame, because `AlternativeLine`
            // borrows its text rather than allocating one per redraw.
            let option_verdicts: Vec<String> = state
                .alternatives
                .iter()
                .skip(1)
                .map(|c| crate::decisions::verdict_sentence(c.policy_verdict))
                .collect();
            // Skip one: the head of the list is the recommendation, which the panel has
            // already drawn in full above.
            let options: Vec<gungnir_ui::panels::intercept_panel::AlternativeLine<'_>> = state
                .alternatives
                .iter()
                .skip(1)
                .zip(&option_verdicts)
                .map(|(course, verdict)| alternative_line(verdict, course))
                .collect();
            let what_if_verdict = state
                .what_if
                .as_ref()
                .map(|c| crate::decisions::verdict_sentence(c.policy_verdict));
            let what_if = state
                .what_if
                .as_ref()
                .zip(what_if_verdict.as_ref())
                .map(|(course, verdict)| alternative_line(verdict, course));
            gungnir_ui::panels::intercept_panel::render_intercept_panel(
                ui,
                &state.last_plan,
                &withheld,
                &fires,
                &handoffs,
                &gungnir_ui::panels::intercept_panel::Alternatives {
                    options: &options,
                    what_if,
                },
            );
        }
        PanelId::SystemHealth => render_sensor_health(ui, state),
        PanelId::Alerts => {
            // GAP-042: warnings owed, late and failed first (DN-03 §7), above the alerts.
            let warnings = crate::warnings::lines(state, None);
            gungnir_ui::panels::alerts::render_warnings(ui, &warnings);
            gungnir_ui::panels::alerts::render_alert_list(ui, &state.alerts);
        }
        // PN-01 is drawn as a top strip by main.rs, not as a docked side panel; a slot
        // for it here would draw it twice.
        PanelId::StatusStrip => {
            ui.label("The status strip is drawn across the top of the window.");
        }
        // The viewport has its own area; a docked slot for it would draw it twice.
        PanelId::Viewport3d => {
            ui.label("The viewport is drawn in the central area.");
        }
        // Everything else is designed and not built. Say so rather than drawing
        // nothing: see the module documentation.
        other => render_not_implemented(ui, other.pn(), other.title(), owning_gap(other)),
    }
    None
}

/// One course of action as PN-05 draws it (GAP-032).
///
/// The rationale is passed through whole rather than summarised here: it is the sentence
/// the crate that generated the option wrote about it, and a panel that rewrote it would
/// be a second author of the explanation.
fn alternative_line<'a>(
    verdict: &'a str,
    course: &'a gungnir_decision::CourseOfAction,
) -> gungnir_ui::panels::intercept_panel::AlternativeLine<'a> {
    gungnir_ui::panels::intercept_panel::AlternativeLine {
        rationale: &course.rationale,
        verdict,
        denied: matches!(
            course.policy_verdict,
            gungnir_policy::PolicyVerdict::Denied { .. }
        ),
        assignments: course.plan.assignments().len(),
    }
}

/// PN-05's handoff rows (GAP-040): delivery state in words, manual stated plainly.
fn handoff_lines(state: &AppState) -> Vec<gungnir_ui::panels::intercept_panel::HandoffLine<'_>> {
    use gungnir_model::handoff::DeliveryState;
    state
        .handoffs
        .iter()
        .map(|h| gungnir_ui::panels::intercept_panel::HandoffLine {
            decision: h.handoff.decision.0,
            endpoint: h.endpoint.as_deref(),
            delivered: h.delivery.is_delivered(),
            detail: match &h.delivery {
                DeliveryState::Manual | DeliveryState::Delivered { .. } => "",
                DeliveryState::Undelivered { .. } => {
                    "No transport carries a handoff yet (GAP-040)."
                }
                DeliveryState::Refused { reason, .. } => reason,
            },
        })
        .collect()
}

/// PN-03. Age and asset are computed here; the threat score is not available.
/// PN-09 (GAP-001, GAP-008, GAP-009, GAP-010, GAP-021, GAP-023).
fn render_sensor_health(ui: &mut egui::Ui, state: &AppState) {
    let sensors = crate::status::sensor_health_lines(state);
    let sync = state.clock_skew.health(state.clock.late_data_policy());
    // GAP-021: which detectors are running, off, or configured and unable.
    let detectors: Vec<gungnir_ui::panels::sensor_health::DetectorLine<'_>> =
        crate::anomaly::detector_status(state)
            .into_iter()
            .map(
                |(name, st)| gungnir_ui::panels::sensor_health::DetectorLine {
                    name,
                    state: match st {
                        crate::anomaly::DetectorStatus::Off => None,
                        crate::anomaly::DetectorStatus::Running => Some(None),
                        crate::anomaly::DetectorStatus::ConfiguredButUnable(r) => Some(Some(r)),
                    },
                },
            )
            .collect();
    let terrain_line = state.terrain.line();
    let feeds = crate::radar::feed_lines(state);
    let cooperative_feeds = crate::cooperative::feed_lines(state);
    let peers_owned = crate::peers::peer_lines(&state.peer_links);
    let peers: Vec<gungnir_ui::panels::sensor_health::PeerLine<'_>> = peers_owned
        .iter()
        .map(|p| gungnir_ui::panels::sensor_health::PeerLine {
            name: &p.name,
            endpoint: &p.endpoint,
            connected: p.connected,
            reason: &p.reason,
        })
        .collect();
    gungnir_ui::panels::sensor_health::render_sensor_health(
        ui,
        &gungnir_ui::panels::sensor_health::SensorHealthView {
            health: &state.health,
            encryption: crate::status::encryption_state(&state.encryption),
            sensors: &sensors,
            clocks: gungnir_ui::panels::sensor_health::ClockSyncLine {
                sources_observed: sync.sources_observed,
                sources_out_of_sync: sync.sources_out_of_sync,
                max_skew_s: sync.max_clock_skew_s,
            },
            detectors: &detectors,
            terrain: gungnir_ui::panels::sensor_health::TerrainLine {
                masking: state.terrain.is_masking(),
                detail: &terrain_line,
            },
            feeds: &feeds,
            cooperative_feeds: &cooperative_feeds,
            peers: &peers,
        },
    );
}

fn render_track_table(ui: &mut egui::Ui, state: &AppState) -> Option<PanelAction> {
    let view = TrackTableView {
        tracks: state.tracking.tracks(),
        now: state.clock.now(),
        assignments: state.last_plan.kind.solutions(),
        scores: Err(ASSESSMENT),
        selected: state.selected_track(),
        vocabulary: &state.config.vocabulary,
    };
    gungnir_ui::panels::track_table::render_track_table(ui, &view).map(PanelAction::SelectTrack)
}

/// PN-10. The registry is real (GAP-003); nothing here reaches a sensor (GAP-004).
fn render_sensor_management(ui: &mut egui::Ui, state: &AppState) -> Option<PanelAction> {
    use gungnir_ui::panels::sensor_management::{SensorAction, SensorManagementView};
    let rows = crate::sustainment::sensor_rows(state);
    let role_name = format!("{:?}", state.role());
    // GAP-037: what the re-tasking planner recommends, or why it could not evaluate.
    let plans = crate::sustainment::sensor_plans(state);
    let lines: Vec<gungnir_ui::panels::sensor_management::RecommendationLine<'_>> = plans
        .as_deref()
        .map(|plans| {
            plans
                .iter()
                .filter_map(|p| {
                    let change = p.changes.first()?;
                    let to = match change.to.as_str() {
                        "Search" => gungnir_model::SensorMode::Search,
                        "Track" => gungnir_model::SensorMode::Track,
                        _ => gungnir_model::SensorMode::Standby,
                    };
                    Some(gungnir_ui::panels::sensor_management::RecommendationLine {
                        sensor: change.sensor.0,
                        from: change.from.as_str(),
                        to,
                        uncovered_closed_m: -p.cost.delta_uncovered_m,
                        redundancy_lost_m: p.cost.redundancy_lost_m,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let recommendations = match &plans {
        Ok(_) => Ok(lines.as_slice()),
        Err(reason) => Err(reason.sentence()),
    };
    let view = SensorManagementView {
        sensors: &rows,
        recommendations,
        may_task: gungnir_security::authz::role_permits(
            state.role(),
            gungnir_security::actions::TASK_SENSOR,
        ),
        role: &role_name,
        control_path: CONTROL_PATH,
        vocabulary: &state.config.vocabulary,
        last_error: state.sensor_error.as_deref(),
    };
    gungnir_ui::panels::sensor_management::render_sensor_management(ui, &view).map(|action| {
        match action {
            SensorAction::Command { sensor, mode } => PanelAction::CommandSensorMode(sensor, mode),
            SensorAction::RecordObserved { sensor, mode } => {
                PanelAction::RecordSensorMode(sensor, mode)
            }
        }
    })
}

/// PN-11. Which coverage layers the viewport draws (GAP-007).
///
/// Reads exactly what the viewport reads, so the panel's counts and the map cannot
/// disagree about how much there is to draw or why there is nothing.
fn render_coverage_layers(ui: &mut egui::Ui, state: &AppState) -> Option<PanelAction> {
    use gungnir_ui::panels::coverage_layers::{CoverageLayersView, LayerCounts, NothingToDraw};
    use gungnir_viewport3d::layers::CoverageLayer;

    let circles = crate::sustainment::coverage_circles(state);
    let layer = crate::sustainment::coverage_layer(state, &circles);
    let report = crate::sustainment::coverage_report(state);
    let names = crate::sustainment::approach_names(state);
    let gaps = report
        .as_ref()
        .map(|r| crate::sustainment::gap_polylines(&names, r))
        .unwrap_or_default();

    let placed = crate::hazards::placed(state);

    let reason;
    let coverage = match &layer {
        CoverageLayer::Circles { .. } => NothingToDraw::Available,
        CoverageLayer::None(no) => {
            reason = no.sentence();
            NothingToDraw::Because { reason: &reason }
        }
    };
    let view = CoverageLayersView {
        rings_visible: state.viewport.layers.rings,
        gaps_visible: state.viewport.layers.gaps,
        hazards_visible: state.viewport.layers.hazards,
        geofences_visible: state.viewport.layers.geofences,
        counts: LayerCounts {
            rings: circles.len(),
            gaps: gaps.len(),
            hazards: placed.len(),
            geofences: crate::geofences::placed(state).len(),
        },
        hazards: gungnir_ui::panels::coverage_layers::HazardCurrency {
            declared: state.hazards.hazards.len(),
            baseline_version: state.hazards.baseline_version,
        },
        coverage,
        comparison: LAYDOWN_COMPARISON,
    };
    gungnir_ui::panels::coverage_layers::render_coverage_layers(ui, &view)
        .map(PanelAction::CoverageLayer)
}

/// Comparing one laydown with another needs the laydown options PN-16 draws (GAP-087).
///
/// That panel is blocked on something PN-11 cannot supply either: **the baseline carries one
/// set of sensor and resource positions**, so there is no second laydown to compare against
/// until a laydown concept exists in the schema.
const LAYDOWN_COMPARISON: Unavailable<'static> = Unavailable {
    owner: "gungnir-ui",
    gap: "GAP-087",
};

/// PN-15. Requirements are real and the tasking is wired (GAP-005); no adapter
/// carries the resulting command to a sensor (GAP-001).
pub fn render_requirements(
    ui: &mut egui::Ui,
    state: &AppState,
    draft: &mut gungnir_ui::panels::requirements::Draft,
) -> Option<PanelAction> {
    use gungnir_sensor_management::SensorRegistry;
    use gungnir_ui::panels::requirements::{
        AreaChoice, CannotState, ListOrigin, RequirementsView, SensorChoice,
    };

    let rows = crate::requirements::rows(state);
    let areas: Vec<AreaChoice<'_>> = state
        .config
        .assets
        .iter()
        .map(|a| AreaChoice { name: &a.name })
        .collect();
    let sensors: Vec<SensorChoice<'_>> = state
        .sensors
        .sensors()
        .iter()
        .map(|s| SensorChoice {
            id: s.id.0,
            modality: &s.modality,
            controllable: s.control_endpoint.is_some(),
        })
        .collect();
    let role_name = format!("{:?}", state.role());
    let view = RequirementsView {
        rows: &rows,
        areas: if areas.is_empty() {
            Err(CannotState::NoAreas)
        } else {
            Ok(&areas)
        },
        sensors: &sensors,
        // Concurrence is `sensor.task`, which the intelligence analyst deliberately
        // does not hold: tasking is a request from them, not authority.
        may_concur: gungnir_security::authz::role_permits(
            state.role(),
            gungnir_security::actions::TASK_SENSOR,
        ),
        role: &role_name,
        // GAP-057: real once somebody signs in; a role never stands in for a person.
        operator_session: state.attributed_operator().is_some(),
        persistence: REQUIREMENT_PERSISTENCE,
        last_error: state.requirement_error.as_deref(),
        origin: match &state.recovered {
            crate::requirements::Recovered::NothingStated => ListOrigin::NothingStated,
            crate::requirements::Recovered::FromJournal { sessions } => ListOrigin::FromRecord {
                sessions: *sessions,
            },
            crate::requirements::Recovered::Unreadable { reason } => {
                ListOrigin::Unreadable { reason }
            }
        },
    };
    gungnir_ui::panels::requirements::render_requirements(ui, &view, draft)
        .map(PanelAction::Requirement)
}

/// PN-06. The queue is real -- the tick routes every plan through the policy chain
/// and queues what clears -- and in this build it is always empty, so the reason it is
/// empty is the part that matters. See `decisions::queue_empty_reason`.
///
/// The handoffs go in beside it (GAP-040): an item leaves the queue when the decision is
/// recorded, and the panel keeps it in sight until an effector has it.
fn render_approval_queue(ui: &mut egui::Ui, state: &AppState) -> Option<PanelAction> {
    let rows = crate::decisions::queue_rows(state);
    let handoffs = crate::handoffs::rows(state);
    let role_name = format!("{:?}", state.role());
    let view = crate::decisions::queue_view(state, &rows, &handoffs, &role_name);
    gungnir_ui::panels::approval_queue::render_approval_queue(ui, &view)
        .map(PanelAction::SelectApproval)
}

/// PN-07, drawn by `main.rs` as a modal on the selected queue item.
///
/// Returns the action for the frame: a decision, or nothing. The dialog state lives in
/// `AppState` because immediate mode has nowhere to keep a half-typed reject reason.
pub fn render_decision_dialog(
    ui: &mut egui::Ui,
    state: &AppState,
    dialog: &mut gungnir_ui::panels::decision_dialog::DecisionDialogState,
) -> Option<PanelAction> {
    let id = state.selected_approval()?;
    let rows = crate::decisions::queue_rows(state);
    let Some(row) = rows.iter().find(|r| r.id == id) else {
        ui.label("That item has left the queue.");
        return None;
    };

    let degraded = degraded_conditions(state);
    let report = crate::decisions::chain_report_for(&state.config);
    let caveats = report.caveats();
    // GAP-032: the options considered beside this one. The held list belongs to the plan
    // in force; a queued item from an earlier plan gets the reason rather than that list,
    // because alternatives to a different plan are not alternatives to this one.
    let stale_reason;
    let denials;
    let rows_for_dialog;
    let alternatives = if row.plan_id == state.last_plan.id.0 {
        denials = alternative_denials(state);
        rows_for_dialog = alternative_rows(state, &denials);
        if rows_for_dialog.is_empty() {
            // **Not `Unavailable` any more.** Since GAP-032 the alternatives are
            // computed, so an empty list is a fact about this picture -- the allocator
            // returned nothing different when a tasked resource was taken out of the pool
            // -- rather than a crate that is unbuilt.
            Section::Empty {
                reason: "every option the allocator returned for this picture was the one \
                         recommended; taking any tasked resource out of the pool changed \
                         nothing",
            }
        } else {
            Section::Present(&rows_for_dialog)
        }
    } else {
        stale_reason = format!(
            "this item was raised for plan #{}, and the options held are for plan #{}, \
             which superseded it",
            row.plan_id, state.last_plan.id.0
        );
        Section::Empty {
            reason: &stale_reason,
        }
    };

    let view = DecisionDialogView {
        row,
        rationale: Err(ASSESSMENT),
        alternatives,
        cost: Err(ASSESSMENT),
        degraded: &degraded,
        engines: &report.engines,
        caveats: &caveats,
        // No operator session exists (GAP-057), so the record will name nobody. A
        // decision surface has to say that rather than implying attribution.
        operator: OperatorIdentity::Unattributed {
            role: &format!("{:?}", state.role()),
            gap: "GAP-057",
        },
        may_accept: row.may_decide,
        may_override: crate::decisions::may_override(state.role()),
    };
    gungnir_ui::panels::decision_dialog::render_decision_dialog(ui, &view, dialog)
        .map(|choice| PanelAction::Decide(id, choice))
}

/// The denial reasons of the held alternatives, owned for the frame.
///
/// Separate from [`alternatives_section`] because `Verdict::Denied` borrows its reason and
/// the strings have to outlive the view; building them in the caller's scope is what lets
/// the section borrow them.
fn alternative_denials(state: &AppState) -> Vec<Option<String>> {
    state
        .alternatives
        .iter()
        .skip(1)
        .map(|c| match c.policy_verdict {
            gungnir_policy::PolicyVerdict::Denied { reason_code } => {
                Some(format!("{reason_code:?}"))
            }
            _ => None,
        })
        .collect()
}

/// PN-07's alternative rows (GAP-032).
///
/// Built from the same held list PN-05 draws, so the panel and the dialog cannot disagree
/// about what was considered. The head of the list is skipped: it is the plan under
/// decision, and the dialog draws that above.
fn alternative_rows<'a>(
    state: &'a AppState,
    denials: &'a [Option<String>],
) -> Vec<gungnir_ui::panels::decision_dialog::Alternative<'a>> {
    use gungnir_ui::panels::approval_queue::Verdict;
    use gungnir_ui::panels::decision_dialog::Alternative;
    state
        .alternatives
        .iter()
        .skip(1)
        .zip(denials)
        .map(|(course, denial)| Alternative {
            plan_id: course.plan.id.0,
            verdict: match denial {
                Some(reason) => Verdict::Denied { reason },
                None => Verdict::RequiresHumanApproval,
            },
            summary: &course.rationale,
        })
        .collect()
}

/// The conditions in force that make a decision a degraded one.
///
/// Read from the health flags and the journal state rather than from a banner: an
/// operator accepting a plan needs to know the tracker is not running, and needs to be
/// able to tell that from the journal not writing.
fn degraded_conditions(state: &AppState) -> Vec<Degraded<'static>> {
    let mut out = Vec::new();
    if !state.health.tracking_healthy {
        out.push(Degraded {
            subsystem: "tracking",
            detail: "the tracking pipeline is not running; the picture is not being updated",
        });
    }
    if !state.health.intercept_healthy {
        out.push(Degraded {
            subsystem: "intercept",
            detail: "the last solve failed; this plan may be stale",
        });
    }
    if !state.health.ingest_healthy {
        out.push(Degraded {
            subsystem: "ingest",
            detail: "the gateway is not receiving from every expected adapter",
        });
    }
    if state.journal_failed {
        out.push(Degraded {
            subsystem: "journal",
            detail: "writes are failing; this decision may not be recorded durably",
        });
    }
    out
}

/// PN-12. The cursor is real; the picture behind it is not rebuilt (GAP-045).
pub fn render_replay(
    ui: &mut egui::Ui,
    state: &AppState,
    replay: &crate::sustainment::ReplayState,
    sessions: &[gungnir_ui::panels::replay::SessionSummary],
) -> Option<PanelAction> {
    let view = crate::sustainment::replay_view(state, replay, sessions);
    gungnir_ui::panels::replay::render_replay(ui, &view).map(PanelAction::Replay)
}

/// PN-13. Counts folded from the journal; the measures catalogue is not computed.
/// PN-20 (GAP-057, GAP-059). Drawn with the sustainment state because the sign-in
/// draft is scratch the panel edits in place.
pub fn render_audit(
    ui: &mut egui::Ui,
    state: &AppState,
    sustainment: &mut crate::sustainment::SustainmentState,
) -> Option<PanelAction> {
    let accounts_owned = crate::session::account_lines(state);
    let accounts: Vec<gungnir_ui::panels::audit::AccountLine> = accounts_owned
        .iter()
        .map(|(op, role)| gungnir_ui::panels::audit::AccountLine {
            operator: *op,
            role,
        })
        .collect();
    let audit = crate::sustainment::audit_lines(state);
    let handoffs = crate::handoffs::rows(state);
    let view = crate::session::audit_view(state, &accounts, &audit, &handoffs);
    gungnir_ui::panels::audit::render_audit(ui, &view, &mut sustainment.sign_in)
        .map(PanelAction::Session)
}

/// PN-13 (GAP-071, GAP-047, GAP-049). Drawn with the sustainment state because the
/// review's draft is scratch the panel edits in place.
pub fn render_reports(
    ui: &mut egui::Ui,
    state: &AppState,
    sustainment: &mut crate::sustainment::SustainmentState,
) -> Option<PanelAction> {
    let lines = sustainment
        .review
        .as_ref()
        .map(crate::review::finding_lines)
        .unwrap_or_default();
    let mut view = crate::sustainment::reports_view(state, &sustainment.reports);
    view.review = crate::review::review_view(state, sustainment, &lines);
    gungnir_ui::panels::reports::render_reports(ui, &view, &mut sustainment.review_draft)
        .map(PanelAction::Reports)
}

/// PN-14. Validate is real, apply persists and audits, editing is not built.
pub fn render_config_editor(
    ui: &mut egui::Ui,
    state: &AppState,
    editor: &crate::sustainment::ConfigEditorState,
) -> Option<PanelAction> {
    let sections = crate::sustainment::config_sections(state);
    let audit = crate::sustainment::audit_lines(state);
    let role_name = format!("{:?}", state.role());
    let validity = crate::sustainment::validity_sentence(state);
    // GAP-086: built here rather than inside the view, because the lines borrow the
    // registry's baselines and have to outlive the call.
    let candidates = state.governance.candidates();
    let lines = crate::sustainment::profile_lines(&candidates);
    let profiles = crate::sustainment::governed_profiles(state, &lines);
    let view = crate::sustainment::config_editor_view(
        state,
        editor,
        &sections,
        &audit,
        &role_name,
        validity.as_deref(),
        profiles,
    );
    gungnir_ui::panels::config_editor::render_config_editor(ui, &view).map(PanelAction::Config)
}

/// PN-04. Opens on selection; with nothing selected it says so rather than drawing an
/// empty card, which would look like a track with no evidence.
fn render_track_detail(ui: &mut egui::Ui, state: &AppState) {
    let Some(id) = state.selected_track() else {
        ui.label("Select a track in the track table to see its evidence.");
        return;
    };
    // A selection whose track has since been deleted is not an error and not a blank
    // card: the track was there and is not now, which is worth saying.
    let Some(track) = state.tracking.tracks().iter().find(|t| t.id == id) else {
        ui.label(format!("Track {} is no longer in the picture.", id.0));
        return;
    };
    // GAP-026: the asset the track threatens and the priority's weight, from the same
    // ranking PN-17 lists; when nothing is scored the section says why.
    let ranking = crate::sustainment::asset_exposure(state);
    let factor_pairs = crate::sustainment::score_factors(state, &ranking, id);
    let factor_lines: Vec<FactorLine> = factor_pairs
        .iter()
        .map(|(name, contribution)| FactorLine {
            name,
            contribution: *contribution,
        })
        .collect();
    let factors = match ranking.reason() {
        Some(reason) => Section::Empty { reason },
        None => Section::Present(&factor_lines),
    };
    // GAP-020: what the track approaches, with the predictor named; GAP-042: the
    // warnings it has raised.
    let outcome = crate::prediction::predict(state);
    let approach_owned = crate::prediction::approaches(state, &outcome, id);
    let approach_lines: Vec<gungnir_ui::panels::track_detail::ApproachLine> = approach_owned
        .iter()
        .map(|a| gungnir_ui::panels::track_detail::ApproachLine {
            asset: &a.asset,
            time_ahead_s: a.time_ahead_s,
            distance_m: a.distance_m,
            arrives: a.arrives,
            predictor: crate::prediction::predictor_name(a.predictor),
        })
        .collect();
    let approach = match outcome.reason() {
        Some(reason) => Section::Empty { reason },
        None if state.config.origin.is_none() => Section::Empty {
            reason: "no local frame origin is declared, so the assets cannot be placed against the track",
        },
        None => Section::Present(&approach_lines),
    };
    let warning_lines = crate::warnings::lines(state, Some(id));
    // GAP-010: the evidence the engine holds for this track, and its conclusion.
    let evidence_owned = crate::cooperative::evidence_lines(state, id);
    let evidence_lines: Vec<gungnir_ui::panels::track_detail::EvidenceLine> = evidence_owned
        .iter()
        .map(|e| gungnir_ui::panels::track_detail::EvidenceLine {
            kind: e.kind,
            source: &e.source,
            weight: e.weight,
            supports: e.supports,
        })
        .collect();
    let evidence = if evidence_lines.is_empty() {
        Section::Empty {
            reason: "no cooperative report has been associated with this track, and no other \
                     evidence source is wired",
        }
    } else {
        Section::Present(&evidence_lines)
    };
    ui.label(
        egui::RichText::new(crate::cooperative::decision_sentence(state, id))
            .color(gungnir_ui::theme::MUTED_TEXT_COLOR),
    );
    // GAP-019: the lineage the resolver holds for this track, across sessions.
    let lineage_owned = crate::identity::lineage_lines(state, id);
    let lineage_lines: Vec<gungnir_ui::panels::track_detail::LineageLine> = lineage_owned
        .iter()
        .map(|l| gungnir_ui::panels::track_detail::LineageLine {
            session: l.session,
            local_track: l.local_track,
            basis: &l.basis,
        })
        .collect();
    let lineage = if lineage_lines.is_empty() {
        Section::Empty {
            reason: "the resolver has not seen this track yet",
        }
    } else {
        Section::Present(&lineage_lines)
    };
    let view = EvidenceCardView {
        track,
        evidence,
        lineage,
        factors,
        approach,
        warnings: &warning_lines,
        // Recording a designation needs `gungnir-identification` and the decision
        // record of `gungnir-command`; neither is constructed, so no control is drawn.
        designation_available: false,
        vocabulary: &state.config.vocabulary,
    };
    gungnir_ui::panels::track_detail::render_evidence_card(ui, &view);
}

/// PN-17. Two of the five sections are real: the plan in force and the delegations,
/// both read from state the desktop already holds.
/// The parts of a handover this build cannot assemble, named rather than drawn as a
/// blank: nothing opens an engagement (GAP-043) or owes a warning (GAP-042), and **a blank
/// line in a handover reads as "all clear"**.
const HANDOVER_GAPS: &[Unavailable<'static>] = &[
    Unavailable {
        owner: "gungnir-intercept-service",
        gap: "GAP-043",
    },
    Unavailable {
        owner: "gungnir-analytics",
        gap: "GAP-042",
    },
];

/// PN-17, with the handover the watch is part-way through completing.
///
/// `notes` is the outgoing watch's scratch buffer; it is session state, not mission
/// state, which is why it is passed in rather than kept on `AppState`.
pub fn render_commander_summary(
    ui: &mut egui::Ui,
    state: &AppState,
    notes: &mut String,
) -> Option<PanelAction> {
    // Same rules PN-01 reads, so the two panels cannot disagree about what is
    // delegated right now.
    let delegations: Vec<DelegationLine<'_>> = state
        .config
        .policy
        .authority
        .rules
        .iter()
        .filter(|r| r.pre_delegated)
        .map(|r| DelegationLine {
            action: r.action.as_str(),
            role: r.role.as_str(),
            layer: r.layer,
            class: r.class.as_deref(),
        })
        .collect();

    // `PlanView::default()` is a real value the desktop starts with, so "is there a
    // plan" is "does it propose anything", not "is the field set".
    let solutions = state.last_plan.kind.solutions();
    let plan = (!solutions.is_empty()).then_some(PlanInForce {
        summary: "Intercept plan",
        assignments: solutions.len(),
    });

    // Real since GAP-038, and the expiry count real since GAP-034. `decided_this_session`
    // subtracts the expiries, because an expiry leaves a record and is not a decision:
    // counting it as one would tell a commander their queue is being worked when it is
    // timing out.
    let expired = crate::decisions::expired_count(state);
    let queue = Ok(gungnir_ui::panels::commander_summary::QueueStats {
        pending: state.approvals.queue().len(),
        decided_this_session: state.approvals.records().len().saturating_sub(expired),
        expired,
    });
    let handover = state.rhythm.handover().map(|h| HandoverView {
        period: h.period,
        open_alerts: h.open_alerts,
        pending_approvals: h.pending_approvals,
        expired_approvals: h.expired_approvals,
        sensors_degraded: h.sensors_degraded.len(),
        maintenance_outstanding: h.maintenance_due.len(),
        acknowledged_by: h.acknowledged_by.as_ref().map(|(w, at)| (w.as_str(), *at)),
        outstanding_work: h.has_outstanding_work(),
        not_assembled: HANDOVER_GAPS,
    });

    let ranking = crate::sustainment::asset_exposure(state);
    let exposure_lines = crate::sustainment::exposure_lines(state, &ranking);
    let view = CommanderSummaryView {
        handover,
        queue,
        delegations: &delegations,
        accepted_gaps: Err(COVERAGE),
        plan,
        outcomes: Ok(crate::engagements::outcome_counts(state)),
        exposure: match ranking.reason() {
            Some(reason) => Err(reason),
            None => Ok(&exposure_lines),
        },
        controls_available: false,
        vocabulary: &state.config.vocabulary,
        warnings: gungnir_ui::panels::commander_summary::WarningCounts {
            open: state.warnings.open().len(),
            late: state.warnings.late_count(),
            failed: state.warnings.failed_count(),
        },
    };
    gungnir_ui::panels::commander_summary::render_commander_summary(ui, &view, notes)
        .map(PanelAction::Handover)
}

/// Whether this panel has a real implementation behind it today.
///
/// Used by the tests below and by anything that wants to report workspace completeness
/// honestly rather than counting slots. "Implemented" means the panel draws the data it
/// owns; a built panel may still have sections that name an unwired crate, and PN-04
/// and PN-17 both do.
#[must_use]
pub fn is_implemented(panel: PanelId) -> bool {
    matches!(
        panel,
        PanelId::StatusStrip
            | PanelId::TrackTable
            | PanelId::TrackDetail
            | PanelId::CommanderSummary
            | PanelId::ApprovalQueue
            | PanelId::DecisionDialog
            | PanelId::Replay
            | PanelId::Reports
            | PanelId::ConfigEditor
            | PanelId::SensorManagement
            | PanelId::Requirements
            | PanelId::CoverageLayers
            | PanelId::InterceptPanel
            | PanelId::SystemHealth
            | PanelId::Alerts
            | PanelId::Viewport3d
            | PanelId::Audit
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_security::Role;
    use gungnir_workflow::WorkspaceLayout;

    /// The register, for checking that a gap a panel names is a gap that exists.
    ///
    /// `None` when it cannot be found, so the tests below skip rather than fail: a
    /// packaged crate has no `docs/` beside it, and a test that broke there would be
    /// testing the build layout rather than the code.
    fn register() -> Option<String> {
        for candidate in [
            "../docs/mission/gap-analysis/gap-register.md",
            "docs/mission/gap-analysis/gap-register.md",
        ] {
            if let Ok(text) = std::fs::read_to_string(candidate) {
                return Some(text);
            }
        }
        None
    }

    /// Every panel either draws something real or names the gap that will build it.
    /// A panel that fell through to a blank frame is the failure this guards.
    ///
    /// **And the gap it names must exist.** Checking only the shape of the string is what
    /// let PN-16 point at three entries in turn that build no panel -- GAP-006, the
    /// coverage-gap function; GAP-026, the defended-asset list; then GAP-055, which merely
    /// *tracks* unbuilt panels. Each looked right and none of them was going to build it.
    #[test]
    fn every_panel_is_either_built_or_names_a_gap_that_exists() {
        let register = register();
        for panel in PanelId::ALL {
            if is_implemented(*panel) {
                continue;
            }
            let gap = owning_gap(*panel);
            assert!(
                gap.starts_with("GAP-"),
                "{panel:?} is not built and names no gap"
            );
            if let Some(register) = &register {
                assert!(
                    register.contains(&format!("| {gap} |")),
                    "{panel:?} names {gap}, which is not an entry in the register"
                );
            }
        }
    }

    /// The built panels must not be reported as unbuilt, or a working screen would
    /// tell an operator it is not working.
    #[test]
    fn built_panels_are_not_marked_unbuilt() {
        for panel in [
            PanelId::TrackTable,
            PanelId::TrackDetail,
            PanelId::CommanderSummary,
            PanelId::ApprovalQueue,
            PanelId::DecisionDialog,
            PanelId::Replay,
            PanelId::Reports,
            PanelId::ConfigEditor,
            PanelId::SensorManagement,
            PanelId::Requirements,
            PanelId::CoverageLayers,
            PanelId::InterceptPanel,
            PanelId::SystemHealth,
            PanelId::Alerts,
        ] {
            assert!(is_implemented(panel));
            assert_eq!(owning_gap(panel), "built");
        }
    }

    /// Every unavailable section this file declares names a crate that exists in the
    /// workspace and a register entry. A stale one would tell an operator to wait for
    /// work that is already done, or for a gap that was never opened.
    #[test]
    fn every_declared_unavailability_names_a_crate_and_a_gap() {
        // `ALTERNATIVES` used to be in this list and was retired with GAP-032: PN-07 now
        // draws the options the allocator actually returned, so declaring the section
        // unavailable would tell an operator to wait for work that is done.
        for u in [
            ASSESSMENT,
            COVERAGE,
            CONTROL_PATH,
            REQUIREMENT_PERSISTENCE,
            LAYDOWN_COMPARISON,
        ] {
            assert!(u.owner.starts_with("gungnir-"), "{u:?}");
            assert!(u.gap.starts_with("GAP-"), "{u:?}");
            // The entry has to be one somebody could go and read. A well-formed
            // identifier for an entry nobody ever opened sends an operator to look for
            // work that does not exist.
            if let Some(register) = register() {
                assert!(
                    register.contains(&format!("| {} |", u.gap)),
                    "{u:?} names a gap that is not in the register"
                );
            }
            assert!(
                std::path::Path::new("..")
                    .join(u.owner)
                    .join("Cargo.toml")
                    .exists()
                    || std::path::Path::new(u.owner).join("Cargo.toml").exists(),
                "{} is not a crate in this workspace",
                u.owner
            );
        }
    }

    /// How much of each role's workspace actually works today. Not an assertion about
    /// a target -- it is a record of the starting point, so the number moving is
    /// visible as the panels land.
    #[test]
    fn report_workspace_completeness() {
        for role in Role::ALL {
            let layout = WorkspaceLayout::for_role(*role);
            let docked: Vec<PanelId> = layout.docked().collect();
            let built = docked.iter().filter(|p| is_implemented(**p)).count();
            println!(
                "{role:?}: {built} of {} docked panels implemented",
                docked.len()
            );
            assert!(!docked.is_empty());
        }
    }
}
