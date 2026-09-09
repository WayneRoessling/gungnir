// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! MISB ST 0601's UAS Datalink Local Set: the KLV metadata format a UAS platform
//! rides alongside its own video (GAP-099; `docs/design/external-standards.md` §8
//! and §8.2).
//!
//! # What is pinned from the primary text, and what is not
//!
//! **Nothing here is pinned to MISB's own text the way `AsterixCat048Codec` is
//! pinned to a EUROCONTROL edition or `AisCodec` to an ITU-R revision.** §8 records
//! why: both NGA registry pages that carry ST 0601 sit behind a bot gateway, so an
//! automated fetch receives a challenge page rather than the document, and this
//! decoder was written without ever independently reading MISB's tag table.
//!
//! What **is** independently verified, byte for byte, against three things that are
//! not the primary text:
//!
//! 1. **The generic KLV/BER-OID tag-length-value framing** (the 16-byte SMPTE-
//!    registered Universal Label, then a BER short-or-long-form length, then that
//!    many value bytes; a local set's items follow the same length encoding with a
//!    one-byte tag) is documented independently of MISB in multiple public sources
//!    and is not specific to this standard.
//! 2. **The tag semantics** -- which tag number means what, and the domain/range a
//!    "mapped" numeric field scales between -- are read from
//!    `github.com/paretech/klvdata` (MIT licence), an open-source Python decoder,
//!    at commit `79028b4ab4ce7192d1b7c04d2266fc31ac337511`. `klvdata/misb0601.py`'s
//!    `key`, `TAG`, `_domain`, `_range` and `_error` class attributes are transcribed
//!    into [`decode_frame`]'s tag table below, one tag at a time, cited by tag number
//!    in each doc comment.
//! 3. **The decoded values** this module produces for the vendored fixture
//!    (`testdata/misb/SOURCE.md`) were cross-checked by running klvdata's own code
//!    against the same bytes (recorded in this crate's history, not re-run here) and
//!    are pinned as the oracle in `gungnir-interop/tests/misb0601_fixtures.rs`.
//!
//! So: **the tag framing is public, independent knowledge; the tag semantics are a
//! secondary source's reading of MISB, not MISB's own text.** A future session that
//! obtains ST 0601 itself (a browser session past the bot gateway, or a printed copy)
//! should diff this tag table against it the way §1.6 diffed `asterix-specs` against
//! the EUROCONTROL PDF, and correct anything that disagrees -- the secondary source
//! governs nothing once the primary text is in hand.
//!
//! # The checksum: a documented, reproducible discrepancy
//!
//! ST 0601's Tag 1 (Checksum) is, per the klvdata maintainer's own description of
//! `klvdata.common.packet_checksum` (`github.com/paretech/klvdata` issue #7, quoting
//! MISB ST 0601.8-08): "the lower 16-bits of summation performed on the entire LS
//! packet" with the packet's own trailing checksum value excluded from the sum, and
//! "All instances of a UAS Datalink LS where the computed checksum is not identical
//! to the included checksum shall be discarded." [`packet_checksum`] reimplements
//! that algorithm, verified byte for byte against `packet_checksum`'s own output.
//!
//! **The vendored fixture's own stated checksum does not validate under this
//! algorithm.** Computed over the fixture's 226 bytes preceding its trailing pair:
//! `0x3E1E`. The fixture's own trailing two bytes: `0xAA43`. This was checked against
//! every plausible alternative byte range (the value only, the value minus the
//! checksum element entirely) and none reproduces the stored bytes, so this is not a
//! misreading of which bytes to sum -- it is the worked example itself, as vendored
//! by a third party from what its own code comments say is "MISB ST0902.5 Annex C...
//! Some errors may have been hand corrected." [`decode_frame`] decodes the fixture's
//! fields regardless (they are well-formed KLV independent of the checksum) and
//! reports the mismatch on [`Misb0601Frame::checksum_valid`] rather than either
//! silently trusting the stated value or refusing to decode a real, MIT-licensed
//! worked example over its one field that does not arithmetically close. The ingest
//! adapter (`gungnir_ingest::adapters::misb`) is where MISB's own "shall be
//! discarded" rule is enforced, matching this workspace's separation of "the codec
//! decodes, the adapter and gateway decide what to trust".
//!
//! # Scope
//!
//! Seventeen tags: the checksum and timestamp housekeeping fields, four identity
//! strings, platform heading/pitch/roll, the platform's own position (Sensor
//! Latitude/Longitude/True Altitude), the sensor's pointing relative to the platform
//! (azimuth/elevation/roll) and its slant range, the ground point it is looking at
//! (Frame Center Latitude/Longitude/Elevation), and the LS version number. Every
//! other tag -- MISB ST 0601 defines upwards of ninety -- is carried in
//! [`Misb0601Frame::carried_raw`] by tag number and raw bytes, never dropped and
//! never guessed at. A known tag whose value is not the width its table entry fixes
//! (2 octets for the platform angles and the two altitudes, 4 for the latitudes,
//! longitudes, sensor-relative angles and slant range -- the widths klvdata's tag table
//! and the vendored fixture use; valid UTF-8 for a string) is carried the same way
//! rather than scaled as if it were. **Made strict 2026-09-09, in review before the
//! adapter was signed**: until then any 1-, 2- or 4-octet value was scaled by the
//! tag's fixed domain, so a 2-octet latitude would have decoded as a value near zero
//! degrees instead of being carried raw as this paragraph already promised. A producer
//! that encodes a field more compactly than its fixed width is a real MISB possibility
//! this decoder does not claim to read.
//!
//! Out of scope entirely: the video essence, MPEG-2 transport stream demultiplexing
//! (ST 1402), the nested Security Local Set (Tag 48, ST 0102), and every tag this
//! module does not name. This is metadata-only, per this gap's own scope: a video
//! transport carrying telemetry, not a detection message and not a decision to
//! render video.

