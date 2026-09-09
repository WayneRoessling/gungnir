// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! EUROCONTROL ASTERIX Category 205, Radio Direction Finder Reports.
//!
//! Built to **EUROCONTROL-SPEC-0149-31, ASTERIX Part 31, Category 205, edition 1.0**
//! (17 March 2020, ISBN 978-2-87497-028-3), fetched and read in full from
//! `https://www.eurocontrol.int/sites/default/files/2020-03/eurocontrol-cat205p31ed10.pdf`,
//! the same free, no-registration terms `docs/design/external-standards.md` §1 already
//! documents for the rest of the ASTERIX family; the survey pinning this edition is
//! §9 of that note. Framing is Part I (`super`); see "Part I edition" below for the
//! one nuance that survey did not have to resolve for Category 048 or 034.
//!
//! **Part I edition.** Category 205 edition 1.0 itself cites Part I edition **2.4**
//! (24 October 2016) in its own bibliography (§2.2), not the edition 3.1 this crate's
//! shared framing (`super`) is built to and that `docs/design/external-standards.md`
//! §1.6 already pinned for Categories 048 and 034. Comparing this specification's own
//! block diagram (§4.4: `CAT | LEN | FSPEC | Data Item...`) against what `super`
//! implements finds no discrepancy in the data block, record or FSPEC structure this
//! module relies on. A difference elsewhere in Part I between 2.4 and 3.1 would not be
//! caught by that comparison and has not been separately checked; recorded here rather
//! than silently assumed away, per this workspace's rule against confidently-wrong
//! documentation.
//!
//! **Cross-checked, not authored from, `asterix-specs`.** The community transcription
//! at `https://zoranbosnjak.github.io/asterix-specs/specs/cat205/cats/cat1.0/` carries
//! edition 1.0's UAP (data item order and lengths) in machine-readable form; it was
//! fetched and its field order compared against Table 3 of the primary PDF above, which
//! is the authority `docs/design/external-standards.md` §1.4 already establishes for
//! this family. They agree.
//!
//! **One discrepancy inside the primary text itself, recorded so a later reader does
//! not "correct" this decoder the wrong way (2026-09-09, found in review before the
//! adapter was signed).** Edition 1.0's own Table 1, its summary of least significant
//! bits, lists I205/070 and I205/080 at 0.1 degrees; the item definitions §5.2.8 and
//! §5.2.9 both state `LSB = 0.01deg`, "in clock-wise notation, starting with 0 degrees
//! for the geographical North", with `0.00 deg <= THETA < 360.00 deg`, and
//! `asterix-specs`' transcription carries 1/100. The item definitions govern; this
//! decoder follows them, and the same review added the range refusals below that
//! those definitions state and nothing downstream (`gungnir_ingest::gateway`'s
//! validation bounds a bearing's variance, not its angle) would otherwise enforce.
//!
//! **What this module does and does not decode.** The lossless layer
//! ([`decode_records`]) types every standard-UAP item Table 3 defines except three the
//! specification itself calls out (§4.6) as "implementation dependent" -- I205/100
//! (Quality of Measurement), I205/120 (Contributing Sensors) and I205/170 (Sensor
//! Identification) -- whose bit or octet meanings the specification hands to each
//! deployment's own Interface Control Document rather than fixing itself; those are
//! carried raw and listed in [`Record::carried_raw`], exactly as Category 048 carries
//! items this build does not interpret. The mapping layer ([`AsterixCat205Codec`])
//! turns a **Sensor Data Report** or a **System Bearing Report** (message types 5 and
//! 2 -- the two that carry a bearing) into `Measurement::Bearing`, per the same
//! bearing-only decision (DN-27, `docs/design/external-standards.md` §7.3) that a
//! direction finder is one of the three motivating feeds for.
//!
//! **What it deliberately does not map.** A **System Position Report** (type 1) or its
//! conflicting-transmission counterpart (type 3) carries the RDF *processing system's*
//! own already-resolved position, in WGS-84 (I205/050) or in a Cartesian frame relative
//! to "an agreed System Reference Point" the message does not itself name (I205/060).
//! Turning either into this system's local ENU frame needs `gungnir-geo`, which this
//! crate may not depend on (`ARCHITECTURE.md` §7, the same constraint `super::RadarSite`
//! already documents), and trusting I205/060's frame to already be this deployment's
//! local frame would be exactly the unstated-convention error DN-27 §4 warns against.
//! Both message types decode losslessly in [`Record`] and map to
//! [`Mapped::NotADetection`]. Category 205 records nothing that STANAG-4676-style
//! tracking would need beyond that; there is no separate service-message boundary here
//! the way Category 034 needs one, because Category 205 has no status/timing message
//! type of its own (its scope note, §1.1, says so: "status information of RDF
//! receivers... should be sent using ASTERIX Category 025").
//!
//! **The angular error, restated because DN-27 makes it the whole question.** No
//! message type in this category carries a stated *angular* error for a bearing:
//! I205/110 (Estimated Uncertainty) is a *positional* radius in metres and Table 2
//! marks it "never present" for the one message type that pairs a bearing with a
//! position (type 2), and I205/100 (Quality of Measurement) is the implementation
//! dependent item above -- its bits carry no fixed unit at all (§4.6: "the actual
//! meanings of the bits are application dependent"). So this codec cannot honestly read
//! a variance off the wire, and it does not invent one: [`DfSite::azimuth_sigma_rad`] is
//! the deployment's own stated accuracy for that direction finder, sourced from its
//! Interface Control Document exactly as §4.6 anticipates, supplied by the caller the
//! same way `RadarSite::origin_enu_m` is. A site with no stated accuracy is not
//! configured at all, and its reports are [`crate::InteropError::UnknownRadar`], the
//! same as a report from a radar no `RadarSite` names. This mirrors
//! `gungnir_ingest::adapters::sapient`'s `range_bearing`, which refuses a bearing with
//! no stated azimuth error rather than defaulting one, and
//! `gungnir_ingest::gateway::validate_detection`, which refuses a non-positive one
//! regardless.

