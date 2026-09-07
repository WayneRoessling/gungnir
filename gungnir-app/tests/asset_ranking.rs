//! Scoring tracks against the defended-asset list (GAP-026, DN-01).
//!
//! `AssetListAssessor` existed from the day the design set landed and **nothing
//! constructed one**: the asset list was in the baseline, validated, and scored against
//! by nobody. These are the tests behind the claim that it is wired.
//!
//! The property worth protecting is the one `AssetListAssessor::is_unconfigured` was
//! written for: **a zero score and an unconfigured system look identical to an operator**,
//! and only one of them means the sector is quiet.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::{asset_assessor, asset_exposure, AssetRanking};
use gungnir_config::{AssetConfig, ConfigBaseline};

fn asset(id: u32, name: &str, priority: &str) -> AssetConfig {
    AssetConfig {
        id,
        name: name.into(),
        position: [0.0, 0.0, 0.0],
        radius_m: Some(500.0),
        priority: priority.into(),
        warning_lead_time_s: None,
        warning_channel: None,
        warning_within_m: None,
        note: None,
    }
}

fn desktop(name: &str, assets: Vec<AssetConfig>, origin: bool) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-assets-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        assets,
        origin: origin.then_some([0.0, 0.0, 0.0]),
        ..ConfigBaseline::default()
    };
    (AppState::with_config(config).expect("desktop state"), dir)
}

/// **The default deployment declares no assets, and says so rather than ranking
/// nothing.** An empty ranking would read as a quiet sector.
#[test]
fn a_deployment_with_no_assets_says_so_rather_than_ranking_nothing() {
    let (state, dir) = desktop("none", Vec::new(), true);

    let ranking = asset_exposure(&state);
    assert!(ranking.scores().is_empty());
    let reason = ranking.reason().expect("a reason");
    assert!(reason.contains("no defended assets"), "{reason}");

    // The assessor itself reports the same thing, which is the flag DN-01 put there.
    let assessor = asset_assessor(&state).expect("an origin is declared");
    assert!(assessor.is_unconfigured());
    let _ = std::fs::remove_dir_all(dir);
}

/// Without a declared origin there is no common frame, so nothing can be ranked at all --
/// and that is a different reason from having no assets. Guessing an origin would rank
/// every track against an asset placed somewhere plausible and wrong.
#[test]
fn without_an_origin_nothing_is_scored_and_the_reason_is_the_frame() {
    let (state, dir) = desktop("no-origin", vec![asset(1, "the harbour", "high")], false);

    assert!(asset_assessor(&state).is_none());
    let ranking = asset_exposure(&state);
    let reason = ranking.reason().expect("a reason");
    assert!(reason.contains("local frame origin"), "{reason}");
    // Not the no-assets reason: one is a missing frame and the other a missing list.
    assert!(!reason.contains("no defended assets"), "{reason}");
    let _ = std::fs::remove_dir_all(dir);
}

/// With assets and an origin the assessor is configured and the ranking is a ranking --
/// empty here only because this build has no tracking pipeline, which is a third thing
/// again and not something this gap can fix.
#[test]
fn a_configured_deployment_is_scored_even_when_there_are_no_tracks() {
    let (state, dir) = desktop(
        "configured",
        vec![
            asset(1, "the harbour", "critical"),
            asset(2, "the depot", "low"),
        ],
        true,
    );

    let assessor = asset_assessor(&state).expect("configured");
    assert!(
        !assessor.is_unconfigured(),
        "two assets read as unconfigured"
    );
    assert_eq!(assessor.baseline_version(), state.config.version);

    match asset_exposure(&state) {
        AssetRanking::Scored { scores, assets } => {
            assert_eq!(assets, 2, "the ranking did not say what it ranked against");
            // No tracks in this build, so nothing to score -- and the ranking says it
            // *was* scored, which is the distinction that matters.
            assert!(scores.is_empty());
        }
        AssetRanking::NotScored { reason } => {
            panic!("a configured deployment refused to score: {reason}")
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The asset list the assessor anchors is the baseline's, priorities included, so a
/// score traces to the list that produced it (DN-01).
#[test]
fn the_ranking_carries_the_baseline_it_came_from() {
    let (state, dir) = desktop("version", vec![asset(1, "the harbour", "high")], true);
    let assessor = asset_assessor(&state).expect("configured");
    assert_eq!(assessor.baseline_version(), state.config.version);
    let _ = std::fs::remove_dir_all(dir);
}

/// An unparsed priority never reaches the assessor, because validation refuses the
/// baseline first. Checked here so the assessor is not relied on to be defensive about
/// something the schema already guarantees.
#[test]
fn an_unknown_priority_is_refused_by_validation_not_by_the_assessor() {
    let baseline = ConfigBaseline {
        assets: vec![asset(1, "the harbour", "extremely")],
        ..ConfigBaseline::default()
    };
    assert!(
        gungnir_config::validate(&baseline).is_err(),
        "an unknown priority reached the assessor"
    );
}
