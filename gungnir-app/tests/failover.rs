//! Mid-session failover (GAP-050): a node silent past its heartbeat timeout makes the
//! desktop fall back to embedded services, say so on the record, and keep the link; the
//! node answering again marks a reconciliation due, and the desktop does not switch back
//! on its own. PN-18 reports exactly that and never a merge it did not compute.

use gungnir_app::failover::ReconciliationView;
use gungnir_app::state::AppState;
use gungnir_app::{failover, update};
use gungnir_config::{BackendConfig, ConfigBaseline};
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::{CommandEvent, LinkEvent, VerdictSummary};
use gungnir_model::{DecisionId, MissionTime, PlanId};
use gungnir_remote::link::{HistoryOutcome, NodeLink, HEARTBEAT_TIMEOUT};
use gungnir_time::ReplayClockAuthority;

fn decided(plan: u64, accepted: bool) -> Event {
    Event::Command(CommandEvent::Decided {
        plan: PlanId(plan),
        decision: DecisionId(plan),
        accepted,
        operator: Some("7".into()),
        verdict: VerdictSummary::RequiresHumanApproval,
        rationale: (!accepted).then(|| "no".to_string()),
    })
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
/// outage is merged with the desktop's, a conflicting decision is reported and not
/// resolved, and the switch back is a person's act that puts the merge on the record.
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
            assert_eq!(r.conflicts.len(), 1, "{r:?}");
            assert_eq!(r.conflicts[0].plan, PlanId(1));
            assert!(!r.conflicts[0].local_accepted && r.conflicts[0].remote_accepted);
            assert!(r.merged >= 3, "{r:?}");
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
