// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mid-session failover (GAP-050): a node silent past its heartbeat timeout makes the
//! desktop fall back to embedded services, say so on the record, and keep the link; the
//! node answering again marks a reconciliation due, and the desktop does not switch back
//! on its own. PN-18 reports exactly that and never a merge it did not compute.
//!
//! Since the owner's GAP-067 walk (2026-09-16), D-03's arbitration rule resolves every
//! conflict it can rank when the reconciliation is computed -- journaled as the rule's
//! verdict and audited as nobody's decision -- and a conflict it cannot rank, a side with
//! no recorded role, waits for a person permitted `plan.decide`, as does the switch back.

use gungnir_app::failover::ReconciliationView;
use gungnir_app::state::AppState;
use gungnir_app::{failover, update};
use gungnir_config::{BackendConfig, ConfigBaseline};
use gungnir_eventing::{Envelope, Event};
use gungnir_model::arbitration::{ArbitrationGround, ConflictSide, SideOutcome};
use gungnir_model::events::{CommandEvent, LinkEvent, VerdictSummary};
use gungnir_model::{DecisionId, MissionTime, PlanId};
use gungnir_remote::link::{HistoryOutcome, NodeLink, HEARTBEAT_TIMEOUT};
use gungnir_time::ReplayClockAuthority;

/// A decision by operator 7 whose role was never recorded, as every decision journaled
/// before 2026-09-16 reads.
fn decided(plan: u128, accepted: bool) -> Event {
    decided_as(plan, accepted, Some("7"), None)
}

/// A decision as a journal holds it, naming the operator and the role the deciding
/// session carried when there was one.
fn decided_as(plan: u128, accepted: bool, operator: Option<&str>, role: Option<&str>) -> Event {
    Event::Command(CommandEvent::Decided {
        plan: PlanId(plan),
        decision: DecisionId(plan),
        accepted,
        operator: operator.map(str::to_string),
        role: role.map(str::to_string),
        verdict: VerdictSummary::RequiresHumanApproval,
        rationale: (!accepted).then(|| "no".to_string()),
    })
}

fn expired(plan: u128, at: f64) -> Event {
    Event::Command(CommandEvent::Expired {
        plan: PlanId(plan),
        at: MissionTime(at),
    })
}

fn node(seq: u64, t: f64, event: Event) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(t),
        event,
    }
}

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-failover-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        backend: BackendConfig::Remote {
            endpoint: "http://node.local:7410".into(),
        },
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    (state, dir)
}

fn at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
    update::tick(state);
}

/// A desktop whose node went silent at T+102 and answered again at T+150, having
/// journaled `local` (mission time, event) meanwhile, with the bus followed from before
/// the outage. The node's half is then supplied by the test, as a socket would supply it;
/// nothing ticks after that, because a tick would poll the scripted link's own fetch.
fn restored_outage(
    name: &str,
    local: Vec<(f64, Event)>,
) -> (
    AppState,
    std::path::PathBuf,
    NodeLink,
    gungnir_eventing::Receiver<Envelope>,
) {
    let (mut state, dir) = desktop(name);
    let link = NodeLink::scripted();
    link.script_session(Some("token".into()), 40);
    state.link = Some(link.clone());
    state.backend = BackendConfig::Remote {
        endpoint: "http://node.local:7410".into(),
    };
    let events = state.events.subscribe();
    let long_ago = std::time::Instant::now()
        .checked_sub(HEARTBEAT_TIMEOUT + std::time::Duration::from_secs(3))
        .expect("up long enough");
    link.script_liveness(false, Some(long_ago));
    at(&mut state, 102.0);
    for (t, event) in local {
        update::publish(&mut state, MissionTime(t), event);
    }
    at(&mut state, 112.0);
    link.script_liveness(true, Some(std::time::Instant::now()));
    at(&mut state, 150.0);
    (state, dir, link, events)
}

