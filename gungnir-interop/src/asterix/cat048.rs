// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! EUROCONTROL ASTERIX Category 048, monoradar target reports.
//!
//! Built to EUROCONTROL-SPEC-0149-4 **edition 1.32** (1 July 2024), with Part I
//! edition 3.1 for the framing and Appendix A edition 1.13 governing the reserved
//! expansion field, which this module carries but does not interpret. The three
//! editions were pinned on 2026-09-06 in `docs/design/external-standards.md` §1.6;
//! section numbers cited as §5.2.x below are that specification's.
//!
//! Two layers, deliberately separate:
//!
//! - [`decode_records`] is the lossless layer: every record becomes a [`Record`]
//!   holding each data item of the standard UAP (§5.3.1, Table 2) either as a typed
//!   value or, for the items this build does not interpret, as raw octets listed in
//!   [`Record::carried_raw`] so nothing is dropped silently.
//! - [`AsterixCat048Codec`] is the mapping layer that turns a record into the model's
//!   `DetectionView`. That mapping is lossy in ways the specification makes
//!   unavoidable (a polar plot has no geometric height; a Mode C level is pressure
//!   altitude), and every loss is written to `Provenance::conversion_loss`.
//!
//! Encoding tracks into Category 048 is not implemented. Gungnir is the surveillance
//! data processing side of this interface, and a fused track is not a monoradar
//! target report; the coalition exchange format for tracks is STANAG 4676 (DN-18).

use super::{data_blocks, sign_extend, Cursor, DataBlock, Fspec};
use crate::{DetectionCodec, InteropError};
use gungnir_model::{DetectionView, MissionTime, Provenance, TrackView};

/// The codec's name in the schema catalog.
pub const CODEC_NAME: &str = "asterix.cat048";
/// The Category 048 edition this decoder is built to.
pub const EDITION: &str = "1.32";
/// The Appendix A (reserved expansion field) edition in force for that edition.
pub const APPENDIX_A_EDITION: &str = "1.13";
/// The Part I edition whose framing rules `super` implements.
pub const PART1_EDITION: &str = "3.1";

const CATEGORY: u8 = 48;
const MAX_FRN: usize = 28;
const NM_M: f64 = 1852.0;
const FT_M: f64 = 0.3048;
const SECONDS_PER_DAY: f64 = 86_400.0;
/// Per-axis variance stamped on a mapped plot, metres squared
/// (docs/design/DN-27-bearing-only-detections.md §8).
///
/// The tracking baseline's own default measurement noise
/// (`gungnir_fusion_async::PipelineSettings::measurement_noise_var`), which is the only
/// stated accuracy this workspace has for a radar plot. **Restated and not imported**:
/// this crate sits beneath that one and may not depend on it. It is the number the
/// gate downstream already assumed, so the migration moves no behaviour -- what changes
/// is that the assumption is now written on the measurement instead of guessed from it.
const BASELINE_PLOT_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

// ---------------------------------------------------------------------------
// Typed record
// ---------------------------------------------------------------------------

/// I048/010 (§5.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DataSource {
    pub sac: u8,
    pub sic: u8,
}

/// Type of report, I048/020 bits 8/6 (§5.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportType {
    /// TYP = 0: no detection; the position, if any, is extrapolated.
    NoDetection,
    SinglePsr,
    SingleSsr,
    SsrPlusPsr,
    ModeSAllCall,
    ModeSRollCall,
    ModeSAllCallPlusPsr,
    ModeSRollCallPlusPsr,
}

impl ReportType {
    fn from_bits(typ: u8) -> Self {
        match typ & 0b111 {
            0 => Self::NoDetection,
            1 => Self::SinglePsr,
            2 => Self::SingleSsr,
            3 => Self::SsrPlusPsr,
            4 => Self::ModeSAllCall,
            5 => Self::ModeSRollCall,
            6 => Self::ModeSAllCallPlusPsr,
            _ => Self::ModeSRollCallPlusPsr,
        }
    }
}

