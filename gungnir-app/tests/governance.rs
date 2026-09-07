// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Which algorithm configuration a deployment is running (GAP-086, DN-24).
//!
//! `gungnir-modelops` shipped with the scaffold and **no crate imported it**: the registry,
//! the promotion state machine and rollback were unreachable from any running system, and
//! `ConfigBaseline.tracking` was validated by `gungnir-config` and read by nobody. These
//! are the tests behind the claim that a deployment now governs what it runs.
//!
//! Since GAP-011 and GAP-053 the promoted configuration **does** filter:
//! `PIPELINE_IMPLEMENTED` is true and the desktop builds its tracker with
//! `PipelineSettings::from_baseline`. What the last test here pins is DN-24 §7's rule at
//! the boundary that still matters — a baseline this build cannot apply is not stamped, so
//! the picture and the governance record disagree visibly rather than quietly.

use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, TrackingConfig, TrackingProfileConfig};
use gungnir_eventing::{Envelope, Event, Receiver};
use gungnir_model::events::GovernanceEvent;

fn candidate(profile: &str, name: &str, promoted: bool) -> TrackingProfileConfig {
    TrackingProfileConfig {
        profile: profile.into(),
        name: name.into(),
        filter_selection: "imm-cv-ct".into(),
        gate_threshold: 9.21,
        promoted,
        validated_by: Some("oracle comparison".into()),
    }
}

fn desktop(name: &str, config: ConfigBaseline) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-governance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..config
    };
    (
        AppState::with_config(config).expect("the desktop starts"),
        dir,
    )
}

fn governance_events(events: &Receiver<Envelope>) -> Vec<GovernanceEvent> {
    events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Governance(g) => Some(g),
            _ => None,
        })
        .collect()
}

