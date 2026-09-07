// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! ADS-B: Mode S 1090 MHz extended squitter (GAP-010, D-24, D-32).
//!
//! # No normative source is pinned, and that is a decision, not an omission
//!
//! Every other codec in this crate names the edition it was built to. This one cannot.
//! `docs/design/external-standards.md` §4 records the search: **no ADS-B specification is
//! both free to obtain and permissively licensed.** ICAO Doc 9871 is USD 403 and DRM
//! locked, Annex 10 Volume IV delegates the message formats to it, RTCA DO-260B and
//! EUROCAE ED-102B are paid, and the DO-260B *draft* that circulates freely as RTCA
//! working paper 1090-WP30-18 carries RTCA copyright and is a draft, so it is neither
//! licensed nor pinnable and is **refused by name**. *The 1090 Megahertz Riddle* is
//! CC BY-NC-SA and unusable on both clauses.
//!
//! So the owner took the other route GAP-010's closing action offers: build the decoder
//! and gate it against two independently written MIT decoders, `rs1090` and `adsb_deku`,
//! over two independently recorded permissively licensed captures
//! (`testdata/adsb/SOURCE.md`). **What that gates is agreement with the open-source
//! consensus, not conformance**, and the verification-capability-table row says exactly
//! that in its oracle column. The residual risk is specific and real: `adsb_deku` cites
//! the same ICAO section numbering and `rs1090` takes inspiration from `pyModeS`, so a
//! misreading shared by both would pass this gate in silence.
//!
//! Two things narrow that risk, and they are the reason this module is split the way it
//! is. The **parity** ([`parity`]) is a polynomial and the **position** ([`cpr`]) is a
//! published algorithm with worked examples; both are checkable by arithmetic, and both
//! are gated that way in `tests/adsb_crc.rs` and `tests/adsb_cpr.rs` rather than by
//! consensus. They are where a shared misreading would be caught by mathematics instead
//! of slipping past three decoders that learned from each other.
//!
//! # What this is and is not
//!
//! A decoder from a frame — AVR text, or IQ samples through [`demod`] — to typed
//! messages. It is not an evidence source: turning a position report into a
//! cooperative-identity claim for `gungnir-identification` needs an adapter that owns
//! the receiver and the local frame, which is the same remaining half GAP-010's AIS
//! work has. Nothing here knows a `SensorId`, and nothing here decides that a track and
//! an ICAO address are the same object.
//!
//! Nothing is dropped in silence. A downlink format this build does not decode becomes
//! [`Downlink::Other`] with its name; a type code it does not interpret becomes
//! [`messages::MeMessage::Carried`] with its octets; and [`DecodeStats`] counts both, so
//! a feed's operator can see what was read and what was not. That is the pattern
//! `asterix::cat048` set with `Record::carried_raw`.

pub mod cpr;
pub mod demod;
pub mod messages;

use std::collections::BTreeMap;

pub use cpr::{CprError, CprFrame, CprKind, Position};
pub use messages::{
    AirbornePosition, AirborneVelocity, AltitudeSource, IcaoAddress, Identification, MeMessage,
    SurfacePosition, SurveillanceStatus, VelocityKind, VerticalRateSource,
};

/// The codec's name in the schema catalogue.
pub const CODEC_NAME: &str = "adsb.1090es";

/// What this decoder was built against, said in full because a codec without a stated
/// source is the "confidently wrong" outcome GAP-064's rule exists to prevent.
pub const SPECIFICATION: &str =
    "none pinned: gated against the open-source consensus of rs1090 0.6.0 and adsb_deku \
     0.7.1 over testdata/adsb/, not against a normative document \
     (docs/design/external-standards.md §4)";

/// A short Mode S frame: 56 bits.
pub const SHORT_OCTETS: usize = 7;
/// A long Mode S frame: 112 bits.
pub const LONG_OCTETS: usize = 14;

/// The Mode S parity polynomial, less its leading term:
/// x^24 + x^23 + x^22 + x^21 + x^20 + x^19 + x^18 + x^17 + x^16 + x^15 + x^14 + x^13 +
/// x^12 + x^10 + x^3 + 1.
pub const GENERATOR: u32 = 0x01FF_F409;

