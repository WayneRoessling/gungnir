//! AIS: ITU-R M.1371 payloads carried in NMEA 0183 `!AIVDM` / `!AIVDO` sentences
//! (GAP-010, D-24, D-32).
//!
//! **Edition pinned 2026-09-06: ITU-R M.1371-6 (02/2026).** The pin and what was checked
//! to make it are in `docs/design/external-standards.md` §3: for the eight message types
//! this system reads (1, 2, 3, 5, 18, 19, 21, 24) the -5 and -6 layouts were compared
//! table by table and are bit-identical; -6 gives meaning to two bits that -5 held spare
//! (Message 24 part B, VDES capabilities) and adds one value to the position-fixing-device
//! enumeration (9 = BDS). Both are read here, and both decode a -5 transmitter unchanged.
//!
//! The sentence framing (six-bit armouring, fragments, the checksum) follows the gpsd
//! project's public "AIVDM/AIVDO protocol decoding" document, which describes what IEC
//! 61162-1 sells; the payload fields follow the Recommendation's Annex 7 tables, cited by
//! table number beside each field in [`messages`].
//!
//! # What this is and is not
//!
//! A decoder from sentence text to typed messages, gated on gpsd's regression captures
//! (`testdata/ais/SOURCE.md`) decoded against gpsd's own output as the oracle. It is not
//! yet an evidence source: turning a position report into a cooperative-identity claim
//! for `gungnir-identification` needs an adapter that owns the receiver and the local
//! frame, which is the remaining half of GAP-010's AIS work. Nothing here knows a
//! `SensorId`.
//!
//! Every value is carried **raw, in the Recommendation's units** (longitude in
//! 1/10 000 minute, speed in 1/10 knot, course in 1/10 degree) with the "not available"
//! sentinels intact, because a decoder that scaled and defaulted would hide exactly the
//! cases a caller must handle. The typed accessors say which sentinel means what.

pub mod messages;

use std::collections::BTreeMap;

pub use messages::{
    AidToNavigationReport, AisMessage, ClassBExtendedReport, ClassBPositionReport, Dimensions,
    Header, PositionReport, RawPosition, StaticDataReport, StaticDataReportPart, StaticExtent,
    StaticVoyageData,
};

/// Why a sentence or a payload could not be read. Every arm names the sentence's own
/// fault; none is a silent skip.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AisError {
    /// The line is not an `!AIVDM` or `!AIVDO` sentence at all (other NMEA talkers share a
    /// feed with AIS, and a caller skips those by this arm).
    #[error("not an AIS sentence")]
    NotAisSentence,
    #[error("malformed sentence: {0}")]
    Malformed(&'static str),
    #[error("checksum {actual:02X} does not match the sentence's {expected:02X}")]
    Checksum { expected: u8, actual: u8 },
    #[error("byte {0:?} is outside the six-bit armour alphabet")]
    Armour(char),
    /// A fragment arrived for a group whose earlier fragments were never seen, or out of
    /// order; the group is dropped and this says so.
    #[error("fragment {fragment} of {of} has no group to join")]
    Fragment { fragment: u8, of: u8 },
    /// The payload is shorter than the message type's table requires.
    #[error(
        "message type {message_type} carries {bits} bits, fewer than the {needed} its table needs"
    )]
    TooShort {
        message_type: u8,
        bits: usize,
        needed: usize,
    },
}

/// One `!AIVDM`/`!AIVDO` sentence, parsed and checksummed but not yet unarmoured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AisSentence {
    /// `!AIVDO`: the receiver's own ship rather than another station.
    pub own_ship: bool,
    pub fragment_count: u8,
    pub fragment_number: u8,
    /// Present on multi-fragment groups.
    pub sequential_id: Option<u8>,
    /// `A` or `B`, when the receiver reported one.
    pub channel: Option<char>,
    /// The six-bit armoured payload, untouched.
    pub payload: String,
    /// Padding bits at the end of this fragment's payload.
    pub fill_bits: u8,
}

