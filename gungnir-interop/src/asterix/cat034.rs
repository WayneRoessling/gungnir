//! EUROCONTROL ASTERIX Category 034, monoradar service messages.
//!
//! Built to EUROCONTROL-SPEC-0149-2b **edition 1.29** (15 March 2021), pinned on
//! 2026-09-06 in `docs/design/external-standards.md` §1.7; section numbers cited as
//! §5.2.x below are that specification's. Framing is Part I edition 3.1 (`super`).
//!
//! A service message is not a detection, which is why this module has its own
//! boundary rather than implementing `DetectionCodec`: the record layer
//! ([`decode_records`]) is lossless over the standard UAP (§5.3, Table 3), and the
//! mapping layer ([`AsterixCat034Codec`]) produces a [`RadarServiceReport`], the
//! sector timing and operational status that Category 048 target reports depend on
//! and that `gungnir-sensor-management` and the time model consume through the
//! ingest adapter (GAP-001).
//!
//! What the specification promises (§4.3): a rotating antenna sends exactly one north
//! marker per revolution, and sector crossings, when sent, are thirty-two per
//! revolution. Neither promise is enforced here; a consumer that counts them can tell
//! a stalled antenna from a silent link, and that is its job, not the decoder's.

use super::{data_blocks, Cursor, DataBlock, Fspec, RadarSite};
use crate::asterix::cat048::{source_time, DataSource};
use crate::{InteropError, ServiceMessageCodec};
use gungnir_model::{MissionTime, SensorId};

/// The codec's name in the schema catalog.
pub const CODEC_NAME: &str = "asterix.cat034";
/// The Category 034 edition this decoder is built to.
pub const EDITION: &str = "1.29";

const CATEGORY: u8 = 34;
const MAX_FRN: usize = 14;

// ---------------------------------------------------------------------------
// Typed record
// ---------------------------------------------------------------------------

/// I034/000 (§5.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    NorthMarker,
    SectorCrossing,
    GeographicalFiltering,
    JammingStrobe,
    SolarStorm,
    SsrJammingStrobe,
    ModeSJammingStrobe,
    /// A value edition 1.29 does not define; carried, not interpreted.
    Undefined(u8),
}

impl MessageType {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::NorthMarker,
            2 => Self::SectorCrossing,
            3 => Self::GeographicalFiltering,
            4 => Self::JammingStrobe,
            5 => Self::SolarStorm,
            6 => Self::SsrJammingStrobe,
            7 => Self::ModeSJammingStrobe,
            other => Self::Undefined(other),
        }
    }
}

/// I034/020 (§5.2.3): the sector, 0 to 255, and its start azimuth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sector {
    pub number: u8,
    /// `number` × 360/256 degrees, clockwise from north.
    pub azimuth_deg: f64,
}

/// I034/050 COM subfield (§5.2.6): status of the system's common elements.
///
/// Field names follow the specification's flags; each is stored as the condition
/// the flag reports, so `true` always means the abnormal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct CommonStatus {
    /// NOGO: operational use is inhibited and an operational SDPS shall discard the data.
    pub operational_use_inhibited: bool,
    /// RDPC: chain 2 is selected (chain 1 otherwise).
    pub rdp_chain_2: bool,
    /// RDPR: the selected chain was reset; expect new track numbers.
    pub rdp_reset: bool,
    /// OVL RDP.
    pub rdp_overload: bool,
    /// OVL XMT.
    pub transmission_overload: bool,
    /// MSC: monitoring system disconnected.
    pub monitoring_disconnected: bool,
    /// TSV: time source invalid.
    pub time_source_invalid: bool,
}

/// I034/050 (§5.2.6). The sensor-specific subfields are carried raw: their bit
/// layouts are stable but nothing in the model consumes them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemStatus {
    pub common: Option<CommonStatus>,
    pub psr: Option<u8>,
    pub ssr: Option<u8>,
    pub mode_s: Option<u16>,
}