/// Why a frame or a line could not be read. Every arm names the frame's own fault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdsbError {
    /// The line is not an AVR frame at all (a receiver's log mixes them with status
    /// lines, and a caller skips those by this arm).
    #[error("not an AVR frame")]
    NotAvrFrame,
    #[error("malformed frame: {0}")]
    Malformed(&'static str),
    #[error("character {found:?} at position {at} is not a hexadecimal digit")]
    Hex { at: usize, found: char },
    /// The octet count does not match what the downlink format declares.
    #[error("downlink format {downlink_format} is {expected} octets, not {found}")]
    Length {
        downlink_format: u8,
        expected: usize,
        found: usize,
    },
    /// An extended squitter whose parity does not clear. The frame is refused rather
    /// than decoded: an extended squitter carries its parity on its own, so a non-zero
    /// residual is a corrupt frame and every field in it is suspect.
    #[error("downlink format {downlink_format}: parity residual {residual:#08x}, expected 0")]
    Parity { downlink_format: u8, residual: u32 },
    /// A type code outside 0 to 31, which a five-bit field cannot hold. Here so no
    /// branch is unreachable rather than because it can happen.
    #[error("type code {type_code} is outside 0 to 31")]
    TypeCode { type_code: u8 },
}

/// The Mode S parity residual over a whole frame, parity field included.
///
/// Zero for an intact extended squitter and for an all-call reply whose interrogator
/// identifier is zero; the aircraft's address for the surveillance and Comm-B formats,
/// which mix parity with address (see [`parity_carries_the_address`]).
#[must_use]
pub fn parity(frame: &[u8]) -> u32 {
    let mut remainder = 0u32;
    for octet in frame {
        remainder ^= u32::from(*octet) << 16;
        for _ in 0..8 {
            remainder <<= 1;
            if remainder & 0x0100_0000 != 0 {
                remainder ^= GENERATOR;
            }
        }
    }
    remainder & 0x00FF_FFFF
}

/// The parity a transmitter would append to `message`, which must be the frame without
/// its last three octets.
///
/// This is [`parity`] of the message alone, and the reason needs saying because it looks
/// like an omission. The register in [`parity`] is loaded a whole octet at a time at the
/// **top** of the twenty-four bits, so the twenty-four zero bits a textbook long division
/// would append are already accounted for by the time the last octet is shifted through.
/// Padding with three zero octets here would divide by `x^24` once too often and give the
/// wrong answer; the test in `tests/adsb_crc.rs` pins the identity against a worked
/// example so this cannot drift back.
#[must_use]
pub fn expected_parity(message: &[u8]) -> u32 {
    parity(message)
}

/// Whether this downlink format's last 24 bits are parity mixed with the aircraft
/// address, in which case the residual is the address and clearing to zero means
/// nothing.
#[must_use]
pub fn parity_carries_the_address(downlink_format: u8) -> bool {
    matches!(downlink_format, 0 | 4 | 5 | 16 | 20 | 21 | 24..=31)
}

/// How many octets a frame of this downlink format has.
#[must_use]
pub fn octets_for(downlink_format: u8) -> usize {
    if downlink_format & 0x10 == 0 {
        SHORT_OCTETS
    } else {
        LONG_OCTETS
    }
}

/// What a downlink format is for, so a count of the ones this build does not decode
/// names something.
#[must_use]
pub fn downlink_format_name(downlink_format: u8) -> &'static str {
    match downlink_format {
        0 => "short air-air surveillance",
        4 => "surveillance altitude reply",
        5 => "surveillance identity reply",
        11 => "all-call reply",
        16 => "long air-air surveillance",
        17 => "extended squitter",
        18 => "extended squitter, non-transponder",
        19 => "extended squitter, military",
        20 => "Comm-B altitude reply",
        21 => "Comm-B identity reply",
        24..=31 => "Comm-D extended length message",
        _ => "downlink format with no assignment",
    }
}