use gungnir_model::MissionTime;

/// The 16-byte SMPTE-registered Universal Label for MISB ST 0601's UAS Datalink
/// Local Set. Transcribed from `klvdata.misb0601.UASLocalMetadataSet.key`
/// (`hexstr_to_bytes('06 0E 2B 34 - 02 0B 01 01 – 0E 01 03 01 - 01 00 00 00')`) and
/// confirmed against the first 16 bytes of the vendored fixture itself.
pub const UDS_KEY: [u8; 16] = [
    0x06, 0x0E, 0x2B, 0x34, 0x02, 0x0B, 0x01, 0x01, 0x0E, 0x01, 0x03, 0x01, 0x01, 0x00, 0x00, 0x00,
];

/// What this decoder's tag semantics were read from, since MISB's own text could
/// not be fetched (see the module doc comment). Not a specification edition the way
/// `AisCodec::EDITION` is one.
pub const CROSS_CHECK_SOURCE: &str =
    "paretech/klvdata @ 79028b4ab4ce7192d1b7c04d2266fc31ac337511 (MIT)";

/// Why a frame did not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Misb0601Error {
    /// The bytes at the front of the buffer are not the UAS Datalink LS key. A
    /// stream carrying more than one MISB local set (ST 0102 security metadata
    /// alongside ST 0601, for instance) reaches this on the sets this module does
    /// not read; a caller resynchronizing after one uses [`find_next_key`].
    #[error("frame does not start with the UAS Datalink LS key")]
    KeyMismatch,
    /// Not a corrupt frame: a live stream has not yet delivered enough bytes for the
    /// frame the header already promises. A caller buffers and retries.
    #[error("need {needed} bytes for a complete frame, have {have}")]
    Truncated { needed: usize, have: usize },
    /// A BER length field's own form is invalid (a long-form byte count of zero, or
    /// one this decoder will not trust past eight octets).
    #[error("a BER length field is malformed")]
    BerLengthMalformed,
    /// A local set item's declared length runs past the end of the set's own value,
    /// which the frame's outer BER length already said was complete -- a malformed
    /// frame, not a supply problem.
    #[error("tag {tag} declares a length that runs past the local set's own end")]
    LocalSetTruncated { tag: u8 },
}

