// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What an ASTERIX Category 129 UAS Identification and Target Report says about a UAS
//! and the ground station that heard it (GAP-101; `docs/design/external-standards.md`
//! §9.3; decoded by `gungnir_interop::asterix::cat129`).
//!
//! Category 129 is a cooperative source in exactly AIS's and ADS-B's sense
//! (`gungnir_ingest::adapters::ais::CooperativeReport`,
//! `gungnir_ingest::adapters::adsb::CooperativeReport`): a UAS -- or the ground station
//! or gateway relaying what it broadcast -- reports its own claimed identity and
//! position, nobody at the ingest boundary verifies either, and
//! `gungnir-identification`'s evidence fusion is what weighs it.
//!
//! **Why this type lives here and not beside its ingest adapter, unlike AIS's and
//! ADS-B's `CooperativeReport`.** [`UasIdentificationReport`] mixes identity (three
//! optional catalogue strings plus a mandatory registration country) with a mandatory
//! absolute position, two altitudes, a GNSS accuracy figure, an operational risk
//! classification, and two velocity components -- closer in shape and in breadth to
//! [`crate::UasPlatformReport`] (GAP-099, whose own module documentation gives the
//! identical reason for living here) than to a single cooperative-identity claim, so a
//! future consumer reads it from the one crate every layer already depends on rather
//! than reaching into an ingest adapter.
//!
//! **Why its position is [`Geodetic`] and not [`crate::EnuPoint`], unlike
//! `UasPlatformReport`.** `UasPlatformReport`'s position is tied to one deployment's own
//! video feed, so converting it to that deployment's ENU at the ingest adapter (which
//! alone holds the `LocalFrame`) loses nothing a consumer would want back. A UAS's own
//! broadcast position is a portable, global fact independent of who is listening, and
//! Category 129's SAC/SIC-keyed attribution (`gungnir_interop::asterix::cat129`) needs
//! no `gungnir-geo` dependency to resolve -- the same reason
//! `gungnir_interop::asterix::cat034`'s `RadarServiceReport::position` stays a plain
//! geodetic value rather than an ENU one instead of forcing a split. Keeping it geodetic
//! here lets `gungnir-interop`'s own codec construct the whole report in one place, the
//! same as it already constructs `RadarServiceReport`, rather than dividing construction
//! across two crates. A consumer that also wants an ENU fix for the tracking pipeline
//! gets one from `DetectionView` (`Measurement::Position`), which the ingest adapter
//! emits alongside this report exactly as it already does for AIS, ADS-B, and MISB
//! ST 0601 -- converting this same [`Geodetic`] value with the `LocalFrame` it holds.

use crate::{Geodetic, MissionTime, SensorId};