use super::{data_blocks, sign_extend, Cursor, DataBlock, Fspec};
use crate::asterix::cat048::DataSource;
use crate::{DetectionCodec, InteropError};
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId, TrackView};

/// The codec's name in the schema catalog.
pub const CODEC_NAME: &str = "asterix.cat205";
/// The Category 205 edition this decoder is built to.
pub const EDITION: &str = "1.0";
/// The Part I edition Category 205 edition 1.0 itself cites (module documentation
/// explains why this differs from the 3.1 the shared framing implements).
pub const PART1_EDITION_CITED: &str = "2.4";

const CATEGORY: u8 = 205;
const MAX_FRN: usize = 28;
/// I205/050 and I205/130: 180 / 2^25 degrees per count (§5.2.6).
const WGS84_LSB_DEG: f64 = 180.0 / 33_554_432.0;

// ---------------------------------------------------------------------------
// Typed record
// ---------------------------------------------------------------------------

/// I205/000 (§5.2.1): the five standardised transactions edition 1.0 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// The RDF processing system's own resolved position of the transmitter.
    SystemPositionReport,
    /// A bearing, paired with the position of the sensor it was measured from.
    SystemBearingReport,
    /// A second, conflicting transmitter's resolved position.
    ConflictingSystemPositionReport,
    SystemDetectionEndReport,
    /// One sensor's own raw bearing, ungrouped by the processing system.
    SensorDataReport,
    /// A code edition 1.0 does not define.
    Undefined(u8),
}

impl MessageType {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::SystemPositionReport,
            2 => Self::SystemBearingReport,
            3 => Self::ConflictingSystemPositionReport,
            4 => Self::SystemDetectionEndReport,
            5 => Self::SensorDataReport,
            other => Self::Undefined(other),
        }
    }
}

/// I205/050 and I205/130 (§5.2.6, §5.2.14): a calculated WGS-84 position, decimal
/// degrees decoded from the specification's own two's-complement fixed point. Not
/// converted to a local frame here -- see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WgsPosition {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}

/// I205/060 and I205/140 (§5.2.7, §5.2.15): a position relative to "an agreed System
/// Reference Point" the specification does not itself name. Not this deployment's
/// local frame unless a deployment says so, which nothing here assumes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CartesianXY {
    pub x_m: f64,
    pub y_m: f64,
}

/// A data item this build carries without interpreting: I205/100, /170 (both marked
/// "implementation dependent" by edition 1.0 §4.6) and the SP field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawItem {
    /// The item's name in the specification, for example `I205/100`.
    pub item: &'static str,
    pub octets: Vec<u8>,
}

