//! **No code path executes without a recorded human decision** (contract C-01, AP-01;
//! GAP-039). Two halves, because the rule is about paths and about the code that has
//! them.
//!
//! The static half reads the sources: the three things that count as execution today --
//! opening an engagement, publishing a plan as approved, building an effector handoff --
//! are constructed in one place each or nowhere, and that place is behind
//! `DecisionRecord::is_actionable`. The runtime half drives the desktop: a plan queued for
//! approval and ticked for a minute opens nothing; the moment a person decides it, an
//! engagement opens, and the decision is on the record **before** the engagement is.
//!
//! The API's decide route is covered where the transport is tested:
//! `gungnir-remote/tests/transport.rs` posts to `/v2/plans/{id}/decision` and is refused
//! (`refuse_decision`), so no decision enters a node through the wire either.

use gungnir_app::decisions;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_command::{ApprovalWorkflow, OperatorDecision, Submission};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_eventing::Event;
use gungnir_model::events::{CommandEvent, EngagementEvent, InterceptEvent};
use gungnir_model::{
    EffectorLayer, InterceptSolutionView, MissionTime, PlanId, PlanKind, PlanView, ResourceId,
    TrackId,
};
use gungnir_policy::PolicyVerdict;
use gungnir_time::ReplayClockAuthority;
use gungnir_ui::panels::approval_queue::PendingId;
use std::path::Path;

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == "tests" || n == "benches")
            {
                continue;
            }
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((path.to_string_lossy().replace('\\', "/"), text));
            }
        }
    }
}

/// The text before the first `#[cfg(test)]`: the part that ships.
fn shipped(text: &str) -> &str {
    text.split("#[cfg(test)]").next().unwrap_or(text)
}

/// **Static.** Every construction of an executing thing is where the record gates it.
#[test]
fn execution_is_constructed_only_behind_a_decision_record() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).expect("workspace").flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("gungnir-"))
        {
            sources(&path.join("src"), &mut files);
        }
    }
    assert!(files.len() > 50);

    let mut engagement_sites = Vec::new();
    let mut approved_sites = Vec::new();
    let mut handoff_sites = Vec::new();
    for (path, text) in &files {
        for (i, line) in shipped(text).lines().enumerate() {
            let t = line.trim();
            if t.starts_with("//") {
                continue;
            }
            let at = format!("{path}:{}", i + 1);
            if t.contains("Engagement::open(") {
                engagement_sites.push(at.clone());
            }
            // A construction, not a pattern: `PlanApproved(x)` on the right of `=>` or
            // as an argument, never `| InterceptEvent::PlanApproved(p)` in a match arm.
            if t.contains("PlanApproved(")
                && !t.contains("=>")
                && !t.contains('|')
                && !t.starts_with("PlanApproved(")
            {
                approved_sites.push(at.clone());
            }
            if t.contains("Handoff::from_decision(") {
                handoff_sites.push(at);
            }
        }
    }
    assert_eq!(
        engagement_sites.len(),
        1,
        "an engagement is opened in exactly one place: {engagement_sites:?}"
    );
    assert!(
        engagement_sites[0].ends_with("gungnir-app/src/engagements.rs:90")
            || engagement_sites[0].contains("gungnir-app/src/engagements.rs"),
        "{engagement_sites:?}"
    );
    let engagements = files
        .iter()
        .find(|(p, _)| p.ends_with("gungnir-app/src/engagements.rs"))
        .map(|(_, t)| t.as_str())
        .expect("engagements.rs");
    let open_fn = engagements
        .split("pub fn open_for")
        .nth(1)
        .expect("open_for");
    assert!(
        open_fn.contains("if !record.is_actionable()"),
        "open_for does not gate on the record's actionability"
    );
    assert!(
        approved_sites.is_empty(),
        "PlanApproved is published somewhere: {approved_sites:?}"
    );
    assert_eq!(
        handoff_sites.len(),
        1,
        "a handoff is built in exactly one place: {handoff_sites:?}"
    );
    assert!(
        handoff_sites[0].contains("gungnir-app/src/handoffs.rs"),
        "{handoff_sites:?}"
    );
    let handoffs = files
        .iter()
        .find(|(p, _)| p.ends_with("gungnir-app/src/handoffs.rs"))
        .map(|(_, t)| t.as_str())
        .expect("handoffs.rs");
    assert!(
        handoffs
            .split("pub fn issue_for")
            .nth(1)
            .is_some_and(|f| f.contains("if !record.is_actionable()")),
        "issue_for does not gate on the record's actionability"
    );
}

/// **Runtime.** A queued plan executes nothing until a person decides it, and the
/// decision precedes the execution on the record.
#[test]
fn a_queued_plan_executes_nothing_until_a_person_decides() {
    let dir = std::env::temp_dir().join(format!("gungnir-no-execution-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![ResourceConfig {
            id: 1,
            position: [0.0, 0.0, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            handoff_endpoint: None,
            intercept_speed_mps: None,
        }],
        ..ConfigBaseline::default()
    };
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 30.0);
    // No expiry configured for the layer: the item is preserved, not swept (DN-10).
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    let events = state.events.subscribe();

    // The queue is fed directly: no planner proposes anything in this build (GAP-011),
    // and everything after this line is the desktop's own path.
    let pending = state
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(7),
                kind: PlanKind::Intercept {
                    solutions: vec![InterceptSolutionView {
                        resource: ResourceId(1),
                        track: TrackId(3),
                        intercept_point: None,
                        time_to_intercept_s: Some(10.0),
                    }],
                },
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(0.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".to_owned(),
        })
        .expect("submit");

    for t in 1..=60 {
        state.clock = Box::new(ReplayClockAuthority {
            current: MissionTime(f64::from(t)),
        });
        update::tick(&mut state);
    }
    assert!(
        state.engagements.is_empty(),
        "an engagement opened with nobody deciding"
    );
    assert_eq!(
        state.approvals.pending().len(),
        1,
        "the item was swept or lost"
    );
    let mut seen: Vec<String> = events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Engagement(_) => Some("engagement".to_string()),
            Event::Intercept(InterceptEvent::PlanApproved(_)) => Some("approved".to_string()),
            Event::Command(CommandEvent::Decided { .. }) => Some("decided".to_string()),
            _ => None,
        })
        .collect();
    assert!(
        seen.is_empty(),
        "the record shows execution before a decision: {seen:?}"
    );

    decisions::decide(&mut state, PendingId(pending.0), OperatorDecision::Accepted)
        .expect("decided");
    update::tick(&mut state);
    assert_eq!(
        state.engagements.len(),
        1,
        "the decision opened no engagement"
    );
    let ordered: Vec<(u64, &str)> = events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Command(CommandEvent::Decided { .. }) => Some((e.seq, "decided")),
            Event::Engagement(EngagementEvent::Opened { .. }) => Some((e.seq, "opened")),
            _ => None,
        })
        .collect();
    seen = ordered.iter().map(|(_, k)| (*k).to_string()).collect();
    assert_eq!(seen, ["decided", "opened"], "{ordered:?}");
    let _ = std::fs::remove_dir_all(dir);
}