/// What a DF 18 control field says the message is, so a caller can tell an aircraft's
/// own report from a ground station's rebroadcast of one.
#[must_use]
pub fn control_field_name(control_field: u8) -> &'static str {
    match control_field {
        0 => "ADS-B from a non-transponder device",
        1 => "ADS-B from a non-transponder device, alternate address space",
        2 => "fine format TIS-B",
        3 => "coarse format TIS-B",
        4 => "TIS-B management",
        5 => "TIS-B relay of an ADS-B message, anonymous address",
        6 => "ADS-B rebroadcast",
        _ => "reserved",
    }
}

/// A DF 17 extended squitter: an aircraft reporting itself.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtendedSquitter {
    /// CA, the transponder capability, three bits, uninterpreted.
    pub capability: u8,
    pub address: IcaoAddress,
    pub message: MeMessage,
}

/// A DF 18 squitter: a non-transponder device, or a ground station rebroadcasting.
///
/// The address is **announced by the sender**, and for control fields 1 and 5 it is not
/// an ICAO aircraft address at all but an anonymous or alternate one. The control field
/// is carried so a caller can refuse to treat those as an identity, which is why this
/// is a separate type from [`ExtendedSquitter`] rather than a flag on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SupplementarySquitter {
    pub control_field: u8,
    pub control_field_name: &'static str,
    pub address: IcaoAddress,
    /// The ME field, decoded for the control fields that carry an ADS-B message
    /// (0, 1, 2, 3, 5 and 6). `None` for control field 4, which is TIS-B management,
    /// and 7, which is reserved: neither carries an ME, so decoding one would be
    /// invention. The octets are in [`Self::octets`] either way.
    pub message: Option<MeMessage>,
    /// The seven ME octets as transmitted.
    pub octets: [u8; 7],
}

/// A frame of a downlink format this build does not decode. Carried with its name and
/// its octets so it can be counted and logged, never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherDownlink {
    pub downlink_format: u8,
    pub name: &'static str,
    /// The parity residual. For the surveillance and Comm-B formats this **is** the
    /// aircraft's address (`parity_carries_the_address`); for anything else it is a
    /// residual and means nothing on its own.
    pub residual: u32,
    pub octets: Vec<u8>,
}

impl OtherDownlink {
    /// The aircraft address the address/parity field encodes, for the formats where the
    /// residual is one. `None` where the residual is not an address, so a caller cannot
    /// read a checksum as an identity by accident.
    #[must_use]
    pub fn address(&self) -> Option<IcaoAddress> {
        parity_carries_the_address(self.downlink_format).then_some(IcaoAddress(self.residual))
    }
}

/// One decoded frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Downlink {
    ExtendedSquitter(ExtendedSquitter),
    Supplementary(SupplementarySquitter),
    Other(OtherDownlink),
}

impl Downlink {
    #[must_use]
    pub fn downlink_format(&self) -> u8 {
        match self {
            Self::ExtendedSquitter(_) => 17,
            Self::Supplementary(_) => 18,
            Self::Other(o) => o.downlink_format,
        }
    }

    /// The ME field, for the two formats that carry one.
    #[must_use]
    pub fn message(&self) -> Option<&MeMessage> {
        match self {
            Self::ExtendedSquitter(e) => Some(&e.message),
            Self::Supplementary(s) => s.message.as_ref(),
            Self::Other(_) => None,
        }
    }

    /// The announced address, for the formats that announce one in clear. `None` for
    /// the address/parity formats: [`OtherDownlink::address`] recovers those, and this
    /// keeps the two kinds of knowledge apart.
    #[must_use]
    pub fn announced_address(&self) -> Option<IcaoAddress> {
        match self {
            Self::ExtendedSquitter(e) => Some(e.address),
            Self::Supplementary(s) => Some(s.address),
            Self::Other(_) => None,
        }
    }
}

