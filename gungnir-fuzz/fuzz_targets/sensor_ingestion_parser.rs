// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fuzz target for CI gate #5 (fuzz-nightly.yml), scheduled nightly per
//! gungnir-workspace-structure.md, not run per-PR. Feeds arbitrary bytes through
//! the recorded-feed line decoder and the gateway validation rules: neither may
//! panic, whatever the input.
#![no_main]
use gungnir_ingest::gateway::{decode_json_line, validate_detection};
use gungnir_model::MissionTime;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(detection) = decode_json_line(data) {
        let _ = validate_detection(&detection, MissionTime(0.0));
    }
});