/// I034/060 (§5.2.7). `common` is (RED-RDP, RED-XMT), the reduction steps in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessingMode {
    pub common: Option<(u8, u8)>,
    pub psr: Option<u8>,
    pub ssr: Option<u8>,
    pub mode_s: Option<u8>,
}

/// One entry of I034/070 (§5.2.8): a report type code (§5.2.8's table) and its count
/// for the last antenna revolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageCount {
    pub report_type: u8,
    pub count: u16,
}

/// I034/100 (§5.2.10): a polar window, ranges in NM, azimuths in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolarWindow {
    pub range_start_nm: f64,
    pub range_end_nm: f64,
    pub azimuth_start_deg: f64,
    pub azimuth_end_deg: f64,
}

/// I034/120 (§5.2.12): the data source's position in WGS 84.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataSourcePosition {
    /// Metres above the WGS 84 ellipsoid.
    pub height_m: f64,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
}

/// I034/090 (§5.2.9).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollimationError {
    pub range_nm: f64,
    pub azimuth_deg: f64,
}

/// A data item this build carries without interpreting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawItem {
    pub item: &'static str,
    pub octets: Vec<u8>,
}

/// One Category 034 record, every UAP item accounted for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// Absolute offset of the record's FSPEC in the input.
    pub offset: usize,
    pub data_source: Option<DataSource>,
    pub message_type: Option<MessageType>,
    /// I034/030: seconds since midnight UTC, resolution 1/128 s.
    pub time_of_day_s: Option<f64>,
    pub sector: Option<Sector>,
    /// I034/041: the antenna rotation period in seconds (the item's name says
    /// "speed"; the specification's definition and unit say period).
    pub rotation_period_s: Option<f64>,
    pub system_status: Option<SystemStatus>,
    pub processing_mode: Option<ProcessingMode>,
    pub message_counts: Vec<MessageCount>,
    pub polar_window: Option<PolarWindow>,
    /// I034/110 filter code (§5.2.11's table).
    pub data_filter: Option<u8>,
    pub position: Option<DataSourcePosition>,
    pub collimation_error: Option<CollimationError>,
    pub carried_raw: Vec<RawItem>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Decode every Category 034 record in `bytes`, losslessly. Every block must be
/// Category 034; see `cat048::decode_records` for why a live feed is split with
/// [`super::data_blocks`] first.
pub fn decode_records(bytes: &[u8]) -> Result<Vec<Record>, InteropError> {
    let mut records = Vec::new();
    for block in data_blocks(CODEC_NAME, bytes)? {
        records.extend(decode_block(&block)?);
    }
    Ok(records)
}

/// Decode the records of one data block that must be Category 034.
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

/// One match arm per FRN of the standard UAP, in Table 3 order.
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
                let b = cur.take(2, "I034/010")?;
                r.data_source = Some(DataSource {
                    sac: b[0],
                    sic: b[1],
                });
            }
            2 => r.message_type = Some(MessageType::from_code(cur.u8("I034/000")?)),
            3 => r.time_of_day_s = Some(f64::from(cur.u24("I034/030")?) / 128.0),
            4 => {
                let n = cur.u8("I034/020")?;
                r.sector = Some(Sector {
                    number: n,
                    azimuth_deg: f64::from(n) * 360.0 / 256.0,
                });
            }
            5 => r.rotation_period_s = Some(f64::from(cur.u16("I034/041")?) / 128.0),
            6 => r.system_status = Some(parse_system_status(cur)?),
            7 => r.processing_mode = Some(parse_processing_mode(cur)?),
            8 => {
                let rep = cur.u8("I034/070 REP")?;
                for _ in 0..rep {
                    let v = cur.u16("I034/070 count")?;
                    r.message_counts.push(MessageCount {
                        report_type: u8::try_from(v >> 11).unwrap_or(u8::MAX),
                        count: v & 0x07FF,
                    });
                }
            }
            9 => {
                let rho_start = cur.u16("I034/100 RHO-START")?;
                let rho_end = cur.u16("I034/100 RHO-END")?;
                let theta_start = cur.u16("I034/100 THETA-START")?;
                let theta_end = cur.u16("I034/100 THETA-END")?;
                r.polar_window = Some(PolarWindow {
                    range_start_nm: f64::from(rho_start) / 256.0,
                    range_end_nm: f64::from(rho_end) / 256.0,
                    azimuth_start_deg: f64::from(theta_start) * 360.0 / 65_536.0,
                    azimuth_end_deg: f64::from(theta_end) * 360.0 / 65_536.0,
                });
            }
            10 => r.data_filter = Some(cur.u8("I034/110")?),
            11 => {
                let height = cur.i16("I034/120 height")?;
                let lat = cur.u24("I034/120 latitude")?;
                let lon = cur.u24("I034/120 longitude")?;
                r.position = Some(DataSourcePosition {
                    height_m: f64::from(height),
                    latitude_deg: f64::from(sign_extend_24(lat)) * 180.0 / 8_388_608.0,
                    longitude_deg: f64::from(sign_extend_24(lon)) * 180.0 / 8_388_608.0,
                });
            }
            12 => {
                let b = cur.take(2, "I034/090")?;
                r.collimation_error = Some(CollimationError {
                    range_nm: f64::from(i8::from_ne_bytes([b[0]])) / 128.0,
                    azimuth_deg: f64::from(i8::from_ne_bytes([b[1]])) * 360.0 / 16_384.0,
                });
            }
            13 => raw(&mut r, "I034/RE", cur.explicit("I034/RE")?),
            14 => raw(&mut r, "I034/SP", cur.explicit("I034/SP")?),
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

