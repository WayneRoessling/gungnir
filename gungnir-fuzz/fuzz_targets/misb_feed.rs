// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fuzz target for the MISB ST 0601 KLV adapter (GAP-213), whose corrupt-length stall was
//! found only in review (GAP-113's item). Arbitrary bytes are a recorded KLV stream; the
//! adapter must decode, skip or count them, never panic and never stop reading. Seeded
//! from `corpus/misb_feed/`: the vendored frame (`testdata/misb/SOURCE.md`), whose own
//! checksum does not validate and so is discarded, and the same frame with its checksum
//! corrected, so the fuzzer starts on the accept path as well as the discard path.
//! `gungnir-ingest/tests/fuzz_seeds.rs` keeps the seeds honest over the same adapter.
#![no_main]
use gungnir_ingest::adapters::misb::{RecordedKlvSource, UasMetadataAdapter};
use gungnir_ingest::ProtocolAdapter;
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let frame = LocalFrame::new(Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    });
    let source = RecordedKlvSource::from_bytes(data.iter().copied(), "fuzz".into());
    let mut adapter = UasMetadataAdapter::new("fuzz", SensorId(1), frame, source);
    for _ in 0..2 {
        let _ = adapter.poll(MissionTime(1_700_000_000.0));
    }
});
