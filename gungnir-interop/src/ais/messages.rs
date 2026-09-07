// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The eight message types this system reads, field by field from ITU-R M.1371-6
//! Annex 7 (the table numbers are that edition's): Messages 1, 2, 3 (Table 46), 5
//! (Table 50), 18 (Table 68), 19 (Table 69), 21 (Table 71), 24 (Tables 76 and 77).
//!
//! Values are raw: the unit and the "not available" sentinel are on each field's doc
//! comment, and the accessors that scale them say what they return for a sentinel.

use super::{AisError, BitField};

/// Sentinels the Recommendation uses across tables.
pub mod sentinel {
    /// Longitude 181°: not available (Table 46).
    pub const LONGITUDE_NOT_AVAILABLE: i64 = 0x0679_1AC0;
    /// Latitude 91°: not available.
    pub const LATITUDE_NOT_AVAILABLE: i64 = 0x0341_2140;
    /// Speed over ground 102.3 knots: not available.
    pub const SOG_NOT_AVAILABLE: u16 = 1023;
    /// Course over ground 360.0°: not available.
    pub const COG_NOT_AVAILABLE: u16 = 3600;
    /// True heading: not available.
    pub const HEADING_NOT_AVAILABLE: u16 = 511;
    /// Rate of turn: no turn information available.
    pub const ROT_NOT_AVAILABLE: i8 = -128;
    /// Time stamp 60: not available.
    pub const SECOND_NOT_AVAILABLE: u8 = 60;
}

/// Fields every message starts with (bits 0–37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub message_type: u8,
    pub repeat_indicator: u8,
    /// The station's Maritime Mobile Service Identity.
    pub mmsi: u32,
}

/// A position in the Recommendation's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawPosition {
    /// 1/10 000 minute, east positive; [`sentinel::LONGITUDE_NOT_AVAILABLE`] when absent.
    pub longitude: i64,
    /// 1/10 000 minute, north positive; [`sentinel::LATITUDE_NOT_AVAILABLE`] when absent.
    pub latitude: i64,
    /// Position accuracy flag: `true` is high (≤ 10 m).
    pub accuracy_high: bool,
}

impl RawPosition {
    /// Degrees, or `None` for either sentinel.
    #[must_use]
    pub fn degrees(&self) -> Option<(f64, f64)> {
        if self.longitude == sentinel::LONGITUDE_NOT_AVAILABLE
            || self.latitude == sentinel::LATITUDE_NOT_AVAILABLE
        {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let scale = |v: i64| v as f64 / 600_000.0;
        Some((scale(self.longitude), scale(self.latitude)))
    }
}

/// The ship's extent around its reported position, metres (Figure 38).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dimensions {
    pub to_bow: u16,
    pub to_stern: u16,
    pub to_port: u8,
    pub to_starboard: u8,
}

/// Messages 1, 2 and 3: the Class A position report (Table 46, 168 bits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionReport {
    pub header: Header,
    /// Navigational status 0–15; 15 is undefined (default).
    pub navigational_status: u8,
    /// `ROTais` as transmitted: ±127 are the "more than 5°/30 s" flags and −128 is not
    /// available.
    pub rate_of_turn: i8,
    /// 1/10 knot; [`sentinel::SOG_NOT_AVAILABLE`] when absent.
    pub speed_over_ground: u16,
    pub position: RawPosition,
    /// 1/10 degree; [`sentinel::COG_NOT_AVAILABLE`] when absent.
    pub course_over_ground: u16,
    /// Degrees; [`sentinel::HEADING_NOT_AVAILABLE`] when absent.
    pub true_heading: u16,
    /// UTC second the report was generated, or 60–63 with the meanings the table gives.
    pub time_stamp: u8,
    pub special_manoeuvre: u8,
    pub raim: bool,
    /// The SOTDMA or ITDMA communication state, 19 bits, uninterpreted.
    pub radio_status: u32,
}

