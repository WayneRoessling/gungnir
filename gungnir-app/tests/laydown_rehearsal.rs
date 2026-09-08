// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A rehearsal against a real, committed test-track fixture (GAP-045).
//!
//! TT-01's own `metadata.json` names it "no drone reaches OPS; at most two reach KAL
//! infrastructure" over 480 s and twelve entities -- a real scenario, not a synthetic
//! one built for this test, so a run against it is the same honest proof
//! `benches/app_tick.rs` already gives the generated case.
//!
//! **What this file does not test.** Whether a laydown's resource placement actually
//! reaches the configuration a run builds is checked in `laydown_rehearsal.rs`'s own
//! unit tests, fast and exactly reproducible, rather than here by comparing two heavy
//! end-to-end runs' track counts: track formation is independent of resource geometry
//! by construction, and separately is not exactly reproducible under heavy concurrent
//! system load on this project's own development hardware (confirmed directly -- five
//! runs of the identical input produced `[4, 5, 4, 4, 4]` tracks under `cargo test
//! --workspace`'s full parallelism and agreed perfectly under `--test-threads=1`).
//! That is a real property of `gungnir-fusion-async`'s pipeline under scheduling
//! variance, not a fault in this harness, and not something this file's own tests
//! should assert past or paper over.

use gungnir_app::laydown_rehearsal::run;
use gungnir_config::ResourceConfig;
use gungnir_model::laydown::{Laydown, LaydownId, ResourcePlacement};
use gungnir_model::{ResourceId, TestTrackNumber};

fn testdata_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata")
}

fn base_resources() -> Vec<ResourceConfig> {
    let text = serde_json::json!([
        {"id": 1, "position": [0.0, 0.0, 0.0], "capacity": 2, "layer": "point"}
    ])
    .to_string();
    serde_json::from_str(&text).expect("the fixture resource config parses")
}

fn laydown(resource_position_enu: [f64; 3]) -> Laydown {
    Laydown {
        id: LaydownId("rehearsed".into()),
        intent: "a candidate placement for the rehearsal test".into(),
        sensors: Vec::new(),
        resources: vec![ResourcePlacement {
            resource: ResourceId(1),
            position_enu: resource_position_enu,
        }],
        current: true,
    }
}

#[test]
fn a_rehearsal_against_a_real_fixture_forms_tracks_and_reports_the_queue_honestly() {
    let record = run(
        &testdata_root(),
        TestTrackNumber(1),
        &laydown([1000.0, 500.0, 0.0]),
        &base_resources(),
    )
    .expect("TT-01's fixture runs");

    assert_eq!(record.scenario, TestTrackNumber(1));
    assert_eq!(record.laydown, LaydownId("rehearsed".into()));
    assert!(
        record.tracks_formed > 0,
        "TT-01 declares twelve entities and 408 detections; a real run should form at \
         least one track"
    );
    // Never a claim beyond what the run actually measured: an expired decision is a
    // subset of raised ones, never more.
    assert!(record.decisions_expired <= record.decisions_raised);
}

#[test]
fn an_unknown_scenario_number_is_a_clear_error_not_a_panic() {
    let err = run(
        &testdata_root(),
        TestTrackNumber(99),
        &laydown([0.0, 0.0, 0.0]),
        &base_resources(),
    )
    .expect_err("TT-99 does not exist");
    assert!(err.to_string().contains("metadata.json"), "{err}");
}