/// One Category 205 record, every UAP item accounted for (Table 3, FRN 1 to 22; FRN 23
/// to 28 are "Reserved for Future Use" and a set FSPEC bit for one of them is a
/// [`InteropError::Malformed`], the same treatment as an FRN past [`MAX_FRN`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// Absolute offset of the record's FSPEC in the input.
    pub offset: usize,
    pub data_source: Option<DataSource>,
    /// I205/015: allocated by the system; no further structure to interpret.
    pub service_id: Option<u8>,
    pub message_type: Option<MessageType>,
    /// I205/030: seconds since midnight UTC, resolution 1/128 s.
    pub time_of_day_s: Option<f64>,
    /// I205/040: 0 to 255, cyclic.
    pub report_number: Option<u8>,
    /// I205/090: the seven raw ASCII characters, unmodified (a byte outside printable
    /// ASCII becomes `?`, never a panic). Not parsed as a frequency: the specification
    /// says outright that "this channel name is not identical with the actual physical
    /// frequency."
    pub radio_channel_name: Option<String>,
    pub position_wgs84: Option<WgsPosition>,
    pub position_cartesian: Option<CartesianXY>,
    /// I205/070: degrees, clockwise from geographic north (§5.2.8), ordinarily paired
    /// with `position_wgs84`.
    pub local_bearing_deg: Option<f64>,
    /// I205/080: degrees, clockwise from geographic north -- the same angular
    /// reference as I205/070 (§5.2.9's own wording is identical) -- ordinarily paired
    /// with `position_cartesian`.
    pub system_bearing_deg: Option<f64>,
    /// I205/110: metres, the radius the transmitter is expected to be within. Never
    /// present for a System Bearing Report (Table 2); decoded here for the position
    /// message types this build does not map to detections.
    pub estimated_uncertainty_m: Option<f64>,
    /// I205/120: raw sensor identifiers; §4.6 leaves their meaning to the deployment's
    /// own Interface Control Document.
    pub contributing_sensors: Vec<u8>,
    pub conflicting_position_wgs84: Option<WgsPosition>,
    pub conflicting_position_cartesian: Option<CartesianXY>,
    pub conflicting_estimated_uncertainty_m: Option<f64>,
    pub track_number: Option<u16>,
    /// I205/180: dBµV, LSB 0.01.
    pub signal_level_dbuv: Option<f64>,
    /// I205/190: 0 to 255, 255 the best quality (§5.2.20 states the whole scale itself,
    /// unlike I205/100, so this is typed rather than carried raw).
    pub signal_quality: Option<u8>,
    /// I205/200: degrees, -90 to 90. Decoded here; not mapped to `elevation_rad` (see
    /// the module documentation and [`AsterixCat205Codec::map`]).
    pub signal_elevation_deg: Option<f64>,
    pub carried_raw: Vec<RawItem>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Decode every Category 205 record in `bytes`, losslessly. Every block must be
/// Category 205; see `cat048::decode_records` for why a live feed is split with
/// [`super::data_blocks`] first. Unlike Category 048 and 034, each block holds exactly
/// one record (see [`decode_block`]).
pub fn decode_records(bytes: &[u8]) -> Result<Vec<Record>, InteropError> {
    let mut records = Vec::new();
    for block in data_blocks(CODEC_NAME, bytes)? {
        records.push(decode_block(&block)?);
    }
    Ok(records)
}

/// Decode the one record of a data block that must be Category 205.
///
/// Unlike Category 048 and 034, Category 205 does not support blocking: "Blocking of
/// multiple records sharing a single CAT and LEN field pair is not supported for
/// Category 205 records" (edition 1.0 §4.4). So this returns one [`Record`] rather than
/// a `Vec`, and octets left over after it decodes are a malformed block rather than a
/// second record silently ignored or silently parsed.
pub fn decode_block(block: &DataBlock<'_>) -> Result<Record, InteropError> {
    if block.category != CATEGORY {
        return Err(InteropError::WrongCategory {
            codec: CODEC_NAME,
            expected: CATEGORY,
            found: block.category,
            offset: block.offset,
        });
    }
    let mut cur = Cursor::new(CODEC_NAME, block.payload, block.offset + 3);
    let record = parse_record(&mut cur)?;
    if !cur.is_empty() {
        return Err(cur.error(format!(
            "{} octets remain after one record; edition {EDITION} §4.4 does not \
             support blocking multiple Category 205 records in one data block",
            cur.remaining()
        )));
    }
    Ok(record)
}

/// One match arm per FRN of the standard UAP, in Table 3 order, so the function reads
/// against the table; splitting it would separate the table from itself (the same
/// choice `cat048::parse_record` makes and explains).
#[allow(clippy::too_many_lines)]
fn parse_record(cur: &mut Cursor<'_>) -> Result<Record, InteropError> {
    let mut r = Record {
        offset: cur.offset(),
        ..Record::default()
    };
    let fspec = Fspec::read(cur)?;
    if let Some(frn) = fspec.beyond(MAX_FRN) {
        return Err(cur.error(format!(
            "FSPEC flags FRN {frn}, which the edition {EDITION} UAP does not define"
        )));
    }
    for frn in 1..=MAX_FRN {
        if !fspec.has(frn) {
            continue;
        }
        match frn {
            1 => {
                let b = cur.take(2, "I205/010")?;
                r.data_source = Some(DataSource {
                    sac: b[0],
                    sic: b[1],
                });
            }
            2 => r.service_id = Some(cur.u8("I205/015")?),
            3 => r.message_type = Some(MessageType::from_code(cur.u8("I205/000")?)),
            4 => r.time_of_day_s = Some(f64::from(cur.u24("I205/030")?) / 128.0),
            5 => r.report_number = Some(cur.u8("I205/040")?),
            6 => r.radio_channel_name = Some(parse_radio_channel_name(cur)?),
            7 => r.position_wgs84 = Some(parse_wgs84(cur, "I205/050")?),
            8 => r.position_cartesian = Some(parse_cartesian(cur, "I205/060")?),
            9 => r.local_bearing_deg = Some(parse_bearing(cur, "I205/070")?),
            10 => r.system_bearing_deg = Some(parse_bearing(cur, "I205/080")?),
            11 => raw(&mut r, "I205/100", cur.take(1, "I205/100")?),
            12 => r.estimated_uncertainty_m = Some(f64::from(cur.u8("I205/110")?) * 100.0),
            13 => {
                let rep = usize::from(cur.u8("I205/120 REP")?);
                for _ in 0..rep {
                    let ident = cur.u8("I205/120 IDENT")?;
                    r.contributing_sensors.push(ident);
                }
            }
            14 => r.conflicting_position_wgs84 = Some(parse_wgs84(cur, "I205/130")?),
            15 => r.conflicting_position_cartesian = Some(parse_cartesian(cur, "I205/140")?),
            16 => {
                r.conflicting_estimated_uncertainty_m =
                    Some(f64::from(cur.u8("I205/150")?) * 100.0);
            }
            17 => r.track_number = Some(cur.u16("I205/160")?),
            18 => raw(&mut r, "I205/170", cur.take(1, "I205/170")?),
            19 => r.signal_level_dbuv = Some(f64::from(cur.i16("I205/180")?) * 0.01),
            20 => r.signal_quality = Some(cur.u8("I205/190")?),
            21 => r.signal_elevation_deg = Some(parse_signal_elevation(cur)?),
            22 => raw(&mut r, "I205/SP", cur.explicit("I205/SP")?),
            23..=28 => {
                return Err(cur.error(format!(
                    "FRN {frn} is \"Reserved for Future Use\" in edition {EDITION}'s \
                     UAP (Table 3) and flagged present"
                )));
            }
            _ => unreachable!("FRN range is bounded by MAX_FRN"),
        }
    }
    Ok(r)
}