/// Message 5: the Class A static and voyage data (Table 50, 424 bits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticVoyageData {
    pub header: Header,
    pub ais_version: u8,
    pub imo_number: u32,
    pub call_sign: String,
    pub name: String,
    pub ship_type: u8,
    pub dimensions: Dimensions,
    /// Type of electronic position fixing device, 0–15 (9 = BDS is new in -6).
    pub position_fixing_device: u8,
    /// Estimated time of arrival: month, day, hour, minute, each with its own "not
    /// available" value (0, 0, 24, 60).
    pub eta: (u8, u8, u8, u8),
    /// 1/10 metre; 0 when not available.
    pub draught: u8,
    pub destination: String,
    /// Data terminal equipment ready (`false` is ready; the bit is "not ready").
    pub dte_not_ready: bool,
}

/// Message 18: the Class B position report (Table 68, 168 bits).
// The table has seven single-bit flags; a bit-set would hide their names.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassBPositionReport {
    pub header: Header,
    pub speed_over_ground: u16,
    pub position: RawPosition,
    pub course_over_ground: u16,
    pub true_heading: u16,
    pub time_stamp: u8,
    pub class_b_unit_cs: bool,
    pub class_b_display: bool,
    pub class_b_dsc: bool,
    pub class_b_band: bool,
    pub class_b_message_22: bool,
    pub assigned_mode: bool,
    pub raim: bool,
    /// 20 bits: the communication state selector flag and the state, uninterpreted.
    pub radio_status: u32,
}

/// Message 19: the extended Class B report (Table 69, 312 bits). Legacy equipment only
/// in -6; still afloat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassBExtendedReport {
    pub header: Header,
    pub speed_over_ground: u16,
    pub position: RawPosition,
    pub course_over_ground: u16,
    pub true_heading: u16,
    pub time_stamp: u8,
    pub name: String,
    pub ship_type: u8,
    pub dimensions: Dimensions,
    pub position_fixing_device: u8,
    pub raim: bool,
    pub dte_not_ready: bool,
    pub assigned_mode: bool,
}

/// Message 21: the aids-to-navigation report (Table 71, 272 to 360 bits).
// Four single-bit flags in the table, each with its own meaning.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AidToNavigationReport {
    pub header: Header,
    /// Type of aid, 0–31 (Table 72).
    pub aid_type: u8,
    /// The name (120 bits).
    pub name: String,
    /// The variable-length name extension after bit 272, kept apart from `name` as the
    /// table keeps it. **Read only when the 120-bit name is full** (twenty characters
    /// with no terminator): the extension exists for names longer than twenty, and
    /// transmitters with a shorter name are seen padding the tail with characters that
    /// are not a name. [`AidToNavigationReport::full_name`] joins the two.
    pub name_extension: String,
    pub position: RawPosition,
    pub dimensions: Dimensions,
    pub position_fixing_device: u8,
    pub time_stamp: u8,
    pub off_position: bool,
    /// Eight bits reserved for regional or local use.
    pub regional: u8,
    pub raim: bool,
    pub virtual_aid: bool,
    pub assigned_mode: bool,
}

impl AidToNavigationReport {
    /// The name with its extension, as a person reads it.
    #[must_use]
    pub fn full_name(&self) -> String {
        format!("{}{}", self.name, self.name_extension)
    }
}

