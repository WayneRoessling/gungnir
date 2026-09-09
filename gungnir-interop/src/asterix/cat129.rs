// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! EUROCONTROL ASTERIX Category 129, UAS Identification and Target Reports.
//!
//! Built to **EUROCONTROL-SPEC-0149-29, ASTERIX Part 29, Category 129, edition 1.2**
//! (12 June 2019, ISBN 978-2-87497-028-3), fetched and read in full from
//! `https://www.eurocontrol.int/sites/default/files/2019-06/cat129p29ed12_0.pdf`
//! (publication page: `https://www.eurocontrol.int/publication/cat129-eurocontrol-
//! specification-surveillance-data-exchange-asterix-part-29-category`), the same free,
//! no-registration terms `docs/design/external-standards.md` §1 already documents for
//! the rest of the ASTERIX family. §9 surveyed this category alongside Category 205 on
//! 2026-09-06 and left it "open for a future gap to pick up" (GAP-100); this is that gap
//! (GAP-101), and §9.3 has what follows in the survey's own words.
//!
//! **The ISBN is the same one already recorded for Category 205.** Checked, not
//! assumed: both PDFs' own cover pages state `978-2-87497-028-3`. EUROCONTROL appears to
//! register one ISBN across a whole ASTERIX Part-N document series rather than one per
//! part; recorded here rather than silently repeated as if it were a coincidence.
//!
//! **Part I edition.** Edition 1.2's own bibliography (§2.2) cites Part I edition
//! **2.4** (24 October 2016) -- the same edition Category 205's own pin cites, and the
//! same nuance: this crate's shared framing (`super`) implements edition 3.1. The two
//! agree on the block/record/FSPEC structure this module depends on (edition 1.2 §4.4's
//! own `CAT | LEN | FSPEC | Data Item...` diagram and its explicit "Blocking of multiple
//! records sharing a single CAT and LEN field pair is not supported for Category 129
//! records" match what `super` implements and what `cat205` already found for the same
//! rule). Not separately checked beyond that, exactly as `cat205`'s own module
//! documentation records for the identical situation.
//!
//! **No machine-readable cross-check exists for this category, unlike Category 205's.**
//! `asterix-specs` (`docs/design/external-standards.md` §1.4) does not carry Category
//! 129 at all -- checked 2026-09-08 against its own specification index, which lists
//! 001, 002, 004, 007 through 011, 015 through 021, 023, 025, 032, 034, 048, 062, 063,
//! 065, 150, 205, 240 and 247, with no 129. `CroatiaControlLtd/asterix` (the source of
//! `cat048.raw`/`cat034.raw`, and of Category 205's own field-definition XML) carries no
//! Category 129 file of any kind under `install/config/` -- checked the same day. So
//! this module is built and cross-checked against the primary EUROCONTROL PDF alone,
//! fetched and converted to text twice by independent means (a plain layout-preserving
//! extraction and a table-aware one) that agreed on every prose passage; where the two
//! renderings of the UAP *table* disagreed with each other, the table-aware one matched
//! the detailed per-item sections and is what this module follows (next paragraph).
//!
//! **A genuine discrepancy in the primary source, recorded rather than silently
//! resolved.** Edition 1.2's own UAP summary (§5.3.1, Table 2) states data item
//! I129/120 (Operational Risk Levels) is one octet long; its own detailed description
//! (§5.2.12) states "Three-octet fixed length Data Item" and diagrams only the first
//! octet's bits. A wire decoder must pick one to keep every following item aligned; this
//! module follows §5.2.12 (three octets), because it is the section that fixes the bit
//! layout every other item's own decode already trusts its analogous section for, and
//! because a one-octet item would leave two of the "fixed length" item's own declared
//! octets undocumented by construction rather than by omission. [`Record::carried_raw`]
//! carries those two undocumented octets raw, named `I129/120 octets 2-3`, rather than
//! asserting they are spare.
//!
//! **Annex A is incomplete in this edition, checked rather than assumed.** I129/120's
//! Air Risk Category (ARC) subfield is defined by Annex A's "Air Risk Categories" list
//! (values 1 through 3); the same annex's "Airspace Encounter Categories" subsection,
//! which the item's own Operational Risk Levels description (§5.2.12) points to for the
//! Airspace Encounter Category (AEC) subfield, is a heading with no defined values
//! anywhere in edition 1.2 -- the document ends one page later. So
//! [`gungnir_model::OperationalRisk::airspace_encounter_category_code`] is carried as a
//! raw 4-bit code rather than a named enum: this build cannot honestly name what a code
//! means when the specification that was meant to does not either.
//!
//! **The WGS-84 position LSB, cross-checked against the specification's own worked
//! decimal.** I129/080 packs latitude and longitude as two 32-bit two's-complement
//! counts (edition 1.2 §5.2.8) at "LSB = 180/2^30 degrees" -- a different divisor from
//! Category 205's I205/050 (180/2^25) despite both being 32-bit fields, so this module
//! defines its own [`WGS84_LSB_DEG`] rather than reusing `cat205`'s. Confirmed against
//! the specification's own stated approximation rather than trusted from one digit
//! string: 180 / 2^30 = 1.67638...e-7, matching the document's own "= 1.6764 * 10-07
//! degrees" to five significant figures (its superscript minus and caret are lost by
//! plain-text extraction, which is why the cross-check is against the decimal value and
//! not the exponent's own rendering).
//!
//! **SAC/SIC is often a placeholder in this category, unlike a radar's or a direction
//! finder's.** I129/010's own note: "For the airborne to ground transmission of
//! category 129 messages it is recommended to set the SAC and SIC to '00/00'." A
//! deployment that only ever receives that recommendation followed configures one
//! [`UasSite`] binding at `(0, 0)` for its one receiving gateway -- the same shape one
//! AIS receiver or one ADS-B receiver already gets one `SensorId`, regardless of how
//! many distinct ships or aircraft it hears. Only a "ground to ground" relay
//! (§4.1's other case, SAC/SIC allocated per Part I chapter 6.4) can meaningfully bind
//! more than one [`UasSite`] on one feed; this module still keys attribution by SAC/SIC,
//! rather than inventing a different mechanism, because the wire item exists and a
//! ground-to-ground deployment is entitled to use it.
//!
//! **What this module does and does not decode.** [`decode_records`] types every
//! standard-UAP item edition 1.2's Table 2 defines (FRN 1 to 15) except the generic SP
//! field (FRN 13, no catalogue number, carried raw by convention) and I129/120's own
//! undocumented trailing octets (above); FRN 16 to 21 are "Reserved for Future Use" and
//! a set FSPEC bit for one of them is an [`crate::InteropError::Malformed`], the same
//! treatment `cat048`/`cat205` give a reserved FRN flagged present. I129/015 (Data
//! Destination Identification) decodes but is not promoted to
//! [`gungnir_model::UasIdentificationReport`]: it names where a ground-to-ground record
//! was routed, not an observation about the platform, the same reasoning that keeps
//! `cat048::TargetDescriptor`-style routing metadata out of a mapped detection.
//!
//! **The mapping layer ([`AsterixCat129Codec`]).** Unlike Category 048 or 205, this
//! category has no message-type discriminator and no "no detection this scan" flag: a
//! record that decodes and names a configured [`UasSite`] always becomes exactly one
//! [`gungnir_model::UasIdentificationReport`], so [`AsterixCat129Codec::map`] returns one
//! directly rather than a `Mapped` enum with a "not an observation" arm. This is a
//! cooperative-identity source in AIS's and ADS-B's sense, not a detection of another
//! target, so the mapping produces [`gungnir_model::UasIdentificationReport`]
//! (`gungnir-model`, not this crate: its own module documentation explains why it lives
//! there and not beside an ingest adapter, and why its position stays a plain
//! [`gungnir_model::Geodetic`] rather than an ENU one) through a dedicated
//! [`crate::UasIdentificationCodec`] boundary rather than [`crate::DetectionCodec`] --
//! the same reasoning `cat034`'s `ServiceMessageCodec` already established for a
//! category whose output is not a detection either. A deployment that also wants this
//! report's position on the tracking picture gets a `DetectionView` (`Measurement::
//! Position`) the same way AIS, ADS-B and MISB ST 0601 already do: from the ingest
//! adapter (`gungnir_ingest::adapters::asterix`), which holds the `LocalFrame` this
//! crate may not depend on (`ARCHITECTURE.md` §7) and converts this same geodetic
//! position for the fusion pipeline as a second, independent step.