/// The computed reconciliation, or a failure naming what PN-18 showed instead.
fn reconciled(state: &AppState) -> (failover::Reconciliation, bool) {
    match failover::reconciliation_view(state) {
        ReconciliationView::Due {
            reconciliation: Some(Ok(r)),
            can_switch_back,
            ..
        } => (r, can_switch_back),
        other => panic!("no reconciliation computed: {other:?}"),
    }
}

/// The side a conflict reports for a decision.
fn side(outcome: SideOutcome, operator: Option<&str>, role: Option<&str>, t: f64) -> ConflictSide {
    ConflictSide {
        outcome,
        operator: operator.map(str::to_string),
        role: role.map(str::to_string),
        at: MissionTime(t),
    }
}

/// Every rule verdict on the bus, as (plan, kept this desktop's, ground).
fn verdicts(events: &[Envelope]) -> Vec<(PlanId, bool, ArbitrationGround)> {
    events
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::ConflictArbitrated {
                plan,
                kept_local,
                ground,
                ..
            }) => Some((*plan, *kept_local, *ground)),
            _ => None,
        })
        .collect()
}

/// Every person's resolution on the bus, as (plan, kept this desktop's).
fn person_resolutions(events: &[Envelope]) -> Vec<(PlanId, bool)> {
    events
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::ConflictResolved {
                plan, kept_local, ..
            }) => Some((*plan, *kept_local)),
            _ => None,
        })
        .collect()
}

/// The conflict count the switch back put on the record.
fn switched_back_conflicts(events: &[Envelope]) -> Vec<usize> {
    events
        .iter()
        .filter_map(|env| match &env.event {
            Event::Link(LinkEvent::SwitchedBack { conflicts, .. }) => Some(*conflicts),
            _ => None,
        })
        .collect()
}

/// Audit rows written under `plan.decide`.
fn decide_audits(state: &AppState) -> Vec<String> {
    use gungnir_security::AuditLog;
    state
        .audit
        .entries()
        .iter()
        .filter(|e| e.action == gungnir_security::actions::DECIDE_PLAN)
        .map(|e| e.detail.clone())
        .collect()
}