/// A platform's KLV metadata, decoded from one complete UAS Datalink LS frame.
///
/// Every field is `Option`: MISB ST 0601 defines far more tags than this decoder
/// reads (see the module doc comment's "Scope"), and a real producer sends whichever
/// subset its platform and payload support, so an absent tag is ordinary rather than
/// an error. All angles are degrees and all distances metres, in the units the tag
/// tables above state -- converting to radians and to the local ENU frame is the
/// ingest adapter's job, not this decoder's, exactly as `cat048::decode_records`
/// leaves the ENU conversion to `AsterixCat048Codec::map`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Misb0601Frame {
    /// Tag 2, microseconds since the Unix epoch.
    pub precision_time_stamp_us: Option<u64>,
    /// Tag 3.
    pub mission_id: Option<String>,
    /// Tag 4.
    pub platform_tail_number: Option<String>,
    /// Tag 5, degrees, `[0, 360)`.
    pub platform_heading_deg: Option<f64>,
    /// Tag 6, degrees.
    pub platform_pitch_deg: Option<f64>,
    /// Tag 7, degrees.
    pub platform_roll_deg: Option<f64>,
    /// Tag 10.
    pub platform_designation: Option<String>,
    /// Tag 11.
    pub image_source_sensor: Option<String>,
    /// Tag 13: the platform's own fix, not where its sensor is looking.
    pub sensor_latitude_deg: Option<f64>,
    /// Tag 14.
    pub sensor_longitude_deg: Option<f64>,
    /// Tag 15, metres.
    pub sensor_true_altitude_m: Option<f64>,
    /// Tag 18, degrees, `[0, 360)`: the sensor's pointing relative to the platform.
    pub sensor_relative_azimuth_deg: Option<f64>,
    /// Tag 19, degrees.
    pub sensor_relative_elevation_deg: Option<f64>,
    /// Tag 20, degrees, `[0, 360)`.
    pub sensor_relative_roll_deg: Option<f64>,
    /// Tag 21, metres.
    pub slant_range_m: Option<f64>,
    /// Tag 23: the ground point the sensor is looking at.
    pub frame_center_latitude_deg: Option<f64>,
    /// Tag 24.
    pub frame_center_longitude_deg: Option<f64>,
    /// Tag 25, metres.
    pub frame_center_elevation_m: Option<f64>,
    /// Tag 65, the producer's own claimed LS version.
    pub uas_lds_version: Option<u8>,
    /// Tag 1's raw two bytes, big-endian, when the frame carried one.
    pub stated_checksum: Option<u16>,
    /// [`packet_checksum`] over this frame's own bytes, computed regardless of
    /// whether a stated checksum was even present, so a caller can always ask
    /// [`Self::checksum_valid`].
    pub computed_checksum: u16,
    /// Every tag this decoder read but does not interpret -- unknown tag numbers,
    /// and known ones whose width this decoder does not support -- by tag number and
    /// raw bytes, in the order encountered. Mirrors `AsterixCat048Codec`'s
    /// `Record::carried_raw` (`docs/design/external-standards.md` §1.8): nothing
    /// read from the wire is ever silently dropped.
    pub carried_raw: Vec<(u8, Vec<u8>)>,
}

impl Misb0601Frame {
    /// Whether [`Self::computed_checksum`] matches [`Self::stated_checksum`]. `false`
    /// both when the frame carried no checksum tag at all and when it carried one
    /// that does not match -- a caller that needs to tell those apart reads
    /// `stated_checksum` directly.
    #[must_use]
    pub fn checksum_valid(&self) -> bool {
        self.stated_checksum == Some(self.computed_checksum)
    }

    /// The frame's own precision time stamp (Tag 2) as `MissionTime`'s Unix seconds,
    /// or `receipt` when the frame carried none. Unlike AIS's bare seconds-within-a-
    /// minute field, Tag 2 is a full absolute timestamp, so no reconstruction against
    /// the receive clock is needed -- the value converts directly.
    #[must_use]
    pub fn source_time(&self, receipt: MissionTime) -> MissionTime {
        match self.precision_time_stamp_us {
            // Exact below 2^53 microseconds (about the year 2255 since the Unix
            // epoch); every timestamp this decoder will plausibly ever see is many
            // orders of magnitude smaller than where an f64 would start rounding.
            #[allow(clippy::cast_precision_loss)]
            Some(us) => MissionTime(us as f64 / 1_000_000.0),
            None => receipt,
        }
    }
}