use super::{data_blocks, sign_extend, Cursor, DataBlock, Fspec};
use crate::asterix::cat048::DataSource;
use crate::asterix::cat205::WgsPosition;
use crate::{InteropError, UasIdentificationCodec};
use gungnir_model::{
    Geodetic, MissionTime, OperationalRisk, SensorId, UasCertificationCategory,
    UasIdentificationReport,
};

/// The codec's name in the schema catalog.
pub const CODEC_NAME: &str = "asterix.cat129";
/// The Category 129 edition this decoder is built to.
pub const EDITION: &str = "1.2";
/// The Part I edition Category 129 edition 1.2 itself cites (module documentation
/// explains why this differs from the 3.1 the shared framing implements).
pub const PART1_EDITION_CITED: &str = "2.4";

const CATEGORY: u8 = 129;
const MAX_FRN: usize = 21;
/// I129/080: 180 / 2^30 degrees per count (§5.2.8) -- see the module documentation for
/// the cross-check against the specification's own worked decimal, and for why this
/// differs from `cat205::WGS84_LSB_DEG`.
const WGS84_LSB_DEG: f64 = 180.0 / 1_073_741_824.0;

// ---------------------------------------------------------------------------
// Typed record
// ---------------------------------------------------------------------------

/// I129/015 (§5.2.2): routing metadata, not an observation. Decoded losslessly but not
/// carried onto [`gungnir_model::UasIdentificationReport`] -- see the module
/// documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataDestination {
    pub dac: u8,
    pub dic: u8,
}

