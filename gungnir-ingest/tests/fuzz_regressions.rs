// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Every input a fuzz target has crashed on, replayed through the adapter the way that
//! target drives it (`gungnir-fuzz/fuzz_targets/<name>.rs`: the same construction and two
//! polls). `gungnir-fuzz` is outside the workspace and runs only nightly, so without this a
//! fixed crash could come back unseen until the next night found it again.
//!
//! The inputs are `gungnir-fuzz/regressions/<target>/`, each the artifact the nightly
//! uploaded, byte for byte. A replay passes by not panicking; what the adapter makes of
//! the input is not asserted, since a crash input is noise around the defect.
//!
//! The first two, from the nightly of 2026-10-29 on GAP-213's new targets:
//! - `misb_feed`: an eight-byte BER length near `usize::MAX` overflowed a frame's end in
//!   `gungnir_interop::misb0601::decode_frame`.
//! - `adsb_feed`: an AVR body even in bytes but odd in characters made no octet, and
//!   `gungnir_interop::adsb::parse_avr` read the first anyway.

use gungnir_ingest::adapters::adsb::{AdsbAdapter, RecordedAvrSource};
use gungnir_ingest::adapters::misb::{RecordedKlvSource, UasMetadataAdapter};
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

/// Every input in `target`'s regressions through `replay`, counting them so an emptied
/// directory fails rather than passes.
fn replay_each(target: &str, replay: impl Fn(&[u8])) {
    let dir = std::path::Path::new("..")
        .join("gungnir-fuzz/regressions")
        .join(target);
    let mut inputs = 0;
    for entry in std::fs::read_dir(&dir).expect("the regressions are there") {
        let bytes = std::fs::read(entry.expect("an entry").path()).expect("an input");
        replay(&bytes);
        inputs += 1;
    }
    assert!(inputs > 0, "{target} has no regression inputs");
}

#[test]
fn the_misb_crashes_replay_without_panicking() {
    replay_each("misb_feed", |bytes| {
        let source = RecordedKlvSource::from_bytes(bytes.iter().copied(), "replay".into());
        let mut adapter = UasMetadataAdapter::new("replay", SensorId(1), frame(0.9, 0.2), source);
        for _ in 0..2 {
            let _ = adapter.poll(NOW);
        }
    });
}

#[test]
fn the_adsb_crashes_replay_without_panicking() {
    replay_each("adsb_feed", |bytes| {
        let lines = String::from_utf8_lossy(bytes)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let source = RecordedAvrSource::from_lines(lines, "replay".into());
        let mut adapter = AdsbAdapter::new("replay", SensorId(1), frame(0.59, -2.06), source);
        for _ in 0..2 {
            let _ = adapter.poll(NOW);
        }
    });
}