/// What a UAS Identification and Target Report (ASTERIX Category 129, edition 1.2) says
/// about one UAS, from one message.
///
/// Every field but [`Self::position`] and [`Self::registration_country`] is `Option`
/// because ten of edition 1.2's fourteen catalogue items are individually optional
/// (`docs/design/external-standards.md` §9.3): a record with only the four mandatory
/// items (data source, registration country, time of day, position) is a valid Category
/// 129 report on its own terms, exactly as `UasPlatformReport`'s own documentation notes
/// for ST 0601's much larger optional set.
#[derive(Debug, Clone, PartialEq)]
pub struct UasIdentificationReport {
    /// The identity of the receiving station or gateway in the sensor registry -- not a
    /// claim the UAS itself makes; nothing in Category 129 carries a per-UAS identifier
    /// the way AIS has an MMSI, so [`Self::manufacturer_id`], [`Self::model_id`] and
    /// [`Self::serial_number`] are the closest this report comes to one. I129/010
    /// (SAC/SIC) is recommended `00/00` by the specification itself for an
    /// airborne-to-ground broadcast (edition 1.2 §5.2.1), so a deployment that only ever
    /// sees that recommendation followed configures one binding at `(0, 0)` for its one
    /// gateway, the same as it would configure one binding for one AIS receiver.
    pub sensor: SensorId,
    /// I129/070, placed on the receipt date the same way `gungnir_interop::asterix::
    /// cat048::source_time` places I048/140 (identical shape: seconds since midnight
    /// UTC, resolution 1/128 s).
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
    /// I129/080: the UAS's own claimed WGS-84 position. Mandatory in every record.
    ///
    /// Its `alt_m` is what [`Self::altitude_reference`] says it is, and **only** a
    /// [`UasAltitudeReference::GeoidCorrected`] one is the WGS-84 ellipsoidal height
    /// every other `Geodetic` in the picture carries (GAP-196, D-123). I129/080 itself
    /// holds latitude and longitude only; the height comes from I129/090.
    pub position: Geodetic,
    /// What [`Self::position`]'s `alt_m` is measured from (GAP-196, D-123, D-124): the
    /// datum is carried as data rather than only as a sentence in
    /// [`Self::conversion_loss`], so a consumer -- the adapter that places the
    /// detection, PN-03 and PN-04 -- can tell a corrected height from one that is still
    /// above mean sea level without reading prose.
    pub altitude_reference: UasAltitudeReference,
    /// I129/090, metres above mean sea level; negative is below MSL (edition 1.2
    /// §5.2.9's own note). Either this or [`Self::altitude_agl_m`] is mandatory per the
    /// specification's own encoding rule for the pair, but this build does not enforce
    /// that jointly -- a report with neither still carries everything else honestly.
    pub altitude_amsl_m: Option<f64>,
    /// I129/100, metres above ground level.
    pub altitude_agl_m: Option<f64>,
    /// I129/110: GNSS accuracy at 50% circular error probability, metres. Edition 1.2's
    /// own note: a value of exactly `0.0` means "unknown or more than 255 m" -- carried
    /// through unconverted rather than collapsed to `None`, because collapsing it would
    /// assert a "this item was absent" fact the wire itself did not state.
    pub gnss_signal_accuracy_m: Option<f64>,
    /// I129/020, three ASCII characters, allocated by the authority named in
    /// [`Self::registration_country`].
    pub manufacturer_id: Option<String>,
    /// I129/030, three ASCII characters, allocated by the manufacturer.
    pub model_id: Option<String>,
    /// I129/040: twelve raw octets. Edition 1.2 states no encoding for this item,
    /// unlike I129/020, I129/030 and I129/050, whose own definitions each say "in ASCII
    /// Characters" -- this build does not assert an encoding the specification itself
    /// does not.
    pub serial_number: Option<[u8; 12]>,
    /// I129/050, ISO 3166-1 alpha-2 (edition 1.2 §5.2.6's own note). Mandatory in every
    /// record.
    pub registration_country: String,
    pub operational_risk: Option<OperationalRisk>,
    /// I129/185: `[east_m_s, north_m_s]`. Edition 1.2 §5.2.13 states the X axis points
    /// geographic east and the Y axis geographic north -- the same true-north reference
    /// `gungnir_model::LocalFrame`'s own ENU frame uses, so this is a direct read of the
    /// wire value and not a projection this crate had to compute.
    pub horizontal_velocity_enu_m_s: Option<[f64; 2]>,
    /// I129/220, metres/second; positive is climbing (edition 1.2 §5.2.14).
    pub vertical_velocity_m_s: Option<f64>,
    /// Anything the mapping could not do faithfully, in words, **apart from the
    /// height** -- the same convention `gungnir_interop::RadarServiceReport::
    /// conversion_loss` uses for a non-detection ASTERIX report: today that is I129/070's
    /// time of day, which needs the same receipt-date fold `cat048::source_time`
    /// performs for every other category.
    ///
    /// The height's own loss is not written here, because it is not settled when the
    /// codec builds the report: the ingest adapter may correct it afterwards (D-123).
    /// It is derived from [`Self::altitude_reference`] instead, and [`Self::losses`]
    /// joins the two -- which is what a reader of the whole report wants.
    pub conversion_loss: Option<String>,
}

impl UasIdentificationReport {
    /// Every loss the report carries, the height's included; what a detection's
    /// `Provenance::conversion_loss` carries for this report (GAP-196). Never empty,
    /// because even a corrected height states the residue it keeps
    /// ([`UasAltitudeReference::loss`]).
    #[must_use]
    pub fn losses(&self) -> String {
        let height = self.altitude_reference.loss();
        match &self.conversion_loss {
            Some(other) => format!("{other}; {height}"),
            None => height,
        }
    }
}