/// NMEA 0183's checksum: the exclusive-or of every byte between `!` and `*`.
fn checksum(body: &str) -> u8 {
    body.bytes().fold(0, |acc, b| acc ^ b)
}

/// Parse one sentence line. Accepts a trailing CR/LF, a leading NMEA 4.10 tag block, and
/// any talker identifier (`AI`, `AB`, `BS`, ...).
///
/// # Errors
///
/// [`AisError::NotAisSentence`] for any other talker, [`AisError::Malformed`] for a
/// sentence with the wrong field count or an unreadable number, [`AisError::Checksum`]
/// when the checksum does not match.
pub fn parse_sentence(line: &str) -> Result<AisSentence, AisError> {
    let line = line.trim_end_matches(['\r', '\n']);
    // A tag block (`\s:...*hh\`) may precede the sentence; the sentence starts at `!`.
    let start = line.find('!').ok_or(AisError::NotAisSentence)?;
    let sentence = &line[start + 1..];
    let (body, sum) = sentence
        .split_once('*')
        .ok_or(AisError::Malformed("no checksum"))?;
    // Any talker may carry the sentence (`AI` a mobile station, `AB` and `BS` base
    // stations); the formatter is what says it is AIS.
    let formatter = body.get(2..5).unwrap_or_default();
    if body.len() < 5 || !(formatter == "VDM" || formatter == "VDO") {
        return Err(AisError::NotAisSentence);
    }
    let expected = u8::from_str_radix(sum.get(..2).unwrap_or_default(), 16)
        .map_err(|_| AisError::Malformed("checksum is not two hex digits"))?;
    let actual = checksum(body);
    if expected != actual {
        return Err(AisError::Checksum { expected, actual });
    }
    let fields: Vec<&str> = body.split(',').collect();
    if fields.len() != 7 {
        return Err(AisError::Malformed("an AIVDM sentence has seven fields"));
    }
    let number = |s: &str, what: &'static str| -> Result<u8, AisError> {
        s.parse().map_err(|_| AisError::Malformed(what))
    };
    let sequential_id = if fields[3].is_empty() {
        None
    } else {
        Some(number(fields[3], "sequential id")?)
    };
    let channel = fields[4].chars().next();
    Ok(AisSentence {
        own_ship: fields[0].ends_with("VDO"),
        fragment_count: number(fields[1], "fragment count")?,
        fragment_number: number(fields[2], "fragment number")?,
        sequential_id,
        channel,
        payload: fields[5].to_owned(),
        fill_bits: number(fields[6], "fill bits")?,
    })
}

/// A payload's bits, most significant first, as the Recommendation numbers them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BitField {
    bits: Vec<bool>,
}