/// Message 24, part A or part B (Tables 76 and 77).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDataReport {
    pub header: Header,
    pub part: StaticDataReportPart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticDataReportPart {
    /// Part A (160 bits): the name.
    A { name: String },
    /// Part B (168 bits).
    B {
        ship_type: u8,
        /// The manufacturer's mnemonic, three characters (Table 77's 18-bit split).
        vendor_id: String,
        unit_model: u8,
        unit_serial: u32,
        call_sign: String,
        /// Dimensions for a vessel; the mother ship's MMSI for an auxiliary craft
        /// (an MMSI beginning `98`).
        extent: StaticExtent,
        position_fixing_device: u8,
        /// **New in -6**: the two bits -5 held spare. 0 = AIS only, 1 = VDES ASM,
        /// 2 = VDES ASM/VDE-TER, 3 = VDES ASM/VDE-TER/VDE-SAT.
        vdes_capabilities: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticExtent {
    Dimensions(Dimensions),
    MotherShip { mmsi: u32 },
}

/// A decoded message, or the type of one this decoder does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AisMessage {
    Position(PositionReport),
    StaticVoyage(StaticVoyageData),
    ClassBPosition(ClassBPositionReport),
    ClassBExtended(ClassBExtendedReport),
    AidToNavigation(AidToNavigationReport),
    StaticData(StaticDataReport),
    /// A type outside the eight in scope. Carried with its header so a caller can count
    /// what it is not reading; never mistaken for a decode.
    Unsupported {
        header: Header,
        bits: usize,
    },
}

impl AisMessage {
    #[must_use]
    pub fn header(&self) -> &Header {
        match self {
            AisMessage::Position(m) => &m.header,
            AisMessage::StaticVoyage(m) => &m.header,
            AisMessage::ClassBPosition(m) => &m.header,
            AisMessage::ClassBExtended(m) => &m.header,
            AisMessage::AidToNavigation(m) => &m.header,
            AisMessage::StaticData(m) => &m.header,
            AisMessage::Unsupported { header, .. } => header,
        }
    }
}

/// A reader that turns a missing field into the one error the caller needs.
struct Fields<'a> {
    bits: &'a BitField,
    message_type: u8,
    needed: usize,
}

impl Fields<'_> {
    fn short(&self) -> AisError {
        AisError::TooShort {
            message_type: self.message_type,
            bits: self.bits.len(),
            needed: self.needed,
        }
    }

    fn u8(&self, start: usize, len: usize) -> Result<u8, AisError> {
        let v = self.bits.u(start, len).ok_or_else(|| self.short())?;
        u8::try_from(v).map_err(|_| self.short())
    }

    fn u16(&self, start: usize, len: usize) -> Result<u16, AisError> {
        let v = self.bits.u(start, len).ok_or_else(|| self.short())?;
        u16::try_from(v).map_err(|_| self.short())
    }

    fn u32(&self, start: usize, len: usize) -> Result<u32, AisError> {
        let v = self.bits.u(start, len).ok_or_else(|| self.short())?;
        u32::try_from(v).map_err(|_| self.short())
    }

    fn i(&self, start: usize, len: usize) -> Result<i64, AisError> {
        self.bits.i(start, len).ok_or_else(|| self.short())
    }

    fn flag(&self, at: usize) -> Result<bool, AisError> {
        self.bits.flag(at).ok_or_else(|| self.short())
    }

    fn text(&self, start: usize, len: usize) -> Result<String, AisError> {
        if start + len > self.bits.len() {
            return Err(self.short());
        }
        Ok(self.bits.text(start, len))
    }

    fn position(&self, start: usize) -> Result<RawPosition, AisError> {
        Ok(RawPosition {
            accuracy_high: self.flag(start)?,
            longitude: self.i(start + 1, 28)?,
            latitude: self.i(start + 29, 27)?,
        })
    }

    fn dimensions(&self, start: usize) -> Result<Dimensions, AisError> {
        Ok(Dimensions {
            to_bow: self.u16(start, 9)?,
            to_stern: self.u16(start + 9, 9)?,
            to_port: self.u8(start + 18, 6)?,
            to_starboard: self.u8(start + 24, 6)?,
        })
    }
}

fn header(bits: &BitField) -> Result<Header, AisError> {
    let f = Fields {
        bits,
        message_type: 0,
        needed: 38,
    };
    Ok(Header {
        message_type: f.u8(0, 6)?,
        repeat_indicator: f.u8(6, 2)?,
        mmsi: f.u32(8, 30)?,
    })
}

