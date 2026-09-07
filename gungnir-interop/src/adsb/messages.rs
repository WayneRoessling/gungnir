//! The 56-bit ME field of a DF17 or DF18 extended squitter, decoded by type code.
//!
//! Four type-code groups are interpreted here — identification (1 to 4), surface
//! position (5 to 8), airborne position (9 to 18 barometric and 20 to 22 GNSS) and
//! airborne velocity (19). **Every other type code is carried, named and counted**
//! ([`MeMessage::Carried`]), the way `asterix::cat048` carries the items it does not
//! interpret: a message this build cannot read is a message a caller must be able to
//! see it did not read.
//!
//! Values are carried **as transmitted**, with the field's own resolution and its
//! "not available" codes intact, for the reason the AIS decoder gives: a decoder that
//! scaled and defaulted would hide the cases a caller has to handle. The accessors that
//! scale say what they return for an unavailable code.
//!
//! **No normative source is pinned** (`docs/design/external-standards.md` §4, GAP-010):
//! the bit layouts here are the open-source consensus of `rs1090` and `adsb_deku`, both
//! MIT, which `tests/adsb_fixtures.rs` gates against. That is a smaller claim than
//! conformance and the verification row says so.

use super::AdsbError;

/// The six-bit character set an identification message uses (a subset of IA-5). Code 32
/// is a space and the `#` positions are codes the alphabet does not define; they are
/// rendered rather than dropped, so a transmitter sending one is visible.
const CHARACTERS: &[u8; 64] = b"#ABCDEFGHIJKLMNOPQRSTUVWXYZ##### ###############0123456789######";

/// A 24-bit ICAO aircraft address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IcaoAddress(pub u32);

impl std::fmt::Display for IcaoAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:06X}", self.0 & 0x00FF_FFFF)
    }
}

/// A reader over the 56 bits of one ME field, numbered as the message numbers them:
/// bit 0 is the most significant bit of the first octet.
#[derive(Debug, Clone, Copy)]
pub struct MeBits<'a> {
    octets: &'a [u8; 7],
}

impl<'a> MeBits<'a> {
    #[must_use]
    pub fn new(octets: &'a [u8; 7]) -> Self {
        Self { octets }
    }

    /// An unsigned field of `len` bits starting at bit `start`.
    ///
    /// The ME field is a fixed 56 bits, so every call site here is in range by
    /// construction; a request that is not returns zero rather than panicking, and the
    /// debug assertion catches it in the test builds.
    #[must_use]
    pub fn u(&self, start: usize, len: usize) -> u64 {
        debug_assert!(len <= 56 && start + len <= 56, "ME field is 56 bits");
        let mut value = 0u64;
        for i in 0..len {
            let bit = start + i;
            let set = self
                .octets
                .get(bit / 8)
                .is_some_and(|o| o & (0x80 >> (bit % 8)) != 0);
            value = (value << 1) | u64::from(set);
        }
        value
    }

    #[must_use]
    pub fn flag(&self, at: usize) -> bool {
        self.u(at, 1) == 1
    }
}

/// The type code, bits 0 to 4 of the ME field.
#[must_use]
pub fn type_code(me: &[u8; 7]) -> u8 {
    me[0] >> 3
}

/// What a type code this build does not interpret is for, so a count of them names
/// something rather than a number.
#[must_use]
pub fn type_code_name(code: u8) -> &'static str {
    match code {
        0 => "no position information",
        1..=4 => "aircraft identification and category",
        5..=8 => "surface position",
        9..=18 => "airborne position, barometric altitude",
        19 => "airborne velocity",
        20..=22 => "airborne position, GNSS height",
        24 => "surface system status",
        23 | 25..=27 => "reserved",
        28 => "aircraft status",
        29 => "target state and status",
        30 => "aircraft operational coordination",
        31 => "aircraft operation status",
        _ => "type code outside 0 to 31",
    }
}

/// Whether the position in an airborne message is barometric or geometric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltitudeSource {
    /// Type codes 9 to 18: pressure altitude, not height above the ellipsoid.
    Barometric,
    /// Type codes 20 to 22: GNSS height above the WGS 84 ellipsoid.
    GnssHeight,
}

/// The two-bit surveillance status of an airborne position message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveillanceStatus {
    NoCondition,
    PermanentAlert,
    TemporaryAlert,
    SpecialPositionIdentification,
}