/// 24-bit two's complement to i32.
fn sign_extend_24(raw: u32) -> i32 {
    let widened = i32::from_ne_bytes((raw << 8).to_ne_bytes());
    widened >> 8
}

/// The primary subfield shared by I034/050 and I034/060: `COM 0 0 PSR SSR MDS 0 FX`.
/// A set spare bit or an extension octet is undefined in edition 1.29, so it is an
/// error rather than a guess at a subfield's length.
fn compound_primary(cur: &mut Cursor<'_>, item: &str) -> Result<u8, InteropError> {
    let primary = cur.extended(item)?;
    if primary.len() > 1 {
        return Err(cur.error(format!(
            "{item} primary subfield extends past one octet, undefined in edition {EDITION}"
        )));
    }
    let p = primary[0];
    if p & 0b0110_0010 != 0 {
        return Err(cur.error(format!(
            "{item} flags a spare subfield, undefined in edition {EDITION}"
        )));
    }
    Ok(p)
}

/// I034/050: COM, PSR, and SSR are one octet each; MDS is two.
fn parse_system_status(cur: &mut Cursor<'_>) -> Result<SystemStatus, InteropError> {
    let p = compound_primary(cur, "I034/050")?;
    let mut s = SystemStatus::default();
    if p & 0x80 != 0 {
        let c = cur.u8("I034/050 COM")?;
        s.common = Some(CommonStatus {
            operational_use_inhibited: c & 0x80 != 0,
            rdp_chain_2: c & 0x40 != 0,
            rdp_reset: c & 0x20 != 0,
            rdp_overload: c & 0x10 != 0,
            transmission_overload: c & 0x08 != 0,
            monitoring_disconnected: c & 0x04 != 0,
            time_source_invalid: c & 0x02 != 0,
        });
    }
    if p & 0x10 != 0 {
        s.psr = Some(cur.u8("I034/050 PSR")?);
    }
    if p & 0x08 != 0 {
        s.ssr = Some(cur.u8("I034/050 SSR")?);
    }
    if p & 0x04 != 0 {
        s.mode_s = Some(cur.u16("I034/050 MDS")?);
    }
    Ok(s)
}