impl BitField {
    /// Unarmour one or more concatenated payload strings, dropping `fill_bits` from the
    /// end.
    ///
    /// # Errors
    ///
    /// [`AisError::Armour`] for a byte outside the alphabet.
    pub fn from_armoured(payload: &str, fill_bits: u8) -> Result<Self, AisError> {
        let mut bits = Vec::with_capacity(payload.len() * 6);
        for c in payload.chars() {
            // The alphabet is `0`..`W` (48..87) then `` ` ``..`w` (96..119); the eight
            // bytes between are outside it.
            let v = match u32::from(c) {
                code @ 48..=87 => code - 48,
                code @ 96..=119 => code - 56,
                _ => return Err(AisError::Armour(c)),
            };
            for shift in (0..6).rev() {
                bits.push((v >> shift) & 1 == 1);
            }
        }
        let keep = bits.len().saturating_sub(usize::from(fill_bits));
        bits.truncate(keep);
        Ok(Self { bits })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bits.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    /// An unsigned field of `len` bits starting at bit `start`. `None` past the end.
    #[must_use]
    pub fn u(&self, start: usize, len: usize) -> Option<u64> {
        let slice = self.bits.get(start..start.checked_add(len)?)?;
        Some(slice.iter().fold(0u64, |acc, &b| (acc << 1) | u64::from(b)))
    }

    /// A two's-complement signed field.
    #[must_use]
    pub fn i(&self, start: usize, len: usize) -> Option<i64> {
        let raw = self.u(start, len)?;
        if len == 0 || len >= 64 {
            return None;
        }
        let sign = 1u64 << (len - 1);
        Some(if raw & sign == 0 {
            i64::try_from(raw).ok()?
        } else {
            i64::try_from(raw).ok()? - i64::try_from(1u64 << len).ok()?
        })
    }

    #[must_use]
    pub fn flag(&self, at: usize) -> Option<bool> {
        self.u(at, 1).map(|v| v == 1)
    }

    /// Six-bit ASCII text (Table 47 of the Recommendation) from `len` bits, cut at the
    /// first `@` (the padding character) and stripped of trailing spaces, which is how
    /// every receiver presents a name. A field shorter than `len` reads what is there.
    #[must_use]
    pub fn text(&self, start: usize, len: usize) -> String {
        let mut out = String::new();
        let end = (start + len).min(self.bits.len());
        let mut at = start;
        while at + 6 <= end {
            let Some(v) = self.u(at, 6) else { break };
            #[allow(clippy::cast_possible_truncation)]
            let v = v as u8;
            let c = if v < 32 { v + 64 } else { v };
            if c == b'@' {
                break;
            }
            out.push(char::from(c));
            at += 6;
        }
        out.truncate(out.trim_end().len());
        out
    }
}

/// A complete payload: one sentence, or a multi-fragment group joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub own_ship: bool,
    pub channel: Option<char>,
    pub bits: BitField,
}

/// Joins fragments into payloads. One per feed: groups are keyed by sequential id and
/// a new group under an id abandons an incomplete one, which is what the sentence
/// standard's seven-value id space assumes.
#[derive(Debug, Default)]
pub struct SentenceAssembler {
    partial: BTreeMap<(bool, u8), Vec<Option<AisSentence>>>,
}

impl SentenceAssembler {
    /// Feed one line. `Ok(None)` while a group is incomplete; `Ok(Some)` when a payload
    /// is whole.
    ///
    /// # Errors
    ///
    /// Any [`AisError`] from parsing or unarmouring; a fragment with no group to join.
    pub fn push(&mut self, line: &str) -> Result<Option<Payload>, AisError> {
        let sentence = parse_sentence(line)?;
        if sentence.fragment_count <= 1 {
            let bits = BitField::from_armoured(&sentence.payload, sentence.fill_bits)?;
            return Ok(Some(Payload {
                own_ship: sentence.own_ship,
                channel: sentence.channel,
                bits,
            }));
        }
        let key = (sentence.own_ship, sentence.sequential_id.unwrap_or(0));
        let count = usize::from(sentence.fragment_count);
        let number = usize::from(sentence.fragment_number);
        if number == 0 || number > count {
            return Err(AisError::Fragment {
                fragment: sentence.fragment_number,
                of: sentence.fragment_count,
            });
        }
        if number == 1 {
            self.partial.insert(key, vec![None; count]);
        }
        let Some(group) = self.partial.get_mut(&key).filter(|g| g.len() == count) else {
            return Err(AisError::Fragment {
                fragment: sentence.fragment_number,
                of: sentence.fragment_count,
            });
        };
        group[number - 1] = Some(sentence);
        if group.iter().any(Option::is_none) {
            return Ok(None);
        }
        let group = self.partial.remove(&key).unwrap_or_default();
        let mut armoured = String::new();
        let mut fill = 0;
        let mut own_ship = false;
        let mut channel = None;
        for fragment in group.into_iter().flatten() {
            armoured.push_str(&fragment.payload);
            fill = fragment.fill_bits;
            own_ship = fragment.own_ship;
            channel = channel.or(fragment.channel);
        }
        let bits = BitField::from_armoured(&armoured, fill)?;
        Ok(Some(Payload {
            own_ship,
            channel,
            bits,
        }))
    }
}