#[test]
fn a_silent_node_makes_the_desktop_fall_back_and_a_returning_one_makes_reconciliation_due() {
    let (mut state, dir) = desktop("fallback");
    // As if a sign-in had established the link: remote backend, a scripted link.
    let link = NodeLink::scripted();
    state.link = Some(link.clone());
    state.backend = BackendConfig::Remote {
        endpoint: "http://node.local:7410".into(),
    };
    let events = state.events.subscribe();
    assert_eq!(
        failover::reconciliation_view(&state),
        ReconciliationView::NothingDue
    );

    // Heard just now: nothing happens.
    link.script_liveness(true, Some(std::time::Instant::now()));
    at(&mut state, 101.0);
    assert!(matches!(state.backend, BackendConfig::Remote { .. }));
    assert!(state.fallback.is_none());

    // Silent past the timeout: fall back, say so, keep the link.
    let long_ago = std::time::Instant::now()
        .checked_sub(HEARTBEAT_TIMEOUT + std::time::Duration::from_secs(3))
        .expect("the process has been up longer than the timeout");
    link.script_liveness(false, Some(long_ago));
    at(&mut state, 102.0);
    assert!(
        matches!(state.backend, BackendConfig::Embedded),
        "fell back"
    );
    let fallback = state.fallback.clone().expect("recorded");
    assert_eq!(fallback.since, MissionTime(102.0));
    assert!(fallback.silent_s > HEARTBEAT_TIMEOUT.as_secs_f64());
    assert!(fallback.restored_at.is_none());
    assert!(state.link.is_some(), "the link keeps retrying");
    assert!(
        state.alerts.iter().any(|a| a.contains("running embedded")),
        "{:?}",
        state.alerts
    );
    assert!(matches!(
        failover::reconciliation_view(&state),
        ReconciliationView::OutageOngoing { .. }
    ));

    // The node answers: reconciliation due, and no switch back.
    link.script_liveness(true, Some(std::time::Instant::now()));
    at(&mut state, 150.0);
    assert!(
        matches!(state.backend, BackendConfig::Embedded),
        "stays embedded until reconciled"
    );
    match failover::reconciliation_view(&state) {
        ReconciliationView::Due {
            from,
            to,
            reconciliation,
            can_switch_back,
            ..
        } => {
            assert_eq!((from, to), (MissionTime(102.0), MissionTime(150.0)));
            // The scripted link signed in with no token, so the node's half is named as
            // unavailable rather than merged from nothing.
            match reconciliation {
                Some(Err(reason)) => assert!(reason.contains("token"), "{reason}"),
                other => panic!("{other:?}"),
            }
            assert!(can_switch_back, "seen, and the node answers");
        }
        other => panic!("{other:?}"),
    }
    let kinds: Vec<&str> = events
        .try_iter()
        .filter_map(|env| match env.event {
            Event::Link(LinkEvent::FellBack { .. }) => Some("fell-back"),
            Event::Link(LinkEvent::Restored { .. }) => Some("restored"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["fell-back", "restored"]);
    // A further tick changes nothing: one outage, one report.
    at(&mut state, 151.0);
    assert_eq!(
        state.fallback.as_ref().and_then(|f| f.restored_at),
        Some(MissionTime(150.0))
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The reconciliation over both journals (GAP-050, D-03): the node's history for the
/// outage is merged with the desktop's, every envelope once, and a conflicting decision
/// the rule cannot rank -- neither side recorded a role -- is reported and left to a
/// person; the switch back is a person's act that puts the merge on the record.
#[test]
#[allow(clippy::too_many_lines)]
fn the_outage_is_reconciled_from_the_nodes_history_and_a_person_switches_back() {
    let (mut state, dir) = desktop("reconcile");
    let link = NodeLink::scripted();
    link.script_session(Some("token".into()), 40);
    state.link = Some(link.clone());
    state.backend = BackendConfig::Remote {
        endpoint: "http://node.local:7410".into(),
    };
    let events = state.events.subscribe();
    let long_ago = std::time::Instant::now()
        .checked_sub(HEARTBEAT_TIMEOUT + std::time::Duration::from_secs(3))
        .expect("up long enough");
    link.script_liveness(false, Some(long_ago));
    at(&mut state, 102.0);
    assert_eq!(state.fallback.as_ref().map(|f| f.last_seq), Some(40));

    // Two decisions taken here during the outage.
    let now = MissionTime(110.0);
    update::publish(&mut state, now, decided(1, false));
    update::publish(&mut state, MissionTime(111.0), decided(2, true));
    at(&mut state, 112.0);

    // The node answers: the fetch is asked for (a scripted link reaches no socket, so
    // the answer is supplied as the socket would have supplied it).
    link.script_liveness(true, Some(std::time::Instant::now()));
    at(&mut state, 150.0);
    assert!(matches!(
        failover::reconciliation_view(&state),
        ReconciliationView::Due {
            reconciliation: None,
            ..
        }
    ));
    let remote = vec![
        Envelope {
            seq: 41,
            mission_time: MissionTime(110.0),
            event: decided(1, true),
        },
        Envelope {
            seq: 42,
            mission_time: MissionTime(111.0),
            event: decided(2, true),
        },
        Envelope {
            seq: 43,
            mission_time: MissionTime(200.0),
            event: decided(3, true),
        },
    ];
    failover::supply_history(&mut state, HistoryOutcome::Complete(remote));
    match failover::reconciliation_view(&state) {
        ReconciliationView::Due {
            reconciliation: Some(Ok(r)),
            can_switch_back,
            ..
        } => {
            assert_eq!(
                r.remote, 2,
                "the envelope after the outage is outside the window"
            );
            assert!(r.local >= 2, "{r:?}");
            // Plan 2's decision is in both journals at the same time: the one duplicate,
            // dropped once, and nothing else.
            assert_eq!(r.duplicates_dropped, 1, "{r:?}");
            assert_eq!(r.merged, r.local + r.remote - 1, "{r:?}");
            assert_eq!(r.conflicts.len(), 1, "{r:?}");
            assert_eq!(r.conflicts[0].plan, PlanId(1));
            assert_eq!(r.conflicts[0].local.outcome, SideOutcome::Rejected);
            assert_eq!(r.conflicts[0].remote.outcome, SideOutcome::Accepted);
            assert!(r.arbitrated.is_empty(), "no role on either side: {r:?}");
            assert!(
                !can_switch_back,
                "not until the conflict is resolved (D-15)"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        failover::switch_back(&mut state).is_err(),
        "the switch refuses while a conflict is open"
    );
    failover::resolve_conflict(&mut state, PlanId(1), true).expect("resolved by a person");
    assert!(matches!(
        failover::reconciliation_view(&state),
        ReconciliationView::Due {
            can_switch_back: true,
            ..
        }
    ));
    assert!(
        matches!(state.backend, BackendConfig::Embedded),
        "nothing switches back on its own"
    );

    // A silent node refuses the switch; an answering one takes it.
    link.script_liveness(false, Some(long_ago));
    assert!(failover::switch_back(&mut state).is_err());
    link.script_liveness(true, Some(std::time::Instant::now()));
    failover::switch_back(&mut state).expect("switches back");
    assert!(matches!(state.backend, BackendConfig::Remote { .. }));
    assert!(state.fallback.is_none());
    assert_eq!(
        failover::reconciliation_view(&state),
        ReconciliationView::NothingDue
    );
    let switched: Vec<(usize, usize, bool)> = events
        .try_iter()
        .filter_map(|env| match env.event {
            Event::Link(LinkEvent::SwitchedBack {
                merged,
                conflicts,
                node_history,
                ..
            }) => Some((merged, conflicts, node_history)),
            _ => None,
        })
        .collect();
    assert_eq!(switched.len(), 1);
    assert_eq!((switched[0].1, switched[0].2), (1, true));
    let _ = std::fs::remove_dir_all(dir);
}

/// An outage longer than the node's window: the merge cannot be computed and PN-18
/// says so; the switch back is still a person's to make, on the record as such.
#[test]
fn a_node_that_no_longer_holds_the_outage_says_so_and_the_switch_records_it() {
    let (mut state, dir) = desktop("gone");
    let link = NodeLink::scripted();
    link.script_session(Some("token".into()), 1);
    state.link = Some(link.clone());
    state.backend = BackendConfig::Remote {
        endpoint: "http://node.local:7410".into(),
    };
    let events = state.events.subscribe();
    let long_ago = std::time::Instant::now()
        .checked_sub(HEARTBEAT_TIMEOUT + std::time::Duration::from_secs(3))
        .expect("up long enough");
    link.script_liveness(false, Some(long_ago));
    at(&mut state, 102.0);
    link.script_liveness(true, Some(std::time::Instant::now()));
    at(&mut state, 150.0);
    failover::supply_history(
        &mut state,
        HistoryOutcome::Gone {
            reason: "seq 2 is older than this node retains".into(),
        },
    );
    match failover::reconciliation_view(&state) {
        ReconciliationView::Due {
            reconciliation: Some(Err(reason)),
            ..
        } => assert!(reason.contains("no longer holds"), "{reason}"),
        other => panic!("{other:?}"),
    }
    failover::switch_back(&mut state).expect("a person may still switch");
    let recorded = events.try_iter().any(|env| {
        matches!(
            env.event,
            Event::Link(LinkEvent::SwitchedBack {
                node_history: false,
                ..
            })
        )
    });
    assert!(recorded, "the record says the node's half was missing");
    let _ = std::fs::remove_dir_all(dir);
}

/// The outage's detections reach the link's outbox (§8.4): what the embedded tracker
/// accepted while the node was away is queued for it, in order, and a person's
/// resolution of a conflict is recorded and audited under `plan.decide`.
#[test]
fn a_conflict_is_resolved_by_a_person_and_the_outage_is_queued_for_the_node() {
    use gungnir_security::{AuditLog, Role};

    let (mut state, dir) = desktop("resolve");
    let link = NodeLink::scripted();
    link.script_session(Some("token".into()), 40);
    state.link = Some(link.clone());
    state.backend = BackendConfig::Remote {
        endpoint: "http://node.local:7410".into(),
    };
    state.set_role(Role::Supervisor);
    let events = state.events.subscribe();
    let long_ago = std::time::Instant::now()
        .checked_sub(HEARTBEAT_TIMEOUT + std::time::Duration::from_secs(3))
        .expect("up long enough");
    link.script_liveness(false, Some(long_ago));
    at(&mut state, 102.0);

    // A detection accepted during the outage is teed to the link.
    let detection = gungnir_model::DetectionView {
        sensor: gungnir_model::SensorId(1),
        source_time: MissionTime(105.0),
        receipt_time: MissionTime(105.0),
        measurement: gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
            variance_m2: [400.0, 400.0, 900.0],
        },
        provenance: gungnir_model::Provenance::default(),
    };
    state
        .tracking
        .submit_detection(detection)
        .expect("accepted");
    assert_eq!(link.outbox_len(), 1, "queued for the node");
    let outbox = failover::outbox_view(&state).expect("a link");
    assert_eq!((outbox.queued, outbox.forwarded, outbox.dropped), (1, 0, 0));

    update::publish(&mut state, MissionTime(110.0), decided(1, false));
    at(&mut state, 112.0);
    link.script_liveness(true, Some(std::time::Instant::now()));
    at(&mut state, 150.0);
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![Envelope {
            seq: 41,
            mission_time: MissionTime(110.0),
            event: decided(1, true),
        }]),
    );
    // Not a conflict of this outage: refused by name.
    assert!(failover::resolve_conflict(&mut state, PlanId(9), true).is_err());
    failover::resolve_conflict(&mut state, PlanId(1), false).expect("resolved");
    match failover::reconciliation_view(&state) {
        ReconciliationView::Due {
            reconciliation: Some(Ok(r)),
            ..
        } => {
            assert!(r.conflicts.is_empty());
            assert_eq!(r.resolved, vec![(PlanId(1), false)]);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        failover::resolve_conflict(&mut state, PlanId(1), true).is_err(),
        "resolved twice"
    );
    let resolved = events.try_iter().any(|env| {
        matches!(
            env.event,
            Event::Link(LinkEvent::ConflictResolved {
                plan: PlanId(1),
                kept_local: false,
                ..
            })
        )
    });
    assert!(resolved, "the resolution is on the record");
    assert!(
        state
            .audit
            .entries()
            .iter()
            .any(|e| e.action == gungnir_security::actions::DECIDE_PLAN
                && e.detail.contains("reconciliation")),
        "audited under plan.decide"
    );
    // A role that may not decide plans may not resolve one either.
    state.set_role(Role::Analyst);
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![Envelope {
            seq: 42,
            mission_time: MissionTime(111.0),
            event: decided(1, true),
        }]),
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// D-03's rank rule settles a Supervisor-versus-Operator conflict when the reconciliation
/// is computed, in either direction and whatever the times: **no person is involved** --
/// the desktop's selected role here may not decide plans at all -- the verdict is on the
/// record as the rule's, nothing is audited as anybody's `plan.decide`, and the switch back
/// needs nobody's resolution.
#[test]
fn a_higher_role_is_kept_by_the_rule_with_no_person_involved() {
    let (mut state, dir, _link, events) = restored_outage(
        "rule-rank",
        vec![
            (110.0, decided_as(1, false, Some("7"), Some("Supervisor"))),
            (111.0, decided_as(2, true, Some("7"), Some("Operator"))),
        ],
    );
    state.set_role(gungnir_security::Role::Analyst);
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![
            // Earlier than this desktop's Supervisor, and still overruled: rank first.
            node(41, 105.0, decided_as(1, true, Some("9"), Some("Operator"))),
            node(
                42,
                115.0,
                decided_as(2, false, Some("9"), Some("Commander")),
            ),
        ]),
    );

    let (r, can_switch_back) = reconciled(&state);
    assert!(r.conflicts.is_empty(), "left to a person: {r:?}");
    assert!(r.resolved.is_empty());
    assert_eq!(r.arbitrated.len(), 2, "{r:?}");
    let plan_1 = &r.arbitrated[0];
    assert_eq!(plan_1.conflict.plan, PlanId(1));
    assert!(
        plan_1.kept_local,
        "the Supervisor here outranks the node's Operator"
    );
    assert_eq!(plan_1.ground, ArbitrationGround::HigherRole);
    assert_eq!(
        plan_1.conflict.local,
        side(SideOutcome::Rejected, Some("7"), Some("Supervisor"), 110.0)
    );
    assert_eq!(
        plan_1.conflict.remote,
        side(SideOutcome::Accepted, Some("9"), Some("Operator"), 105.0)
    );
    let plan_2 = &r.arbitrated[1];
    assert_eq!(plan_2.conflict.plan, PlanId(2));
    assert!(
        !plan_2.kept_local,
        "the node's Commander outranks the Operator here"
    );
    assert_eq!(plan_2.ground, ArbitrationGround::HigherRole);
    assert!(can_switch_back, "the rule left nothing for a person");

    let bus: Vec<Envelope> = events.try_iter().collect();
    assert_eq!(
        verdicts(&bus),
        vec![
            (PlanId(1), true, ArbitrationGround::HigherRole),
            (PlanId(2), false, ArbitrationGround::HigherRole),
        ],
        "the verdicts are on the record as the rule's"
    );
    let journaled_sides = bus.iter().find_map(|env| match &env.event {
        Event::Link(LinkEvent::ConflictArbitrated {
            plan: PlanId(1),
            local,
            remote,
            ..
        }) => Some((local.clone(), remote.clone())),
        _ => None,
    });
    assert_eq!(
        journaled_sides,
        Some((
            plan_1.conflict.local.clone(),
            plan_1.conflict.remote.clone()
        )),
        "the verdict carries both sides as the rule read them"
    );
    assert!(
        person_resolutions(&bus).is_empty(),
        "a rule's verdict was recorded as a person's"
    );
    assert!(
        decide_audits(&state).is_empty(),
        "a rule's verdict was audited as somebody's plan.decide: {:?}",
        decide_audits(&state)
    );

    // Not a person's to overturn from PN-18, even for a role that may decide plans, and
    // the refusal says why.
    state.set_role(gungnir_security::Role::Operator);
    let refused = failover::resolve_conflict(&mut state, PlanId(1), false)
        .expect_err("the rule already resolved it");
    assert!(refused.contains("arbitration rule"), "{refused}");

    failover::switch_back(&mut state).expect("nobody's resolution is needed");
    let bus: Vec<Envelope> = events.try_iter().collect();
    assert_eq!(
        switched_back_conflicts(&bus),
        vec![2],
        "the switch counts the rule's conflicts"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// D-03's tie-break: on equal rank the earlier decision is kept, from either journal; and
/// an exact tie -- equal rank at the same mission time -- keeps this desktop's, which the
/// rule reads first, under a ground that does not call either decision the earlier.
#[test]
fn equal_rank_keeps_the_earlier_decision_and_an_exact_tie_keeps_this_desktops() {
    let (mut state, dir, _link, events) = restored_outage(
        "rule-tie",
        vec![
            (110.0, decided_as(1, false, Some("7"), Some("Operator"))),
            (111.0, decided_as(2, true, Some("7"), Some("Supervisor"))),
            (106.0, decided_as(3, true, Some("7"), Some("Operator"))),
        ],
    );
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![
            node(41, 108.0, decided_as(1, true, Some("9"), Some("Operator"))),
            node(
                42,
                111.0,
                decided_as(2, false, Some("9"), Some("Supervisor")),
            ),
            node(43, 109.0, decided_as(3, false, Some("9"), Some("Operator"))),
        ]),
    );

    let (r, can_switch_back) = reconciled(&state);
    assert!(r.conflicts.is_empty(), "{r:?}");
    let settled: Vec<(PlanId, bool, ArbitrationGround)> = r
        .arbitrated
        .iter()
        .map(|a| (a.conflict.plan, a.kept_local, a.ground))
        .collect();
    assert_eq!(
        settled,
        vec![
            (PlanId(1), false, ArbitrationGround::EarlierOnEqualRank),
            (PlanId(2), true, ArbitrationGround::SameTimeOnEqualRank),
            (PlanId(3), true, ArbitrationGround::EarlierOnEqualRank),
        ]
    );
    assert!(can_switch_back);
    let bus: Vec<Envelope> = events.try_iter().collect();
    assert_eq!(verdicts(&bus), settled);
    assert!(decide_audits(&state).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// DN-10 §9 through the reconciliation: an expiry on either side against a decision keeps
/// the decision, **with no role recorded anywhere**, because somebody chose and nobody
/// else did -- a decision facing an expiry is always rankable.
#[test]
fn an_expiry_against_a_decision_keeps_the_decision_without_consulting_rank() {
    let (mut state, dir, _link, events) = restored_outage(
        "rule-expiry",
        vec![
            (110.0, expired(1, 110.0)),
            (111.0, decided_as(2, false, None, None)),
        ],
    );
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![
            node(41, 111.0, decided_as(1, true, Some("9"), None)),
            node(42, 105.0, expired(2, 105.0)),
        ]),
    );

    let (r, can_switch_back) = reconciled(&state);
    assert!(
        r.conflicts.is_empty(),
        "an expiry was left to a person: {r:?}"
    );
    let settled: Vec<(PlanId, bool, ArbitrationGround)> = r
        .arbitrated
        .iter()
        .map(|a| (a.conflict.plan, a.kept_local, a.ground))
        .collect();
    assert_eq!(
        settled,
        vec![
            (PlanId(1), false, ArbitrationGround::DecisionOverExpiry),
            (PlanId(2), true, ArbitrationGround::DecisionOverExpiry),
        ]
    );
    assert_eq!(r.arbitrated[0].conflict.local.outcome, SideOutcome::Expired);
    assert!(can_switch_back);
    let bus: Vec<Envelope> = events.try_iter().collect();
    assert_eq!(verdicts(&bus), settled);
    let _ = std::fs::remove_dir_all(dir);
}

/// The owner's condition: a conflict the rule cannot rank honestly -- a side whose role was
/// never recorded, or names no role this build knows -- stays for a person. The switch
/// back is refused until **a person permitted `plan.decide`** has resolved every one; a
/// role that may not decide is refused too; and each resolution is journaled and audited
/// as it always was.
#[test]
fn a_conflict_with_no_recorded_role_waits_for_a_person_permitted_to_decide() {
    use gungnir_security::Role;

    let (mut state, dir, _link, events) = restored_outage(
        "rule-unranked",
        vec![
            (110.0, decided_as(1, false, Some("7"), Some("Supervisor"))),
            (111.0, decided_as(2, true, Some("7"), Some("Quartermaster"))),
        ],
    );
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![
            // Journaled before roles were recorded.
            node(41, 111.0, decided_as(1, true, Some("9"), None)),
            node(42, 112.0, decided_as(2, false, Some("9"), Some("Operator"))),
        ]),
    );

    let (r, can_switch_back) = reconciled(&state);
    assert!(r.arbitrated.is_empty(), "the rule guessed a rank: {r:?}");
    assert_eq!(
        r.conflicts.iter().map(|c| c.plan).collect::<Vec<_>>(),
        vec![PlanId(1), PlanId(2)]
    );
    assert!(failover::unranked_reason(&r.conflicts[0])
        .contains("the node's decision has no recorded role"));
    assert!(failover::unranked_reason(&r.conflicts[1]).contains("\"Quartermaster\""));
    assert!(!can_switch_back);
    assert!(failover::switch_back(&mut state).is_err());

    // Nobody signed in and a selected role that may not decide: refused, still waiting.
    state.set_role(Role::Analyst);
    assert!(failover::resolve_conflict(&mut state, PlanId(1), false).is_err());
    assert!(failover::switch_back(&mut state).is_err());

    state.set_role(Role::Operator);
    failover::resolve_conflict(&mut state, PlanId(1), false).expect("a person resolves it");
    assert!(
        failover::switch_back(&mut state).is_err(),
        "one conflict is still open"
    );
    failover::resolve_conflict(&mut state, PlanId(2), true).expect("a person resolves it");

    let bus: Vec<Envelope> = events.try_iter().collect();
    assert!(
        verdicts(&bus).is_empty(),
        "a person's resolution was recorded as the rule's"
    );
    assert_eq!(
        person_resolutions(&bus),
        vec![(PlanId(1), false), (PlanId(2), true)]
    );
    let audited = decide_audits(&state);
    assert_eq!(audited.len(), 2, "{audited:?}");
    assert!(
        audited.iter().all(|d| d.contains("reconciliation")),
        "{audited:?}"
    );

    failover::switch_back(&mut state).expect("every conflict is resolved");
    let bus: Vec<Envelope> = events.try_iter().collect();
    assert_eq!(switched_back_conflicts(&bus), vec![2]);
    let _ = std::fs::remove_dir_all(dir);
}