/// The largest magnitude the EGM2008 geoid separation reaches anywhere on Earth, metres,
/// rounded up to the centimetre: the pinned 2.5-arc-minute grid
/// (`gungnir_data::geoid::EGM2008_GRID_FILE`) runs from -106.909 m to +85.824 m (GDAL
/// 3.11.3's `gdalinfo -mm` over the whole pinned file, 2026-09-26; the record item
/// `docs/record/2026-09-26/a-uas-height-reaches-the-wgs-84-ellipsoid.md`).
///
/// A height above mean sea level that no geoid model corrected is off the ellipsoid by
/// the local separation, which is unknown without the grid and bounded by this; D-124
/// widens such a detection's vertical variance by its square, so the tracker weighs a
/// bias it cannot remove instead of trusting the number as if it were ellipsoidal.
pub const EGM2008_MAX_ABS_SEPARATION_M: f64 = 106.91;

/// What a Category 129 report's height is measured from (GAP-196; D-123 decides the
/// correction, D-124 what happens without it).
///
/// Edition 1.2 of the specification, the latest EUROCONTROL publishes (checked
/// 2026-09-26), defines I129/090 as "Altitude above Mean Sea Level (AMSL)" and names no
/// geoid model and no source for it, and I129/080's WGS-84 position carries no height at
/// all (`gungnir_interop::asterix::cat129`'s module documentation). So the only height a
/// report states on an absolute scale is one above mean sea level, which is not the
/// WGS-84 ellipsoidal height the picture reads.
#[derive(Debug, Clone, PartialEq)]
pub enum UasAltitudeReference {
    /// `position.alt_m` is I129/090 plus the geoid separation at the report's position:
    /// a WGS-84 ellipsoidal height, as the picture reads every other one (D-123).
    GeoidCorrected {
        /// The model the separation came from, as it names itself (`"EGM2008"`).
        model: String,
        /// The separation N added, metres: I129/090 + N is the ellipsoidal height.
        separation_m: f64,
    },
    /// `position.alt_m` is I129/090 **as sent**: a height above mean sea level, **not**
    /// a WGS-84 ellipsoidal one, and off it by the local geoid separation, anywhere up
    /// to [`EGM2008_MAX_ABS_SEPARATION_M`] (D-124). Never read as ellipsoidal: PN-03 and
    /// PN-04 flag it, and the detection placed from it carries a vertical variance
    /// widened by that bound.
    MeanSeaLevelUncorrected {
        /// Why no correction was applied, in words an operator reads: no geoid model on
        /// this binary, the grid missing or refused, the position off the grid.
        reason: String,
    },
    /// The record carries no I129/090, so there is no absolute height: `position.alt_m`
    /// is set to 0 and is not a measurement. I129/100 (above ground level) cannot stand
    /// in without a ground elevation model (GAP-199).
    NoAbsoluteHeight,
}

impl UasAltitudeReference {
    /// Whether `position.alt_m` is a WGS-84 ellipsoidal height.
    #[must_use]
    pub fn is_ellipsoidal(&self) -> bool {
        matches!(self, UasAltitudeReference::GeoidCorrected { .. })
    }

    /// Whether `position.alt_m` is a height above mean sea level no model corrected.
    #[must_use]
    pub fn is_msl_uncorrected(&self) -> bool {
        matches!(self, UasAltitudeReference::MeanSeaLevelUncorrected { .. })
    }

    /// What placing the height cost, in words. Even a corrected height states one:
    /// edition 1.2 names no geoid, so a sender whose receiver reports mean sea level
    /// over EGM96 or a coarser built-in model differs from EGM2008 by decimetres to
    /// metres, and that residue is said rather than assumed away (D-123).
    #[must_use]
    pub fn loss(&self) -> String {
        match self {
            UasAltitudeReference::GeoidCorrected {
                model,
                separation_m,
            } => format!(
                "I129/090 (height above mean sea level) is placed on the WGS-84 \
                 ellipsoid by adding the {model} geoid separation, {separation_m:.2} m, at \
                 the report's position; edition 1.2 names no geoid, and a sender whose \
                 mean sea level is EGM96 or a receiver's coarser model differs from \
                 {model} by decimetres to metres"
            ),
            UasAltitudeReference::MeanSeaLevelUncorrected { reason } => format!(
                "I129/090 is a height above mean sea level and is NOT corrected to the \
                 WGS-84 ellipsoid ({reason}); it is off by the local geoid separation, up \
                 to {EGM2008_MAX_ABS_SEPARATION_M} m, and is carried as mean sea level, \
                 never as an ellipsoidal height"
            ),
            UasAltitudeReference::NoAbsoluteHeight => "neither I129/090 nor a usable \
                 absolute altitude is present in this record; I129/100 (above ground \
                 level) cannot substitute without a ground elevation model, so altitude is \
                 set to 0"
                .to_string(),
        }
    }

