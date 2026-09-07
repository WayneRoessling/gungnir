// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The Mode S parity, checked by arithmetic (GAP-010, closing action point 3).
//!
//! **This file names no oracle on purpose.** The rest of the ADS-B gate is agreement
//! with `rs1090` and `adsb_deku`, which is agreement with the open-source consensus and
//! not conformance (`docs/design/external-standards.md` §4). The parity is a polynomial
//! over GF(2), so it has properties that are true or false independently of what any
//! decoder believes, and those are what this file checks: the generator's own identity,
//! linearity, the error patterns the polynomial is guaranteed to detect, and the one
//! pattern of the guaranteed length that it provably cannot. A misreading shared by
//! every open decoder would not survive here.
//!
//! The corpora are `testdata/adsb/` (`SOURCE.md` for their provenance and licences):
//! `lax-messages-first40000.txt`, real off-air AVR frames from Los Angeles, and
//! `modes1.bin`, an independent recording at the radio layer.

use std::path::PathBuf;

use gungnir_interop::adsb::{
    demod, expected_parity, parity, parse_avr, GENERATOR, LONG_OCTETS, SHORT_OCTETS,
};

/// The frame the ADS-B literature uses as its worked example.
const WORKED_EXAMPLE: &str = "*8D4840D6202CC371C32CE0576098;";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/adsb")
        .join(name)
}

fn avr_frames() -> Vec<Vec<u8>> {
    let text = std::fs::read_to_string(fixture("lax-messages-first40000.txt"))
        .expect("the vendored AVR corpus is present");
    text.lines()
        .filter_map(|line| parse_avr(line).ok())
        .collect()
}

/// The parity field a transmitter appends is the remainder of the message under the
/// generator, so recomputing it over the message alone reproduces the three octets on
/// the wire. One identity, one frame, no oracle.
#[test]
fn the_parity_field_of_the_worked_example_is_the_remainder_of_its_message() {
    let frame = parse_avr(WORKED_EXAMPLE).expect("parses");
    assert_eq!(frame.len(), LONG_OCTETS);
    let field = u32::from_be_bytes([0, frame[11], frame[12], frame[13]]);
    assert_eq!(field, 0x0057_6098);
    assert_eq!(expected_parity(&frame[..11]), field);
    assert_eq!(parity(&frame), 0);
}

/// A CRC with a zero initial value is a linear map over GF(2). If this fails, the
/// implementation is not the polynomial division it claims to be, whatever answers it
/// happens to agree with.
#[test]
fn the_parity_is_linear_over_the_frames_it_is_taken_of() {
    let frames = avr_frames();
    let long: Vec<&Vec<u8>> = frames.iter().filter(|f| f.len() == LONG_OCTETS).collect();
    assert!(long.len() > 1000, "the corpus holds long frames");
    let mut checked = 0;
    let (pairs, _odd_trailing_frame) = long.as_chunks::<2>();
    for pair in pairs.iter().take(2000) {
        let xor: Vec<u8> = pair[0]
            .iter()
            .zip(pair[1].iter())
            .map(|(a, b)| a ^ b)
            .collect();
        assert_eq!(
            parity(&xor),
            parity(pair[0]) ^ parity(pair[1]),
            "parity is not linear on a pair of frames"
        );
        checked += 1;
    }
    assert!(checked >= 1000, "only {checked} pairs were checked");
}

/// Every extended squitter in the vendored corpus carries its own parity, so the
/// residual over the whole frame is zero. Thirteen thousand real transmitters agreeing
/// with this implementation of the polynomial is not a consensus of decoders; it is the
/// transmitters themselves.
#[test]
fn every_extended_squitter_in_the_corpus_clears_its_parity() {
    let frames = avr_frames();
    let squitters: Vec<&Vec<u8>> = frames
        .iter()
        .filter(|f| matches!(f[0] >> 3, 17 | 18) && f.len() == LONG_OCTETS)
        .collect();
    assert_eq!(
        squitters.len(),
        13_324,
        "the vendored prefix holds a fixed number of extended squitters; \
         a different count means the fixture changed"
    );
    let dirty = squitters.iter().filter(|f| parity(f) != 0).count();
    assert_eq!(dirty, 0, "{dirty} extended squitters do not clear");
}

/// The all-call replies (DF 11) mix the interrogator identifier into the parity, so a
/// clean residual means "interrogator identifier zero" rather than "corrupt". The
/// corpus is unfiltered receiver output and holds noise at every downlink format, so
/// this asserts that a substantial number clear rather than that all do — the honest
/// statement about a corpus nobody filtered.
#[test]
fn the_all_call_replies_that_clear_are_the_ones_with_interrogator_identifier_zero() {
    let frames = avr_frames();
    let all_calls: Vec<&Vec<u8>> = frames
        .iter()
        .filter(|f| f[0] >> 3 == 11 && f.len() == SHORT_OCTETS)
        .collect();
    let clean = all_calls.iter().filter(|f| parity(f) == 0).count();
    assert!(
        clean >= 4000,
        "only {clean} of {} all-call replies cleared",
        all_calls.len()
    );
}