impl SurveillanceStatus {
    fn from_bits(bits: u64) -> Self {
        match bits & 0b11 {
            0 => Self::NoCondition,
            1 => Self::PermanentAlert,
            2 => Self::TemporaryAlert,
            _ => Self::SpecialPositionIdentification,
        }
    }
}

/// Type codes 1 to 4: the callsign and the wake-vortex category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identification {
    pub type_code: u8,
    /// The three-bit category, whose meaning depends on the type code: type code 4 is
    /// category set A (normal aircraft), 3 is set B, 2 is set C (surface vehicles), and
    /// 1 is set D (reserved). Carried as the pair rather than as an interpreted name,
    /// because the sets are not a single enumeration.
    pub category_code: u8,
    /// Eight six-bit characters with trailing spaces removed. Interior spaces are kept:
    /// dropping them would silently change what the aircraft transmitted.
    pub callsign: String,
}

impl Identification {
    /// The category set letter the type code selects, or `None` for a type code outside
    /// 1 to 4.
    #[must_use]
    pub fn category_set(&self) -> Option<char> {
        match self.type_code {
            1 => Some('D'),
            2 => Some('C'),
            3 => Some('B'),
            4 => Some('A'),
            _ => None,
        }
    }
}

/// Type codes 5 to 8: a position on the airport surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfacePosition {
    pub type_code: u8,
    /// MOV, seven bits. 0 is "not available" and 124 to 127 are the reserved and
    /// "decelerating" codes; [`Self::ground_speed_kt`] says which is which.
    pub movement_code: u8,
    /// The ground-track status bit: `false` means the track field carries nothing.
    pub track_valid: bool,
    /// TRK, seven bits, resolution 360/128 degrees clockwise from true north.
    pub track_code: u8,
    /// The T bit: the time of applicability is synchronised to a UTC tick.
    pub time_synchronised: bool,
    pub cpr: super::cpr::CprFrame,
}

impl SurfacePosition {
    /// The movement code as a ground speed in knots, or `None` when the code says the
    /// speed is not available or is one of the reserved values.
    ///
    /// The scale is piecewise, which is why this is an accessor and the code is what the
    /// structure carries.
    #[must_use]
    pub fn ground_speed_kt(&self) -> Option<f64> {
        let code = f64::from(self.movement_code);
        match self.movement_code {
            0 | 125..=u8::MAX => None,
            1 => Some(0.0),
            2..=8 => Some((code - 2.0).mul_add(0.125, 0.125)),
            9..=12 => Some((code - 9.0).mul_add(0.25, 1.0)),
            13..=38 => Some((code - 13.0).mul_add(0.5, 2.0)),
            39..=93 => Some((code - 39.0).mul_add(1.0, 15.0)),
            94..=108 => Some((code - 94.0).mul_add(2.0, 70.0)),
            109..=123 => Some((code - 109.0).mul_add(5.0, 100.0)),
            124 => Some(175.0),
        }
    }

    /// The ground track in degrees clockwise from true north, or `None` when the status
    /// bit says the field carries nothing.
    #[must_use]
    pub fn ground_track_deg(&self) -> Option<f64> {
        self.track_valid
            .then(|| f64::from(self.track_code) * (360.0 / 128.0))
    }
}

/// Type codes 9 to 18 and 20 to 22: a position in the air.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirbornePosition {
    pub type_code: u8,
    pub surveillance_status: SurveillanceStatus,
    /// The single-antenna flag in ADS-B version 0 and 1, the NIC supplement B in
    /// version 2. Carried as the bit, because which it is depends on a version this
    /// message does not carry.
    pub antenna_or_nic_supplement: bool,
    /// The twelve-bit altitude code, as transmitted. 0 means not available;
    /// [`Self::altitude_ft`] decodes the two encodings the field can be in.
    pub altitude_code: u16,
    pub altitude_source: AltitudeSource,
    /// The T bit: the time of applicability is synchronised to a UTC tick.
    pub time_synchronised: bool,
    pub cpr: super::cpr::CprFrame,
}