/// A data item this build carries without interpreting: the SP field (FRN 13, no
/// catalogue number) and I129/120's own two undocumented trailing octets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawItem {
    /// The item's name, for example `I129/SP`. Not always a catalogue number: the SP
    /// field has none, matching `cat048`'s and `cat205`'s own convention.
    pub item: &'static str,
    pub octets: Vec<u8>,
}

/// One Category 129 record, every UAP item accounted for (Table 2, FRN 1 to 21; FRN 16
/// to 21 are "Reserved for Future Use" and a set FSPEC bit for one of them is an
/// [`InteropError::Malformed`], the same treatment as an FRN past [`MAX_FRN`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// Absolute offset of the record's FSPEC in the input.
    pub offset: usize,
    pub data_source: Option<DataSource>,
    pub data_destination: Option<DataDestination>,
    /// I129/020: three ASCII characters. A byte outside printable ASCII becomes `?`,
    /// never a panic -- the same discipline `cat205::parse_radio_channel_name` applies.
    pub manufacturer_id: Option<String>,
    /// I129/030: three ASCII characters, same treatment as [`Self::manufacturer_id`].
    pub model_id: Option<String>,
    /// I129/040: twelve raw octets. Edition 1.2 states no encoding for this item (module
    /// documentation), so this build does not assert ASCII the way it does for
    /// [`Self::manufacturer_id`], [`Self::model_id`] and [`Self::registration_country`].
    pub serial_number: Option<[u8; 12]>,
    /// I129/050: two ASCII characters, ISO 3166-1 alpha-2.
    pub registration_country: Option<String>,
    /// I129/070: seconds since midnight UTC, resolution 1/128 s.
    pub time_of_day_s: Option<f64>,
    /// I129/080: the UAS's own claimed WGS-84 position. No altitude: this item is
    /// latitude and longitude only (§5.2.8's own eight octets hold nothing else).
    pub position: Option<WgsPosition>,
    /// I129/090: metres above mean sea level, two's complement (negative is below MSL).
    pub altitude_amsl_m: Option<f64>,
    /// I129/100: metres above ground level, decoded with the identical 24-bit two's
    /// complement structure edition 1.2 gives I129/090 (its own diagram states no
    /// narrower range for this item).
    pub altitude_agl_m: Option<f64>,
    /// I129/110: metres, 50% circular error probability. The specification's own note:
    /// `0.0` means "unknown or more than 255 m" -- kept exactly as decoded, not
    /// collapsed to `None`, the same restraint `Self::registration_country` and every
    /// other item here applies to a sentinel this build does not invent meaning for.
    pub gnss_signal_accuracy_m: Option<f64>,
    /// I129/120: the one octet edition 1.2 actually describes. The item's own
    /// undocumented remaining octets are in [`Self::carried_raw`] as `I129/120 octets
    /// 2-3` (module documentation).
    pub operational_risk: Option<OperationalRisk>,
    /// I129/185: `[east_m_s, north_m_s]`, target-centric Cartesian (module
    /// documentation on why this needs no ENU conversion).
    pub horizontal_velocity_enu_m_s: Option<[f64; 2]>,
    /// I129/220: metres/second; positive is climbing.
    pub vertical_velocity_m_s: Option<f64>,
    pub carried_raw: Vec<RawItem>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Decode every Category 129 record in `bytes`, losslessly. Every block must be