/// PN-18 as drawn: a conflict the rule settled shows its verdict -- the side kept, over
/// which, and why -- with no button, and only the conflict left to a person offers the two
/// keep buttons.
#[test]
fn pn_18_shows_the_rules_verdicts_and_offers_keep_buttons_only_for_a_persons_conflicts() {
    let (mut state, dir, _link, _events) = restored_outage(
        "rule-pn18",
        vec![
            (110.0, decided_as(1, false, Some("7"), Some("Supervisor"))),
            (111.0, decided_as(2, true, Some("7"), Some("Supervisor"))),
        ],
    );
    failover::supply_history(
        &mut state,
        HistoryOutcome::Complete(vec![
            node(41, 105.0, decided_as(1, true, Some("9"), Some("Operator"))),
            node(42, 112.0, decided_as(2, false, Some("9"), None)),
        ]),
    );

    let probe = gungnir_ui::harness::RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        gungnir_app::workspace::render_panel(ui, gungnir_workflow::PanelId::Reconciliation, &state)
    });
    let drawn = frame.joined();
    assert!(
        frame.says("resolved by the arbitration rule; no person was asked"),
        "{drawn}"
    );
    assert!(
        frame.says(
            "plan 1: kept this desktop's (rejected by operator 7 as Supervisor at T+110 s) \
             over the node's (accepted by operator 9 as Operator at T+105 s): the higher \
             role wins (D-03)."
        ),
        "{drawn}"
    );
    assert!(frame.says("the arbitration rule cannot rank"), "{drawn}");
    assert!(
        frame.says("the node's decision has no recorded role"),
        "{drawn}"
    );
    let buttons = |label: &str| frame.texts.iter().filter(|t| t.as_str() == label).count();
    assert_eq!(buttons("keep this desktop's"), 1, "{drawn}");
    assert_eq!(buttons("keep the node's"), 1, "{drawn}");
    let _ = std::fs::remove_dir_all(dir);
}
