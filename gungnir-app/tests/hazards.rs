//! The static hazard layer reaches the picture (GAP-017, DN-14).
//!
//! `HazardLayer` existed with its tests and nothing built one. These are the tests behind
//! the claim that the desktop now does, and that it says what it cannot do.

use gungnir_app::hazards;
use gungnir_app::state::AppState;
use gungnir_config::{ConfigBaseline, HazardConfig, HazardShapeConfig};

const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0];

fn boom() -> HazardConfig {
    HazardConfig {
        name: "harbour boom".into(),
        kind: "boom".into(),
        shape: HazardShapeConfig::Polyline {
            // Roughly 300 m of boom running east from the origin.
            points: vec![ORIGIN, [ORIGIN[0], ORIGIN[1] + 0.000_08, 0.0]],
        },
        blocks_surface: true,
        height_m: Some(1.0),
    }
}

fn shoal() -> HazardConfig {
    HazardConfig {
        name: "the bar".into(),
        kind: "shoal".into(),
        shape: HazardShapeConfig::Circle {
            center: [ORIGIN[0] + 0.000_05, ORIGIN[1], 0.0],
            radius_m: 60.0,
        },
        blocks_surface: false,
        height_m: None,
    }
}

fn desktop(name: &str, origin: bool) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-hazards-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        revision: 4,
        origin: origin.then_some(ORIGIN),
        hazards: vec![boom(), shoal()],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

/// **The layer carries the baseline revision it came from** (DN-14 §5 as amended, the
/// verification row's third criterion): the honest limit of a static layer, stated. The
/// revision, not the schema version, which is the same number for every survey.
#[test]
fn the_layer_is_stamped_with_the_baseline_revision() {
    let (state, dir) = desktop("stamped", true);
    assert_eq!(state.hazards.baseline_version, 4);
    assert_ne!(state.hazards.baseline_version, state.config.version);
    assert_eq!(state.hazards.hazards.len(), 2);
    assert!(state.hazards.hazards[0].blocks_surface);
    assert!(!state.hazards.hazards[1].blocks_surface);
    let _ = std::fs::remove_dir_all(dir);
}

/// With an origin both hazards are placed in ENU: the boom as its two points, the shoal
/// as a closed ring of the declared radius.
#[test]
fn hazards_are_placed_in_the_local_frame() {
    let (state, dir) = desktop("placed", true);
    let placed = hazards::placed(&state);
    assert_eq!(placed.len(), 2);

    let boom = &placed[0];
    assert_eq!(boom.samples.len(), 2);
    assert!(
        boom.samples[0][0].abs() < 1.0,
        "the boom starts at the origin"
    );
    assert!(
        (boom.samples[1][0] - 300.0).abs() < 30.0,
        "the boom runs about 300 m east, not {} m",
        boom.samples[1][0]
    );

    let shoal = &placed[1];
    let [ce, cn, _] = gungnir_app::sustainment::local_frame(&state)
        .expect("origin")
        .to_enu(match &state.hazards.hazards[1].extent {
            gungnir_geo::HazardExtent::Circle { center, .. } => *center,
            gungnir_geo::HazardExtent::Polyline { .. } => unreachable!(),
        });
    assert_eq!(shoal.samples.first(), shoal.samples.last(), "a ring closes");
    for [e, n, _] in &shoal.samples {
        let r = ((e - ce).powi(2) + (n - cn).powi(2)).sqrt();
        assert!(
            (r - 60.0).abs() < 0.01,
            "a sample sits {r} m from the centre"
        );
    }

    let outlines = hazards::outlines(&placed);
    assert_eq!(outlines[0].kind, "boom");
    assert_eq!(outlines[1].kind, "shoal");
    assert!(outlines[0].blocks_surface && !outlines[1].blocks_surface);
    let _ = std::fs::remove_dir_all(dir);
}

/// **Without an origin nothing is placed, and the layer is still declared.** The two
/// counts PN-11 shows are how an operator tells "unplaceable" from "none": the hazards
/// are not missing, the map cannot put them anywhere (the same claim GAP-007 makes for
/// coverage).
#[test]
fn without_an_origin_hazards_are_declared_but_unplaceable() {
    let (state, dir) = desktop("no-origin", false);
    assert_eq!(state.hazards.hazards.len(), 2);
    assert!(hazards::placed(&state).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A baseline that reached the desktop without validation and names a kind the geometry
/// crate has no variant for is refused, not built short one hazard.
#[test]
fn an_unknown_kind_refuses_the_start_rather_than_dropping_the_hazard() {
    let dir = std::env::temp_dir().join(format!("gungnir-hazards-unknown-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut odd = boom();
    odd.kind = "minefield".into();
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        hazards: vec![odd],
        ..ConfigBaseline::default()
    };
    assert!(gungnir_config::validate(&config).is_err());
    assert!(AppState::with_config(config).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
