// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The `asterix_feed` fuzz corpus is real input, and stays decodable.
//!
//! Same reasoning as `fuzz_corpus.rs`: `gungnir-fuzz` is outside the workspace, so a
//! seed the adapter stopped accepting would rot unseen and the fuzzer would start from
//! the error path. The seeds are the public capture's 100 datagrams, its two standalone
//! blocks, and the hand-built Category 205 and Category 129 records
//! (`testdata/asterix/SOURCE.md`).
//!
//! **The adapter here binds what the fuzz target binds**
//! (`gungnir-fuzz/fuzz_targets/asterix_feed.rs`): the capture's seven radars, and the
//! direction finder and UAS gateway the two hand-built records name. Without those two
//! bindings a Category 205 or 129 block stops at the site lookup as an unknown radar, so
//! the fuzzer would never reach either category's mapping and this test would refuse
//! both seeds.

use gungnir_ingest::adapters::asterix::{
    AsterixFeedAdapter, DfBinding, RadarBinding, ReplayDatagramSource, UasBinding,
};
use gungnir_model::{Geodetic, LocalFrame, Measurement, MissionTime, SensorId};

fn corpus_dir() -> std::path::PathBuf {
    std::path::Path::new("..").join("gungnir-fuzz/corpus/asterix_feed")
}

fn origin() -> Geodetic {
    Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    }
}

/// `cat205.raw`'s I205/010 is SAC 99, SIC 1 (`testdata/asterix/SOURCE.md`).
fn direction_finder() -> DfBinding {
    DfBinding {
        sac: 99,
        sic: 1,
        sensor: SensorId(8),
        position: origin(),
        // A stand-in: the record is hand-built, so no direction finder's Interface Control
        // Document states an accuracy for it. The value only has to be the finite, positive
        // kind `gungnir-config` requires of a real one.
        azimuth_sigma_rad: 1.5_f64.to_radians(),
    }
}

/// `cat129.raw`'s I129/010 is SAC 0, SIC 0, the placeholder edition 1.2 recommends for an
/// airborne-to-ground broadcast (`testdata/asterix/SOURCE.md`).
const UAS_GATEWAY: UasBinding = UasBinding {
    sac: 0,
    sic: 0,
    sensor: SensorId(9),
};

fn adapter() -> AsterixFeedAdapter<ReplayDatagramSource> {
    let frame = LocalFrame::new(origin());
    let bindings: Vec<RadarBinding> = [11u8, 12, 13, 14, 201, 204, 205]
        .iter()
        .enumerate()
        .map(|(i, sic)| RadarBinding {
            sac: 25,
            sic: *sic,
            sensor: SensorId(u32::try_from(i).expect("few") + 1),
            position: origin(),
        })
        .collect();
    AsterixFeedAdapter::new("seeds", ReplayDatagramSource::default(), &frame, &bindings)
        .with_df_sites(&[direction_finder()], &frame)
        .with_uas_sites(&[UAS_GATEWAY])
}

/// Every seed frames, decodes, and maps: nothing malformed, nothing from an unknown
/// radar, and every seed produced at least one detection or service report.
#[test]
fn every_fuzz_seed_is_a_datagram_the_adapter_accepts() {
    let dir = corpus_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("the fuzz corpus is missing at {}", dir.display());
    };
    let mut adapter = adapter();
    let mut checked = 0;
    let mut rejected = Vec::new();
    for entry in entries.flatten() {
        let bytes = std::fs::read(entry.path()).expect("seed readable");
        let before = adapter.stats();
        let detections = adapter.handle_datagram(&bytes, MissionTime(1_462_433_756.5));
        let reports = adapter.drain_service_reports();
        // Drained so one seed's report is never counted as another's.
        adapter.drain_uas_reports();
        let after = adapter.stats();
        let trouble = (
            after.malformed_datagrams,
            after.malformed_blocks,
            after.unknown_radar,
            after.unsupported_category_blocks,
        ) != (
            before.malformed_datagrams,
            before.malformed_blocks,
            before.unknown_radar,
            before.unsupported_category_blocks,
        );
        if trouble || (detections.is_empty() && reports.is_empty()) {
            rejected.push(entry.file_name().to_string_lossy().into_owned());
        }
        checked += 1;
    }
    assert!(
        checked >= 100,
        "only {checked} seeds; the corpus is not being found or was not written"
    );
    assert!(
        rejected.is_empty(),
        "seeds the adapter rejects start the fuzzer on the error path: {rejected:?}"
    );
}

/// The two hand-built seeds are there, are the fixtures `testdata/asterix/SOURCE.md`
/// documents byte for byte, and each reaches its own category's mapping through the
/// binding above: the test before this one would pass with both seeds deleted.
#[test]
fn the_category_205_and_129_seeds_are_the_documented_fixtures_and_map_through_their_bindings() {
    let seed = |name: &str| {
        let path = corpus_dir().join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    };
    let cat205 = seed("cat205-raw");
    let cat129 = seed("cat129-raw");
    assert_eq!(
        cat205,
        include_bytes!("../../testdata/asterix/cat205.raw"),
        "the corpus seed has drifted from the documented Category 205 fixture"
    );
    assert_eq!(
        cat129,
        include_bytes!("../../testdata/asterix/cat129.raw"),
        "the corpus seed has drifted from the documented Category 129 fixture"
    );

    let mut adapter = adapter();
    let receipt = MissionTime(1_462_433_756.5);

    // SOURCE.md: one Sensor Data Report, local bearing 4500 counts at 0.01 degrees.
    let bearings = adapter.handle_datagram(&cat205, receipt);
    assert_eq!(bearings.len(), 1, "{bearings:#?}");
    assert_eq!(bearings[0].sensor, SensorId(8));
    match bearings[0].measurement {
        Measurement::Bearing { azimuth_rad, .. } => {
            assert!((azimuth_rad - 45.0_f64.to_radians()).abs() < 1e-12);
        }
        ref other => panic!("a direction finder reports a bearing, not {other:?}"),
    }

    // SOURCE.md: one record from the UAS gateway at SAC 0, SIC 0, registered in "US".
    let positions = adapter.handle_datagram(&cat129, receipt);
    assert_eq!(positions.len(), 1, "{positions:#?}");
    assert_eq!(positions[0].sensor, SensorId(9));
    assert!(matches!(
        positions[0].measurement,
        Measurement::Position { .. }
    ));
    let reports = adapter.drain_uas_reports();
    assert_eq!(reports.len(), 1, "{reports:#?}");
    assert_eq!(reports[0].registration_country, "US");

    let stats = adapter.stats();
    assert_eq!((stats.blocks_cat205, stats.blocks_cat129), (1, 1));
    assert_eq!((stats.detections, stats.uas_reports), (2, 1));
    assert_eq!(
        stats.malformed_datagrams
            + stats.malformed_blocks
            + stats.unknown_radar
            + stats.unsupported_category_blocks,
        0
    );
}