/// The polynomial has degree 24, so it detects every single-bit error. Checked
/// exhaustively over every bit position of a hundred real frames rather than sampled.
#[test]
fn every_single_bit_error_is_detected() {
    let frames = avr_frames();
    let squitters: Vec<&Vec<u8>> = frames
        .iter()
        .filter(|f| matches!(f[0] >> 3, 17 | 18) && f.len() == LONG_OCTETS)
        .take(100)
        .collect();
    assert_eq!(squitters.len(), 100);
    let mut checked = 0usize;
    for frame in squitters {
        for bit in 0..LONG_OCTETS * 8 {
            let mut broken = frame.clone();
            broken[bit / 8] ^= 0x80 >> (bit % 8);
            assert_ne!(parity(&broken), 0, "bit {bit} flip went undetected");
            checked += 1;
        }
    }
    assert_eq!(checked, 100 * 112);
}

/// A CRC of degree 24 detects every burst error of 24 bits or fewer. Checked
/// exhaustively: every start position and every burst length from 1 to 24, with the
/// burst set to all ones, over ten real frames.
#[test]
fn every_burst_error_of_twenty_four_bits_or_fewer_is_detected() {
    let frames = avr_frames();
    let squitters: Vec<&Vec<u8>> = frames
        .iter()
        .filter(|f| matches!(f[0] >> 3, 17 | 18) && f.len() == LONG_OCTETS)
        .take(10)
        .collect();
    let bits = LONG_OCTETS * 8;
    let mut checked = 0usize;
    for frame in squitters {
        for start in 0..bits {
            for length in 1..=24usize {
                if start + length > bits {
                    continue;
                }
                let mut broken = frame.clone();
                for bit in start..start + length {
                    broken[bit / 8] ^= 0x80 >> (bit % 8);
                }
                assert_ne!(
                    parity(&broken),
                    0,
                    "a burst of {length} bits at {start} went undetected"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 20_000, "only {checked} bursts were checked");
}

/// And the boundary, stated rather than left implied: a burst of **25** bits is not
/// guaranteed to be detected, and exactly one such pattern per position is missed — the
/// generator polynomial itself. Constructing it and watching the parity still clear is
/// the proof that this implementation is that polynomial and not one that happens to
/// agree on the easy cases.
#[test]
fn the_one_undetectable_twenty_five_bit_burst_is_the_generator_itself() {
    let frame = parse_avr(WORKED_EXAMPLE).expect("parses");
    // The generator with its leading term restored: x^24 plus the tail the constant
    // holds, which is 25 bits wide and is the only 25-bit burst the division cannot see.
    let pattern = GENERATOR | (1 << 24);
    let mut undetected = 0usize;
    for start in 0..=(LONG_OCTETS * 8 - 25) {
        let mut broken = frame.clone();
        for i in 0..25 {
            if pattern & (1 << (24 - i)) != 0 {
                let bit = start + i;
                broken[bit / 8] ^= 0x80 >> (bit % 8);
            }
        }
        if parity(&broken) == 0 {
            undetected += 1;
        }
    }
    assert_eq!(
        undetected,
        LONG_OCTETS * 8 - 24,
        "the generator pattern must be undetectable at every position it fits"
    );
}

/// The second recording, and the only one at the radio layer: every frame the
/// demodulator recovers from `modes1.bin` clears its parity, because clearing it is the
/// rule by which the demodulator keeps a frame at all. What this test adds is the
/// count — 198 frames, 141 of them extended squitters — so a change in the
/// demodulator or the capture cannot pass unnoticed.
#[test]
fn the_iq_capture_demodulates_to_frames_that_clear_their_parity() {
    let raw = std::fs::read(fixture("modes1.bin")).expect("the vendored IQ capture is present");
    assert_eq!(
        raw.len(),
        713_736,
        "the capture is 8-bit IQ pairs at 2 MS/s"
    );
    let (frames, stats) = demod::frames_from_iq(&raw);
    assert_eq!(stats.samples, 356_868);
    assert_eq!(frames.len(), 198, "frames recovered from the IQ capture");
    assert_eq!(stats.frames, 198);
    for frame in &frames {
        assert_eq!(parity(&frame.octets), 0, "a kept frame must clear");
    }
    let squitters = frames
        .iter()
        .filter(|f| matches!(f.octets[0] >> 3, 17 | 18))
        .count();
    assert_eq!(squitters, 141);
    // The rest are all-call replies with interrogator identifier zero.
    assert_eq!(frames.len() - squitters, 57);
    assert!(
        stats.unverifiable_candidates > 0,
        "the capture holds surveillance replies whose parity carries the address; \
         they must be counted, not silently dropped"
    );
}