/// I048/020 first octet (§5.2.2). Extension octets are kept raw.
///
/// The four flags are the specification's own single-bit fields, kept as bits.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TargetDescriptor {
    pub report_type: ReportType,
    /// SIM: simulated target report.
    pub simulated: bool,
    /// RDP: report from RDP chain 2 rather than 1.
    pub rdp_chain_2: bool,
    /// SPI: special position identification.
    pub spi: bool,
    /// RAB: report from a field monitor (fixed transponder).
    pub fixed_transponder: bool,
    /// First and later extension octets, FX bit included, uninterpreted.
    pub extensions: Vec<u8>,
}

/// I048/040 (§5.2.4): slant range and azimuth, radar-centred, azimuth clockwise from north.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolarPosition {
    pub slant_range_nm: f64,
    pub azimuth_deg: f64,
}

/// I048/042 (§5.2.5): the radar's local Cartesian grid, x east and y north.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CartesianPosition {
    pub x_nm: f64,
    pub y_nm: f64,
}

/// I048/070 (§5.2.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode3A {
    pub validated: bool,
    pub garbled: bool,
    /// L: code derived from the local tracker rather than a reply.
    pub from_local_tracker: bool,
    /// The four octal digits packed as twelve bits (A4 A2 A1 B4 B2 B1 C4 C2 C1 D4 D2 D1).
    pub code: u16,
}

impl Mode3A {
    /// The code as it is spoken, four octal digits.
    pub fn octal(&self) -> String {
        format!("{:04o}", self.code)
    }
}

/// I048/090 (§5.2.12). One flight level is 100 ft of pressure altitude.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightLevel {
    pub validated: bool,
    pub garbled: bool,
    pub level: f64,
}

/// I048/130 (§5.2.16). Every subfield is optional.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlotCharacteristics {
    pub ssr_runlength_deg: Option<f64>,
    pub ssr_replies: Option<u8>,
    pub ssr_amplitude_dbm: Option<i8>,
    pub psr_runlength_deg: Option<f64>,
    pub psr_amplitude_dbm: Option<i8>,
    pub psr_ssr_range_difference_nm: Option<f64>,
    pub psr_ssr_azimuth_difference_deg: Option<f64>,
}

/// One entry of I048/250 (§5.2.25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BdsRegister {
    pub data: [u8; 7],
    pub bds1: u8,
    pub bds2: u8,
}

/// I048/200 (§5.2.20).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolarVelocity {
    pub ground_speed_nm_s: f64,
    /// Relative to geographic north at the aircraft.
    pub heading_deg: f64,
}

/// Which sensors maintain the track, I048/170 bits 7/6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSensor {
    Combined,
    Psr,
    SsrModeS,
    Invalid,
}

/// I048/170 first octet (§5.2.19). Extension octets are kept raw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackStatus {
    pub tentative: bool,
    pub sensor: TrackSensor,
    pub low_association_confidence: bool,
    pub horizontal_manoeuvre: bool,
    /// CDM: 0 maintaining, 1 climbing, 2 descending, 3 unknown.
    pub climb_descent: u8,
    pub extensions: Vec<u8>,
}

/// I048/210 (§5.2.21).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackQuality {
    pub sigma_x_nm: f64,
    pub sigma_y_nm: f64,
    pub sigma_v_nm_s: f64,
    pub sigma_h_deg: f64,
}

/// I048/120 (§5.2.15).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RadialDoppler {
    /// Subfield 1: (doubtful, speed in m/s, sign per the sending system's ICD).
    pub calculated: Option<(bool, i32)>,
    /// Subfield 2 entries: (doppler m/s, ambiguity range m/s, transmitter frequency MHz).
    pub raw: Vec<(u16, u16, u16)>,
}

/// A data item this build carries without interpreting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawItem {
    /// The item's name in the specification, for example `I048/230`.
    pub item: &'static str,
    pub octets: Vec<u8>,
}