/// Reads a BER length (SMPTE ST 336's short/long form) starting at `buf[0]`: fewer
/// than 128 is the length itself; 128 or more has its low seven bits name how many
/// of the following bytes hold the big-endian length. Returns the decoded length and
/// how many bytes the length field itself occupied.
fn read_ber_length(buf: &[u8]) -> Result<(usize, usize), Misb0601Error> {
    let first = *buf
        .first()
        .ok_or(Misb0601Error::Truncated { needed: 1, have: 0 })?;
    if first < 0x80 {
        return Ok((usize::from(first), 1));
    }
    let count = usize::from(first & 0x7F);
    if count == 0 || count > 8 {
        return Err(Misb0601Error::BerLengthMalformed);
    }
    let bytes = buf.get(1..1 + count).ok_or(Misb0601Error::Truncated {
        needed: 1 + count,
        have: buf.len(),
    })?;
    let mut value: u64 = 0;
    for &b in bytes {
        value = (value << 8) | u64::from(b);
    }
    let length = usize::try_from(value).map_err(|_| Misb0601Error::BerLengthMalformed)?;
    Ok((length, 1 + count))
}

/// The lower 16 bits of the sum of 16-bit big-endian words over `packet`, excluding
/// `packet`'s own trailing two bytes from the sum -- MISB ST 0601's UAS Datalink LS
/// checksum algorithm, reconstructed from `klvdata.common.packet_checksum`
/// (`paretech/klvdata`, MIT) and independently re-derived by hand before being
/// trusted here (see the module doc comment's "checksum" section). Takes the *whole*
/// framed packet from the 16-byte key onward, per the klvdata maintainer's own
/// description of the function's contract: the packet's final two bytes are always
/// treated as the checksum value to exclude, whether or not they happen to match
/// what this function computes.
#[must_use]
pub fn packet_checksum(packet: &[u8]) -> u16 {
    let summed_len = packet.len().saturating_sub(2);
    let summed = &packet[..summed_len];
    let mut total: u32 = 0;
    let mut pos = 0usize;
    while pos + 2 <= summed.len() {
        total = total.wrapping_add(u32::from(u16::from_be_bytes([
            summed[pos],
            summed[pos + 1],
        ])));
        pos += 2;
    }
    if pos < summed.len() {
        total = total.wrapping_add(u32::from(summed[pos]) << 8);
    }
    u16::try_from(total & 0xFFFF).unwrap_or(0) // masked above; never actually truncates
}

/// The byte offset of the next occurrence of [`UDS_KEY`] at or after `from`, for a
/// caller resynchronizing a stream after [`Misb0601Error::KeyMismatch`] or a local-set
/// decode error.
#[must_use]
pub fn find_next_key(buf: &[u8], from: usize) -> Option<usize> {
    let window = buf.get(from..)?;
    window
        .windows(UDS_KEY.len())
        .position(|w| w == UDS_KEY)
        .map(|p| p + from)
}