impl AirbornePosition {
    /// The altitude in feet, or `None` when the code says it is not available or is a
    /// Gillham code that is not a valid altitude.
    ///
    /// Two encodings share the field. With the Q bit set the value is in 25 ft steps
    /// from -1000 ft; with it clear the field is a Mode C Gillham code in 100 ft steps,
    /// which [`gillham_altitude_ft`] decodes. For type codes 20 to 22 the number is a
    /// height above the WGS 84 ellipsoid rather than a pressure altitude, which
    /// [`Self::altitude_source`] says and this method does not convert.
    #[must_use]
    pub fn altitude_ft(&self) -> Option<i32> {
        if self.altitude_code == 0 {
            return None;
        }
        if self.altitude_code & 0x0010 == 0 {
            // The Q bit is clear: re-insert the M bit position the 12-bit field drops
            // and read the result as a Mode C code.
            let thirteen = ((self.altitude_code & 0x0FC0) << 1) | (self.altitude_code & 0x003F);
            return gillham_altitude_ft(thirteen);
        }
        let n = ((self.altitude_code & 0x0FE0) >> 1) | (self.altitude_code & 0x000F);
        Some(i32::from(n) * 25 - 1000)
    }
}

/// Which altitude the vertical rate of a velocity message is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalRateSource {
    Barometric,
    Gnss,
}

/// The 22 bits of a velocity message whose meaning the subtype selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VelocityKind {
    /// Subtypes 1 and 2: velocity over the ground as east-west and north-south
    /// components. Subtype 2 is the supersonic scale, four knots to the count.
    GroundSpeed {
        /// `true` when the component is westward.
        west: bool,
        /// Ten bits. 0 is "not available"; the value is the count less one.
        east_west_code: u16,
        /// `true` when the component is southward.
        south: bool,
        /// Ten bits. 0 is "not available"; the value is the count less one.
        north_south_code: u16,
        /// `true` for subtype 2, where each count is four knots rather than one.
        supersonic: bool,
    },
    /// Subtypes 3 and 4: airspeed and heading, sent when the transmitter has no
    /// ground-referenced velocity.
    Airspeed {
        heading_available: bool,
        /// Ten bits, resolution 360/1024 degrees.
        heading_code: u16,
        /// `false` is indicated airspeed, `true` is true airspeed.
        true_airspeed: bool,
        /// Ten bits. 0 is "not available"; the value is the count less one.
        airspeed_code: u16,
        supersonic: bool,
    },
    /// Subtypes 0 and 5 to 7 have no defined layout. The 22 bits are carried so a count
    /// of them can be shown beside what they held.
    Reserved { subtype: u8, bits: u32 },
}

/// Type code 19: how fast, and which way.
// The message has four single-bit flags with four unrelated meanings; a bit-set would
// hide their names, which is the same argument `ais::messages` makes for Message 18.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirborneVelocity {
    pub subtype: u8,
    /// The intent-change flag.
    pub intent_change: bool,
    /// The IFR-capability flag.
    pub ifr_capable: bool,
    /// Three bits: `NUCr` in ADS-B version 0, `NACv` in versions 1 and 2.
    pub velocity_accuracy_code: u8,
    pub velocity: VelocityKind,
    pub vertical_rate_source: VerticalRateSource,
    /// `true` when the aircraft is descending.
    pub descending: bool,
    /// Nine bits. 0 is "not available"; otherwise 64 ft/min per count less one.
    pub vertical_rate_code: u16,
    /// `true` when GNSS height is below barometric altitude.
    pub gnss_below_barometric: bool,
    /// Seven bits. 0 and 1 are "not available"; otherwise 25 ft per count less one.
    pub gnss_barometric_difference_code: u8,
}

impl AirborneVelocity {
    /// Speed over the ground in knots, or `None` for an airspeed subtype or a component
    /// whose code says it is not available.
    #[must_use]
    pub fn ground_speed_kt(&self) -> Option<f64> {
        let (east_west, north_south) = self.ground_components_kt()?;
        Some(east_west.hypot(north_south))
    }

    /// Track over the ground, degrees clockwise from true north, or `None` as above.
    #[must_use]
    pub fn ground_track_deg(&self) -> Option<f64> {
        let (east_west, north_south) = self.ground_components_kt()?;
        let degrees = east_west.atan2(north_south).to_degrees();
        Some(if degrees < 0.0 {
            degrees + 360.0
        } else {
            degrees
        })
    }

    /// The signed east-west and north-south components in knots.
    #[must_use]
    pub fn ground_components_kt(&self) -> Option<(f64, f64)> {
        let VelocityKind::GroundSpeed {
            west,
            east_west_code,
            south,
            north_south_code,
            supersonic,
        } = self.velocity
        else {
            return None;
        };
        if east_west_code == 0 || north_south_code == 0 {
            return None;
        }
        let scale = if supersonic { 4.0 } else { 1.0 };
        let magnitude = |code: u16| f64::from(code - 1) * scale;
        let east_west = magnitude(east_west_code) * if west { -1.0 } else { 1.0 };
        let north_south = magnitude(north_south_code) * if south { -1.0 } else { 1.0 };
        Some((east_west, north_south))
    }

