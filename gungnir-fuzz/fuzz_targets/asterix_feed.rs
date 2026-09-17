// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fuzz target for the ASTERIX radar adapter (GAP-001), the gate named in
//! `docs/agentic-workflow.md` for parsers in `gungnir-ingest` adapters. Arbitrary bytes
//! are one datagram; the adapter must count or decode them and never panic. Seeded from
//! `corpus/asterix_feed/`, the public capture's datagrams and the hand-built Category 205
//! and 129 records (`testdata/asterix/SOURCE.md`); `gungnir-ingest/tests/asterix_seeds.rs`
//! keeps the seeds honest, over an adapter bound the same way as this one.
#![no_main]
use gungnir_ingest::adapters::asterix::{
    AsterixFeedAdapter, DfBinding, RadarBinding, ReplayDatagramSource, UasBinding,
};
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
    // The direction finder and the UAS gateway the hand-built seeds name (SAC 99 SIC 1 and
    // SAC 0 SIC 0, `testdata/asterix/SOURCE.md`), for the same reason: unbound, a Category
    // 205 or 129 block stops at the site lookup and its mapping is never fuzzed. The bearing
    // accuracy is a stand-in, since no real direction finder stands behind the seed.
    let df_sites = [DfBinding {
        sac: 99,
        sic: 1,
        sensor: SensorId(8),
        position: origin,
        azimuth_sigma_rad: 1.5_f64.to_radians(),
    }];
    let uas_sites = [UasBinding {
        sac: 0,
        sic: 0,
        sensor: SensorId(9),
    }];
    let mut adapter =
        AsterixFeedAdapter::new("fuzz", ReplayDatagramSource::default(), &frame, &bindings)
            .with_df_sites(&df_sites, &frame)
            .with_uas_sites(&uas_sites);
    let _ = adapter.handle_datagram(data, MissionTime(1_462_433_756.5));
    let _ = adapter.drain_service_reports();
    let _ = adapter.drain_uas_reports();
});