/// Decode one payload.
///
/// # Errors
///
/// [`AisError::TooShort`] when the payload ends before its table does.
pub fn decode(bits: &BitField) -> Result<AisMessage, AisError> {
    let header = header(bits)?;
    let message_type = header.message_type;
    let fields = |needed| Fields {
        bits,
        message_type,
        needed,
    };
    Ok(match header.message_type {
        1..=3 => AisMessage::Position(position_report(header, &fields(168))?),
        5 => AisMessage::StaticVoyage(static_voyage(header, &fields(424))?),
        18 => AisMessage::ClassBPosition(class_b(header, &fields(168))?),
        19 => AisMessage::ClassBExtended(class_b_extended(header, &fields(312))?),
        21 => AisMessage::AidToNavigation(aid_to_navigation(header, &fields(272))?),
        24 => AisMessage::StaticData(static_data(header, bits)?),
        _ => AisMessage::Unsupported {
            header,
            bits: bits.len(),
        },
    })
}

fn position_report(header: Header, f: &Fields<'_>) -> Result<PositionReport, AisError> {
    let rot = f.i(42, 8)?;
    Ok(PositionReport {
        header,
        navigational_status: f.u8(38, 4)?,
        rate_of_turn: i8::try_from(rot).map_err(|_| f.short())?,
        speed_over_ground: f.u16(50, 10)?,
        position: f.position(60)?,
        course_over_ground: f.u16(116, 12)?,
        true_heading: f.u16(128, 9)?,
        time_stamp: f.u8(137, 6)?,
        special_manoeuvre: f.u8(143, 2)?,
        raim: f.flag(148)?,
        radio_status: f.u32(149, 19)?,
    })
}

fn static_voyage(header: Header, f: &Fields<'_>) -> Result<StaticVoyageData, AisError> {
    Ok(StaticVoyageData {
        header,
        ais_version: f.u8(38, 2)?,
        imo_number: f.u32(40, 30)?,
        call_sign: f.text(70, 42)?,
        name: f.text(112, 120)?,
        ship_type: f.u8(232, 8)?,
        dimensions: f.dimensions(240)?,
        position_fixing_device: f.u8(270, 4)?,
        eta: (f.u8(274, 4)?, f.u8(278, 5)?, f.u8(283, 5)?, f.u8(288, 6)?),
        draught: f.u8(294, 8)?,
        destination: f.text(302, 120)?,
        dte_not_ready: f.flag(422)?,
    })
}

fn class_b(header: Header, f: &Fields<'_>) -> Result<ClassBPositionReport, AisError> {
    Ok(ClassBPositionReport {
        header,
        speed_over_ground: f.u16(46, 10)?,
        position: f.position(56)?,
        course_over_ground: f.u16(112, 12)?,
        true_heading: f.u16(124, 9)?,
        time_stamp: f.u8(133, 6)?,
        class_b_unit_cs: f.flag(141)?,
        class_b_display: f.flag(142)?,
        class_b_dsc: f.flag(143)?,
        class_b_band: f.flag(144)?,
        class_b_message_22: f.flag(145)?,
        assigned_mode: f.flag(146)?,
        raim: f.flag(147)?,
        radio_status: f.u32(148, 20)?,
    })
}

fn class_b_extended(header: Header, f: &Fields<'_>) -> Result<ClassBExtendedReport, AisError> {
    Ok(ClassBExtendedReport {
        header,
        speed_over_ground: f.u16(46, 10)?,
        position: f.position(56)?,
        course_over_ground: f.u16(112, 12)?,
        true_heading: f.u16(124, 9)?,
        time_stamp: f.u8(133, 6)?,
        name: f.text(143, 120)?,
        ship_type: f.u8(263, 8)?,
        dimensions: f.dimensions(271)?,
        position_fixing_device: f.u8(301, 4)?,
        raim: f.flag(305)?,
        dte_not_ready: f.flag(306)?,
        assigned_mode: f.flag(307)?,
    })
}