    /// Vertical rate in feet per minute, positive up, or `None` when not available.
    #[must_use]
    pub fn vertical_rate_ft_min(&self) -> Option<i32> {
        if self.vertical_rate_code == 0 {
            return None;
        }
        let magnitude = i32::from(self.vertical_rate_code - 1) * 64;
        Some(if self.descending {
            -magnitude
        } else {
            magnitude
        })
    }

    /// GNSS height less barometric altitude, in feet, or `None` when not available.
    #[must_use]
    pub fn gnss_minus_barometric_ft(&self) -> Option<i32> {
        if self.gnss_barometric_difference_code <= 1 {
            return None;
        }
        let magnitude = i32::from(self.gnss_barometric_difference_code - 1) * 25;
        Some(if self.gnss_below_barometric {
            -magnitude
        } else {
            magnitude
        })
    }
}

/// One ME field, decoded or carried.
#[derive(Debug, Clone, PartialEq)]
pub enum MeMessage {
    Identification(Identification),
    SurfacePosition(SurfacePosition),
    AirbornePosition(AirbornePosition),
    AirborneVelocity(AirborneVelocity),
    /// A type code this build does not interpret, including type code 0 (no position).
    /// The octets are kept so a caller can log or forward what it could not read, and
    /// `name` says what the code is for. Never mistaken for a decode.
    Carried {
        type_code: u8,
        name: &'static str,
        octets: [u8; 7],
    },
}

impl MeMessage {
    #[must_use]
    pub fn type_code(&self) -> u8 {
        match self {
            Self::Identification(m) => m.type_code,
            Self::SurfacePosition(m) => m.type_code,
            Self::AirbornePosition(m) => m.type_code,
            Self::AirborneVelocity(_) => 19,
            Self::Carried { type_code, .. } => *type_code,
        }
    }

    /// The CPR fields of whichever position message this is, with the kind that says how
    /// to decode them. `None` for anything that is not a position.
    #[must_use]
    pub fn position(&self) -> Option<(super::cpr::CprFrame, super::cpr::CprKind)> {
        match self {
            Self::SurfacePosition(m) => Some((m.cpr, super::cpr::CprKind::Surface)),
            Self::AirbornePosition(m) => Some((m.cpr, super::cpr::CprKind::Airborne)),
            _ => None,
        }
    }
}

/// Decode one 56-bit ME field.
///
/// # Errors
///
/// [`AdsbError::TypeCode`] for a type code outside 0 to 31, which cannot happen from a
/// five-bit field and is here so the match has no unreachable arm.
pub fn decode(octets: &[u8; 7]) -> Result<MeMessage, AdsbError> {
    let bits = MeBits::new(octets);
    let code = type_code(octets);
    Ok(match code {
        1..=4 => MeMessage::Identification(identification(code, bits)),
        5..=8 => MeMessage::SurfacePosition(surface_position(code, bits)),
        9..=18 | 20..=22 => MeMessage::AirbornePosition(airborne_position(code, bits)),
        19 => MeMessage::AirborneVelocity(airborne_velocity(bits)),
        0 | 23..=31 => MeMessage::Carried {
            type_code: code,
            name: type_code_name(code),
            octets: *octets,
        },
        other => return Err(AdsbError::TypeCode { type_code: other }),
    })
}

fn identification(code: u8, bits: MeBits<'_>) -> Identification {
    let mut callsign = String::with_capacity(8);
    for i in 0..8 {
        let index = bits.u(8 + i * 6, 6);
        // The index is six bits and the table is 64 long, so this cannot miss; the
        // fallback keeps the arithmetic out of an unwrap.
        let c = usize::try_from(index).unwrap_or(0);
        callsign.push(char::from(CHARACTERS.get(c).copied().unwrap_or(b'#')));
    }
    callsign.truncate(callsign.trim_end().len());
    Identification {
        type_code: code,
        #[allow(clippy::cast_possible_truncation)]
        category_code: bits.u(5, 3) as u8,
        callsign,
    }
}

