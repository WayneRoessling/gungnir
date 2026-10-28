// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fuzz target for the SAPIENT detection adapter (GAP-213), the largest parser in
//! `gungnir-ingest`. Arbitrary bytes are a recorded session, one message per line; the
//! adapter must register, count or refuse each and never panic. Seeded from
//! `corpus/sapient_feed/`, cut from `testdata/sapient/` (`SOURCE.md` there);
//! `gungnir-ingest/tests/fuzz_seeds.rs` keeps the seeds honest over the same adapter.
#![no_main]
use gungnir_ingest::adapters::sapient::{
    RecordedSapientSource, SapientDetectionAdapter, SPOTTER_NODE_TYPE,
};
use gungnir_ingest::ProtocolAdapter;
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let frame = LocalFrame::new(Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    });
    let lines = String::from_utf8_lossy(data)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let source = RecordedSapientSource::from_lines(lines, "fuzz".into());
    let mut adapter = SapientDetectionAdapter::new(
        "fuzz",
        SensorId(1),
        frame,
        [0.0, 0.0, 0.0],
        source,
        SPOTTER_NODE_TYPE,
    );
    for _ in 0..2 {
        let _ = adapter.poll(MissionTime(1_700_000_000.0));
    }
});