/// Reads a big-endian integer of 1, 2 or 4 bytes -- the widths this decoder's tag
/// table uses -- signed or unsigned per the field's own domain, staying inside `i64`
/// so the "is this the not-available sentinel" check runs in integer space rather
/// than ever comparing floats for exact equality.
fn read_be_i64(bytes: &[u8], signed: bool) -> Option<i64> {
    Some(match (bytes.len(), signed) {
        (1, true) => i64::from(i8::from_be_bytes([bytes[0]])),
        (1, false) => i64::from(bytes[0]),
        (2, true) => i64::from(i16::from_be_bytes([bytes[0], bytes[1]])),
        (2, false) => i64::from(u16::from_be_bytes([bytes[0], bytes[1]])),
        (4, true) => i64::from(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        (4, false) => i64::from(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        _ => return None,
    })
}

/// `klvdata.common.linear_map`, transcribed: `slope * (src - src_min) + dst_min`
/// with `slope = (dst_max - dst_min) / (src_max - src_min)`.
fn linear_map(src: f64, src_min: f64, src_max: f64, dst_min: f64, dst_max: f64) -> f64 {
    let slope = (dst_max - dst_min) / (src_max - src_min);
    slope * (src - src_min) + dst_min
}

/// One MISB "mapped" element: an integer of exactly `width` octets read at the field's
/// own domain, linearly scaled onto its stated range, with `error` (when the field has
/// one) read back as `None` rather than as a data point. Mirrors
/// `klvdata.common.bytes_to_float`, except that an item of any other width is `None`
/// -- carried raw by the caller -- rather than scaled by a domain that assumes the
/// fixed width (the module doc comment's "Scope").
fn mapped(
    item: &[u8],
    width: usize,
    signed: bool,
    domain: (i64, i64),
    range: (f64, f64),
) -> Option<f64> {
    mapped_with_error(item, width, signed, domain, range, None)
}

fn mapped_with_error(
    item: &[u8],
    width: usize,
    signed: bool,
    domain: (i64, i64),
    range: (f64, f64),
    error: Option<i64>,
) -> Option<f64> {
    if item.len() != width {
        return None;
    }
    let raw = read_be_i64(item, signed)?;
    if error == Some(raw) {
        return None;
    }
    // Every domain bound this decoder's tag table uses is within +-2^32, which an
    // f64's 52-bit mantissa represents exactly.
    #[allow(clippy::cast_precision_loss)]
    let (raw_f, min_f, max_f) = (raw as f64, domain.0 as f64, domain.1 as f64);
    Some(linear_map(raw_f, min_f, max_f, range.0, range.1))
}

fn as_string(item: &[u8]) -> Option<String> {
    std::str::from_utf8(item).ok().map(str::to_owned)
}

fn set_opt<T>(field: &mut Option<T>, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *field = Some(v);
            true
        }
        None => false,
    }
}

