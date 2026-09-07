// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The 1090 MHz pulse-position demodulator: 8-bit unsigned IQ at 2 MS/s in, Mode S
//! frames out.
//!
//! **Why this is here.** GAP-010's closing action asks for two independent recordings
//! rather than one, and the second of them — `antirez/dump1090`'s BSD-3-Clause
//! `testfiles/modes1.bin` — is at the radio layer, not the message layer. Without a
//! demodulator that capture would sit in `testdata/adsb/` decoded by nothing, and the
//! "two independent recordings" the register asks for would be one.
//!
//! **What it is not.** It is not a receiver. There is no sample source, no gain control,
//! no timing recovery beyond the two-sample-per-bit alignment the pulse position gives,
//! and no error correction: a frame whose parity does not clear is counted and dropped,
//! never repaired. dump1090 corrects single-bit errors and this deliberately does not,
//! because a corrected frame is a guess and a fixture built on guesses proves nothing.
//!
//! **What it can and cannot check.** Mode S puts the parity of a surveillance reply in
//! the same 24 bits as the aircraft address (the address/parity field), so a DF 0, 4, 5,
//! 16, 20 or 21 frame can only be validated against a list of addresses already known to
//! be in the air. This demodulator has no such list, so it keeps only the frames whose
//! parity stands on its own — the extended squitters (DF 17 and 18) and the all-call
//! replies with interrogator identifier zero (DF 11) — and **counts** the rest as
//! candidates it could not check ([`DemodStats::unverifiable_candidates`]) rather than
//! passing them on unvalidated.

use super::{parity, LONG_OCTETS};

/// Samples per bit at 2 MS/s: a Mode S bit is 1 µs and each half carries a pulse.
const SAMPLES_PER_BIT: usize = 2;
/// The preamble is 8 µs, so sixteen samples before the first data bit.
const PREAMBLE_SAMPLES: usize = 16;
/// Below this magnitude difference the two halves of a bit are too close to call, and
/// the bit is taken from its predecessor. dump1090's constant on the same magnitude
/// scale; a frame that needs it usually fails the parity check anyway.
const UNDECIDED_DELTA: i32 = 256;
/// The magnitude scale: `hypot` of the two signed 8-bit components times this, which
/// keeps the whole range inside a `u16`.
const MAGNITUDE_SCALE: f64 = 360.0;

/// One frame the demodulator recovered, and where in the capture it began.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemodulatedFrame {
    /// Index of the first preamble sample, so a frame can be located in the capture.
    pub sample: usize,
    /// Seven or fourteen octets, parity included.
    pub octets: Vec<u8>,
}

/// What the demodulator saw, so a capture that yields nothing says why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DemodStats {
    /// IQ sample pairs read.
    pub samples: usize,
    /// Positions whose magnitudes matched the preamble pattern.
    pub preambles: usize,
    /// Frames whose parity cleared without an address: kept.
    pub frames: usize,
    /// Frames of a downlink format whose parity is mixed with the aircraft address, so
    /// it cannot be checked without a list of addresses already known to be in the air.
    /// Counted, never returned.
    pub unverifiable_candidates: usize,
    /// Preambles whose bits did not produce a frame with clean parity: noise, a
    /// collision, or a real frame this build declines to repair.
    pub parity_failures: usize,
}

/// The magnitude of each IQ pair, on the scale [`MAGNITUDE_SCALE`] sets.
///
/// A 256 by 256 table rather than a `hypot` per sample: a 0.18 s capture is 356 868
/// samples and the table is computed once.
fn magnitudes(raw: &[u8]) -> Vec<u16> {
    let mut table = vec![0u16; 256 * 256];
    for i in 0..256usize {
        for q in 0..256usize {
            #[allow(clippy::cast_precision_loss)]
            let (di, dq) = (i as f64 - 127.5, q as f64 - 127.5);
            let scaled = di.hypot(dq) * MAGNITUDE_SCALE;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let value = scaled.round().min(f64::from(u16::MAX)) as u16;
            table[i * 256 + q] = value;
        }
    }
    let (pairs, _odd_trailing_byte) = raw.as_chunks::<2>();
    pairs
        .iter()
        .map(|pair| table[usize::from(pair[0]) * 256 + usize::from(pair[1])])
        .collect()
}