/// Parse one AVR line: `*` then an even number of hexadecimal digits then `;`.
///
/// Accepts a trailing CR or LF and the `@`, `%` and `<` timestamped prefixes some
/// receivers emit, whose leading timestamp octets are **not** returned: a caller that
/// wants receiver timestamps needs the Beast binary format, and pretending a timestamp
/// is part of the frame would shift every field.
///
/// # Errors
///
/// [`AdsbError::NotAvrFrame`] for a line that is not one, [`AdsbError::Hex`] for a
/// non-hexadecimal digit, [`AdsbError::Length`] when the octet count does not match the
/// downlink format's.
pub fn parse_avr(line: &str) -> Result<Vec<u8>, AdsbError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let body = line
        .strip_prefix('*')
        .ok_or(AdsbError::NotAvrFrame)?
        .strip_suffix(';')
        .ok_or(AdsbError::Malformed("an AVR frame ends with a semicolon"))?;
    if body.is_empty() || body.len() % 2 != 0 {
        return Err(AdsbError::Malformed(
            "an AVR frame is an even number of hexadecimal digits",
        ));
    }
    let mut octets = Vec::with_capacity(body.len() / 2);
    let digits: Vec<char> = body.chars().collect();
    let (pairs, _even_by_the_check_above) = digits.as_chunks::<2>();
    for (i, pair) in pairs.iter().enumerate() {
        let value = |c: char, at: usize| {
            c.to_digit(16)
                .ok_or(AdsbError::Hex { at, found: c })
                .map(|d| u8::try_from(d).unwrap_or(0))
        };
        let high = value(pair[0], i * 2)?;
        let low = value(pair[1], i * 2 + 1)?;
        octets.push((high << 4) | low);
    }
    let format = octets[0] >> 3;
    let expected = octets_for(format);
    if octets.len() != expected {
        return Err(AdsbError::Length {
            downlink_format: format,
            expected,
            found: octets.len(),
        });
    }
    Ok(octets)
}

/// Decode one frame.
///
/// # Errors
///
/// [`AdsbError::Length`] for a frame whose length does not match its downlink format,
/// [`AdsbError::Parity`] for an extended squitter whose parity does not clear.
pub fn decode_frame(octets: &[u8]) -> Result<Downlink, AdsbError> {
    let format = *octets
        .first()
        .ok_or(AdsbError::Malformed("a frame is at least one octet"))?
        >> 3;
    let expected = octets_for(format);
    if octets.len() != expected {
        return Err(AdsbError::Length {
            downlink_format: format,
            expected,
            found: octets.len(),
        });
    }
    let residual = parity(octets);
    if matches!(format, 17 | 18) {
        if residual != 0 {
            return Err(AdsbError::Parity {
                downlink_format: format,
                residual,
            });
        }
        let address = IcaoAddress(u32::from_be_bytes([0, octets[1], octets[2], octets[3]]));
        let mut me = [0u8; 7];
        me.copy_from_slice(&octets[4..11]);
        if format == 17 {
            return Ok(Downlink::ExtendedSquitter(ExtendedSquitter {
                capability: octets[0] & 0b111,
                address,
                message: messages::decode(&me)?,
            }));
        }
        let control_field = octets[0] & 0b111;
        let message = match control_field {
            0..=3 | 5 | 6 => Some(messages::decode(&me)?),
            _ => None,
        };
        return Ok(Downlink::Supplementary(SupplementarySquitter {
            control_field,
            control_field_name: control_field_name(control_field),
            address,
            message,
            octets: me,
        }));
    }
    Ok(Downlink::Other(OtherDownlink {
        downlink_format: format,
        name: downlink_format_name(format),
        residual,
        octets: octets.to_vec(),
    }))
}

/// What a run of frames held, so nothing this build could not read is invisible.
///
/// The counters are separate on purpose. A feed with a rising `parity_failures` has a
/// radio problem; one with a rising `carried_type_codes` has a decoder that is behind
/// its transmitters; and the two need different people.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodeStats {
    /// Frames offered to [`AdsbCodec::decode_frame`], whatever became of them.
    pub frames: u64,
    /// Lines that were not AVR frames at all.
    pub not_avr_lines: u64,
    /// Frames whose length did not match their downlink format.
    pub length_failures: u64,
    /// Extended squitters whose parity did not clear.
    pub parity_failures: u64,
    /// Frames per downlink format this build does not decode.
    pub other_downlink_formats: BTreeMap<u8, u64>,
    /// Extended-squitter type codes this build does not interpret.
    pub carried_type_codes: BTreeMap<u8, u64>,
    /// DF 18 control fields whose message this build does not read (4 and 7).
    pub carried_control_fields: BTreeMap<u8, u64>,
    /// Messages decoded, per type-code group name.
    pub decoded: BTreeMap<&'static str, u64>,
}