fn aid_to_navigation(header: Header, f: &Fields<'_>) -> Result<AidToNavigationReport, AisError> {
    let name = f.text(43, 120)?;
    // The extension is whatever whole six-bit characters follow bit 272, and it is a
    // name only when the twenty-character field before it is full.
    let tail = f.bits.len().saturating_sub(272);
    let name_extension = if tail >= 6 && name.chars().count() == 20 {
        f.bits.text(272, tail - tail % 6)
    } else {
        String::new()
    };
    Ok(AidToNavigationReport {
        header,
        aid_type: f.u8(38, 5)?,
        name,
        name_extension,
        position: f.position(163)?,
        dimensions: f.dimensions(219)?,
        position_fixing_device: f.u8(249, 4)?,
        time_stamp: f.u8(253, 6)?,
        off_position: f.flag(259)?,
        regional: f.u8(260, 8)?,
        raim: f.flag(268)?,
        virtual_aid: f.flag(269)?,
        assigned_mode: f.flag(270)?,
    })
}

fn static_data(header: Header, bits: &BitField) -> Result<StaticDataReport, AisError> {
    let probe = Fields {
        bits,
        message_type: 24,
        needed: 40,
    };
    let part_number = probe.u8(38, 2)?;
    let part = if part_number == 0 {
        let f = Fields {
            bits,
            message_type: 24,
            needed: 160,
        };
        StaticDataReportPart::A {
            name: f.text(40, 120)?,
        }
    } else {
        let f = Fields {
            bits,
            message_type: 24,
            needed: 168,
        };
        // An auxiliary craft's MMSI is 98MIDXXXX (Rec. ITU-R M.585), and its part B
        // carries the mother ship's identity where a vessel carries its dimensions.
        let auxiliary = (980_000_000..990_000_000).contains(&header.mmsi);
        let extent = if auxiliary {
            StaticExtent::MotherShip {
                mmsi: f.u32(132, 30)?,
            }
        } else {
            StaticExtent::Dimensions(f.dimensions(132)?)
        };
        StaticDataReportPart::B {
            ship_type: f.u8(40, 8)?,
            vendor_id: f.text(48, 18)?,
            unit_model: f.u8(66, 4)?,
            unit_serial: f.u32(70, 20)?,
            call_sign: f.text(90, 42)?,
            extent,
            // -5 transmitters send 162..168 as zero; -6 gives the last two meaning.
            position_fixing_device: f.u8(162, 4)?,
            vdes_capabilities: f.u8(166, 2)?,
        }
    };
    Ok(StaticDataReport { header, part })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_1_report_decodes_to_the_table() {
        let bits = BitField::from_armoured("177KQJ5000G?tO`K>RA1wUbN0TKH", 0).expect("armoured");
        let AisMessage::Position(m) = decode(&bits).expect("decodes") else {
            panic!("not a position report");
        };
        assert_eq!(m.header.message_type, 1);
        assert_eq!(m.header.mmsi, 477_553_000);
        assert_eq!(m.navigational_status, 5);
        assert_eq!(m.rate_of_turn, 0);
        assert_eq!(m.speed_over_ground, 0);
        assert_eq!(m.true_heading, 181);
        let (lon, lat) = m.position.degrees().expect("available");
        assert!((lon - (-122.345_833_3)).abs() < 1e-6, "{lon}");
        assert!((lat - 47.582_833_3).abs() < 1e-6, "{lat}");
    }

    #[test]
    fn a_short_payload_names_what_it_lacks() {
        let bits = BitField::from_armoured("177KQJ5000G?", 0).expect("armoured");
        assert_eq!(
            decode(&bits),
            Err(AisError::TooShort {
                message_type: 1,
                bits: 72,
                needed: 168
            })
        );
    }

    #[test]
    fn a_type_outside_scope_is_reported_and_not_invented() {
        // Type 4 (base station report) header only.
        let bits = BitField::from_armoured("4000000000000000000000000000", 0).expect("armoured");
        assert!(matches!(
            decode(&bits),
            Ok(AisMessage::Unsupported {
                header: Header {
                    message_type: 4,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn a_sentinel_position_is_not_a_place() {
        let p = RawPosition {
            longitude: sentinel::LONGITUDE_NOT_AVAILABLE,
            latitude: 0,
            accuracy_high: false,
        };
        assert_eq!(p.degrees(), None);
    }
}