/// One Category 048 record, every UAP item accounted for.
///
/// Items are typed when the model or a plausible consumer can use them and raw
/// otherwise; the raw ones are listed in [`Self::carried_raw`] with their item name,
/// which is how "unsupported fields reported" in the verification table is met.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// Absolute offset of the record's FSPEC in the input.
    pub offset: usize,
    pub data_source: Option<DataSource>,
    /// I048/140: seconds since midnight UTC, resolution 1/128 s.
    pub time_of_day_s: Option<f64>,
    pub descriptor: Option<TargetDescriptor>,
    pub polar: Option<PolarPosition>,
    pub mode_3a: Option<Mode3A>,
    pub flight_level: Option<FlightLevel>,
    pub plot_characteristics: Option<PlotCharacteristics>,
    /// I048/220: 24-bit ICAO aircraft address.
    pub aircraft_address: Option<u32>,
    /// I048/240: up to eight characters, trailing spaces removed.
    pub aircraft_identification: Option<String>,
    pub bds_registers: Vec<BdsRegister>,
    /// I048/161, 0 to 4095.
    pub track_number: Option<u16>,
    pub cartesian: Option<CartesianPosition>,
    pub velocity: Option<PolarVelocity>,
    pub track_status: Option<TrackStatus>,
    pub track_quality: Option<TrackQuality>,
    /// I048/030 codes, in order.
    pub warnings: Vec<u8>,
    /// I048/110: height from a 3D radar, feet above mean sea level.
    pub height_3d_ft: Option<f64>,
    pub doppler: Option<RadialDoppler>,
    pub carried_raw: Vec<RawItem>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Decode every Category 048 record in `bytes`, losslessly.
///
/// Input may hold several data blocks, all of which must be Category 048; a block
/// of another category is a [`InteropError::WrongCategory`]. A live radar feed does
/// not satisfy that: the public capture in `testdata/asterix/` shows one datagram
/// carrying a Category 048 block followed by a Category 034 block. The adapter that
/// owns the feed therefore splits with [`super::data_blocks`] and hands each block
/// to [`decode_block`] or to the Category 034 decoder by its category.
pub fn decode_records(bytes: &[u8]) -> Result<Vec<Record>, InteropError> {
    let mut records = Vec::new();
    for block in data_blocks(CODEC_NAME, bytes)? {
        records.extend(decode_block(&block)?);
    }
    Ok(records)
}

/// Decode the records of one data block that must be Category 048.
pub fn decode_block(block: &DataBlock<'_>) -> Result<Vec<Record>, InteropError> {
    if block.category != CATEGORY {
        return Err(InteropError::WrongCategory {
            codec: CODEC_NAME,
            expected: CATEGORY,
            found: block.category,
            offset: block.offset,
        });
    }
    let mut records = Vec::new();
    let mut cur = Cursor::new(CODEC_NAME, block.payload, block.offset + 3);
    while !cur.is_empty() {
        records.push(parse_record(&mut cur)?);
    }
    Ok(records)
}