fn raw(r: &mut Record, item: &'static str, octets: &[u8]) {
    r.carried_raw.push(RawItem {
        item,
        octets: octets.to_vec(),
    });
}

/// I205/070 and I205/080 (§5.2.8, §5.2.9): an unsigned 16-bit count at 0.01 degrees,
/// clockwise from geographical north, which the specification bounds to
/// `0.00 <= THETA < 360.00`. A count at or past 36 000 is outside what the edition
/// defines and is refused as malformed rather than wrapped or passed on -- the
/// gateway's own validation bounds a bearing's variance, not its angle, so nothing
/// downstream would catch it (2026-09-09).
fn parse_bearing(cur: &mut Cursor<'_>, item: &'static str) -> Result<f64, InteropError> {
    let raw = cur.u16(item)?;
    if raw >= 36_000 {
        return Err(cur.error(format!(
            "{item} = {raw} x 0.01 deg is outside the 0 <= THETA < 360 deg range edition \
             {EDITION} states"
        )));
    }
    Ok(f64::from(raw) * 0.01)
}

/// I205/200 (§5.2.21): a signed 16-bit count at 0.01 degrees, which the specification
/// bounds to `-90.00 <= ELEVATION <= 90.00`; refused outside it for the same reason as
/// [`parse_bearing`].
fn parse_signal_elevation(cur: &mut Cursor<'_>) -> Result<f64, InteropError> {
    let raw = cur.i16("I205/200")?;
    if !(-9_000..=9_000).contains(&raw) {
        return Err(cur.error(format!(
            "I205/200 = {raw} x 0.01 deg is outside the -90 <= ELEVATION <= 90 deg range \
             edition {EDITION} states"
        )));
    }
    Ok(f64::from(raw) * 0.01)
}

/// I205/050 and I205/130 (§5.2.6, §5.2.14): two 32-bit two's complement fields, LSB
/// 180/2^25 degrees each.
fn parse_wgs84(cur: &mut Cursor<'_>, item: &str) -> Result<WgsPosition, InteropError> {
    let lat = cur.i32(&format!("{item} latitude"))?;
    let lon = cur.i32(&format!("{item} longitude"))?;
    Ok(WgsPosition {
        latitude_deg: f64::from(lat) * WGS84_LSB_DEG,
        longitude_deg: f64::from(lon) * WGS84_LSB_DEG,
    })
}

/// I205/060 and I205/140 (§5.2.7, §5.2.15): two 24-bit two's complement fields, LSB
/// 0.5 m each.
fn parse_cartesian(cur: &mut Cursor<'_>, item: &str) -> Result<CartesianXY, InteropError> {
    let x = cur.u24(&format!("{item} X"))?;
    let y = cur.u24(&format!("{item} Y"))?;
    Ok(CartesianXY {
        x_m: f64::from(sign_extend(x, 24)) * 0.5,
        y_m: f64::from(sign_extend(y, 24)) * 0.5,
    })
}

