// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The SAPIENT, MISB, ADS-B and AIS fuzz corpora are real input, and stay decodable
//! (GAP-213).
//!
//! Same reasoning as `asterix_seeds.rs`: `gungnir-fuzz` is outside the workspace, so a
//! seed the adapter stopped accepting would rot unseen and the fuzzer would start from the
//! error path. Each corpus here is cut from the matching directory under `testdata/`, and
//! **the adapter is built the way its fuzz target builds it**
//! (`gungnir-fuzz/fuzz_targets/<name>.rs`), so a seed that reaches a detection here reaches
//! one there.

use gungnir_ingest::adapters::adsb::{AdsbAdapter, RecordedAvrSource};
use gungnir_ingest::adapters::ais::{AisReceiverAdapter, RecordedNmeaSource};
use gungnir_ingest::adapters::misb::{RecordedKlvSource, UasMetadataAdapter};
use gungnir_ingest::adapters::sapient::{
    RecordedSapientSource, SapientDetectionAdapter, SPOTTER_NODE_TYPE,
};
use gungnir_ingest::ProtocolAdapter;
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};

const NOW: MissionTime = MissionTime(1_700_000_000.0);

fn frame(lat_rad: f64, lon_rad: f64) -> LocalFrame {
    LocalFrame::new(Geodetic {
        lat_rad,
        lon_rad,
        alt_m: 0.0,
    })
}

fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Every seed in `target`'s corpus through `run`, which polls an adapter built as the fuzz
/// target builds it and answers with its detections: each seed polls without error, and
/// the corpus as a whole reaches the decoder's success path.
fn seeds_reach_detections(
    target: &str,
    run: impl Fn(&[u8]) -> Result<usize, gungnir_ingest::IngestError>,
) {
    let dir = std::path::Path::new("..")
        .join("gungnir-fuzz/corpus")
        .join(target);
    let mut seeds = 0;
    let mut detections = 0;
    for entry in std::fs::read_dir(&dir).expect("the corpus is there") {
        let path = entry.expect("an entry").path();
        let bytes = std::fs::read(&path).expect("a seed");
        detections +=
            run(&bytes).unwrap_or_else(|e| panic!("{} did not poll: {e}", path.display()));
        seeds += 1;
    }
    assert!(seeds > 0, "{target} has no seeds");
    assert!(
        detections > 0,
        "no seed in {target} reaches a detection, so the fuzzer starts on the error path"
    );
}

#[test]
fn the_sapient_seeds_reach_detections() {
    seeds_reach_detections("sapient_feed", |bytes| {
        let source = RecordedSapientSource::from_lines(lines(bytes), "seed".into());
        let mut adapter = SapientDetectionAdapter::new(
            "seed",
            SensorId(1),
            frame(0.9, 0.2),
            [0.0, 0.0, 0.0],
            source,
            SPOTTER_NODE_TYPE,
        );
        adapter.poll(NOW).map(|d| d.len())
    });
}

#[test]
fn the_misb_seeds_reach_detections() {
    seeds_reach_detections("misb_feed", |bytes| {
        let source = RecordedKlvSource::from_bytes(bytes.iter().copied(), "seed".into());
        let mut adapter = UasMetadataAdapter::new("seed", SensorId(1), frame(0.9, 0.2), source);
        adapter.poll(NOW).map(|d| d.len())
    });
}

#[test]
fn the_adsb_seeds_reach_detections() {
    seeds_reach_detections("adsb_feed", |bytes| {
        let source = RecordedAvrSource::from_lines(lines(bytes), "seed".into());
        let mut adapter = AdsbAdapter::new("seed", SensorId(1), frame(0.59, -2.06), source);
        adapter.poll(NOW).map(|d| d.len())
    });
}

#[test]
fn the_ais_seeds_reach_detections() {
    seeds_reach_detections("ais_feed", |bytes| {
        let source = RecordedNmeaSource::from_lines(lines(bytes), "seed".into());
        let mut adapter = AisReceiverAdapter::new("seed", SensorId(1), frame(0.9, 0.2), source);
        adapter.poll(NOW).map(|d| d.len())
    });
}