impl DecodeStats {
    /// Everything that did not become a decoded message, for a health line that has to
    /// be one number before it is a table.
    #[must_use]
    pub fn not_decoded(&self) -> u64 {
        let sum = |m: &BTreeMap<u8, u64>| m.values().sum::<u64>();
        self.not_avr_lines
            + self.length_failures
            + self.parity_failures
            + sum(&self.other_downlink_formats)
            + sum(&self.carried_type_codes)
            + sum(&self.carried_control_fields)
    }
}

/// The decoder: frames in, typed messages out, with a running account of what it could
/// not read.
#[derive(Debug, Default)]
pub struct AdsbCodec {
    stats: DecodeStats,
}

impl AdsbCodec {
    /// See [`SPECIFICATION`]: this decoder names no normative edition, on purpose.
    pub const SPECIFICATION: &'static str = SPECIFICATION;

    #[must_use]
    pub fn stats(&self) -> &DecodeStats {
        &self.stats
    }

    /// Decode one frame and count it.
    ///
    /// # Errors
    ///
    /// See [`AdsbError`].
    pub fn decode_frame(&mut self, octets: &[u8]) -> Result<Downlink, AdsbError> {
        self.stats.frames += 1;
        let decoded = decode_frame(octets);
        match &decoded {
            Ok(Downlink::Other(o)) => {
                *self
                    .stats
                    .other_downlink_formats
                    .entry(o.downlink_format)
                    .or_default() += 1;
            }
            Ok(other) => {
                if let Downlink::Supplementary(s) = other {
                    if s.message.is_none() {
                        *self
                            .stats
                            .carried_control_fields
                            .entry(s.control_field)
                            .or_default() += 1;
                    }
                }
                match other.message() {
                    Some(MeMessage::Carried { type_code, .. }) => {
                        *self.stats.carried_type_codes.entry(*type_code).or_default() += 1;
                    }
                    Some(message) => {
                        *self
                            .stats
                            .decoded
                            .entry(messages::type_code_name(message.type_code()))
                            .or_default() += 1;
                    }
                    None => {}
                }
            }
            Err(AdsbError::Parity { .. }) => self.stats.parity_failures += 1,
            Err(AdsbError::Length { .. }) => self.stats.length_failures += 1,
            Err(_) => self.stats.not_avr_lines += 1,
        }
        decoded
    }