/// The decoder: sentences in, typed messages out.
#[derive(Debug, Default)]
pub struct AisCodec {
    assembler: SentenceAssembler,
}

impl AisCodec {
    /// The edition the payload tables were written from.
    pub const EDITION: &'static str = "ITU-R M.1371-6 (02/2026)";

    /// Decode one line of a feed. `Ok(None)` while a fragment group is incomplete.
    ///
    /// # Errors
    ///
    /// See [`AisError`]. A line from another NMEA talker is
    /// [`AisError::NotAisSentence`], which a caller reading a mixed feed skips.
    pub fn decode_line(&mut self, line: &str) -> Result<Option<AisMessage>, AisError> {
        match self.assembler.push(line)? {
            None => Ok(None),
            Some(payload) => messages::decode(&payload.bits).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // From the gpsd document's own worked example (the sentence framing, not a copied
    // table): a type 1 report.
    const TYPE1: &str = "!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5C";

    #[test]
    fn a_sentence_parses_and_its_checksum_is_checked() {
        let s = parse_sentence(TYPE1).expect("parses");
        assert_eq!(s.fragment_count, 1);
        assert_eq!(s.channel, Some('B'));
        assert_eq!(s.fill_bits, 0);
        assert!(matches!(
            parse_sentence("!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5D"),
            Err(AisError::Checksum { .. })
        ));
        assert_eq!(
            parse_sentence("$GPRMC,213950.00,A,5250.53669,N,00542.34920,E,0.020,,070420,,,A*7D"),
            Err(AisError::NotAisSentence)
        );
    }

    #[test]
    fn the_armour_alphabet_is_the_six_bit_one() {
        let bits = BitField::from_armoured("0", 0).expect("zero");
        assert_eq!(bits.u(0, 6), Some(0));
        let bits = BitField::from_armoured("w", 0).expect("63");
        assert_eq!(bits.u(0, 6), Some(63));
        // `X` (88) is the gap in the alphabet.
        assert_eq!(BitField::from_armoured("X", 0), Err(AisError::Armour('X')));
        // Fill bits are dropped from the end.
        let bits = BitField::from_armoured("ww", 4).expect("two");
        assert_eq!(bits.len(), 8);
    }

    #[test]
    fn signed_fields_are_twos_complement() {
        let bits = BitField::from_armoured("w", 0).expect("all ones");
        assert_eq!(bits.i(0, 6), Some(-1));
        let bits = BitField::from_armoured("P", 0).expect("100000");
        assert_eq!(bits.i(0, 6), Some(-32));
    }

    #[test]
    fn six_bit_text_stops_at_the_padding_character() {
        // "ABC@@@" in six-bit: A=1, B=2, C=3, @=0.
        let armoured = "123000";
        let bits = BitField::from_armoured(armoured, 0).expect("armoured");
        assert_eq!(bits.text(0, 36), "ABC");
    }

    #[test]
    fn fragments_join_in_order_and_a_stray_fragment_is_refused() {
        let mut a = SentenceAssembler::default();
        let first = a
            .push("!AIVDM,2,1,9,B,55R3Vn82=ILTQ3KKS>1<D60Dq@E918U<F222221J1`?164vc03S1CCAD,0*2A")
            .expect("first fragment");
        assert!(first.is_none());
        let whole = a
            .push("!AIVDM,2,2,9,B,`88888888888880,2*76")
            .expect("second fragment")
            .expect("whole");
        assert_eq!(whole.bits.u(0, 6), Some(5));
        assert_eq!(whole.bits.len(), 424);
        // The second fragment alone has nothing to join.
        let mut b = SentenceAssembler::default();
        assert!(matches!(
            b.push("!AIVDM,2,2,9,B,`88888888888880,2*76"),
            Err(AisError::Fragment { fragment: 2, of: 2 })
        ));
    }
}
