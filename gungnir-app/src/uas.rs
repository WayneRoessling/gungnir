// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cooperative identity from ASTERIX Category 129 on the desktop (GAP-101), on the same
//! shape [`crate::cooperative`] established for AIS and [`crate::adsb`] followed for
//! ADS-B: the reports a bound radar feed decodes, associated with tracks, and the
//! identification engine fed from them.
//!
//! **A cooperative report is a claim**, and `gungnir_model::uas_identification`'s own
//! module documentation says Category 129 is one "in exactly AIS's and ADS-B's sense":
//! a UAS -- or the ground station relaying what it broadcast -- reports its own claimed
//! identity and position, nobody at the ingest boundary verifies either, and
//! `gungnir-identification`'s evidence fusion is what weighs it. So the evidence
//! submitted here argues for `Neutral` at [`crate::cooperative::COOPERATIVE_CONFIDENCE`]
//! and never declares on its own, exactly as the other two sources do. The picture's own
//! classification is not rewritten here.
//!
//! Association is by proximity, same gate and same reasoning as AIS
//! ([`crate::cooperative::ASSOCIATION_GATE_M`]). The report's position arrives as a
//! `Geodetic` rather than the ENU the other two sources carry -- `UasIdentificationReport`
//! keeps it geodetic on purpose, and its own module documentation gives the reason -- so
//! this module places it in the deployment's frame itself, once per tick.
//!
//! **What this module deliberately does not do.**
//!
//! *No platform class, unlike AIS's `surface.*` table and ADS-B's `air.crewed`
//! (GAP-027).* Those two map a field the wire actually carries -- an M.1371-6 ship type,
//! a DO-260B category -- onto a class the mission document already draws. Category 129
//! carries no such field: I129/020 and I129/030 are a manufacturer and model string
//! allocated by a registration authority, not a capability category, and the class
//! catalogue (`docs/test-tracks/classes.yaml`) draws its air classes by threat type. A
//! mapping from a manufacturer string to a lethality weight would be a class invented
//! here, which is the same thing `crate::adsb` declined for category sets B, C and D.
//!
//! *No disagreement flag, unlike AIS's `LastCooperative::disagrees`.* That flag exists
//! because DN-15's cooperative detectors read it; nothing reads one for this source, and
//! adding the field without the detector would be state nobody consults.
//!
//! `gungnir-node` binds the same Category 129 gateways and fuses none of this, for the
//! reason its own AIS and ADS-B bindings already record: that binary has no edge to
//! `gungnir-identification`. It drains the same sink on its loop so the queue cannot
//! grow without bound (`gungnir-node/src/main.rs::discard_uas_reports`).

use std::collections::{HashMap, HashSet};

use gungnir_identification::{IdentificationEngine, IdentificationEvidence};
use gungnir_model::{
    Classification, Geodetic, LocalFrame, MissionTime, TrackId, UasIdentificationReport,
};

use crate::cooperative::{ASSOCIATION_GATE_M, COOPERATIVE_CONFIDENCE};
use crate::state::AppState;

/// The last Category 129 report associated with a track, and the counters for the two
/// ways a report can end. Same role as [`crate::cooperative::CooperativeState`].
#[derive(Debug, Default)]
pub struct UasState {
    /// The claim each track's most recent report made, for PN-04's line and for a
    /// reader that wants the identity behind the evidence.
    pub by_track: HashMap<TrackId, LastUasIdentification>,
    /// (track, claimed identity) pairs already submitted, so a UAS broadcasting every
    /// second does not accumulate into certainty -- the same guard, and the same
    /// reason, as `CooperativeState::submitted`.
    pub submitted: HashSet<(TrackId, String)>,
    pub matched: u64,
    /// Reports that no track was near.
    pub unmatched: u64,
}

/// What the last report on a track claimed.
#[derive(Debug, Clone, PartialEq)]
pub struct LastUasIdentification {
    /// The claimed identity as [`claim_of`] renders it; also the evidence's source
    /// label, so PN-04's line and this record cannot drift apart.
    pub claim: String,
    /// I129/050, the registration country, which every record carries.
    pub registration_country: String,
    pub at: MissionTime,
    pub separation_m: f64,
}

/// The claimed identity in one line, for the evidence label and the record.
///
/// Category 129 carries **no per-UAS identifier** the way AIS has an MMSI or ADS-B an
/// ICAO address (`gungnir_model::UasIdentificationReport::sensor`'s own documentation
/// says so): the serial number and the manufacturer/model pair are the closest it comes,
/// and ten of the fourteen catalogue items are individually optional, so a valid record
/// may carry neither. The registration country is mandatory and is therefore always the
/// last thing left to name. A record with no identifying item at all still produces a
/// stable string, which matters because that string is half of the deduplication key:
/// two anonymous records from the same country on the same track are one claim, not two.
#[must_use]
pub fn claim_of(report: &UasIdentificationReport) -> String {
    let mut claim = format!("UAS {}", report.registration_country);
    if let (Some(manufacturer), Some(model)) = (&report.manufacturer_id, &report.model_id) {
        claim.push(' ');
        claim.push_str(manufacturer);
        claim.push('/');
        claim.push_str(model);
    } else if let Some(one) = report.manufacturer_id.as_ref().or(report.model_id.as_ref()) {
        claim.push(' ');
        claim.push_str(one);
    }
    if let Some(serial) = &report.serial_number {
        use std::fmt::Write;
        claim.push_str(" serial ");
        for byte in serial {
            let _ = write!(claim, "{byte:02x}");
        }
    }
    claim
}

