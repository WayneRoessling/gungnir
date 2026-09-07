// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The `asterix_feed` fuzz corpus is real input, and stays decodable.
//!
//! Same reasoning as `fuzz_corpus.rs`: `gungnir-fuzz` is outside the workspace, so a
//! seed the adapter stopped accepting would rot unseen and the fuzzer would start from
//! the error path. The seeds are the public capture's 100 datagrams and the two
//! standalone blocks (`testdata/asterix/SOURCE.md`).

use gungnir_ingest::adapters::asterix::{AsterixFeedAdapter, RadarBinding, ReplayDatagramSource};
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};

fn corpus_dir() -> std::path::PathBuf {
    std::path::Path::new("..").join("gungnir-fuzz/corpus/asterix_feed")
}

fn adapter() -> AsterixFeedAdapter<ReplayDatagramSource> {
    let origin = Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    };
    let bindings: Vec<RadarBinding> = [11u8, 12, 13, 14, 201, 204, 205]
        .iter()
        .enumerate()
        .map(|(i, sic)| RadarBinding {
            sac: 25,
            sic: *sic,
            sensor: SensorId(u32::try_from(i).expect("few") + 1),
            position: origin,
        })
        .collect();
    AsterixFeedAdapter::new(
        "seeds",
        ReplayDatagramSource::default(),
        &LocalFrame::new(origin),
        &bindings,
    )
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