/// Applies one local-set item to `frame`. A tag this decoder does not know, or knows
/// but cannot decode at the width given, is carried raw rather than guessed at.
///
/// One match arm per tag makes this long in lines, not in complexity -- each arm is
/// the same three-part shape (domain, range, error sentinel) `misb0601.py`'s own
/// per-tag classes use, cited by tag number in `Misb0601Frame`'s field docs.
#[allow(clippy::too_many_lines)]
fn apply_tag(frame: &mut Misb0601Frame, tag: u8, item: &[u8]) {
    let recognized = match tag {
        1 => match item {
            [a, b] => set_opt(
                &mut frame.stated_checksum,
                Some(u16::from_be_bytes([*a, *b])),
            ),
            _ => false,
        },
        2 => match item {
            &[a, b, c, d, e, f, g, h] => set_opt(
                &mut frame.precision_time_stamp_us,
                Some(u64::from_be_bytes([a, b, c, d, e, f, g, h])),
            ),
            _ => false,
        },
        3 => set_opt(&mut frame.mission_id, as_string(item)),
        4 => set_opt(&mut frame.platform_tail_number, as_string(item)),
        5 => set_opt(
            &mut frame.platform_heading_deg,
            mapped(item, 2, false, (0, 65_535), (0.0, 360.0)),
        ),
        6 => set_opt(
            &mut frame.platform_pitch_deg,
            mapped_with_error(
                item,
                2,
                true,
                (-32_767, 32_767),
                (-20.0, 20.0),
                Some(-32_768),
            ),
        ),
        7 => set_opt(
            &mut frame.platform_roll_deg,
            mapped_with_error(
                item,
                2,
                true,
                (-32_767, 32_767),
                (-50.0, 50.0),
                Some(-32_768),
            ),
        ),
        10 => set_opt(&mut frame.platform_designation, as_string(item)),
        11 => set_opt(&mut frame.image_source_sensor, as_string(item)),
        13 => set_opt(
            &mut frame.sensor_latitude_deg,
            mapped_with_error(
                item,
                4,
                true,
                (-2_147_483_647, 2_147_483_647),
                (-90.0, 90.0),
                Some(-2_147_483_648),
            ),
        ),
        14 => set_opt(
            &mut frame.sensor_longitude_deg,
            mapped_with_error(
                item,
                4,
                true,
                (-2_147_483_647, 2_147_483_647),
                (-180.0, 180.0),
                Some(-2_147_483_648),
            ),
        ),
        15 => set_opt(
            &mut frame.sensor_true_altitude_m,
            mapped(item, 2, false, (0, 65_535), (-900.0, 19_000.0)),
        ),
        18 => set_opt(
            &mut frame.sensor_relative_azimuth_deg,
            mapped(item, 4, false, (0, 4_294_967_295), (0.0, 360.0)),
        ),
        19 => set_opt(
            &mut frame.sensor_relative_elevation_deg,
            mapped_with_error(
                item,
                4,
                true,
                (-2_147_483_647, 2_147_483_647),
                (-180.0, 180.0),
                Some(-2_147_483_648),
            ),
        ),
        20 => set_opt(
            &mut frame.sensor_relative_roll_deg,
            mapped(item, 4, false, (0, 4_294_967_295), (0.0, 360.0)),
        ),
        21 => set_opt(
            &mut frame.slant_range_m,
            mapped(item, 4, false, (0, 4_294_967_295), (0.0, 5_000_000.0)),
        ),
        23 => set_opt(
            &mut frame.frame_center_latitude_deg,
            mapped_with_error(
                item,
                4,
                true,
                (-2_147_483_647, 2_147_483_647),
                (-90.0, 90.0),
                Some(-2_147_483_648),
            ),
        ),
        24 => set_opt(
            &mut frame.frame_center_longitude_deg,
            mapped_with_error(
                item,
                4,
                true,
                (-2_147_483_647, 2_147_483_647),
                (-180.0, 180.0),
                Some(-2_147_483_648),
            ),
        ),
        25 => set_opt(
            &mut frame.frame_center_elevation_m,
            mapped(item, 2, false, (0, 65_535), (-900.0, 19_000.0)),
        ),
        65 => match item {
            [v] => set_opt(&mut frame.uas_lds_version, Some(*v)),
            _ => false,
        },
        _ => false,
    };
    if !recognized {
        frame.carried_raw.push((tag, item.to_vec()));
    }
}

fn decode_local_set(value: &[u8], full_frame: &[u8]) -> Result<Misb0601Frame, Misb0601Error> {
    let mut frame = Misb0601Frame::default();
    let mut pos = 0usize;
    while pos < value.len() {
        let tag = value[pos];
        pos += 1;
        let rest = value
            .get(pos..)
            .ok_or(Misb0601Error::LocalSetTruncated { tag })?;
        let (len, len_bytes) =
            read_ber_length(rest).map_err(|_| Misb0601Error::LocalSetTruncated { tag })?;
        pos += len_bytes;
        let item = value
            .get(pos..pos + len)
            .ok_or(Misb0601Error::LocalSetTruncated { tag })?;
        pos += len;
        apply_tag(&mut frame, tag, item);
    }
    frame.computed_checksum = packet_checksum(full_frame);
    Ok(frame)
}