/// One match arm per FRN of the standard UAP, in Table 2 order, so the function reads
/// against the table; splitting it would separate the table from itself.
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
                let b = cur.take(2, "I048/010")?;
                r.data_source = Some(DataSource {
                    sac: b[0],
                    sic: b[1],
                });
            }
            2 => r.time_of_day_s = Some(f64::from(cur.u24("I048/140")?) / 128.0),
            3 => {
                let octets = cur.extended("I048/020")?;
                let first = octets[0];
                r.descriptor = Some(TargetDescriptor {
                    report_type: ReportType::from_bits(first >> 5),
                    simulated: first & 0x10 != 0,
                    rdp_chain_2: first & 0x08 != 0,
                    spi: first & 0x04 != 0,
                    fixed_transponder: first & 0x02 != 0,
                    extensions: octets[1..].to_vec(),
                });
            }
            4 => {
                let rho = cur.u16("I048/040 RHO")?;
                let theta = cur.u16("I048/040 THETA")?;
                r.polar = Some(PolarPosition {
                    slant_range_nm: f64::from(rho) / 256.0,
                    azimuth_deg: f64::from(theta) * 360.0 / 65_536.0,
                });
            }
            5 => {
                let v = cur.u16("I048/070")?;
                r.mode_3a = Some(Mode3A {
                    validated: v & 0x8000 == 0,
                    garbled: v & 0x4000 != 0,
                    from_local_tracker: v & 0x2000 != 0,
                    code: v & 0x0FFF,
                });
            }
            6 => {
                let v = cur.u16("I048/090")?;
                r.flight_level = Some(FlightLevel {
                    validated: v & 0x8000 == 0,
                    garbled: v & 0x4000 != 0,
                    level: f64::from(sign_extend(v & 0x3FFF, 14)) / 4.0,
                });
            }
            7 => r.plot_characteristics = Some(parse_plot_characteristics(cur)?),
            8 => r.aircraft_address = Some(cur.u24("I048/220")?),
            9 => {
                let b = cur.take(6, "I048/240")?;
                r.aircraft_identification = Some(decode_ia5_6bit(b));
            }
            10 => {
                let rep = usize::from(cur.u8("I048/250 REP")?);
                for _ in 0..rep {
                    let b = cur.take(8, "I048/250 register")?;
                    let mut data = [0u8; 7];
                    data.copy_from_slice(&b[..7]);
                    r.bds_registers.push(BdsRegister {
                        data,
                        bds1: b[7] >> 4,
                        bds2: b[7] & 0x0F,
                    });
                }
            }
            11 => r.track_number = Some(cur.u16("I048/161")? & 0x0FFF),
            12 => {
                let x = cur.i16("I048/042 X")?;
                let y = cur.i16("I048/042 Y")?;
                r.cartesian = Some(CartesianPosition {
                    x_nm: f64::from(x) / 128.0,
                    y_nm: f64::from(y) / 128.0,
                });
            }
            13 => {
                let gs = cur.u16("I048/200 speed")?;
                let hdg = cur.u16("I048/200 heading")?;
                r.velocity = Some(PolarVelocity {
                    ground_speed_nm_s: f64::from(gs) / 16_384.0,
                    heading_deg: f64::from(hdg) * 360.0 / 65_536.0,
                });
            }
            14 => {
                let octets = cur.extended("I048/170")?;
                let first = octets[0];
                r.track_status = Some(TrackStatus {
                    tentative: first & 0x80 != 0,
                    sensor: match (first >> 5) & 0b11 {
                        0 => TrackSensor::Combined,
                        1 => TrackSensor::Psr,
                        2 => TrackSensor::SsrModeS,
                        _ => TrackSensor::Invalid,
                    },
                    low_association_confidence: first & 0x10 != 0,
                    horizontal_manoeuvre: first & 0x08 != 0,
                    climb_descent: (first >> 1) & 0b11,
                    extensions: octets[1..].to_vec(),
                });
            }
            15 => {
                let b = cur.take(4, "I048/210")?;
                r.track_quality = Some(TrackQuality {
                    sigma_x_nm: f64::from(b[0]) / 128.0,
                    sigma_y_nm: f64::from(b[1]) / 128.0,
                    sigma_v_nm_s: f64::from(b[2]) / 16_384.0,
                    sigma_h_deg: f64::from(b[3]) * 360.0 / 4096.0,
                });
            }
            16 => {
                let octets = cur.extended("I048/030")?;
                r.warnings = octets.iter().map(|o| o >> 1).collect();
            }
            17 => raw(&mut r, "I048/080", cur.take(2, "I048/080")?),
            18 => raw(&mut r, "I048/100", cur.take(4, "I048/100")?),
            19 => {
                let v = cur.u16("I048/110")?;
                r.height_3d_ft = Some(f64::from(sign_extend(v & 0x3FFF, 14)) * 25.0);
            }
            20 => r.doppler = Some(parse_doppler(cur)?),
            21 => raw(&mut r, "I048/230", cur.take(2, "I048/230")?),
            22 => raw(&mut r, "I048/260", cur.take(7, "I048/260")?),
            23 => raw(&mut r, "I048/055", cur.take(1, "I048/055")?),
            24 => raw(&mut r, "I048/050", cur.take(2, "I048/050")?),
            25 => raw(&mut r, "I048/065", cur.take(1, "I048/065")?),
            26 => raw(&mut r, "I048/060", cur.take(2, "I048/060")?),
            27 => raw(&mut r, "I048/SP", cur.explicit("I048/SP")?),
            28 => raw(&mut r, "I048/RE", cur.explicit("I048/RE")?),
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