#[allow(clippy::cast_possible_truncation)]
fn surface_position(code: u8, bits: MeBits<'_>) -> SurfacePosition {
    SurfacePosition {
        type_code: code,
        movement_code: bits.u(5, 7) as u8,
        track_valid: bits.flag(12),
        track_code: bits.u(13, 7) as u8,
        time_synchronised: bits.flag(20),
        cpr: super::cpr::CprFrame {
            odd: bits.flag(21),
            lat_cpr: bits.u(22, 17) as u32,
            lon_cpr: bits.u(39, 17) as u32,
        },
    }
}

#[allow(clippy::cast_possible_truncation)]
fn airborne_position(code: u8, bits: MeBits<'_>) -> AirbornePosition {
    AirbornePosition {
        type_code: code,
        surveillance_status: SurveillanceStatus::from_bits(bits.u(5, 2)),
        antenna_or_nic_supplement: bits.flag(7),
        altitude_code: bits.u(8, 12) as u16,
        altitude_source: if code >= 20 {
            AltitudeSource::GnssHeight
        } else {
            AltitudeSource::Barometric
        },
        time_synchronised: bits.flag(20),
        cpr: super::cpr::CprFrame {
            odd: bits.flag(21),
            lat_cpr: bits.u(22, 17) as u32,
            lon_cpr: bits.u(39, 17) as u32,
        },
    }
}

#[allow(clippy::cast_possible_truncation)]
fn airborne_velocity(bits: MeBits<'_>) -> AirborneVelocity {
    let subtype = bits.u(5, 3) as u8;
    let velocity = match subtype {
        1 | 2 => VelocityKind::GroundSpeed {
            west: bits.flag(13),
            east_west_code: bits.u(14, 10) as u16,
            south: bits.flag(24),
            north_south_code: bits.u(25, 10) as u16,
            supersonic: subtype == 2,
        },
        3 | 4 => VelocityKind::Airspeed {
            heading_available: bits.flag(13),
            heading_code: bits.u(14, 10) as u16,
            true_airspeed: bits.flag(24),
            airspeed_code: bits.u(25, 10) as u16,
            supersonic: subtype == 4,
        },
        other => VelocityKind::Reserved {
            subtype: other,
            bits: bits.u(13, 22) as u32,
        },
    };
    AirborneVelocity {
        subtype,
        intent_change: bits.flag(8),
        ifr_capable: bits.flag(9),
        velocity_accuracy_code: bits.u(10, 3) as u8,
        velocity,
        vertical_rate_source: if bits.flag(35) {
            VerticalRateSource::Gnss
        } else {
            VerticalRateSource::Barometric
        },
        descending: bits.flag(36),
        vertical_rate_code: bits.u(37, 9) as u16,
        gnss_below_barometric: bits.flag(48),
        gnss_barometric_difference_code: bits.u(49, 7) as u8,
    }
}