/// Category 129; see `cat048::decode_records` for why a live feed is split with
/// [`super::data_blocks`] first. Like Category 205 and unlike 048/034, each block holds
/// exactly one record (see [`decode_block`]).
pub fn decode_records(bytes: &[u8]) -> Result<Vec<Record>, InteropError> {
    let mut records = Vec::new();
    for block in data_blocks(CODEC_NAME, bytes)? {
        records.push(decode_block(&block)?);
    }
    Ok(records)
}

/// Decode the one record of a data block that must be Category 129.
///
/// Edition 1.2 §4.4: "Blocking of multiple records sharing a single CAT and LEN field
/// pair is not supported for Category 129 records." So this returns one [`Record`]
/// rather than a `Vec`, and octets left over after it decodes are a malformed block
/// rather than a second record silently ignored or silently parsed -- the identical rule
/// `cat205::decode_block` enforces for the identical reason.
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
             support blocking multiple Category 129 records in one data block",
            cur.remaining()
        )));
    }
    Ok(record)
}

/// One match arm per FRN of the standard UAP, in Table 2 order, so the function reads
/// against the table; splitting it would separate the table from itself (the same
/// choice `cat048::parse_record` and `cat205::parse_record` make and explain).
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
                let b = cur.take(2, "I129/010")?;
                r.data_source = Some(DataSource {
                    sac: b[0],
                    sic: b[1],
                });
            }
            2 => {
                let b = cur.take(2, "I129/015")?;
                r.data_destination = Some(DataDestination {
                    dac: b[0],
                    dic: b[1],
                });
            }
            3 => r.manufacturer_id = Some(decode_ascii(cur.take(3, "I129/020")?)),
            4 => r.model_id = Some(decode_ascii(cur.take(3, "I129/030")?)),
            5 => {
                let b = cur.take(12, "I129/040")?;
                let mut serial = [0u8; 12];
                serial.copy_from_slice(b);
                r.serial_number = Some(serial);
            }
            6 => r.registration_country = Some(decode_ascii(cur.take(2, "I129/050")?)),
            7 => r.time_of_day_s = Some(f64::from(cur.u24("I129/070")?) / 128.0),
            8 => r.position = Some(parse_wgs84(cur)?),
            9 => {
                let raw = cur.u24("I129/090")?;
                r.altitude_amsl_m = Some(f64::from(sign_extend(raw, 24)) * 0.1);
            }
            10 => {
                let raw = cur.u24("I129/100")?;
                r.altitude_agl_m = Some(f64::from(sign_extend(raw, 24)) * 0.1);
            }
            11 => r.gnss_signal_accuracy_m = Some(f64::from(cur.u16("I129/110")?)),
            12 => r.operational_risk = Some(parse_operational_risk(cur, &mut r.carried_raw)?),
            13 => raw(&mut r, "I129/SP", cur.explicit("I129/SP")?),
            14 => r.horizontal_velocity_enu_m_s = Some(parse_horizontal_velocity(cur)?),
            15 => {
                let raw24 = cur.u24("I129/220")?;
                let vv = sign_extend(raw24 & 0x000F_FFFF, 20);
                r.vertical_velocity_m_s = Some(f64::from(vv) * 0.01);
            }
            16..=21 => {
                return Err(cur.error(format!(
                    "FRN {frn} is \"Reserved for Future Use\" in edition {EDITION}'s \
                     UAP (Table 2) and flagged present"
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

/// I129/080 (§5.2.8): two 32-bit two's complement fields, LSB [`WGS84_LSB_DEG`] each. No
/// altitude in this item (module documentation on [`Record::position`]).
fn parse_wgs84(cur: &mut Cursor<'_>) -> Result<WgsPosition, InteropError> {
    let lat = cur.i32("I129/080 latitude")?;
    let lon = cur.i32("I129/080 longitude")?;
    Ok(WgsPosition {
        latitude_deg: f64::from(lat) * WGS84_LSB_DEG,
        longitude_deg: f64::from(lon) * WGS84_LSB_DEG,
    })
}

/// I129/120 (§5.2.12): the specification's own three-octet length (module
/// documentation on the Table 2 discrepancy), with only the first octet's bits
/// described. UCC (bits 8/7), ARC (bits 6/5) and AEC (bits 4/1) come from that octet;
/// the other two are carried raw.
fn parse_operational_risk(
    cur: &mut Cursor<'_>,
    carried_raw: &mut Vec<RawItem>,
) -> Result<OperationalRisk, InteropError> {
    let b = cur.take(3, "I129/120")?;
    let first = b[0];
    carried_raw.push(RawItem {
        item: "I129/120 octets 2-3",
        octets: b[1..].to_vec(),
    });
    let certification_category = match (first >> 6) & 0b11 {
        0 => UasCertificationCategory::Unknown,
        1 => UasCertificationCategory::Open,
        2 => UasCertificationCategory::Specific,
        _ => UasCertificationCategory::Certified,
    };
    Ok(OperationalRisk {
        certification_category,
        air_risk_category_code: (first >> 4) & 0b11,
        airspace_encounter_category_code: first & 0b1111,
    })
}

/// I129/185 (§5.2.13): a five-octet (40-bit) field packing two 20-bit two's complement
/// subfields with no octet alignment between them -- HVX in the top 20 bits, HVY in the
/// bottom 20, each LSB 0.01 m/s.
fn parse_horizontal_velocity(cur: &mut Cursor<'_>) -> Result<[f64; 2], InteropError> {
    let b = cur.take(5, "I129/185")?;
    let packed = u64::from(b[0]) << 32
        | u64::from(b[1]) << 24
        | u64::from(b[2]) << 16
        | u64::from(b[3]) << 8
        | u64::from(b[4]);
    let east_raw = ((packed >> 20) & 0x000F_FFFF) as u32;
    let north_raw = (packed & 0x000F_FFFF) as u32;
    let east = f64::from(sign_extend(east_raw, 20)) * 0.01;
    let north = f64::from(sign_extend(north_raw, 20)) * 0.01;
    Ok([east, north])
}

/// I129/020, I129/030 and I129/050 (§5.2.3, §5.2.4, §5.2.6): plain 8-bit-per-character
/// ASCII, unlike `cat048::decode_ia5_6bit`'s packed six-bit coding. A byte outside
/// printable ASCII is not a character any of these items' encodings define and is
/// rendered as `?` rather than dropped or turned into a panic -- the same discipline
/// `cat205::parse_radio_channel_name` applies to I205/090.
fn decode_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&c| {
            if (0x20..=0x7E).contains(&c) {
                char::from(c)
            } else {
                '?'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Mapping to the model
// ---------------------------------------------------------------------------

/// One UAS Identification and Target Report source a Category 129 decoder accepts
/// records from: normally one receiving gateway per feed, keyed by whatever SAC/SIC it
/// sends (module documentation on why that is often the placeholder `(0, 0)`).
///
/// Unlike [`super::RadarSite`] or `cat205::DfSite`, this carries no antenna position:
/// nothing this category maps needs one, because the report already carries the UAS's
/// own absolute position rather than a range or bearing that would need a sensor origin
/// to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UasSite {
    pub sac: u8,
    pub sic: u8,
    pub sensor: SensorId,
}

/// Category 129 decoder configured with the gateways it may attribute reports to.
///
/// A record whose SAC/SIC is not configured is an [`InteropError::UnknownRadar`], not a
/// report with a made-up sensor -- the same refusal `cat048`, `cat034` and `cat205`
/// already make for their own site tables. The `Default` codec knows no sites, so it
/// decodes records losslessly ([`decode_records`]) and attributes none.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AsterixCat129Codec {
    sites: Vec<UasSite>,
}

impl AsterixCat129Codec {
    pub fn new(sites: Vec<UasSite>) -> Self {
        Self { sites }
    }

    pub fn sites(&self) -> &[UasSite] {
        &self.sites
    }

    fn site(&self, source: DataSource) -> Option<&UasSite> {
        self.sites
            .iter()
            .find(|s| s.sac == source.sac && s.sic == source.sic)
    }

    /// Map one record to the report it names. Every field of
    /// [`gungnir_model::UasIdentificationReport`] but [`Self::site`]'s own `sensor`
    /// comes from the record; there is no "valid record, not an observation" case in
    /// this category (module documentation), so this returns the report directly
    /// rather than a `Mapped` enum the way `cat048`/`cat205` must.
    ///
    /// # Errors
    ///
    /// [`InteropError::Malformed`] when I129/010, I129/050 or I129/080 -- each
    /// mandatory in every record per edition 1.2's own encoding rules -- is absent.
    /// [`InteropError::UnknownRadar`] when I129/010 names a SAC/SIC no [`UasSite`]
    /// configures.
    pub fn map(
        &self,
        record: &Record,
        receipt_time: MissionTime,
    ) -> Result<UasIdentificationReport, InteropError> {
        let source = record.data_source.ok_or_else(|| InteropError::Malformed {
            codec: CODEC_NAME,
            offset: record.offset,
            reason: "I129/010 absent; a report with no data source cannot be attributed".into(),
        })?;
        let site = self.site(source).ok_or(InteropError::UnknownRadar {
            codec: CODEC_NAME,
            sac: source.sac,
            sic: source.sic,
        })?;
        let position = record.position.ok_or_else(|| InteropError::Malformed {
            codec: CODEC_NAME,
            offset: record.offset,
            reason: "I129/080 absent; edition 1.2 requires a position in every record".into(),
        })?;
        let registration_country =
            record
                .registration_country
                .clone()
                .ok_or_else(|| InteropError::Malformed {
                    codec: CODEC_NAME,
                    offset: record.offset,
                    reason: "I129/050 absent; edition 1.2 requires it in every record".into(),
                })?;

        let mut losses: Vec<String> = Vec::new();
        let (source_time, time_loss) =
            super::cat048::source_time(record.time_of_day_s, receipt_time);
        losses.extend(time_loss.map(str::to_owned));

        // Geodetic::alt_m is nominally a WGS-84 ellipsoidal height. I129/090 (AMSL) is
        // an orthometric height; the two differ by the local geoid undulation, which
        // this build does not correct for (it has no geoid model to correct with) but
        // does record as a loss rather than presenting the substitution as exact.
        // I129/100 (AGL) is deliberately never used here even alone: without a ground
        // elevation model, an above-ground-level height cannot honestly become an
        // absolute one, and inventing a ground elevation would be exactly the
        // confidently-wrong move this workspace forbids.
        let alt_m = if let Some(amsl) = record.altitude_amsl_m {
            losses.push(
                "I129/090 (orthometric height above mean sea level) is placed on a \
                 nominally WGS-84 ellipsoidal altitude field; the local geoid \
                 undulation is not corrected for"
                    .to_owned(),
            );
            amsl
        } else {
            losses.push(
                "neither I129/090 nor a usable absolute altitude is present in this \
                 record; I129/100 (above ground level) cannot substitute without a \
                 ground elevation model, so altitude is set to 0"
                    .to_owned(),
            );
            0.0
        };

        Ok(UasIdentificationReport {
            sensor: site.sensor,
            source_time,
            receipt_time,
            position: Geodetic {
                lat_rad: position.latitude_deg.to_radians(),
                lon_rad: position.longitude_deg.to_radians(),
                alt_m,
            },
            altitude_amsl_m: record.altitude_amsl_m,
            altitude_agl_m: record.altitude_agl_m,
            gnss_signal_accuracy_m: record.gnss_signal_accuracy_m,
            manufacturer_id: record.manufacturer_id.clone(),
            model_id: record.model_id.clone(),
            serial_number: record.serial_number,
            registration_country,
            operational_risk: record.operational_risk,
            horizontal_velocity_enu_m_s: record.horizontal_velocity_enu_m_s,
            vertical_velocity_m_s: record.vertical_velocity_m_s,
            conversion_loss: if losses.is_empty() {
                None
            } else {
                Some(losses.join("; "))
            },
        })
    }
}

impl UasIdentificationCodec for AsterixCat129Codec {
    fn name(&self) -> &'static str {
        CODEC_NAME
    }

    /// Every record, mapped, in wire order.
    fn decode(
        &self,
        bytes: &[u8],
        receipt_time: MissionTime,
    ) -> Result<Vec<UasIdentificationReport>, InteropError> {
        let mut out = Vec::new();
        for record in decode_records(bytes)? {
            out.push(self.map(&record, receipt_time)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One record, every FRN except the reserved ones and the SP field: FSPEC flags
    /// FRN 1, 6, 7 (octet 1), FRN 8, 9, 11 (octet 2) -- `0x83, 0x34`. SAC 0 SIC 0
    /// (the airborne-to-ground placeholder), country "US", time 12:00:00 (43 200 s x
    /// 128 = `0x54_6000`, the same constant `cat048`'s and `cat205`'s own hand-built
    /// fixtures use), a position near 10 deg N 20 deg W, AMSL 500.0 m (5000 x 0.1 =
    /// `0x00_1388`), GNSS accuracy 12 m.
    fn hand_built_block() -> Vec<u8> {
        let mut b = vec![0x81, 0x00, 0x00]; // CAT = 129, LEN patched below
        b.push(0x87); // FSPEC octet 1: FRN 1, 6, 7 + FX
        b.push(0xD0); // FSPEC octet 2: FRN 8, 9, 11, no FX
        b.extend_from_slice(&[0, 0]); // I129/010
        b.extend_from_slice(b"US"); // I129/050
        b.extend_from_slice(&[0x54, 0x60, 0x00]); // I129/070
        let raw_lat: i32 = 59_652_323; // ~10.00003 deg
        let raw_lon: i32 = -119_304_647; // ~-20.00003 deg
        b.extend_from_slice(&raw_lat.to_be_bytes());
        b.extend_from_slice(&raw_lon.to_be_bytes()); // I129/080
        b.extend_from_slice(&[0x00, 0x13, 0x88]); // I129/090 = 5000
        b.extend_from_slice(&[0x00, 0x0C]); // I129/110 = 12
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());
        b
    }

    fn site() -> UasSite {
        UasSite {
            sac: 0,
            sic: 0,
            sensor: SensorId(31),
        }
    }

    #[test]
    fn decodes_hand_built_record_to_specified_lsbs() {
        let recs = decode_records(&hand_built_block()).expect("decodes");
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.data_source, Some(DataSource { sac: 0, sic: 0 }));
        assert_eq!(r.registration_country.as_deref(), Some("US"));
        assert!((r.time_of_day_s.expect("tod") - 43_200.0).abs() < 1e-9);
        let pos = r.position.expect("I129/080");
        assert!((pos.latitude_deg - 10.0).abs() < 0.001);
        assert!((pos.longitude_deg + 20.0).abs() < 0.001);
        assert!((r.altitude_amsl_m.expect("I129/090") - 500.0).abs() < 1e-9);
        assert!(r.altitude_agl_m.is_none());
        assert!((r.gnss_signal_accuracy_m.expect("I129/110") - 12.0).abs() < 1e-9);
        assert!(r.manufacturer_id.is_none());
        assert!(r.operational_risk.is_none());
        assert!(r.carried_raw.is_empty());
    }

    #[test]
    fn maps_to_a_uas_identification_report() {
        let codec = AsterixCat129Codec::new(vec![site()]);
        let receipt = MissionTime(20_000.0 * 86_400.0 + 43_205.0);
        let reports = codec.decode(&hand_built_block(), receipt).expect("decodes");
        assert_eq!(reports.len(), 1);
        let rep = &reports[0];
        assert_eq!(rep.sensor, SensorId(31));
        assert!((rep.source_time.0 - (20_000.0 * 86_400.0 + 43_200.0)).abs() < 1e-6);
        assert_eq!(rep.registration_country, "US");
        assert!((rep.position.lat_rad.to_degrees() - 10.0).abs() < 0.001);
        assert!((rep.position.lon_rad.to_degrees() + 20.0).abs() < 0.001);
        assert!((rep.position.alt_m - 500.0).abs() < 1e-9);
        assert!((rep.altitude_amsl_m.expect("amsl") - 500.0).abs() < 1e-9);
        assert!((rep.gnss_signal_accuracy_m.expect("gnss") - 12.0).abs() < 1e-9);
        let loss = rep
            .conversion_loss
            .as_deref()
            .expect("the geoid-undulation approximation is recorded");
        assert!(loss.contains("geoid"));
    }

    #[test]
    fn unknown_site_is_an_error_not_a_guessed_sensor() {
        let codec = AsterixCat129Codec::default();
        assert!(matches!(
            codec.decode(&hand_built_block(), MissionTime(0.0)),
            Err(InteropError::UnknownRadar { sac: 0, sic: 0, .. })
        ));
    }

    #[test]
    fn operational_risk_and_velocities_decode_with_the_arc_label_offset() {
        // FSPEC flags FRN 1, 6, 7 (octet 1); FRN 8, 12, 14 (octet 2, no FX); a minimal
        // position and time, then I129/120 (UCC=Specific=2, ARC code=1 -> label 2,
        // AEC=5) and I129/185 (HVX=+300 -> 3.00 m/s, HVY=-150 -> -1.50 m/s).
        let mut b = vec![0x81, 0x00, 0x00];
        b.push(0x87); // FRN 1, 6, 7 + FX
        b.push(0x8A); // FRN 8, 12, 14, no FX
        b.extend_from_slice(&[7, 9]); // I129/010
        b.extend_from_slice(b"DE"); // I129/050
        b.extend_from_slice(&[0x54, 0x60, 0x00]); // I129/070
        b.extend_from_slice(&[0, 0, 0, 0]); // I129/080 latitude = 0
        b.extend_from_slice(&[0, 0, 0, 0]); // I129/080 longitude = 0
        let first = (2u8 << 6) | (1u8 << 4) | 5u8; // UCC=2 ARC=1 AEC=5
        b.extend_from_slice(&[first, 0xAA, 0xBB]); // I129/120, 3 octets
                                                   // I129/185: HVX = 300 (12 bits set within top 20), HVY = -150, packed 40 bits.
        let hvx: i64 = 300;
        let hvy: i64 = -150;
        let packed: u64 =
            (hvx.cast_unsigned() & 0x000F_FFFF) << 20 | (hvy.cast_unsigned() & 0x000F_FFFF);
        b.extend_from_slice(&packed.to_be_bytes()[3..8]);
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());

        let recs = decode_records(&b).expect("decodes");
        let r = &recs[0];
        let risk = r.operational_risk.expect("I129/120");
        assert_eq!(
            risk.certification_category,
            UasCertificationCategory::Specific
        );
        assert_eq!(risk.air_risk_category_code, 1);
        assert_eq!(risk.air_risk_category_label(), 2);
        assert_eq!(risk.airspace_encounter_category_code, 5);
        assert_eq!(
            r.carried_raw
                .iter()
                .find(|i| i.item == "I129/120 octets 2-3")
                .map(|i| i.octets.clone()),
            Some(vec![0xAA, 0xBB])
        );
        let v = r.horizontal_velocity_enu_m_s.expect("I129/185");
        assert!((v[0] - 3.00).abs() < 1e-9);
        assert!((v[1] - (-1.50)).abs() < 1e-9);
    }

    #[test]
    fn blocking_two_records_in_one_data_block_is_refused() {
        let one = hand_built_block();
        let mut two = one.clone();
        two.extend_from_slice(&one[3..]);
        let len = u16::try_from(two.len()).expect("small");
        two[1..3].copy_from_slice(&len.to_be_bytes());
        assert!(matches!(
            decode_records(&two),
            Err(InteropError::Malformed { .. })
        ));
    }

    #[test]
    fn wrong_category_and_reserved_frn_are_refused() {
        let mut other = hand_built_block();
        other[0] = 0x30; // category 48
        assert!(matches!(
            decode_records(&other),
            Err(InteropError::WrongCategory { found: 48, .. })
        ));
        // A "Reserved for Future Use" FRN (16) flagged present: FRN 16 is the second
        // local position of the third FSPEC octet (global FRN 15-21), so reaching it
        // needs FX set on the first two octets (`0x01, 0x01`) before the third flags it
        // (bit 6, `0x40`) with FX clear.
        let reserved_frn_16 = [0x81, 0x00, 0x06, 0x01, 0x01, 0x40];
        assert!(matches!(
            decode_records(&reserved_frn_16),
            Err(InteropError::Malformed { .. })
        ));
        // FSPEC with four octets: the fourth flags FRN 22, past MAX_FRN (21).
        let beyond_max_frn = [0x81, 0x00, 0x07, 0x01, 0x01, 0x01, 0x80];
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
            if n == 0 {
                assert!(result.expect("empty input").is_empty());
            } else {
                assert!(result.is_err(), "length {n} should not decode");
            }
        }
    }

    #[test]
    fn non_ascii_bytes_in_a_character_item_become_question_marks() {
        assert_eq!(decode_ascii(&[b'A', 0x00, 0x7F, b'1']), "A??1");
    }
}