/// Decodes one complete UAS Datalink LS frame from the front of `bytes`.
///
/// # Errors
///
/// [`Misb0601Error::KeyMismatch`] when `bytes` does not start with [`UDS_KEY`]:
/// either a different KLV set entirely, or a stream not yet resynchronized after a
/// corrupt frame. [`Misb0601Error::Truncated`] when the frame's own header promises
/// more bytes than `bytes` currently holds -- not a corrupt frame, a caller reading a
/// live stream buffers and retries once more bytes arrive.
/// [`Misb0601Error::BerLengthMalformed`] or [`Misb0601Error::LocalSetTruncated`] for a
/// frame whose own declared length is self-contradictory, which is always a corrupt
/// frame.
///
/// # Returns
///
/// The decoded frame and how many bytes it occupied, so a caller reading a
/// continuous stream slices `&bytes[consumed..]` for the next frame.
pub fn decode_frame(bytes: &[u8]) -> Result<(Misb0601Frame, usize), Misb0601Error> {
    if bytes.len() < 16 {
        return Err(Misb0601Error::Truncated {
            needed: 16,
            have: bytes.len(),
        });
    }
    if bytes[..16] != UDS_KEY[..] {
        return Err(Misb0601Error::KeyMismatch);
    }
    let (value_len, length_field_len) = read_ber_length(&bytes[16..])?;
    let header_len = 16 + length_field_len;
    let total_len = header_len + value_len;
    if bytes.len() < total_len {
        return Err(Misb0601Error::Truncated {
            needed: total_len,
            have: bytes.len(),
        });
    }
    let value = &bytes[header_len..total_len];
    let frame = decode_local_set(value, &bytes[..total_len])?;
    Ok((frame, total_len))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a well-formed frame from local-set items (each `(tag, value)`, using
    /// short-form BER lengths only, which every value below is short enough for),
    /// with a correct trailing checksum computed by this module's own
    /// `packet_checksum` -- a hand-built structural fixture in the sense
    /// `docs/design/external-standards.md` §1.8 already uses for ASTERIX ("hand-built
    /// records ... at the specification's own least significant bits"), not a
    /// substitute for the vendored corpus `misb0601_fixtures.rs` gates on.
    fn build_frame(items: &[(u8, &[u8])]) -> Vec<u8> {
        let mut value = Vec::new();
        for (tag, bytes) in items {
            value.push(*tag);
            value.push(u8::try_from(bytes.len()).expect("test values stay short-form"));
            value.extend_from_slice(bytes);
        }
        // Reserve the checksum element (tag 1, two bytes) before computing over the
        // whole packet, then patch its value in.
        value.push(1);
        value.push(2);
        let checksum_at = 16 + 1 + value.len(); // key + short-form outer length byte
        value.push(0);
        value.push(0);
        let mut frame = Vec::new();
        frame.extend_from_slice(&UDS_KEY);
        frame.push(u8::try_from(value.len()).expect("test frames stay short-form"));
        frame.extend_from_slice(&value);
        let cs = packet_checksum(&frame);
        let cs_bytes = cs.to_be_bytes();
        frame[checksum_at] = cs_bytes[0];
        frame[checksum_at + 1] = cs_bytes[1];
        frame
    }

    #[test]
    fn ber_length_short_form_is_the_byte_itself() {
        assert_eq!(read_ber_length(&[0x0A, 0xFF]), Ok((10, 1)));
    }

    #[test]
    fn ber_length_long_form_matches_the_fixtures_own_outer_length() {
        // The vendored fixture's own outer length bytes (testdata/misb/SOURCE.md):
        // 0x81 0xD2 -- one length byte follows, value 0xD2 = 210.
        assert_eq!(read_ber_length(&[0x81, 0xD2]), Ok((210, 2)));
    }

    #[test]
    fn a_zero_or_overlong_ber_count_is_malformed() {
        assert_eq!(
            read_ber_length(&[0x80]),
            Err(Misb0601Error::BerLengthMalformed)
        );
        assert_eq!(
            read_ber_length(&[0xFF; 9]),
            Err(Misb0601Error::BerLengthMalformed)
        );
    }

    #[test]
    fn the_heading_worked_example_matches_klvdatas_own_reading() {
        // Tag 5, raw bytes 0x71 0xC2, from the vendored fixture. klvdata's own
        // PlatformHeadingAngle(value).value on these bytes is 159.97436484321355.
        let v = mapped(&[0x71, 0xC2], 2, false, (0, 65_535), (0.0, 360.0));
        assert!((v.expect("present") - 159.974_364_843_213_55).abs() < 1e-9);
    }

    #[test]
    fn an_error_sentinel_reads_as_not_available_rather_than_a_value() {
        // Pitch's error sentinel is -2^15, encoded as the two bytes 0x80 0x00.
        assert_eq!(
            mapped_with_error(
                &[0x80, 0x00],
                2,
                true,
                (-32_767, 32_767),
                (-20.0, 20.0),
                Some(-32_768)
            ),
            None
        );
    }

    #[test]
    fn packet_checksum_sums_16_bit_words_and_excludes_its_own_trailing_pair() {
        // Hand-computed: words 0x0001 and 0x0002 sum to 0x0003; the trailing 0xFFFF
        // is the checksum slot this function always excludes, regardless of its
        // value.
        let bytes = [0x00, 0x01, 0x00, 0x02, 0xFF, 0xFF];
        assert_eq!(packet_checksum(&bytes), 0x0003);
    }

    #[test]
    fn packet_checksum_folds_a_trailing_odd_byte_as_a_high_byte() {
        // Three bytes precede the excluded pair: 0x00 0x01 (word 0x0001) then a lone
        // 0x02, folded as 0x02 << 8 = 0x0200. Total 0x0201.
        let bytes = [0x00, 0x01, 0x02, 0xAA, 0xBB];
        assert_eq!(packet_checksum(&bytes), 0x0201);
    }

    #[test]
    fn a_self_consistent_frame_decodes_and_its_checksum_validates() {
        let bytes = build_frame(&[(65, &[6]), (10, b"Predator")]);
        let (frame, consumed) = decode_frame(&bytes).expect("decodes");
        assert_eq!(consumed, bytes.len());
        assert_eq!(frame.uas_lds_version, Some(6));
        assert_eq!(frame.platform_designation.as_deref(), Some("Predator"));
        assert!(frame.checksum_valid());
    }

    #[test]
    fn an_unknown_tag_is_carried_raw_and_does_not_stop_the_frame() {
        let bytes = build_frame(&[(65, &[6]), (99, &[0xDE, 0xAD])]);
        let (frame, _) = decode_frame(&bytes).expect("decodes");
        assert_eq!(frame.uas_lds_version, Some(6));
        assert_eq!(frame.carried_raw, vec![(99u8, vec![0xDE, 0xAD])]);
    }

    #[test]
    fn a_frame_missing_bytes_asks_for_more_rather_than_erroring() {
        let bytes = build_frame(&[(65, &[6])]);
        let short = &bytes[..bytes.len() - 3];
        assert_eq!(
            decode_frame(short),
            Err(Misb0601Error::Truncated {
                needed: bytes.len(),
                have: short.len(),
            })
        );
    }

    #[test]
    fn bytes_with_no_key_at_all_are_a_key_mismatch() {
        assert_eq!(decode_frame(&[0u8; 32]), Err(Misb0601Error::KeyMismatch));
    }

    #[test]
    fn find_next_key_resynchronizes_past_garbage() {
        let mut buf = vec![0xFFu8; 5];
        buf.extend_from_slice(&UDS_KEY);
        buf.extend_from_slice(&[0, 0]);
        assert_eq!(find_next_key(&buf, 0), Some(5));
        assert_eq!(find_next_key(&buf, 6), None);
    }

    /// The width rule the module doc comment promises (2026-09-09): a known tag at a
    /// width its table entry does not fix is carried raw, never scaled by a domain
    /// that assumed the fixed width. Before this, a 2-octet Sensor Latitude decoded as
    /// a value near zero degrees.
    #[test]
    fn a_known_tag_at_the_wrong_width_is_carried_raw_not_scaled() {
        // Tag 13 at 2 octets instead of 4, Tag 5 at its correct 2 octets beside it.
        let value: [u8; 8] = [13, 2, 0x40, 0x00, 5, 2, 0x80, 0x00];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&UDS_KEY);
        bytes.push(u8::try_from(value.len()).expect("short-form"));
        bytes.extend_from_slice(&value);
        let (frame, consumed) = decode_frame(&bytes).expect("frames");
        assert_eq!(consumed, bytes.len());
        assert_eq!(
            frame.sensor_latitude_deg, None,
            "a 2-octet latitude is not a latitude"
        );
        assert_eq!(frame.carried_raw, vec![(13, vec![0x40, 0x00])]);
        let heading = frame
            .platform_heading_deg
            .expect("the correctly sized tag decodes");
        assert!((heading - 360.0 * 32_768.0 / 65_535.0).abs() < 1e-9);
    }
}
