// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The MISB ST 0601 decoder against the vendored worked-example frame
//! (`testdata/misb/SOURCE.md`). The oracle values below are not computed by this
//! crate at all: they are what `paretech/klvdata`'s own Python (commit
//! `79028b4ab4ce7192d1b7c04d2266fc31ac337511`, read and *run* against this exact
//! file, per `testdata/misb/SOURCE.md`) reported for each tag, transcribed here by
//! hand. A field that disagrees is a bug in this crate's decoder, not a reading of
//! the fixture -- the same rule `asterix_fixtures.rs` and `ais_fixtures.rs` state for
//! their own oracles.
//!
//! This is the one place the checksum's own mismatch (documented at length in
//! `gungnir_interop::misb0601`'s module doc comment and in `testdata/misb/SOURCE.md`)
//! is asserted rather than merely described: the fixture's stated checksum does not
//! validate, and a passing test here means that is being detected, not papered over.

use std::path::PathBuf;

use gungnir_interop::misb0601::decode_frame;

fn fixture_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/misb/DynamicConstantMISMMSPacketData.bin");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_whole_frame_is_consumed_and_no_trailing_bytes_remain() {
    let bytes = fixture_bytes();
    let (_, consumed) = decode_frame(&bytes).expect("the fixture decodes");
    assert_eq!(consumed, bytes.len(), "228 bytes, one complete frame");
    assert_eq!(bytes.len(), 228);
}

/// Every tag this decoder interprets, checked against klvdata's own reading of these
/// exact bytes.
#[test]
fn every_interpreted_tag_matches_klvdatas_own_decode() {
    let bytes = fixture_bytes();
    let (frame, _) = decode_frame(&bytes).expect("the fixture decodes");

    // Tag 2: klvdata's PrecisionTimeStamp.value is `2009-01-12 22:08:22+00:00`,
    // which is 1_231_798_102 whole seconds since the epoch (no fraction -- the
    // microsecond field's low six digits are all zero in this worked example).
    assert_eq!(frame.precision_time_stamp_us, Some(1_231_798_102_000_000));

    assert_eq!(frame.mission_id.as_deref(), Some("Mission 12"));
    assert_eq!(frame.platform_designation.as_deref(), Some("Predator"));
    assert_eq!(frame.image_source_sensor.as_deref(), Some("EO Nose"));
    // No tag 4 (Platform Tail Number) in this worked example.
    assert_eq!(frame.platform_tail_number, None);

    assert!(close(
        frame.platform_heading_deg.expect("tag 5 present"),
        159.974_364_843_213_55
    ));
    assert!(close(
        frame.platform_pitch_deg.expect("tag 6 present"),
        -0.431_531_723_990_598_7
    ));
    assert!(close(
        frame.platform_roll_deg.expect("tag 7 present"),
        3.405_865_657_521_289_3
    ));

    assert!(close(
        frame.sensor_latitude_deg.expect("tag 13 present"),
        60.176_822_966_978_335
    ));
    assert!(close(
        frame.sensor_longitude_deg.expect("tag 14 present"),
        128.426_759_042_044_52
    ));
    assert!(close(
        frame.sensor_true_altitude_m.expect("tag 15 present"),
        14_190.719_462_882_427
    ));

    assert!(close(
        frame.sensor_relative_azimuth_deg.expect("tag 18 present"),
        160.719_211_436_975_57
    ));
    assert!(close(
        frame.sensor_relative_elevation_deg.expect("tag 19 present"),
        -168.792_324_833_940_85
    ));
    assert!(close(
        frame.sensor_relative_roll_deg.expect("tag 20 present"),
        176.865_437_649_391_94
    ));
    assert!(close(
        frame.slant_range_m.expect("tag 21 present"),
        68_590.983_298_744_77
    ));

    assert!(close(
        frame.frame_center_latitude_deg.expect("tag 23 present"),
        -10.542_388_633_146_132
    ));
    assert!(close(
        frame.frame_center_longitude_deg.expect("tag 24 present"),
        29.157_890_122_923_02
    ));
    assert!(close(
        frame.frame_center_elevation_m.expect("tag 25 present"),
        3_216.037_232_013_427_5
    ));

    assert_eq!(frame.uas_lds_version, Some(6));
}

/// Tags this decoder does not interpret -- 12 (Image Coordinate System, a string this
/// decoder does not carry), 16/17 (horizontal/vertical field of view), 22 (target
/// width), 48 (the nested ST 0102 Security Local Set) and 94 (unknown even to
/// klvdata itself, which reports it as its own `UnknownElement`) -- are carried raw
/// rather than silently dropped, each with klvdata's own reported byte length for a
/// cross-check that this decoder split the local set into the same items klvdata
/// did, not merely that it recognized a similar-looking subset.
#[test]
fn every_tag_this_decoder_does_not_interpret_is_carried_raw() {
    let bytes = fixture_bytes();
    let (frame, _) = decode_frame(&bytes).expect("the fixture decodes");
    let carried: std::collections::HashMap<u8, usize> = frame
        .carried_raw
        .iter()
        .map(|(tag, bytes)| (*tag, bytes.len()))
        .collect();
    assert_eq!(carried.get(&12), Some(&14), "Image Coordinate System");
    assert_eq!(
        carried.get(&16),
        Some(&2),
        "Sensor Horizontal Field of View"
    );
    assert_eq!(carried.get(&17), Some(&2), "Sensor Vertical Field of View");
    assert_eq!(carried.get(&22), Some(&2), "Target Width");
    assert_eq!(carried.get(&48), Some(&28), "nested Security Local Set");
    assert_eq!(carried.get(&94), Some(&34), "unknown even to klvdata");
    // Every carried tag is one this test names; an unnamed extra would mean this
    // decoder split the value into different items than klvdata did.
    assert_eq!(carried.len(), 6, "carried_raw: {carried:?}");
}

/// The finding `gungnir_interop::misb0601`'s module doc comment and
/// `testdata/misb/SOURCE.md` document at length: this specific worked example's own
/// stated checksum does not arithmetically close. Decoding still succeeds -- KLV
/// framing does not depend on the checksum -- and the mismatch is reported rather
/// than hidden or silently accepted.
#[test]
fn the_fixtures_own_checksum_mismatch_is_detected_not_hidden() {
    let bytes = fixture_bytes();
    let (frame, _) = decode_frame(&bytes).expect("the fixture decodes despite the checksum");
    assert_eq!(frame.stated_checksum, Some(0xAA43));
    assert_eq!(frame.computed_checksum, 0x3E1E);
    assert!(!frame.checksum_valid());
}
