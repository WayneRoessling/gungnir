//! Fuzz target for the ASTERIX radar adapter (GAP-001), the gate named in
//! `docs/agentic-workflow.md` for parsers in `gungnir-ingest` adapters. Arbitrary bytes
//! are one datagram; the adapter must count or decode them and never panic. Seeded from
//! `corpus/asterix_feed/`, the public capture's datagrams
//! (`testdata/asterix/SOURCE.md`); `gungnir-ingest/tests/asterix_seeds.rs` keeps the
//! seeds honest.
#![no_main]
use gungnir_ingest::adapters::asterix::{AsterixFeedAdapter, RadarBinding, ReplayDatagramSource};
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let origin = Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 0.0,
    };
    let frame = LocalFrame::new(origin);
    // The capture's seven radars, so the mapping layer is fuzzed too, not just the parser.
    let bindings: Vec<RadarBinding> = [11u8, 12, 13, 14, 201, 204, 205]
        .iter()
        .enumerate()
        .map(|(i, sic)| RadarBinding {
            sac: 25,
            sic: *sic,
            sensor: SensorId(i as u32 + 1),
            position: origin,
        })
        .collect();
    let mut adapter = AsterixFeedAdapter::new("fuzz", ReplayDatagramSource::default(), &frame, &bindings);
    let _ = adapter.handle_datagram(data, MissionTime(1_462_433_756.5));
    let _ = adapter.drain_service_reports();
});