/// I048/130: a compound item whose primary subfield may extend (FX). Edition 1.32
/// defines seven one-octet subfields and no extension, so a set FX or a subfield the
/// edition does not define is an error rather than a guess at its length.
fn parse_plot_characteristics(cur: &mut Cursor<'_>) -> Result<PlotCharacteristics, InteropError> {
    let primary = cur.extended("I048/130 primary subfield")?;
    if primary.len() > 1 {
        return Err(cur.error(format!(
            "I048/130 primary subfield extends past one octet, undefined in edition {EDITION}"
        )));
    }
    let p = primary[0];
    let mut pc = PlotCharacteristics::default();
    if p & 0x80 != 0 {
        pc.ssr_runlength_deg = Some(f64::from(cur.u8("I048/130 SRL")?) * 360.0 / 8192.0);
    }
    if p & 0x40 != 0 {
        pc.ssr_replies = Some(cur.u8("I048/130 SRR")?);
    }
    if p & 0x20 != 0 {
        pc.ssr_amplitude_dbm = Some(i8::from_ne_bytes([cur.u8("I048/130 SAM")?]));
    }
    if p & 0x10 != 0 {
        pc.psr_runlength_deg = Some(f64::from(cur.u8("I048/130 PRL")?) * 360.0 / 8192.0);
    }
    if p & 0x08 != 0 {
        pc.psr_amplitude_dbm = Some(i8::from_ne_bytes([cur.u8("I048/130 PAM")?]));
    }
    if p & 0x04 != 0 {
        let d = i8::from_ne_bytes([cur.u8("I048/130 RPD")?]);
        pc.psr_ssr_range_difference_nm = Some(f64::from(d) / 256.0);
    }
    if p & 0x02 != 0 {
        let d = i8::from_ne_bytes([cur.u8("I048/130 APD")?]);
        pc.psr_ssr_azimuth_difference_deg = Some(f64::from(d) * 360.0 / 16_384.0);
    }
    Ok(pc)
}

/// I048/120: subfield 1 is two octets, subfield 2 is `REP` groups of six octets.
fn parse_doppler(cur: &mut Cursor<'_>) -> Result<RadialDoppler, InteropError> {
    let primary = cur.extended("I048/120 primary subfield")?;
    if primary.len() > 1 {
        return Err(cur.error(format!(
            "I048/120 primary subfield extends past one octet, undefined in edition {EDITION}"
        )));
    }
    let p = primary[0];
    if p & 0b0011_1110 != 0 {
        return Err(cur.error(format!(
            "I048/120 flags a spare subfield, undefined in edition {EDITION}"
        )));
    }
    let mut d = RadialDoppler::default();
    if p & 0x80 != 0 {
        let v = cur.u16("I048/120 CAL")?;
        d.calculated = Some((v & 0x8000 != 0, sign_extend(v & 0x03FF, 10)));
    }
    if p & 0x40 != 0 {
        let rep = cur.u8("I048/120 REP")?;
        for _ in 0..rep {
            let dop = cur.u16("I048/120 DOP")?;
            let amb = cur.u16("I048/120 AMB")?;
            let frq = cur.u16("I048/120 FRQ")?;
            d.raw.push((dop, amb, frq));
        }
    }
    Ok(d)
}