/// **The first registry this workspace has ever constructed.** A deployment declaring
/// profiles governs one candidate per profile, and the desktop can say which.
#[test]
fn a_deployment_with_profiles_governs_the_promoted_candidate() {
    let (state, dir) = desktop(
        "declared",
        ConfigBaseline {
            mission_profiles: vec!["air-defence".into(), "counter-uas".into()],
            tracking_profiles: vec![
                candidate("air-defence", "imm baseline", true),
                candidate("air-defence", "tighter gate", false),
                candidate("counter-uas", "short range", true),
            ],
            active_profile: Some("air-defence".into()),
            ..ConfigBaseline::default()
        },
    );

    let in_force = state.governance.in_force().expect("something is in force");
    assert_eq!(in_force.id.name, "imm baseline");
    assert_eq!(in_force.id.profile.as_str(), "air-defence");
    assert_eq!(
        state.governance.candidates().len(),
        2,
        "the operating profile's other candidate is what a rollback would restore"
    );
    assert!(state.governance.unavailable().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// **Which configuration a session ran under reaches the journal**, once, at the start.
/// It is `InForceAtStart` and not `Promoted`: nobody promoted anything, the file said so,
/// and there is nobody signed in to attribute an act to.
#[test]
fn the_session_journals_what_it_opened_with_and_calls_it_what_it_is() {
    let (mut state, dir) = desktop(
        "journaled",
        ConfigBaseline {
            mission_profiles: vec!["air-defence".into()],
            tracking_profiles: vec![candidate("air-defence", "imm baseline", true)],
            ..ConfigBaseline::default()
        },
    );
    let events = state.events.subscribe();

    update::tick(&mut state);
    let published = governance_events(&events);
    assert_eq!(published.len(), 1, "{published:?}");
    match &published[0] {
        GovernanceEvent::InForceAtStart { baseline, .. } => {
            assert_eq!(baseline.to_string(), "air-defence/imm baseline");
        }
        other => panic!("the session opened with a {other:?}"),
    }

    // Once per session, not once per frame: which configuration a session ran under is a
    // fact about the session, and repeating it would bury the acts that are facts about
    // moments in it.
    for _ in 0..3 {
        update::tick(&mut state);
    }
    assert!(
        governance_events(&events).is_empty(),
        "the session re-announced its configuration every tick"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **"Nothing was in force" and "we did not record it" are opposite claims** to whoever
/// reads the journal afterwards, so the default deployment says the first.
#[test]
fn a_deployment_governing_nothing_says_so_in_the_record() {
    let (mut state, dir) = desktop("nothing", ConfigBaseline::default());
    let events = state.events.subscribe();

    assert!(state.governance.in_force().is_none());
    assert!(state.governance.profiles().is_empty());

    update::tick(&mut state);
    let published = governance_events(&events);
    assert!(
        matches!(
            published.as_slice(),
            [GovernanceEvent::NoneInForce { profile: None, .. }]
        ),
        "{published:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A baseline written before DN-24 governs its one configuration under the implicit
/// `default` profile. **Nothing existing breaks**, and the deployment can still say what
/// it is running.
#[test]
fn a_baseline_with_only_tracking_governs_its_one_configuration() {
    let (state, dir) = desktop(
        "implicit",
        ConfigBaseline {
            tracking: Some(TrackingConfig {
                filter_selection: "imm-cv-ct".into(),
                gate_threshold: 9.21,
            }),
            ..ConfigBaseline::default()
        },
    );
    let in_force = state.governance.in_force().expect("something is in force");
    assert_eq!(in_force.id.profile.as_str(), "default");
    assert_eq!(
        gungnir_app::governance::in_force_label(&state.governance),
        "default/configured"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A baseline the registry refuses does not take the console away from an operator: the
/// desktop starts, governs nothing, and **says why on the alert list** rather than leaving
/// the panel to imply a configuration is in force.
#[test]
fn a_refused_baseline_leaves_the_desktop_running_and_honest() {
    // Validation would refuse this baseline outright, so it is constructed past validation
    // the way a deployment reaching this state would: a candidate the registry's own gate
    // rejects.
    let mut bad = candidate("air-defence", "broken", true);
    bad.gate_threshold = f64::NAN;
    let dir =
        std::env::temp_dir().join(format!("gungnir-governance-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        mission_profiles: vec!["air-defence".into()],
        tracking_profiles: vec![bad],
        ..ConfigBaseline::default()
    };
    // The desktop deliberately starts under a baseline that does not validate (GAP-052);
    // the objection is recorded on the mission and shown, and governance is refused.
    let state = AppState::with_config(config).expect("the desktop starts anyway");

    assert!(state.governance.in_force().is_none());
    let reason = state
        .governance
        .unavailable()
        .expect("a refusal without a reason is not a refusal");
    assert!(reason.contains("broken"), "{reason}");
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("No algorithm configuration is in force")),
        "nobody was told: {:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **The rule DN-24 §7 states, at the one boundary GAP-053 left.** The identity a track
/// would carry is available and is *not* stamped here, because this baseline names a filter
/// this build does not implement — so a governed-looking version on the track would be a
/// claim nothing downstream could check. A baseline this build *can* apply is stamped.
#[test]
fn a_governed_configuration_is_not_stamped_on_tracks_that_nothing_produced_from_it() {
    let (state, dir) = desktop(
        "stamp",
        ConfigBaseline {
            mission_profiles: vec!["air-defence".into()],
            tracking_profiles: vec![candidate("air-defence", "imm baseline", true)],
            ..ConfigBaseline::default()
        },
    );

    // The identity exists and is resolvable.
    let would = gungnir_app::governance::would_stamp(&state.governance).expect("an identity");
    assert_eq!(would.to_string(), "air-defence/imm baseline");

    // And nothing carries it, because nothing applied it. **The reason moved on
    // 2026-09-06** (GAP-053): the pipeline does read a promoted baseline now, and this
    // one names `imm-cv-ct`, which this build does not implement. The desktop refuses
    // to run a different filter under that baseline's identity and says so.
    assert!(
        gungnir_tracking_service::UNGOVERNED_ALGORITHM_VERSION.contains("ungoverned"),
        "the tracking service began claiming a governed configuration before it applies one"
    );
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("is not applied") && a.contains("ungoverned")),
        "the desktop did not say that the promoted baseline is not being applied: {:?}",
        state.alerts
    );
    assert!(state.tracking.tracks().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// The other half of DN-24 §7: a baseline naming a filter this build *does* implement is
/// applied, and then the desktop raises no complaint about it.
#[test]
fn a_promoted_baseline_this_build_implements_is_applied_without_complaint() {
    let (state, dir) = desktop(
        "applied",
        ConfigBaseline {
            mission_profiles: vec!["air-defence".into()],
            tracking_profiles: vec![TrackingProfileConfig {
                filter_selection: "kf-cv".into(),
                ..candidate("air-defence", "kf baseline", true)
            }],
            ..ConfigBaseline::default()
        },
    );
    assert!(
        !state.alerts.iter().any(|a| a.contains("is not applied")),
        "a baseline this build implements was refused: {:?}",
        state.alerts
    );
    let would = gungnir_app::governance::would_stamp(&state.governance).expect("an identity");
    assert_eq!(would.to_string(), "air-defence/kf baseline");
    let _ = std::fs::remove_dir_all(dir);
}