    /// One short line for a panel: what the height is, with the reason when it is not
    /// ellipsoidal.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            UasAltitudeReference::GeoidCorrected {
                model,
                separation_m,
            } => format!(
                "WGS-84 ellipsoidal: mean sea level + {model} separation {separation_m:.2} m"
            ),
            UasAltitudeReference::MeanSeaLevelUncorrected { reason } => {
                format!("above mean sea level, NOT corrected to the ellipsoid: {reason}")
            }
            UasAltitudeReference::NoAbsoluteHeight => {
                "no absolute height reported (altitude set to 0, not a measurement)".to_string()
            }
        }
    }
}

/// I129/120 Operational Risk Levels (edition 1.2 §5.2.12), decoded from the one octet
/// the specification describes -- see `gungnir_interop::asterix::cat129`'s module
/// documentation for the document's own contradiction over the item's length and why
/// one octet is the reading this build follows (the owner's review, 2026-09-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationalRisk {
    pub certification_category: UasCertificationCategory,
    /// The wire's own 2-bit code (0 to 3). Edition 1.2 labels these "ARC = 1" through
    /// "ARC = 4" -- the spoken number is always the code plus one; use
    /// [`Self::air_risk_category_label`] rather than re-deriving the offset.
    pub air_risk_category_code: u8,
    /// The 4-bit Airspace Encounter Category code. Edition 1.2's own Annex A defines Air
    /// Risk Category values 1 through 3 but its "Airspace Encounter Categories"
    /// subsection is a heading with no defined values in this edition -- checked against
    /// the primary PDF, not assumed absent. Carried raw because this build cannot
    /// honestly name what a code means when the specification that was meant to does
    /// not either.
    pub airspace_encounter_category_code: u8,
}

impl OperationalRisk {
    /// The spoken Air Risk Category, 1 through 4 (edition 1.2 §5.2.12's own "ARC = 1"
    /// through "ARC = 4" labels for wire codes 0 through 3).
    #[must_use]
    pub fn air_risk_category_label(&self) -> u8 {
        self.air_risk_category_code + 1
    }
}

/// I129/120's UCC subfield (edition 1.2 §5.2.12). Unlike the AEC subfield beside it, all
/// four wire codes are exhaustively defined by the specification itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UasCertificationCategory {
    Unknown,
    Open,
    Specific,
    Certified,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_risk_category_label_is_the_wire_code_plus_one() {
        let risk = OperationalRisk {
            certification_category: UasCertificationCategory::Open,
            air_risk_category_code: 0,
            airspace_encounter_category_code: 0,
        };
        assert_eq!(risk.air_risk_category_label(), 1);
        let risk = OperationalRisk {
            air_risk_category_code: 3,
            ..risk
        };
        assert_eq!(risk.air_risk_category_label(), 4);
    }

    /// Each datum says what it is, and only a corrected height calls itself
    /// ellipsoidal; an uncorrected one names its reason and never the word
    /// "ellipsoidal" as a claim about itself (D-124).
    #[test]
    fn each_altitude_reference_says_what_the_height_is() {
        let corrected = UasAltitudeReference::GeoidCorrected {
            model: "EGM2008".into(),
            separation_m: 34.92,
        };
        assert!(corrected.is_ellipsoidal() && !corrected.is_msl_uncorrected());
        assert!(corrected
            .loss()
            .contains("EGM2008 geoid separation, 34.92 m"));
        assert!(corrected.loss().contains("EGM96"), "the residue is stated");

        let msl = UasAltitudeReference::MeanSeaLevelUncorrected {
            reason: "no grid".into(),
        };
        assert!(!msl.is_ellipsoidal() && msl.is_msl_uncorrected());
        assert!(msl.loss().contains("NOT corrected") && msl.loss().contains("no grid"));
        assert!(msl
            .summary()
            .starts_with("above mean sea level, NOT corrected"));

        let none = UasAltitudeReference::NoAbsoluteHeight;
        assert!(!none.is_ellipsoidal() && !none.is_msl_uncorrected());
        assert!(none.loss().contains("set to 0"));
    }
}