/// Decode a thirteen-bit Mode C altitude code (the Gillham encoding) to feet, or `None`
/// when the code is not a valid altitude.
///
/// The 500 ft digits are a reflected binary code and the 100 ft digits a three-bit one,
/// so neither is read as a plain number; the illegal combinations the encoding leaves
/// over are refused rather than turned into a plausible height.
#[must_use]
pub fn gillham_altitude_ft(code: u16) -> Option<i32> {
    // Re-order the interleaved code into the D-A-B-C digit groups it is drawn as.
    let mut gillham = 0u16;
    for (from, to) in [
        (0x1000, 0x0010), // C1
        (0x0800, 0x1000), // A1
        (0x0400, 0x0020), // C2
        (0x0200, 0x2000), // A2
        (0x0100, 0x0040), // C4
        (0x0080, 0x4000), // A4
        (0x0020, 0x0100), // B1
        (0x0010, 0x0001), // D1
        (0x0008, 0x0200), // B2
        (0x0004, 0x0002), // D2
        (0x0002, 0x0400), // B4
        (0x0001, 0x0004), // D4
    ] {
        if code & from != 0 {
            gillham |= to;
        }
    }
    // D1 set, or any of the three always-zero bits set, or no C bit at all: not an
    // altitude. A receiver that read one anyway would report a height off by hundreds
    // of feet with nothing to say it had.
    if gillham & 0x8889 != 0 || gillham & 0x00F0 == 0 {
        return None;
    }
    let mut hundreds = 0u32;
    for (bit, pattern) in [(0x0010, 0x007), (0x0020, 0x003), (0x0040, 0x001)] {
        if gillham & bit != 0 {
            hundreds ^= pattern;
        }
    }
    // The encoding uses 7 where 5 belongs and the other way round.
    if hundreds & 5 == 5 {
        hundreds ^= 2;
    }
    if hundreds > 5 {
        return None;
    }
    let mut five_hundreds = 0u32;
    for (bit, pattern) in [
        (0x0002, 0x0FF), // D2
        (0x0004, 0x07F), // D4
        (0x1000, 0x03F), // A1
        (0x2000, 0x01F), // A2
        (0x4000, 0x00F), // A4
        (0x0100, 0x007), // B1
        (0x0200, 0x003), // B2
        (0x0400, 0x001), // B4
    ] {
        if gillham & bit != 0 {
            five_hundreds ^= pattern;
        }
    }
    // Every other 500 ft band counts its 100 ft digits downwards.
    if five_hundreds & 1 != 0 && hundreds <= 6 {
        hundreds = 6 - hundreds;
    }
    let steps = five_hundreds * 5 + hundreds;
    if steps < 13 {
        return None;
    }
    i32::try_from(steps - 13).ok().map(|n| n * 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identification message of the frame the ADS-B literature uses as its worked
    /// example, `8D4840D6202CC371C32CE0576098`: callsign KLM1023, category set A.
    const IDENTIFICATION_ME: [u8; 7] = [0x20, 0x2C, 0xC3, 0x71, 0xC3, 0x2C, 0xE0];

    /// The airborne position half of the pair the same literature uses,
    /// `8D40621D58C382D690C8AC2863A7`: 38 000 ft, even format.
    const AIRBORNE_POSITION_ME: [u8; 7] = [0x58, 0xC3, 0x82, 0xD6, 0x90, 0xC8, 0xAC];

    #[test]
    fn an_identification_message_reads_its_callsign_and_category() {
        let MeMessage::Identification(m) = decode(&IDENTIFICATION_ME).expect("decodes") else {
            panic!("not an identification message");
        };
        assert_eq!(m.type_code, 4);
        assert_eq!(m.callsign, "KLM1023");
        assert_eq!(m.category_set(), Some('A'));
        assert_eq!(m.category_code, 0);
    }

    #[test]
    fn an_airborne_position_reads_its_altitude_and_cpr_fields() {
        let MeMessage::AirbornePosition(m) = decode(&AIRBORNE_POSITION_ME).expect("decodes") else {
            panic!("not an airborne position");
        };
        assert_eq!(m.type_code, 11);
        assert_eq!(m.altitude_source, AltitudeSource::Barometric);
        assert_eq!(m.altitude_ft(), Some(38_000));
        assert!(!m.cpr.odd);
        assert_eq!(m.cpr.lat_cpr, 93_000);
        assert_eq!(m.cpr.lon_cpr, 51_372);
        assert_eq!(m.surveillance_status, SurveillanceStatus::NoCondition);
    }

    #[test]
    fn an_altitude_code_of_zero_is_absent_not_sea_level() {
        let mut me = AIRBORNE_POSITION_ME;
        me[0] &= 0xF0;
        me[1] = 0x00;
        me[2] &= 0x0F;
        let MeMessage::AirbornePosition(m) = decode(&me).expect("decodes") else {
            panic!("not an airborne position");
        };
        assert_eq!(m.altitude_code, 0);
        assert_eq!(m.altitude_ft(), None);
    }

    #[test]
    fn a_type_code_outside_the_four_groups_is_carried_and_named() {
        // Type code 31, aircraft operation status.
        let me = [0xF8, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00];
        let decoded = decode(&me).expect("decodes");
        assert_eq!(
            decoded,
            MeMessage::Carried {
                type_code: 31,
                name: "aircraft operation status",
                octets: me,
            }
        );
        assert!(decoded.position().is_none());
    }

    #[test]
    fn an_invalid_gillham_code_is_refused_rather_than_read() {
        // D1 set is illegal in the Mode C encoding.
        assert_eq!(gillham_altitude_ft(0x0010), None);
        // No C bit at all is illegal too.
        assert_eq!(gillham_altitude_ft(0x0000), None);
    }

    #[test]
    fn a_velocity_message_of_a_reserved_subtype_keeps_its_bits() {
        // Type code 19, subtype 0.
        let me = [0x98, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let MeMessage::AirborneVelocity(m) = decode(&me).expect("decodes") else {
            panic!("not a velocity message");
        };
        assert_eq!(
            m.velocity,
            VelocityKind::Reserved {
                subtype: 0,
                bits: 0
            }
        );
        assert_eq!(m.ground_speed_kt(), None);
        assert_eq!(m.vertical_rate_ft_min(), None);
    }
}