/// The tick step: associate every report with a track and feed the engine. Same shape as
/// [`crate::cooperative::tick`], with the geodetic-to-ENU placement this source needs.
pub fn tick(state: &mut AppState) {
    if state.uas_sinks.is_empty() {
        return;
    }
    let mut drained = Vec::new();
    for sink in &state.uas_sinks {
        if let Ok(mut q) = sink.lock() {
            drained.extend(q.drain(..));
        }
    }
    if drained.is_empty() {
        return;
    }
    // A feed is only bound when the baseline declares an origin (`crate::radar::
    // bind_feeds`), so this is unreachable with a sink in hand; it refuses rather than
    // placing a report at an origin it invented.
    let Some(frame) = state.config.origin.map(|[lat_rad, lon_rad, alt_m]| {
        LocalFrame::new(Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
    }) else {
        return;
    };
    let tracks: Vec<(TrackId, [f64; 3])> = state
        .tracking
        .tracks()
        .iter()
        .map(|t| (t.id, t.position_enu()))
        .collect();
    for report in drained {
        associate(state, &frame, &tracks, &report);
    }
}

fn associate(
    state: &mut AppState,
    frame: &LocalFrame,
    tracks: &[(TrackId, [f64; 3])],
    report: &UasIdentificationReport,
) {
    let pos = frame.to_enu(report.position);
    let nearest = tracks
        .iter()
        .map(|(id, p)| {
            let d = ((p[0] - pos[0]).powi(2) + (p[1] - pos[1]).powi(2)).sqrt();
            (*id, d)
        })
        .filter(|(_, d)| *d <= ASSOCIATION_GATE_M)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((track, separation_m)) = nearest else {
        state.uas.unmatched += 1;
        return;
    };
    state.uas.matched += 1;
    let claim = claim_of(report);
    state.uas.by_track.insert(
        track,
        LastUasIdentification {
            claim: claim.clone(),
            registration_country: report.registration_country.clone(),
            at: report.receipt_time,
            separation_m,
        },
    );
    if state.uas.submitted.insert((track, claim.clone())) {
        state
            .identification
            .submit_evidence(IdentificationEvidence {
                track_id: track,
                source: claim,
                suggested: Classification::Neutral,
                confidence: COOPERATIVE_CONFIDENCE,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(country: &str) -> UasIdentificationReport {
        UasIdentificationReport {
            sensor: gungnir_model::SensorId(4),
            source_time: MissionTime(10.0),
            receipt_time: MissionTime(10.5),
            position: Geodetic {
                lat_rad: 0.9,
                lon_rad: 0.2,
                alt_m: 120.0,
            },
            altitude_amsl_m: Some(120.0),
            altitude_agl_m: None,
            gnss_signal_accuracy_m: Some(3.0),
            manufacturer_id: None,
            model_id: None,
            serial_number: None,
            registration_country: country.into(),
            operational_risk: None,
            horizontal_velocity_enu_m_s: None,
            vertical_velocity_m_s: None,
            conversion_loss: None,
        }
    }

    #[test]
    fn a_record_with_no_identifying_item_still_names_its_registration_country() {
        assert_eq!(claim_of(&report("NL")), "UAS NL");
    }

    #[test]
    fn a_manufacturer_and_model_and_serial_are_all_named() {
        let mut r = report("PL");
        r.manufacturer_id = Some("ABC".into());
        r.model_id = Some("X1M".into());
        r.serial_number = Some([0x01, 0x02, 0x03, 0x04, 0, 0, 0, 0, 0, 0, 0, 0xff]);
        assert_eq!(
            claim_of(&r),
            "UAS PL ABC/X1M serial 0102030400000000000000ff"
        );
    }

    /// Ten of the fourteen items are individually optional, so half a pair is a record
    /// this build will really see; it names the half it has rather than dropping both.
    #[test]
    fn half_of_the_manufacturer_model_pair_is_still_named() {
        let mut r = report("DE");
        r.model_id = Some("Q7".into());
        assert_eq!(claim_of(&r), "UAS DE Q7");
        let mut r = report("DE");
        r.manufacturer_id = Some("ACM".into());
        assert_eq!(claim_of(&r), "UAS DE ACM");
    }

    /// The claim is half the deduplication key, so two anonymous reports from one
    /// country must render identically or the guard against accumulating certainty
    /// would not hold for them.
    #[test]
    fn two_anonymous_records_from_one_country_render_the_same_claim() {
        assert_eq!(claim_of(&report("FR")), claim_of(&report("FR")));
    }
}