/// I034/060: every subfield is one octet.
fn parse_processing_mode(cur: &mut Cursor<'_>) -> Result<ProcessingMode, InteropError> {
    let p = compound_primary(cur, "I034/060")?;
    let mut m = ProcessingMode::default();
    if p & 0x80 != 0 {
        let c = cur.u8("I034/060 COM")?;
        m.common = Some(((c >> 4) & 0b111, (c >> 1) & 0b111));
    }
    if p & 0x10 != 0 {
        m.psr = Some(cur.u8("I034/060 PSR")?);
    }
    if p & 0x08 != 0 {
        m.ssr = Some(cur.u8("I034/060 SSR")?);
    }
    if p & 0x04 != 0 {
        m.mode_s = Some(cur.u8("I034/060 MDS")?);
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// The service-message boundary
// ---------------------------------------------------------------------------

/// What a service message announces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServiceEvent {
    /// The antenna crossed north: one per revolution (§4.3).
    NorthMarker,
    /// The antenna entered this sector: thirty-two per revolution when sent (§4.3).
    SectorCrossing(Sector),
    GeographicalFiltering,
    JammingStrobe,
    SolarStorm,
    SsrJammingStrobe,
    ModeSJammingStrobe,
    /// A message type edition 1.29 does not define.
    Undefined(u8),
}

/// The operational status a service message carries for the whole system, from
/// I034/050's common subfield. Absent when the message did not include it: a
/// consumer must not read absence as "released".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct OperationalStatus {
    /// NOGO clear: the data may be used operationally.
    pub released_for_operational_use: bool,
    /// The radar data processor or the transmission subsystem is overloaded.
    pub overloaded: bool,
    /// TSV: the radar's own time stamps are not to be trusted.
    pub time_source_invalid: bool,
    /// RDPR: track numbers in following Category 048 reports restart.
    pub track_numbers_reset: bool,
}

/// One service message, attributed and timed: the unit the ingest adapter hands to
/// `gungnir-sensor-management` (status, rotation) and the time model (sector timing).
#[derive(Debug, Clone, PartialEq)]
pub struct RadarServiceReport {
    pub sensor: SensorId,
    /// I034/030 placed on the receipt date (`cat048::source_time`).
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
    pub event: ServiceEvent,
    pub status: Option<OperationalStatus>,
    /// I034/041, when sent.
    pub rotation_period_s: Option<f64>,
    /// I034/120, when sent: where the radar says it is.
    pub position: Option<DataSourcePosition>,
    /// Anything the mapping could not do faithfully, in words.
    pub conversion_loss: Option<String>,
}

/// Category 034 decoder configured with the radars it may attribute messages to.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AsterixCat034Codec {
    sites: Vec<RadarSite>,
}

impl AsterixCat034Codec {
    pub fn new(sites: Vec<RadarSite>) -> Self {
        Self { sites }
    }

    pub fn sites(&self) -> &[RadarSite] {
        &self.sites
    }