    /// Decode one AVR line and count it. A line that is not an AVR frame is counted and
    /// reported, which is how a caller reading a receiver's mixed log skips it.
    ///
    /// # Errors
    ///
    /// See [`AdsbError`].
    pub fn decode_avr(&mut self, line: &str) -> Result<Downlink, AdsbError> {
        match parse_avr(line) {
            Ok(octets) => self.decode_frame(&octets),
            Err(e) => {
                // A line whose octet count does not match its downlink format is a
                // malformed **frame**, not a line that was never a frame; counting the
                // two apart is what lets an operator tell a truncating receiver from a
                // log with status lines in it.
                if matches!(e, AdsbError::Length { .. }) {
                    self.stats.length_failures += 1;
                } else {
                    self.stats.not_avr_lines += 1;
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame the ADS-B literature uses as its worked example: DF 17, ICAO 4840D6,
    /// identification KLM1023.
    const IDENTIFICATION: &str = "*8D4840D6202CC371C32CE0576098;";

    #[test]
    fn an_avr_line_parses_and_its_length_matches_its_downlink_format() {
        let octets = parse_avr(IDENTIFICATION).expect("parses");
        assert_eq!(octets.len(), LONG_OCTETS);
        assert_eq!(octets[0] >> 3, 17);
        assert_eq!(parse_avr("8D4840D6"), Err(AdsbError::NotAvrFrame));
        assert!(matches!(
            parse_avr("*8D4840D6;"),
            Err(AdsbError::Length {
                downlink_format: 17,
                expected: 14,
                found: 4
            })
        ));
        assert!(matches!(
            parse_avr("*8Z4840D6;"),
            Err(AdsbError::Hex { .. })
        ));
    }

    #[test]
    fn the_parity_of_an_intact_extended_squitter_clears() {
        let octets = parse_avr(IDENTIFICATION).expect("parses");
        assert_eq!(parity(&octets), 0);
        assert_eq!(expected_parity(&octets[..11]), 0x0057_6098);
    }

    #[test]
    fn a_corrupted_extended_squitter_is_refused_not_decoded() {
        let mut octets = parse_avr(IDENTIFICATION).expect("parses");
        octets[6] ^= 0x01;
        assert!(matches!(
            decode_frame(&octets),
            Err(AdsbError::Parity {
                downlink_format: 17,
                ..
            })
        ));
    }

    /// A DF 18 frame with the given control field and message field, parity appended
    /// the way a transmitter would. The vendored captures hold only control fields 1, 5
    /// and 6, so the two this build refuses to read a message from have to be built.
    fn supplementary(control_field: u8, me: [u8; 7]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(LONG_OCTETS);
        frame.push((18 << 3) | (control_field & 0b111));
        frame.extend_from_slice(&[0x4D, 0x20, 0x23]);
        frame.extend_from_slice(&me);
        let parity = expected_parity(&frame);
        frame.extend_from_slice(&parity.to_be_bytes()[1..]);
        frame
    }

    /// Control fields 4 and 7 carry no ADS-B message field, so reading one would be
    /// invention. The octets are kept and the control field is named instead.
    #[test]
    fn a_control_field_that_carries_no_message_is_not_given_one() {
        let me = [0x20, 0x2C, 0xC3, 0x71, 0xC3, 0x2C, 0xE0];
        for (control_field, name) in [(4u8, "TIS-B management"), (7, "reserved")] {
            let frame = supplementary(control_field, me);
            let Downlink::Supplementary(s) = decode_frame(&frame).expect("decodes") else {
                panic!("DF 18 is a supplementary squitter");
            };
            assert_eq!(s.control_field, control_field);
            assert_eq!(s.control_field_name, name);
            assert_eq!(s.message, None, "control field {control_field}");
            assert_eq!(s.octets, me, "the octets are kept even so");
        }
        // And the control fields that do carry one are read.
        for control_field in [0u8, 1, 2, 3, 5, 6] {
            let frame = supplementary(control_field, me);
            let Downlink::Supplementary(s) = decode_frame(&frame).expect("decodes") else {
                panic!("DF 18 is a supplementary squitter");
            };
            assert!(
                matches!(s.message, Some(MeMessage::Identification(_))),
                "control field {control_field} carries an ADS-B message"
            );
        }
    }

    #[test]
    fn a_downlink_format_this_build_does_not_decode_is_named_and_kept() {
        // DF 11, an all-call reply: seven octets, and its parity carries the
        // interrogator identifier rather than an address.
        let octets = parse_avr("*5D4D20237A55A6;").expect("parses");
        let Downlink::Other(o) = decode_frame(&octets).expect("decodes") else {
            panic!("DF 11 is not decoded by this build");
        };
        assert_eq!(o.downlink_format, 11);
        assert_eq!(o.name, "all-call reply");
        assert_eq!(o.address(), None, "an all-call residual is not an address");
        assert_eq!(o.octets.len(), SHORT_OCTETS);
    }

    #[test]
    fn the_statistics_account_for_every_frame_offered() {
        let mut codec = AdsbCodec::default();
        codec.decode_avr(IDENTIFICATION).expect("identification");
        codec.decode_avr("*5D4D20237A55A6;").expect("all-call");
        // Type code 31, aircraft operation status: carried, not interpreted. A real
        // frame from `testdata/adsb/lax-messages-first40000.txt`, not an invented one,
        // so its parity is a transmitter's and not this file's arithmetic.
        codec
            .decode_avr("*8DAC259FF8132006005AB8DFA302;")
            .expect("operation status");
        assert!(codec.decode_avr("not a frame").is_err());
        let stats = codec.stats().clone();
        assert_eq!(stats.frames, 3);
        assert_eq!(stats.not_avr_lines, 1);
        assert_eq!(stats.other_downlink_formats.get(&11), Some(&1));
        assert_eq!(stats.carried_type_codes.get(&31), Some(&1));
        assert_eq!(
            stats.decoded.get("aircraft identification and category"),
            Some(&1)
        );
        assert_eq!(stats.not_decoded(), 3);
    }
}
