// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Validation/quarantine rules, kept separate from the `IngestGateway` struct in
//! lib.rs so the policy can be unit-tested against synthetic malformed/malicious
//! payloads independent of any real adapter. The `sensor_ingestion_parser` fuzz
//! target in `gungnir-fuzz` exercises [`validate_detection`] through
//! [`decode_json_line`].

use crate::{DetectionView, IngestError};
use gungnir_model::MissionTime;

/// Largest receipt-time lead over "now" tolerated before a message is treated as
/// mis-clocked, seconds.
pub const MAX_FUTURE_RECEIPT_S: f64 = 5.0;
/// Largest source-time lead over receipt time tolerated (a sensor clock slightly
/// ahead of ours), seconds.
pub const MAX_SOURCE_AHEAD_OF_RECEIPT_S: f64 = 1.0;
/// Largest measurement magnitude accepted, meters; anything beyond is not a
/// plausible local-frame observation.
pub const MAX_MEASUREMENT_MAGNITUDE_M: f64 = 1.0e7;

/// The schema and sanity rules every observation must pass.
pub fn validate_detection(d: &DetectionView, now: MissionTime) -> Result<(), IngestError> {
    let quarantine = |reason: String| IngestError::Quarantined {
        sensor: d.sensor,
        reason,
    };
    if !d.measurement.is_finite() {
        return Err(quarantine("non-finite measurement".into()));
    }
    // The magnitude rule applies to a place, and only two of the three variants are one
    // (docs/design/DN-27-bearing-only-detections.md §4). A bearing has no magnitude to
    // check, and checking one would mean giving it a range -- which is what §2 forbids.
    // What replaces it for the angular variants is a check on the numbers they do carry.
    match &d.measurement {
        gungnir_model::Measurement::Position { enu, variance_m2 } => {
            if enu.norm() > MAX_MEASUREMENT_MAGNITUDE_M {
                return Err(quarantine("measurement magnitude out of range".into()));
            }
            if variance_m2.iter().any(|v| *v <= 0.0) {
                return Err(quarantine(
                    "a position axis states a variance of zero or less".into(),
                ));
            }
        }
        gungnir_model::Measurement::RangeAzimuthElevation {
            range_m, variance, ..
        } => {
            if *range_m < 0.0 || *range_m > MAX_MEASUREMENT_MAGNITUDE_M {
                return Err(quarantine("range out of range".into()));
            }
            if variance.iter().any(|v| *v <= 0.0) {
                return Err(quarantine(
                    "a polar component states a variance of zero or less".into(),
                ));
            }
        }
        gungnir_model::Measurement::Bearing {
            azimuth_variance_rad2,
            elevation_rad,
            elevation_variance_rad2,
            ..
        } => {
            // For a bearing the error *is* the information (DN-27 §4), so a bearing
            // with no stated error is refused here rather than given a default one
            // somewhere downstream.
            if *azimuth_variance_rad2 <= 0.0 {
                return Err(quarantine(
                    "a bearing states an azimuth variance of zero or less, and the \
                     angular error is the only thing a bearing measures well"
                        .into(),
                ));
            }
            if elevation_variance_rad2.is_some_and(|v| v <= 0.0) {
                return Err(quarantine(
                    "a bearing states an elevation variance of zero or less".into(),
                ));
            }
            // A stated elevation error with no elevation is a message about nothing,
            // and the pair being out of step means the producer is confused about
            // which it measured.
            if elevation_rad.is_some() != elevation_variance_rad2.is_some() {
                return Err(quarantine(
                    "a bearing carries an elevation without its error, or an error \
                     without an elevation"
                        .into(),
                ));
            }
        }
    }
    if !(d.source_time.0.is_finite() && d.receipt_time.0.is_finite()) {
        return Err(quarantine("non-finite timestamp".into()));
    }
    if d.source_time.0 > d.receipt_time.0 + MAX_SOURCE_AHEAD_OF_RECEIPT_S {
        return Err(quarantine("source time is ahead of receipt time".into()));
    }
    if d.receipt_time.0 > now.0 + MAX_FUTURE_RECEIPT_S {
        return Err(quarantine("receipt time is in the future".into()));
    }
    Ok(())
}

/// Parse one JSON-encoded `DetectionView` (the recorded-feed line format).
pub fn decode_json_line(line: &[u8]) -> Result<DetectionView, IngestError> {
    serde_json::from_slice(line).map_err(|e| IngestError::SchemaInvalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Provenance, SensorId};

    fn base() -> DetectionView {
        DetectionView {
            sensor: SensorId(1),
            source_time: MissionTime(9.5),
            receipt_time: MissionTime(10.0),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        }
    }

    #[test]
    fn well_formed_detection_is_accepted() {
        assert!(validate_detection(&base(), MissionTime(10.0)).is_ok());
    }

    #[test]
    fn source_time_far_ahead_of_receipt_is_quarantined() {
        let d = DetectionView {
            source_time: MissionTime(20.0),
            ..base()
        };
        assert!(matches!(
            validate_detection(&d, MissionTime(10.0)),
            Err(IngestError::Quarantined { .. })
        ));
    }

    #[test]
    fn future_receipt_is_quarantined() {
        let d = DetectionView {
            receipt_time: MissionTime(100.0),
            ..base()
        };
        assert!(matches!(
            validate_detection(&d, MissionTime(10.0)),
            Err(IngestError::Quarantined { .. })
        ));
    }

    #[test]
    fn garbage_json_is_a_schema_error_not_a_panic() {
        assert!(matches!(
            decode_json_line(b"{not json"),
            Err(IngestError::SchemaInvalid(_))
        ));
        assert!(matches!(
            decode_json_line(&[0xff, 0xfe]),
            Err(IngestError::SchemaInvalid(_))
        ));
    }
}
