// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A configured `misb_feeds` entry is bound into the desktop's real gateway at start
//! and its platform position reaches the gateway through a real `update::tick`
//! (GAP-099), the same standard `gungnir-app/tests/cooperative_identity.rs` already
//! holds AIS's own wiring to and `gungnir-ingest/tests/misb_feed.rs` already holds the
//! adapter itself to.

use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, MisbFeedConfig, MisbSource, SensorConfig};

/// MISB ST 0601's UAS Datalink LS key (`gungnir_interop::misb0601::UDS_KEY`), copied
/// here because this binary has no dependency edge on `gungnir-interop`
/// (`ARCHITECTURE.md` draws none) and the key is not re-exported through
/// `gungnir_ingest::adapters::misb`.
const UDS_KEY: [u8; 16] = [
    0x06, 0x0E, 0x2B, 0x34, 0x02, 0x0B, 0x01, 0x01, 0x0E, 0x01, 0x03, 0x01, 0x01, 0x00, 0x00, 0x00,
];

/// MISB ST 0601.8-08's checksum algorithm (`gungnir_interop::misb0601::packet_checksum`),
/// duplicated for the same reason as [`UDS_KEY`] above: the lower 16 bits of the sum of
/// 16-bit big-endian words over `packet`, excluding `packet`'s own trailing two bytes.
fn packet_checksum(packet: &[u8]) -> u16 {
    let summed = &packet[..packet.len() - 2];
    let mut total: u32 = 0;
    let mut pos = 0usize;
    while pos + 2 <= summed.len() {
        total = total.wrapping_add(u32::from(u16::from_be_bytes([
            summed[pos],
            summed[pos + 1],
        ])));
        pos += 2;
    }
    if pos < summed.len() {
        total = total.wrapping_add(u32::from(summed[pos]) << 8);
    }
    u16::try_from(total & 0xFFFF).unwrap_or(0)
}

/// A well-formed KLV frame carrying a real platform position: the same Sensor
/// Latitude/Longitude raw bytes `gungnir-ingest/tests/misb_feed.rs` uses, copied
/// verbatim from the vendored fixture (`testdata/misb/SOURCE.md`) and already pinned
/// by `misb0601_fixtures.rs` as decoding to 60.176822966978335 / 128.42675904204452 --
/// reused rather than re-derived so this test needs no second, untested
/// implementation of MISB's "mapped" encoding to build its own fixture.
fn well_formed_frame() -> Vec<u8> {
    let items: [(u8, &[u8]); 2] = [
        (13, &[0x55, 0x95, 0xB6, 0x6D]),
        (14, &[0x5B, 0x53, 0x60, 0xC4]),
    ];
    let mut value = Vec::new();
    for (tag, bytes) in items {
        value.push(tag);
        value.push(u8::try_from(bytes.len()).expect("short-form"));
        value.extend_from_slice(bytes);
    }
    value.push(1);
    value.push(2);
    let checksum_at = UDS_KEY.len() + 1 + value.len();
    value.push(0);
    value.push(0);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&UDS_KEY);
    bytes.push(u8::try_from(value.len()).expect("short-form"));
    bytes.extend_from_slice(&value);
    let cs = packet_checksum(&bytes).to_be_bytes();
    bytes[checksum_at] = cs[0];
    bytes[checksum_at + 1] = cs[1];
    bytes
}

/// Near the fixture's own Sensor Latitude/Longitude (about 60.18N, 128.43E), so the
/// placed detection has a small, readable ENU magnitude rather than spanning a
/// hemisphere -- the same origin `gungnir-ingest/tests/misb_feed.rs` uses.
fn origin() -> [f64; 3] {
    [60.0_f64.to_radians(), 128.0_f64.to_radians(), 0.0]
}

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-misb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let recording = dir.join("frame.klv");
    std::fs::write(&recording, well_formed_frame()).expect("recording");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(origin()),
        sensors: vec![SensorConfig {
            id: 40,
            modality: "misb".into(),
            position: origin(),
            max_range_m: 50_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }],
        misb_feeds: vec![MisbFeedConfig {
            name: "uas-1".into(),
            sensor_id: 40,
            source: MisbSource::File {
                path: recording.to_string_lossy().into_owned(),
            },
        }],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("starts");
    (state, dir)
}

#[test]
fn a_configured_misb_feed_is_bound_and_its_position_reaches_the_gateway() {
    let (mut state, dir) = desktop("reachable");
    assert_eq!(state.misb_stats.len(), 1, "{:?}", state.alerts);
    assert_eq!(state.misb_stats[0].0, "uas-1");

    update::tick(&mut state);

    let gateway_stats = state.ingest.stats();
    assert_eq!(gateway_stats.accepted, 1, "{gateway_stats:?}");
    assert_eq!(gateway_stats.quarantined, 0, "{gateway_stats:?}");
    assert_eq!(gateway_stats.adapter_failures, 0, "{gateway_stats:?}");

    // The adapter's own counters, published through the stats sink this module
    // attaches (see `gungnir_app::misb`'s module doc comment).
    let feed_stats = state.misb_stats[0]
        .1
        .lock()
        .map(|s| *s)
        .expect("stats lock");
    assert_eq!(feed_stats.frames_decoded, 1);
    assert_eq!(feed_stats.positions_placed, 1);
    assert_eq!(feed_stats.checksum_mismatches, 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_misb_feed_with_no_local_frame_origin_binds_nothing_and_alerts() {
    let dir = std::env::temp_dir().join(format!("gungnir-misb-no-origin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let recording = dir.join("frame.klv");
    std::fs::write(&recording, well_formed_frame()).expect("recording");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: None,
        sensors: vec![SensorConfig {
            id: 42,
            modality: "misb".into(),
            position: [0.9, 0.2, 0.0],
            max_range_m: 50_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }],
        misb_feeds: vec![MisbFeedConfig {
            name: "uas-3".into(),
            sensor_id: 42,
            source: MisbSource::File {
                path: recording.to_string_lossy().into_owned(),
            },
        }],
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    assert!(
        state.misb_stats.is_empty(),
        "no local frame means no receiver is bound"
    );
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("MISB") && a.contains("local frame")),
        "{:?}",
        state.alerts
    );

    update::tick(&mut state);
    assert_eq!(state.ingest.stats().accepted, 0);

    let _ = std::fs::remove_dir_all(dir);
}