/// ICAO Annex 10 six-bit character set as used by I048/240: 1..=26 are `A`..`Z`,
/// 32 is space, 48..=57 are `0`..`9`. Anything else is not a character the coding
/// defines and is rendered as `?` rather than dropped.
fn decode_ia5_6bit(b: &[u8]) -> String {
    let bits = u64::from_be_bytes([0, 0, b[0], b[1], b[2], b[3], b[4], b[5]]);
    let mut s = String::with_capacity(8);
    for i in (0..8).rev() {
        let code = ((bits >> (6 * i)) & 0x3F) as u8;
        s.push(match code {
            1..=26 => char::from(b'A' + code - 1),
            32 => ' ',
            48..=57 => char::from(b'0' + code - 48),
            _ => '?',
        });
    }
    s.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// Mapping to the model
// ---------------------------------------------------------------------------

pub use super::RadarSite;

/// What one record became.
#[derive(Debug, Clone, PartialEq)]
// A `Mapped` is transient -- one per record, mapped and matched at once -- so the
// size gap between a detection and a reason is not a cost worth a box (GAP-002 widened
// `DetectionView` past the lint's threshold on 2026-09-06).
#[allow(clippy::large_enum_variant)]
pub enum Mapped {
    Detection(DetectionView),
    /// The record is valid Category 048 but is not an observation: the reason says why.
    NotADetection(&'static str),
}

/// Category 048 decoder configured with the radars it may attribute reports to.
///
/// A record whose SAC/SIC is not configured is an [`InteropError::UnknownRadar`], not
/// a detection with a made-up sensor. The `Default` codec knows no radars, so it
/// decodes records losslessly ([`decode_records`]) and attributes none.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AsterixCat048Codec {
    sites: Vec<RadarSite>,
}

impl AsterixCat048Codec {
    pub fn new(sites: Vec<RadarSite>) -> Self {
        Self { sites }
    }

    pub fn sites(&self) -> &[RadarSite] {
        &self.sites
    }

    fn site(&self, source: DataSource) -> Option<&RadarSite> {
        self.sites
            .iter()
            .find(|s| s.sac == source.sac && s.sic == source.sic)
    }

    /// Map one record. `receipt_time` is when this system received the block; it also
    /// supplies the date that I048/140 lacks (see [`source_time`]).
    pub fn map(&self, record: &Record, receipt_time: MissionTime) -> Result<Mapped, InteropError> {
        let source = record.data_source.ok_or_else(|| InteropError::Malformed {
            codec: CODEC_NAME,
            offset: record.offset,
            reason: "I048/010 absent; a report with no data source cannot be attributed".into(),
        })?;
        let site = self.site(source).ok_or(InteropError::UnknownRadar {
            codec: CODEC_NAME,
            sac: source.sac,
            sic: source.sic,
        })?;
        if let Some(d) = &record.descriptor {
            if d.report_type == ReportType::NoDetection {
                return Ok(Mapped::NotADetection(
                    "I048/020 TYP = 0: no detection this scan; any position is extrapolated",
                ));
            }
        }

        let mut losses: Vec<&'static str> = Vec::new();
        let (source_time, time_loss) = source_time(record.time_of_day_s, receipt_time);
        losses.extend(time_loss);

        // Height above the radar's own level, when the report gives one.
        let height_m = if let Some(h) = record.height_3d_ft {
            Some(h * FT_M)
        } else if let Some(fl) = record.flight_level {
            losses.push(
                "height is pressure altitude from Mode C flight level (I048/090), not geometric",
            );
            Some(fl.level * 100.0 * FT_M)
        } else {
            None
        };

        let (east, north) = if let Some(p) = record.polar {
            let slant_m = p.slant_range_nm * NM_M;
            let ground_m = if let Some(h) = height_m {
                let dh = h - site.origin_enu_m[2];
                let g2 = slant_m * slant_m - dh * dh;
                if g2 < 0.0 {
                    losses.push("height exceeds slant range; ground range clamped to zero");
                    0.0
                } else {
                    g2.sqrt()
                }
            } else {
                losses.push("no height in report; slant range used as ground range");
                slant_m
            };
            let az = p.azimuth_deg.to_radians();
            (ground_m * az.sin(), ground_m * az.cos())
        } else if let Some(c) = record.cartesian {
            losses.push(
                "position is the radar's calculated Cartesian (I048/042), not the measured plot",
            );
            (c.x_nm * NM_M, c.y_nm * NM_M)
        } else {
            return Ok(Mapped::NotADetection(
                "no position (neither I048/040 nor I048/042); not an observation",
            ));
        };

        let up = if let Some(h) = height_m {
            h
        } else {
            losses.push("no height in report; up set to the radar site's height");
            site.origin_enu_m[2]
        };

        // DN-27 §8: a radar plot is a position and stays one; what changes is that the
        // position now states its error instead of leaving the gate downstream to guess
        // one. The variance is the tracking baseline's own
        // (`gungnir_fusion_async::PipelineSettings::measurement_noise_var`), restated
        // here rather than imported because this crate sits beneath that one; a
        // deployment with a stated per-radar accuracy should carry it on the
        // `RadarSite`, which is a change to this codec's configuration and not to the
        // mapping.
        let measurement = gungnir_model::Measurement::Position {
            enu: nalgebra::Vector3::new(
                site.origin_enu_m[0] + east,
                site.origin_enu_m[1] + north,
                up,
            ),
            variance_m2: BASELINE_PLOT_VARIANCE_M2,
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

/// I048/140 is seconds since midnight UTC with no date. The date is taken from
/// `receipt_time`, which in the live profiles is Unix time (`gungnir_model::time`),
/// and the result is folded to within twelve hours of receipt so a report time-stamped
/// just before midnight and received just after it lands on the right day.
///
/// With no time of day at all, the source time is the receipt time and that is
/// recorded as a loss.
pub fn source_time(
    time_of_day_s: Option<f64>,
    receipt_time: MissionTime,
) -> (MissionTime, Option<&'static str>) {
    let Some(tod) = time_of_day_s else {
        return (
            receipt_time,
            Some("no time of day (I048/140); source time set to receipt time"),
        );
    };
    if !receipt_time.0.is_finite() {
        return (
            receipt_time,
            Some("receipt time is not finite; cannot place time of day on a date"),
        );
    }
    let midnight = receipt_time.0 - receipt_time.0.rem_euclid(SECONDS_PER_DAY);
    let mut t = midnight + tod;
    if t - receipt_time.0 > SECONDS_PER_DAY / 2.0 {
        t -= SECONDS_PER_DAY;
    } else if receipt_time.0 - t > SECONDS_PER_DAY / 2.0 {
        t += SECONDS_PER_DAY;
    }
    (MissionTime(t), None)
}

impl DetectionCodec for AsterixCat048Codec {
    fn name(&self) -> &'static str {
        CODEC_NAME
    }

    /// Every record that is an observation, in wire order. Records that are valid but
    /// not observations (`TYP = 0`, or no position) are left out; use
    /// [`decode_records`] and [`AsterixCat048Codec::map`] to see them and why.
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

    /// Not implemented, by design: see the module documentation.
    fn encode(&self, _tracks: &[TrackView]) -> Result<Vec<u8>, InteropError> {
        Err(InteropError::NotImplemented(CODEC_NAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::SensorId;

    /// One record with I048/010, /140, /020, /040, /090: FSPEC 0xF4 (FRNs 1, 2, 3, 4, 6).
    /// Time 12:00:00 (43 200 s × 128 = `0x54_6000`), TYP = 2 (single SSR), rho 1 NM,
    /// theta 90°, FL 10 (× 4 = 40).
    fn hand_built_block() -> Vec<u8> {
        let mut b = vec![0x30, 0x00, 0x00, 0xF4];
        b.extend_from_slice(&[0x01, 0x02]); // 010
        b.extend_from_slice(&[0x54, 0x60, 0x00]); // 140
        b.extend_from_slice(&[0x40]); // 020: TYP 2, no FX
        b.extend_from_slice(&[0x01, 0x00, 0x40, 0x00]); // 040
        b.extend_from_slice(&[0x00, 0x28]); // 090
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());
        b
    }

    fn site() -> RadarSite {
        RadarSite {
            sac: 1,
            sic: 2,
            sensor: SensorId(7),
            origin_enu_m: [100.0, 200.0, 50.0],
        }
    }

    #[test]
    fn decodes_hand_built_record_to_specified_lsbs() {
        let recs = decode_records(&hand_built_block()).expect("decodes");
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.data_source, Some(DataSource { sac: 1, sic: 2 }));
        assert!((r.time_of_day_s.expect("tod") - 43_200.0).abs() < 1e-9);
        assert_eq!(
            r.descriptor.as_ref().map(|d| d.report_type),
            Some(ReportType::SingleSsr)
        );
        let p = r.polar.expect("polar");
        assert!((p.slant_range_nm - 1.0).abs() < 1e-12);
        assert!((p.azimuth_deg - 90.0).abs() < 1e-12);
        let fl = r.flight_level.expect("fl");
        assert!(fl.validated && !fl.garbled);
        assert!((fl.level - 10.0).abs() < 1e-12);
        assert!(r.carried_raw.is_empty());
    }

    #[test]
    fn maps_polar_plot_into_local_frame_with_losses_recorded() {
        let codec = AsterixCat048Codec::new(vec![site()]);
        // Receipt at 12:00:05 on day 20 000 of Unix time.
        let receipt = MissionTime(20_000.0 * 86_400.0 + 43_205.0);
        let dets = codec.decode(&hand_built_block(), receipt).expect("decodes");
        assert_eq!(dets.len(), 1);
        let d = &dets[0];
        assert_eq!(d.sensor, SensorId(7));
        assert!((d.source_time.0 - (20_000.0 * 86_400.0 + 43_200.0)).abs() < 1e-6);
        // FL 10 = 1000 ft = 304.8 m above sea level; radar sits at 50 m, so the plot is
        // 254.8 m above the antenna and the ground range is sqrt(1852² − 254.8²).
        let h = 304.8;
        let ground = (1852.0_f64.powi(2) - (h - 50.0_f64).powi(2)).sqrt();
        let enu = d
            .measurement
            .position_enu()
            .expect("a mapped Category 048 report is a position");
        assert!((enu[0] - (100.0 + ground)).abs() < 1e-6);
        assert!(enu[1].abs() - 200.0 < 1e-6);
        assert!((enu[2] - h).abs() < 1e-9);
        let loss = d
            .provenance
            .conversion_loss
            .as_deref()
            .expect("loss recorded");
        assert!(loss.contains("pressure altitude"));
        assert_eq!(d.provenance.algorithm_version, "asterix.cat048/ed1.32");
    }

    #[test]
    fn unknown_radar_is_an_error_not_a_guess() {
        let codec = AsterixCat048Codec::default();
        assert!(matches!(
            codec.decode(&hand_built_block(), MissionTime(0.0)),
            Err(InteropError::UnknownRadar { sac: 1, sic: 2, .. })
        ));
    }

    #[test]
    fn no_detection_reports_are_not_detections() {
        let mut b = hand_built_block();
        b[9] = 0x00; // TYP = 0
        let codec = AsterixCat048Codec::new(vec![site()]);
        let recs = decode_records(&b).expect("decodes");
        assert!(matches!(
            codec.map(&recs[0], MissionTime(0.0)),
            Ok(Mapped::NotADetection(_))
        ));
        assert!(codec.decode(&b, MissionTime(0.0)).expect("ok").is_empty());
    }

    #[test]
    fn wrong_category_and_undefined_frn_are_refused() {
        let mut other = hand_built_block();
        other[0] = 0x22;
        assert!(matches!(
            decode_records(&other),
            Err(InteropError::WrongCategory { found: 34, .. })
        ));
        // FSPEC with five octets: the fifth flags FRN 29.
        let bad = [0x30, 0x00, 0x08, 0x01, 0x01, 0x01, 0x01, 0x80];
        assert!(matches!(
            decode_records(&bad),
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
            } else if n == 3 {
                assert!(result.expect("empty block").is_empty());
            } else {
                assert!(result.is_err(), "length {n} should not decode");
            }
        }
    }

    #[test]
    fn source_time_folds_across_midnight() {
        let day = 20_000.0 * SECONDS_PER_DAY;
        // Reported 23:59:59, received 00:00:02 the next day: previous day's stamp.
        let (t, loss) = source_time(Some(86_399.0), MissionTime(day + 2.0));
        assert!(loss.is_none());
        assert!((t.0 - (day - 1.0)).abs() < 1e-6);
        // Reported 00:00:01, received 23:59:58 the day before: next day's stamp.
        let (t, _) = source_time(Some(1.0), MissionTime(day - 2.0));
        assert!((t.0 - (day + 1.0)).abs() < 1e-6);
        let (t, loss) = source_time(None, MissionTime(5.0));
        assert!((t.0 - 5.0).abs() < 1e-12);
        assert!(loss.is_some());
    }

    #[test]
    fn six_bit_identification_decodes() {
        // "ABC123  " in six-bit: A=1 B=2 C=3 '1'=49 '2'=50 '3'=51 ' '=32 ' '=32.
        let codes: [u64; 8] = [1, 2, 3, 49, 50, 51, 32, 32];
        let mut bits = 0u64;
        for c in codes {
            bits = (bits << 6) | c;
        }
        let b = bits.to_be_bytes();
        assert_eq!(decode_ia5_6bit(&b[2..]), "ABC123");
    }

    #[test]
    fn encode_stays_not_implemented() {
        assert!(matches!(
            AsterixCat048Codec::default().encode(&[]),
            Err(InteropError::NotImplemented(CODEC_NAME))
        ));
    }
}
