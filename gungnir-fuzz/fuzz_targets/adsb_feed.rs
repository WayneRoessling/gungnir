// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fuzz target for the ADS-B adapter (GAP-213). Arbitrary bytes are a recorded AVR
//! session, one Mode S message per line; the adapter must decode, pair or count each and
//! never panic. Seeded from `corpus/adsb_feed/`, cut from `testdata/adsb/` (`SOURCE.md`
//! there); `gungnir-ingest/tests/fuzz_seeds.rs` keeps the seeds honest over the same adapter.
#![no_main]
use gungnir_ingest::adapters::adsb::{AdsbAdapter, RecordedAvrSource};
use gungnir_ingest::ProtocolAdapter;
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let frame = LocalFrame::new(Geodetic {
        lat_rad: 0.59,
        lon_rad: -2.06,
        alt_m: 0.0,
    });
    let lines = String::from_utf8_lossy(data)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let source = RecordedAvrSource::from_lines(lines, "fuzz".into());
    let mut adapter = AdsbAdapter::new("fuzz", SensorId(1), frame, source);
    for _ in 0..2 {
        let _ = adapter.poll(MissionTime(1_700_000_000.0));
    }
});