/// The Mode S preamble: pulses in bit halves 0, 2, 7 and 9 of the eight microseconds,
/// and quiet in between.
fn looks_like_a_preamble(m: &[u16], at: usize) -> bool {
    let Some(w) = m.get(at..at + PREAMBLE_SAMPLES) else {
        return false;
    };
    // The four pulses, and the quiet either side of each.
    if !(w[0] > w[1]
        && w[1] < w[2]
        && w[2] > w[3]
        && w[3] < w[0]
        && w[4] < w[0]
        && w[5] < w[0]
        && w[6] < w[0]
        && w[7] > w[8]
        && w[8] < w[9]
        && w[9] > w[6])
    {
        return false;
    }
    // The mean of the four pulses, less a third: samples 4, 5 and 11 to 14 must be
    // below it or this is not a preamble but a run of noise with the right shape.
    let high = (u32::from(w[0]) + u32::from(w[2]) + u32::from(w[7]) + u32::from(w[9])) / 6;
    let quiet = |i: usize| u32::from(w[i]) < high;
    quiet(4) && quiet(5) && quiet(11) && quiet(12) && quiet(13) && quiet(14)
}

/// Demodulate a whole capture.
///
/// Returns the frames whose parity cleared, in the order they appear in the capture,
/// with the statistics that say what else was there.
#[must_use]
pub fn frames_from_iq(raw: &[u8]) -> (Vec<DemodulatedFrame>, DemodStats) {
    let m = magnitudes(raw);
    let mut stats = DemodStats {
        samples: m.len(),
        ..DemodStats::default()
    };
    let mut frames = Vec::new();
    let window = PREAMBLE_SAMPLES + LONG_OCTETS * 8 * SAMPLES_PER_BIT;
    let mut at = 0usize;
    while at + window <= m.len() {
        if !looks_like_a_preamble(&m, at) {
            at += 1;
            continue;
        }
        stats.preambles += 1;
        let octets = demodulate_bits(&m, at + PREAMBLE_SAMPLES);
        let format = octets[0] >> 3;
        let length = super::octets_for(format);
        let frame = &octets[..length];
        if parity(frame) == 0 && frame.iter().any(|&b| b != 0) {
            stats.frames += 1;
            frames.push(DemodulatedFrame {
                sample: at,
                octets: frame.to_vec(),
            });
            // Step past the frame: a preamble cannot start inside one.
            at += PREAMBLE_SAMPLES + length * 8 * SAMPLES_PER_BIT;
            continue;
        }
        if super::parity_carries_the_address(format) {
            stats.unverifiable_candidates += 1;
        } else {
            stats.parity_failures += 1;
        }
        at += 1;
    }
    (frames, stats)
}

/// The 112 data bits after a preamble, packed most significant bit first.
///
/// Each bit is two samples: the first louder than the second is a one, the other way
/// round is a zero, and a pair too close to call takes the previous bit's value, which
/// is what makes the parity check rather than this function the arbiter.
fn demodulate_bits(m: &[u16], start: usize) -> [u8; LONG_OCTETS] {
    let mut octets = [0u8; LONG_OCTETS];
    let mut previous = false;
    for bit in 0..LONG_OCTETS * 8 {
        let first = i32::from(m.get(start + bit * 2).copied().unwrap_or(0));
        let second = i32::from(m.get(start + bit * 2 + 1).copied().unwrap_or(0));
        let delta = first - second;
        let one = if delta.abs() < UNDECIDED_DELTA {
            previous
        } else {
            delta > 0
        };
        previous = one;
        if one {
            octets[bit / 8] |= 0x80 >> (bit % 8);
        }
    }
    octets
}

/// Frames as AVR text, the form `testdata/adsb/lax-messages-first40000.txt` is in, so a
/// demodulated capture and a recorded one can be fed to the same reader.
#[must_use]
pub fn to_avr(frame: &DemodulatedFrame) -> String {
    use std::fmt::Write as _;
    let mut line = String::with_capacity(2 + frame.octets.len() * 2);
    line.push('*');
    for octet in &frame.octets {
        // Writing into a `String` cannot fail, and the result is discarded rather than
        // unwrapped so this stays inside the no-`unwrap` rule.
        let _ = write!(line, "{octet:02X}");
    }
    line.push(';');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_of_silence_yields_nothing_and_says_so() {
        let quiet = vec![127u8; 4096];
        let (frames, stats) = frames_from_iq(&quiet);
        assert!(frames.is_empty());
        assert_eq!(stats.samples, 2048);
        assert_eq!(stats.frames, 0);
    }

    #[test]
    fn an_empty_capture_is_not_an_error() {
        let (frames, stats) = frames_from_iq(&[]);
        assert!(frames.is_empty());
        assert_eq!(stats, DemodStats::default());
    }

    #[test]
    fn avr_text_round_trips_through_the_frame_reader() {
        let frame = DemodulatedFrame {
            sample: 0,
            octets: vec![
                0x8D, 0x48, 0x40, 0xD6, 0x20, 0x2C, 0xC3, 0x71, 0xC3, 0x2C, 0xE0, 0x57, 0x60, 0x98,
            ],
        };
        let line = to_avr(&frame);
        assert_eq!(line, "*8D4840D6202CC371C32CE0576098;");
        assert_eq!(
            super::super::parse_avr(&line).expect("parses"),
            frame.octets
        );
    }
}