/// I205/090 (§5.2.10): seven octets of ASCII digits or a decimal point. A byte outside
/// printable ASCII is not a character the coding defines and is rendered as `?` rather
/// than dropped or turned into a panic (the same discipline `cat048::decode_ia5_6bit`
/// applies to I048/240).
fn parse_radio_channel_name(cur: &mut Cursor<'_>) -> Result<String, InteropError> {
    let b = cur.take(7, "I205/090")?;
    Ok(b.iter()
        .map(|&c| {
            if (0x20..=0x7E).contains(&c) {
                char::from(c)
            } else {
                '?'
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Mapping to the model
// ---------------------------------------------------------------------------

/// One radio direction finder a Category 205 decoder accepts bearings from.
///
/// The frame anchor and the stated accuracy both come from the caller, not from here,
/// for the same reason `RadarSite`'s anchor does (`super::RadarSite`): this crate does
/// not depend on `gungnir-geo`, and -- the reason unique to this category -- edition
/// 1.0 gives no wire item this codec could honestly read an angular error from (module
/// documentation above; `docs/design/DN-27-bearing-only-detections.md` §4). A site with
/// no stated accuracy is simply not configured, the same as a radar with no `RadarSite`.
///
/// `origin_enu_m` is carried for the same reason `RadarSite`'s is -- one place per
/// SAC/SIC for a deployment's site registry to state where its sensor stands -- but
/// `Measurement::Bearing` has no position field, so [`AsterixCat205Codec::map`] does not
/// read it today. DN-27's own words on this: nothing in this workspace yet resolves a
/// bearing-emitting sensor's position for the tracking pipeline, and this codec does
/// not attempt to be the first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DfSite {
    pub sac: u8,
    pub sic: u8,
    pub sensor: SensorId,
    /// The antenna's position in the local ENU frame, metres.
    pub origin_enu_m: [f64; 3],
    /// The direction finder's own stated one-sigma bearing accuracy, radians, from its
    /// Interface Control Document. **Never invented by this codec**: a bearing without
    /// a real stated error is refused rather than defaulted, matching
    /// `gungnir_ingest::adapters::sapient`'s `range_bearing` and the gateway's own
    /// validation rule.
    pub azimuth_sigma_rad: f64,
}

/// What one record became.
#[derive(Debug, Clone, PartialEq)]
// A `Mapped` is transient -- one per record, mapped and matched at once -- so the size
// gap between a detection and a reason is not a cost worth a box, the same call
// `cat048::Mapped` already makes and explains.
#[allow(clippy::large_enum_variant)]
pub enum Mapped {
    Detection(DetectionView),
    /// The record is valid Category 205 but is not an observation this build maps:
    /// the reason says why.
    NotADetection(&'static str),
}

/// Category 205 decoder configured with the direction finders it may attribute
/// bearings to.
///
/// A record whose SAC/SIC is not configured is an [`InteropError::UnknownRadar`], not
/// a bearing with a made-up sensor or a made-up accuracy. The `Default` codec knows no
/// sites, so it decodes records losslessly ([`decode_records`]) and attributes none.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AsterixCat205Codec {
    sites: Vec<DfSite>,
}

impl AsterixCat205Codec {
    pub fn new(sites: Vec<DfSite>) -> Self {
        Self { sites }
    }

    pub fn sites(&self) -> &[DfSite] {
        &self.sites
    }

    fn site(&self, source: DataSource) -> Option<&DfSite> {
        self.sites
            .iter()
            .find(|s| s.sac == source.sac && s.sic == source.sic)
    }

    /// Map one record. `receipt_time` is when this system received the block; it also
    /// supplies the date I205/030 lacks, via `cat048::source_time` (Category 205's
    /// time-of-day item has the identical shape: seconds since midnight UTC at 1/128 s
    /// resolution, so this build reuses that fold rather than writing a second copy).
    pub fn map(&self, record: &Record, receipt_time: MissionTime) -> Result<Mapped, InteropError> {
        let source = record.data_source.ok_or_else(|| InteropError::Malformed {
            codec: CODEC_NAME,
            offset: record.offset,
            reason: "I205/010 absent; a report with no data source cannot be attributed".into(),
        })?;
        let site = self.site(source).ok_or(InteropError::UnknownRadar {
            codec: CODEC_NAME,
            sac: source.sac,
            sic: source.sic,
        })?;

        let azimuth_deg = match record.message_type {
            Some(MessageType::SensorDataReport) => record.local_bearing_deg,
            // Both bearing items share one angular reference (module documentation
            // above), so either stands in for the other when a sender used only one
            // position/bearing pairing of the two Table 2 allows.
            Some(MessageType::SystemBearingReport) => {
                record.local_bearing_deg.or(record.system_bearing_deg)
            }
            Some(
                MessageType::SystemPositionReport | MessageType::ConflictingSystemPositionReport,
            ) => {
                return Ok(Mapped::NotADetection(
                    "I205/000 is a System Position Report: the processing system's own \
                     already-resolved position, which this crate may not convert to the \
                     local frame (ARCHITECTURE.md §7); only message types 2 and 5 (a \
                     bearing) map to detections",
                ));
            }
            Some(MessageType::SystemDetectionEndReport) => {
                return Ok(Mapped::NotADetection(
                    "I205/000 = System Detection End Report: no bearing or position; \
                     not an observation",
                ));
            }
            Some(MessageType::Undefined(_)) | None => {
                return Ok(Mapped::NotADetection(
                    "I205/000 message type is absent or not one edition 1.0 defines",
                ));
            }
        };
        let Some(azimuth_deg) = azimuth_deg else {
            return Ok(Mapped::NotADetection(
                "no bearing item (I205/070 or I205/080) present; not an observation",
            ));
        };

        let mut losses: Vec<&'static str> = Vec::new();
        let (source_time, time_loss) =
            super::cat048::source_time(record.time_of_day_s, receipt_time);
        losses.extend(time_loss);

        // I205/200 has no companion angular error anywhere in this category, and the
        // gateway refuses an elevation whose error is unstated rather than defaulting
        // one (DN-27 §4). Dropped rather than refusing the whole report, because the
        // azimuth is still a measurement -- the same choice
        // `gungnir_ingest::adapters::sapient::range_bearing` makes for the identical
        // situation.
        if record.signal_elevation_deg.is_some() {
            losses.push(
                "I205/200 signal elevation has no stated error anywhere in this \
                 category; dropped, azimuth retained",
            );
        }

        let measurement = gungnir_model::Measurement::Bearing {
            azimuth_rad: azimuth_deg.to_radians(),
            elevation_rad: None,
            azimuth_variance_rad2: site.azimuth_sigma_rad * site.azimuth_sigma_rad,
            elevation_variance_rad2: None,
        };
        Ok(Mapped::Detection(DetectionView {
            sensor: site.sensor,
            source_time,
            receipt_time,
            measurement,
            provenance: Provenance {
                source_sensor_ids: vec![site.sensor.0],
                calibration_baseline_version: None,
                algorithm_version: format!("{CODEC_NAME}/ed{EDITION}"),
                peer: None,
                conversion_loss: if losses.is_empty() {
                    None
                } else {
                    Some(losses.join("; "))
                },
                authentication: gungnir_model::SourceAuthentication::default(),
            },
        }))
    }
}

impl DetectionCodec for AsterixCat205Codec {
    fn name(&self) -> &'static str {
        CODEC_NAME
    }

    /// Every record that is a bearing this build maps, in wire order. Records that are
    /// valid Category 205 but not one of the two bearing message types are left out;
    /// use [`decode_records`] and [`AsterixCat205Codec::map`] to see them and why.
    fn decode(
        &self,
        bytes: &[u8],
        receipt_time: MissionTime,
    ) -> Result<Vec<DetectionView>, InteropError> {
        let mut out = Vec::new();
        for record in decode_records(bytes)? {
            if let Mapped::Detection(d) = self.map(&record, receipt_time)? {
                out.push(d);
            }
        }
        Ok(out)
    }

    /// Not implemented: Gungnir is the receiving side of this interface, the same
    /// reason `AsterixCat048Codec::encode` gives.
    fn encode(&self, _tracks: &[TrackView]) -> Result<Vec<u8>, InteropError> {
        Err(InteropError::NotImplemented(CODEC_NAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One record, message type 5 (Sensor Data Report): FSPEC flags FRN 1, 3, 4, 5, 6,
    /// 9, 19, 20, 21 -- `0xBD, 0x41, 0x0E`. SAC 99 SIC 1, time 12:00:00
    /// (43 200 s x 128 = `0x54_6000`, the same constant `cat048`'s own hand-built
    /// fixture uses), report number 1, channel "121.500", local bearing 45.00 deg
    /// (4500 x 0.01 = `0x1194`), signal level 55.00 dBuV (5500 x 0.01 = `0x157C`),
    /// signal quality 200, signal elevation 12.50 deg (1250 x 0.01 = `0x04E2`).
    fn hand_built_block() -> Vec<u8> {
        let mut b = vec![0xCD, 0x00, 0x00]; // CAT = 205, LEN patched below
        b.push(0xBD); // FSPEC octet 1: FRN 1,3,4,5,6 + FX
        b.push(0x41); // FSPEC octet 2: FRN 9 + FX
        b.push(0x0E); // FSPEC octet 3: FRN 19,20,21, no FX
        b.extend_from_slice(&[99, 1]); // I205/010
        b.push(0x05); // I205/000 = 5
        b.extend_from_slice(&[0x54, 0x60, 0x00]); // I205/030
        b.push(0x01); // I205/040
        b.extend_from_slice(b"121.500"); // I205/090, 7 octets
        b.extend_from_slice(&[0x11, 0x94]); // I205/070 = 4500
        b.extend_from_slice(&[0x15, 0x7C]); // I205/180 = 5500
        b.push(200); // I205/190
        b.extend_from_slice(&[0x04, 0xE2]); // I205/200 = 1250
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());
        b
    }

    fn site() -> DfSite {
        DfSite {
            sac: 99,
            sic: 1,
            sensor: SensorId(11),
            origin_enu_m: [10.0, 20.0, 5.0],
            azimuth_sigma_rad: 2.0_f64.to_radians(),
        }
    }

    #[test]
    fn decodes_hand_built_record_to_specified_lsbs() {
        let recs = decode_records(&hand_built_block()).expect("decodes");
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.data_source, Some(DataSource { sac: 99, sic: 1 }));
        assert_eq!(r.message_type, Some(MessageType::SensorDataReport));
        assert!((r.time_of_day_s.expect("tod") - 43_200.0).abs() < 1e-9);
        assert_eq!(r.report_number, Some(1));
        assert_eq!(r.radio_channel_name.as_deref(), Some("121.500"));
        assert!((r.local_bearing_deg.expect("bearing") - 45.0).abs() < 1e-9);
        assert!((r.signal_level_dbuv.expect("level") - 55.0).abs() < 1e-9);
        assert_eq!(r.signal_quality, Some(200));
        assert!((r.signal_elevation_deg.expect("elev") - 12.5).abs() < 1e-9);
        assert!(r.carried_raw.is_empty());
        assert!(r.position_wgs84.is_none() && r.position_cartesian.is_none());
    }

    #[test]
    fn maps_sensor_data_report_to_a_bearing_with_the_configured_accuracy() {
        let codec = AsterixCat205Codec::new(vec![site()]);
        let receipt = MissionTime(20_000.0 * 86_400.0 + 43_205.0);
        let dets = codec.decode(&hand_built_block(), receipt).expect("decodes");
        assert_eq!(dets.len(), 1);
        let d = &dets[0];
        assert_eq!(d.sensor, SensorId(11));
        assert!((d.source_time.0 - (20_000.0 * 86_400.0 + 43_200.0)).abs() < 1e-6);
        match d.measurement {
            gungnir_model::Measurement::Bearing {
                azimuth_rad,
                elevation_rad,
                azimuth_variance_rad2,
                elevation_variance_rad2,
            } => {
                assert!((azimuth_rad - 45.0_f64.to_radians()).abs() < 1e-9);
                // I205/200 is decoded (previous test) but never reaches the
                // measurement: no companion error exists to pair it with.
                assert!(elevation_rad.is_none());
                assert!(elevation_variance_rad2.is_none());
                let sigma = 2.0_f64.to_radians();
                assert!((azimuth_variance_rad2 - sigma * sigma).abs() < 1e-18);
            }
            ref other => panic!("expected a bearing, got {other:?}"),
        }
        assert!(d.measurement.is_finite());
        assert!(d.measurement.position_enu().is_none());
        let loss = d
            .provenance
            .conversion_loss
            .as_deref()
            .expect("elevation drop recorded");
        assert!(loss.contains("signal elevation"));
        assert_eq!(d.provenance.algorithm_version, "asterix.cat205/ed1.0");
    }

    #[test]
    fn unknown_site_is_an_error_not_a_guessed_accuracy() {
        let codec = AsterixCat205Codec::default();
        assert!(matches!(
            codec.decode(&hand_built_block(), MissionTime(0.0)),
            Err(InteropError::UnknownRadar {
                sac: 99,
                sic: 1,
                ..
            })
        ));
    }

    #[test]
    fn system_position_reports_are_not_mapped_but_decode_losslessly() {
        // Same record, message type 1 (System Position Report) instead of 5, with a
        // WGS-84 position (FRN 7) in place of the sensor-report fields; FSPEC widened
        // to flag FRN 1, 3, 4, 7 only.
        let mut b = vec![0xCD, 0x00, 0x00];
        b.push(0b1011_0010); // FRN 1, 3, 4, 7 (bit2 = FRN7), FX clear
        b.extend_from_slice(&[25, 210]);
        b.push(0x01); // message type 1
        b.extend_from_slice(&[0x54, 0x60, 0x00]);
        // I205/050: two arbitrary 32-bit two's complement counts, LSB 180/2^25 degrees
        // each (§5.2.6). Chosen rather than derived from a round degree value, since
        // the LSB does not divide one evenly; the expected value below is computed the
        // same way the specification's own rule reads the count, not inverted from it.
        let raw_lat: i32 = 1_864_135;
        let raw_lon: i32 = -3_728_270;
        b.extend_from_slice(&raw_lat.to_be_bytes());
        b.extend_from_slice(&raw_lon.to_be_bytes());
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());

        let recs = decode_records(&b).expect("decodes");
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.message_type, Some(MessageType::SystemPositionReport));
        let pos = r.position_wgs84.expect("I205/050 present");
        assert!((pos.latitude_deg - f64::from(raw_lat) * WGS84_LSB_DEG).abs() < 1e-9);
        assert!((pos.longitude_deg - f64::from(raw_lon) * WGS84_LSB_DEG).abs() < 1e-9);
        // Sanity: the chosen counts land near 10 deg and -20 deg, not nonsense values.
        assert!((pos.latitude_deg - 10.0).abs() < 0.01);
        assert!((pos.longitude_deg + 20.0).abs() < 0.01);

        let codec = AsterixCat205Codec::new(vec![DfSite {
            sac: 25,
            sic: 210,
            sensor: SensorId(1),
            origin_enu_m: [0.0; 3],
            azimuth_sigma_rad: 0.01,
        }]);
        assert!(matches!(
            codec.map(&recs[0], MissionTime(0.0)),
            Ok(Mapped::NotADetection(_))
        ));
        assert!(codec.decode(&b, MissionTime(0.0)).expect("ok").is_empty());
    }

    #[test]
    fn blocking_two_records_in_one_data_block_is_refused() {
        let one = hand_built_block();
        let mut two = one.clone();
        // Double the declared length and append a second copy of the record body.
        two.extend_from_slice(&one[3..]);
        let len = u16::try_from(two.len()).expect("small");
        two[1..3].copy_from_slice(&len.to_be_bytes());
        assert!(matches!(
            decode_records(&two),
            Err(InteropError::Malformed { .. })
        ));
    }

    #[test]
    fn wrong_category_and_undefined_frn_are_refused() {
        let mut other = hand_built_block();
        other[0] = 0x30; // category 48
        assert!(matches!(
            decode_records(&other),
            Err(InteropError::WrongCategory { found: 48, .. })
        ));
        // A "Reserved for Future Use" FRN (23) flagged present: FSPEC = [all of FRN
        // 1-21 absent with FX set three times, then FRN23 flagged with FX clear], no
        // data item follows because the reserved arm errors before reading one.
        let reserved_frn_23 = [0xCD, 0x00, 0x07, 0x01, 0x01, 0x01, 0x40];
        assert!(matches!(
            decode_records(&reserved_frn_23),
            Err(InteropError::Malformed { .. })
        ));
        // FSPEC with five octets: the fifth flags FRN 29, past MAX_FRN (28).
        let beyond_max_frn = [0xCD, 0x00, 0x08, 0x01, 0x01, 0x01, 0x01, 0x80];
        assert!(matches!(
            decode_records(&beyond_max_frn),
            Err(InteropError::Malformed { .. })
        ));
    }

    #[test]
    fn truncation_is_an_error_at_every_length_never_a_panic() {
        let full = hand_built_block();
        for n in 0..full.len() {
            let mut cut = full[..n].to_vec();
            if n >= 3 {
                let len = u16::try_from(n).expect("small");
                cut[1..3].copy_from_slice(&len.to_be_bytes());
            }
            let result = decode_records(&cut);
            if n < 4 {
                // Below 4 octets there is no complete data block header (CAT+LEN is 3
                // octets and the FSPEC needs at least one more); 0 is no blocks at all.
                if n == 0 {
                    assert!(result.expect("empty input").is_empty());
                } else {
                    assert!(result.is_err(), "length {n} should not decode");
                }
            } else {
                assert!(
                    result.is_err(),
                    "length {n} truncates a complete record and should not decode"
                );
            }
        }
    }

    #[test]
    fn encode_stays_not_implemented() {
        assert!(matches!(
            AsterixCat205Codec::default().encode(&[]),
            Err(InteropError::NotImplemented(CODEC_NAME))
        ));
    }

    /// The hand-built block with one two-octet item's bytes replaced, so a range test
    /// changes exactly the item under test and nothing else.
    fn with_item_bytes(from: [u8; 2], to: [u8; 2]) -> Vec<u8> {
        let mut b = hand_built_block();
        let at = b
            .windows(2)
            .position(|w| w == from)
            .expect("the item's bytes are in the hand-built block");
        b[at..at + 2].copy_from_slice(&to);
        b
    }

    /// §5.2.8: `0.00 deg <= THETA < 360.00 deg`. 35 999 counts is the last value the
    /// edition defines; 36 000 is refused, not wrapped to zero and not passed on as a
    /// 360-degree bearing nothing downstream would question (2026-09-09).
    #[test]
    fn a_bearing_at_or_past_360_degrees_is_refused_as_the_edition_bounds_it() {
        let last_defined = with_item_bytes([0x11, 0x94], [0x8C, 0x9F]);
        let recs = decode_records(&last_defined).expect("359.99 deg decodes");
        assert!((recs[0].local_bearing_deg.expect("bearing") - 359.99).abs() < 1e-9);

        let past = with_item_bytes([0x11, 0x94], [0x8C, 0xA0]);
        let err = decode_records(&past).expect_err("360.00 deg is outside the edition");
        assert!(matches!(err, InteropError::Malformed { .. }), "{err}");
        assert!(err.to_string().contains("I205/070"), "{err}");
    }

    /// §5.2.21: `-90.00 deg <= ELEVATION <= 90.00 deg`, refused outside it.
    #[test]
    fn a_signal_elevation_outside_ninety_degrees_is_refused() {
        let at_the_bound = with_item_bytes([0x04, 0xE2], [0xDC, 0xD8]); // -9000
        let recs = decode_records(&at_the_bound).expect("-90.00 deg decodes");
        assert!((recs[0].signal_elevation_deg.expect("elev") + 90.0).abs() < 1e-9);

        let past = with_item_bytes([0x04, 0xE2], [0x23, 0x29]); // 9001
        let err = decode_records(&past).expect_err("90.01 deg is outside the edition");
        assert!(err.to_string().contains("I205/200"), "{err}");
    }
}