    /// Map one record. I034/010 and I034/000 are mandatory in every message
    /// (§5.2.1's table), so their absence is malformed input; a missing time of day
    /// is recorded as a loss.
    pub fn map(
        &self,
        record: &Record,
        receipt_time: MissionTime,
    ) -> Result<RadarServiceReport, InteropError> {
        let source = record.data_source.ok_or_else(|| InteropError::Malformed {
            codec: CODEC_NAME,
            offset: record.offset,
            reason: "I034/010 absent; a service message with no data source cannot be attributed"
                .into(),
        })?;
        let site = self
            .sites
            .iter()
            .find(|s| s.sac == source.sac && s.sic == source.sic)
            .ok_or(InteropError::UnknownRadar {
                codec: CODEC_NAME,
                sac: source.sac,
                sic: source.sic,
            })?;
        let message_type = record.message_type.ok_or_else(|| InteropError::Malformed {
            codec: CODEC_NAME,
            offset: record.offset,
            reason: "I034/000 absent; every service message carries its type".into(),
        })?;
        let mut losses: Vec<&'static str> = Vec::new();
        let (source_time, time_loss) = source_time(record.time_of_day_s, receipt_time);
        losses.extend(time_loss);

        let event = match message_type {
            MessageType::NorthMarker => ServiceEvent::NorthMarker,
            MessageType::SectorCrossing => match record.sector {
                Some(s) => ServiceEvent::SectorCrossing(s),
                None => {
                    return Err(InteropError::Malformed {
                        codec: CODEC_NAME,
                        offset: record.offset,
                        reason: "sector crossing message without I034/020".into(),
                    })
                }
            },
            MessageType::GeographicalFiltering => ServiceEvent::GeographicalFiltering,
            MessageType::JammingStrobe => ServiceEvent::JammingStrobe,
            MessageType::SolarStorm => ServiceEvent::SolarStorm,
            MessageType::SsrJammingStrobe => ServiceEvent::SsrJammingStrobe,
            MessageType::ModeSJammingStrobe => ServiceEvent::ModeSJammingStrobe,
            MessageType::Undefined(code) => ServiceEvent::Undefined(code),
        };
        if matches!(
            event,
            ServiceEvent::GeographicalFiltering
                | ServiceEvent::JammingStrobe
                | ServiceEvent::SsrJammingStrobe
                | ServiceEvent::ModeSJammingStrobe
        ) {
            losses.push("polar window and data filter of this message are decoded on the record but not carried on the report");
        }

        let status = record
            .system_status
            .and_then(|s| s.common)
            .map(|c| OperationalStatus {
                released_for_operational_use: !c.operational_use_inhibited,
                overloaded: c.rdp_overload || c.transmission_overload,
                time_source_invalid: c.time_source_invalid,
                track_numbers_reset: c.rdp_reset,
            });

        Ok(RadarServiceReport {
            sensor: site.sensor,
            source_time,
            receipt_time,
            event,
            status,
            rotation_period_s: record.rotation_period_s,
            position: record.position,
            conversion_loss: if losses.is_empty() {
                None
            } else {
                Some(losses.join("; "))
            },
        })
    }
}

