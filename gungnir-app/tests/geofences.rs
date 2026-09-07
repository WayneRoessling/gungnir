//! Geofences reach the policy chain from the baseline (GAP-088).
//!
//! The geofence engine ran against an empty service on both binaries and could only
//! pass. With a fence declared, the chain's report counts it, the service holds it, and
//! the map can place it.

use gungnir_app::decisions;
use gungnir_app::geofences;
use gungnir_app::state::AppState;
use gungnir_config::{ConfigBaseline, GeofenceConfig};
use gungnir_geo::GeoService;

const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0];

fn desktop(name: &str, fences: Vec<GeofenceConfig>) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-geofences-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        geofences: fences,
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

#[test]
fn a_declared_fence_reaches_the_service_the_chain_reads_and_the_map() {
    let (state, dir) = desktop(
        "declared",
        vec![GeofenceConfig {
            name: "harbour approach".into(),
            center: ORIGIN,
            radius_m: 800.0,
            no_go: true,
        }],
    );
    let service = geofences::service_from_config(&state.config);
    assert_eq!(service.geofences().len(), 1);
    assert!(service.geofences()[0].no_go);
    assert!(
        !decisions::chain_report_for(&state.config).no_geofences_configured,
        "the caveat still says nothing is configured"
    );
    let placed = geofences::placed(&state);
    assert_eq!(placed.len(), 1);
    assert!(placed[0].center_enu[0].abs() < 1.0 && placed[0].center_enu[1].abs() < 1.0);
    let _ = std::fs::remove_dir_all(dir);
}

/// Nothing declared is still said, not assumed: the caveat stands and the service is
/// empty, which is the state every deployment was silently in before this gap.
#[test]
fn no_fences_keeps_the_caveat() {
    let (state, dir) = desktop("none", Vec::new());
    assert!(decisions::chain_report_for(&state.config).no_geofences_configured);
    assert!(geofences::service_from_config(&state.config)
        .geofences()
        .is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