impl ServiceMessageCodec for AsterixCat034Codec {
    fn name(&self) -> &'static str {
        CODEC_NAME
    }

    fn decode(
        &self,
        bytes: &[u8],
        receipt_time: MissionTime,
    ) -> Result<Vec<RadarServiceReport>, InteropError> {
        decode_records(bytes)?
            .iter()
            .map(|r| self.map(r, receipt_time))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sector crossing with status: FSPEC 0xFE (FRNs 1 to 7), 010 = 25/11, type 2,
    /// time 06:00:00 (21 600 × 128 = `0x2A_3000`), sector 64 (90°), rotation 4 s
    /// (512 = `0x0200`), 050 with COM (NOGO clear, TSV set) and MDS, 060 with COM.
    fn hand_built_block() -> Vec<u8> {
        let mut b = vec![0x22, 0x00, 0x00, 0xFE];
        b.extend_from_slice(&[25, 11]); // 010
        b.push(2); // 000
        b.extend_from_slice(&[0x2A, 0x30, 0x00]); // 030
        b.push(64); // 020
        b.extend_from_slice(&[0x02, 0x00]); // 041
        b.extend_from_slice(&[0x84, 0x02, 0x80, 0x00]); // 050: COM+MDS; COM TSV; MDS ANT=1
        b.extend_from_slice(&[0x80, 0x24]); // 060: COM; RED-RDP 2, RED-XMT 2
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());
        b
    }

    fn site() -> RadarSite {
        RadarSite {
            sac: 25,
            sic: 11,
            sensor: SensorId(3),
            origin_enu_m: [0.0, 0.0, 0.0],
        }
    }

    #[test]
    fn decodes_hand_built_record_to_specified_lsbs() {
        let recs = decode_records(&hand_built_block()).expect("decodes");
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.data_source, Some(DataSource { sac: 25, sic: 11 }));
        assert_eq!(r.message_type, Some(MessageType::SectorCrossing));
        assert!((r.time_of_day_s.expect("tod") - 21_600.0).abs() < 1e-9);
        let s = r.sector.expect("sector");
        assert_eq!(s.number, 64);
        assert!((s.azimuth_deg - 90.0).abs() < 1e-12);
        assert!((r.rotation_period_s.expect("period") - 4.0).abs() < 1e-12);
        let st = r.system_status.expect("status");
        let c = st.common.expect("common");
        assert!(!c.operational_use_inhibited && c.time_source_invalid && !c.rdp_reset);
        assert_eq!(st.mode_s, Some(0x8000));
        assert_eq!(st.psr, None);
        assert_eq!(r.processing_mode.expect("mode").common, Some((2, 2)));
        assert!(r.carried_raw.is_empty());
    }

    #[test]
    fn maps_to_a_service_report() {
        let codec = AsterixCat034Codec::new(vec![site()]);
        let receipt = MissionTime(20_000.0 * 86_400.0 + 21_605.0);
        let reports = codec.decode(&hand_built_block(), receipt).expect("maps");
        assert_eq!(reports.len(), 1);
        let rep = &reports[0];
        assert_eq!(rep.sensor, SensorId(3));
        assert!((rep.source_time.0 - (20_000.0 * 86_400.0 + 21_600.0)).abs() < 1e-6);
        assert!(matches!(
            rep.event,
            ServiceEvent::SectorCrossing(Sector { number: 64, .. })
        ));
        let st = rep.status.expect("status");
        assert!(st.released_for_operational_use && st.time_source_invalid && !st.overloaded);
        assert_eq!(rep.rotation_period_s, Some(4.0));
        assert!(rep.conversion_loss.is_none());
    }

    #[test]
    fn unknown_radar_and_missing_mandatory_items_are_errors() {
        let b = hand_built_block();
        assert!(matches!(
            AsterixCat034Codec::default().decode(&b, MissionTime(0.0)),
            Err(InteropError::UnknownRadar {
                sac: 25,
                sic: 11,
                ..
            })
        ));
        // Only I034/010: no message type.
        let no_type = [0x22, 0x00, 0x06, 0x80, 25, 11];
        let recs = decode_records(&no_type).expect("decodes");
        assert!(matches!(
            AsterixCat034Codec::new(vec![site()]).map(&recs[0], MissionTime(0.0)),
            Err(InteropError::Malformed { .. })
        ));
    }

    #[test]
    fn undefined_subfields_and_categories_are_refused() {
        let mut spare = hand_built_block();
        spare[13] = 0x86; // 050 primary with spare bit 2 set
        assert!(matches!(
            decode_records(&spare),
            Err(InteropError::Malformed { .. })
        ));
        let mut other = hand_built_block();
        other[0] = 0x30;
        assert!(matches!(
            decode_records(&other),
            Err(InteropError::WrongCategory { found: 48, .. })
        ));
        for n in 4..hand_built_block().len() {
            let mut cut = hand_built_block()[..n].to_vec();
            let len = u16::try_from(n).expect("small");
            cut[1..3].copy_from_slice(&len.to_be_bytes());
            assert!(
                decode_records(&cut).is_err(),
                "length {n} should not decode"
            );
        }
    }

    #[test]
    fn position_and_counts_decode() {
        // FSPEC 0x81 0x90: FRN 1, then FRN 8 (070, bit 8) and FRN 11 (120, bit 5).
        let mut b = vec![0x22, 0x00, 0x00, 0x81, 0x90, 25, 11];
        b.extend_from_slice(&[0x02, 0x08, 0x05, 0x10, 0x03]); // 070: REP 2; (1, 5), (2, 3)
                                                              // 120: height -5 m; lat 45° = 45 × 2^23/180 = 2 097 152 = 0x20_0000; lon -90°.
        b.extend_from_slice(&[0xFF, 0xFB, 0x20, 0x00, 0x00, 0xC0, 0x00, 0x00]);
        let len = u16::try_from(b.len()).expect("small");
        b[1..3].copy_from_slice(&len.to_be_bytes());
        let r = &decode_records(&b).expect("decodes")[0];
        assert_eq!(
            r.message_counts,
            vec![
                MessageCount {
                    report_type: 1,
                    count: 5
                },
                MessageCount {
                    report_type: 2,
                    count: 3
                }
            ]
        );
        let p = r.position.expect("position");
        assert!((p.height_m + 5.0).abs() < 1e-12);
        assert!((p.latitude_deg - 45.0).abs() < 1e-9);
        assert!((p.longitude_deg + 90.0).abs() < 1e-9);
    }
}
