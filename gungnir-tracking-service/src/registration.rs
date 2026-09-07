//! Sensor registration at the service layer: `docs/mission/gap-analysis/gap-register.md`
//! GAP-014, "Registration evidence into the tracker".
//!
//! # Why the translation lives here and not in `gungnir-track-fusion`
//!
//! GAP-014's action reads "Define a calibration-evidence event in `gungnir-model` and
//! consume it in `gungnir-track-fusion` registration". The first half is done where it
//! says. The second half cannot be, and the reason is the workspace's first hard rule
//! rather than a preference:
//!
//! * `gungnir-track-fusion` is a **tracking core** crate. `ARCHITECTURE.md` §7.1 allows
//!   it `track` and `coord`, and nothing else.
//! * `gungnir-model` is the **foundation** layer.
//! * `CLAUDE.md`: "Direction is one-way: core, then service facades, then
//!   productization, then UI."
//!
//! A core crate consuming a foundation-layer event is an upward edge, and adding one to
//! satisfy the wording of a register entry would be the register overruling the
//! architecture. So the event is `gungnir_model::CalibrationEvent`,
//! `gungnir_track_fusion::ReferenceObservation` is the same information in a form the
//! core crate owns, and this module -- in the one crate that already depends on both --
//! is the join. `gungnir-tracking-service` is exactly where §7.1 says the core crates
//! and the model meet.
//!
//! # What registration against surveyed truth buys
//!
//! Registering two platforms against each other recovers their offset from one another.
//! If both are displaced the same way, the pair looks perfectly registered and the whole
//! picture is in the wrong place. A surveyed reference is absolute, so it fixes one
//! sensor to the ground and every relative registration made afterwards inherits that.

use gungnir_model::events::CalibrationEvent;
use gungnir_model::{MissionTime, SensorId};
use gungnir_track_fusion::{ReferenceObservation, SensorRegistration, TrackFusionError};
use std::collections::HashMap;

/// A residual spread past this, in metres, is reported as a refusal rather than applied.
///
/// A single translation fitted to something that is not a translation -- an orientation
/// error, or a set of references that were not all the same object -- produces a
/// plausible-looking offset. The spread is what gives it away, and applying an offset
/// whose own fit says it does not explain the data would move the picture confidently in
/// the wrong direction. The threshold is generous: this is a check for a mis-modelled
/// bias, not a tuning parameter.
pub const MAX_RESIDUAL_SPREAD_M: f64 = 25.0;

/// Collects calibration evidence per sensor and turns it into registrations.
#[derive(Debug, Default)]
pub struct RegistrationLedger {
    references: HashMap<SensorId, Vec<ReferenceObservation>>,
    applied: HashMap<SensorId, [f64; 3]>,
}

impl RegistrationLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The offset currently applied to a sensor's reports, if one has been.
    #[must_use]
    pub fn offset_for(&self, sensor: SensorId) -> Option<[f64; 3]> {
        self.applied.get(&sensor).copied()
    }

    /// How many references have been gathered for a sensor.
    #[must_use]
    pub fn reference_count(&self, sensor: SensorId) -> usize {
        self.references.get(&sensor).map_or(0, Vec::len)
    }

    /// Record one observation of a surveyed reference.
    ///
    /// Every other [`CalibrationEvent`] variant is a report *about* registration rather
    /// than evidence *for* it, so this returns whether the event was evidence. Silently
    /// accepting a `RegistrationApplied` as though it were an observation would let the
    /// ledger register a sensor against its own previous answer.
    pub fn observe(&mut self, event: &CalibrationEvent) -> bool {
        let CalibrationEvent::ReferenceObserved {
            sensor,
            truth_enu,
            observed_enu,
            observation_variance_m2,
            ..
        } = event
        else {
            return false;
        };
        self.references
            .entry(*sensor)
            .or_default()
            .push(ReferenceObservation {
                truth_enu: *truth_enu,
                observed_enu: *observed_enu,
                variance_m2: *observation_variance_m2,
            });
        true
    }

    /// Estimate and apply a registration for one sensor from everything gathered.
    ///
    /// Returns the event to journal either way. **A refusal is an event, not an absence**:
    /// a deployment that gathered evidence and could not use it needs to know that, and
    /// the alternative is a sensor that quietly stays unregistered while the log shows
    /// references arriving.
    pub fn register(&mut self, sensor: SensorId, at: MissionTime) -> CalibrationEvent {
        let Some(references) = self.references.get(&sensor) else {
            return CalibrationEvent::RegistrationRefused {
                sensor,
                reason: "no surveyed references have been observed for this sensor".to_owned(),
                at,
            };
        };
        let mut registration = SensorRegistration::default();
        let outcome = registration.estimate_from_references(references);
        let count = u32::try_from(references.len()).unwrap_or(u32::MAX);
        match outcome {
            Err(error) => CalibrationEvent::RegistrationRefused {
                sensor,
                reason: registration_reason(&error),
                at,
            },
            Ok(bias) => {
                let spread = registration.residual_spread().unwrap_or(f64::INFINITY);
                if spread > MAX_RESIDUAL_SPREAD_M {
                    return CalibrationEvent::RegistrationRefused {
                        sensor,
                        reason: format!(
                            "a single offset left a residual spread of {spread:.1} m over \
                             {count} references, past the {MAX_RESIDUAL_SPREAD_M} m a \
                             translation is expected to explain; the disagreement is not a \
                             position bias"
                        ),
                        at,
                    };
                }
                let offset = [bias[0], bias[1], bias[2]];
                self.applied.insert(sensor, offset);
                CalibrationEvent::RegistrationApplied {
                    sensor,
                    offset_enu: offset,
                    residual_spread_m: spread,
                    references: count,
                    at,
                }
            }
        }
    }
}

/// The fusion error's own words, kept rather than paraphrased.
fn registration_reason(error: &TrackFusionError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(sensor: u32, truth: [f64; 3], offset: [f64; 3]) -> CalibrationEvent {
        CalibrationEvent::ReferenceObserved {
            sensor: SensorId(sensor),
            reference: "survey-point".to_owned(),
            truth_enu: truth,
            observed_enu: [
                truth[0] - offset[0],
                truth[1] - offset[1],
                truth[2] - offset[2],
            ],
            observation_variance_m2: [4.0, 4.0, 9.0],
            at: MissionTime(0.0),
        }
    }

    #[test]
    fn evidence_becomes_an_applied_registration() {
        let mut ledger = RegistrationLedger::new();
        let offset = [3.0, -6.0, 1.0];
        for truth in [[0.0, 0.0, 0.0], [500.0, 100.0, 20.0], [-200.0, 900.0, 5.0]] {
            assert!(ledger.observe(&observed(7, truth, offset)));
        }
        assert_eq!(ledger.reference_count(SensorId(7)), 3);

        let event = ledger.register(SensorId(7), MissionTime(12.0));
        let CalibrationEvent::RegistrationApplied {
            offset_enu,
            references,
            residual_spread_m,
            ..
        } = event
        else {
            panic!("expected an applied registration, got {event:?}");
        };
        assert_eq!(references, 3);
        assert!(residual_spread_m < 1e-9);
        for axis in 0..3 {
            assert!((offset_enu[axis] - offset[axis]).abs() < 1e-3);
        }
        assert_eq!(ledger.offset_for(SensorId(7)), Some(offset_enu));
    }

    #[test]
    fn a_sensor_with_no_evidence_is_refused_and_stays_unregistered() {
        let mut ledger = RegistrationLedger::new();
        let event = ledger.register(SensorId(1), MissionTime(0.0));
        assert!(matches!(
            event,
            CalibrationEvent::RegistrationRefused { .. }
        ));
        assert_eq!(ledger.offset_for(SensorId(1)), None);
    }

    /// The check `MAX_RESIDUAL_SPREAD_M` exists for: a rotation is not a translation,
    /// and fitting one to the other must be refused rather than applied.
    #[test]
    fn a_disagreement_a_translation_cannot_explain_is_refused() {
        let mut ledger = RegistrationLedger::new();
        let angle = 0.05_f64;
        for truth in [
            [1000.0, 0.0, 0.0],
            [0.0, 1000.0, 0.0],
            [-1000.0, 0.0, 0.0],
            [0.0, -1000.0, 0.0],
        ] {
            let (s, c) = angle.sin_cos();
            ledger.observe(&CalibrationEvent::ReferenceObserved {
                sensor: SensorId(3),
                reference: "survey-point".to_owned(),
                truth_enu: truth,
                observed_enu: [
                    truth[0] * c - truth[1] * s,
                    truth[0] * s + truth[1] * c,
                    truth[2],
                ],
                observation_variance_m2: [4.0, 4.0, 9.0],
                at: MissionTime(0.0),
            });
        }
        let event = ledger.register(SensorId(3), MissionTime(1.0));
        let CalibrationEvent::RegistrationRefused { reason, .. } = event else {
            panic!("a rotation was accepted as a translation: {event:?}");
        };
        assert!(
            reason.contains("residual spread"),
            "the refusal must say what was wrong: {reason}"
        );
        assert_eq!(
            ledger.offset_for(SensorId(3)),
            None,
            "a refused registration must not be applied"
        );
    }

    /// A report about registration is not evidence for it. Accepting one would let the
    /// ledger register a sensor against its own previous answer.
    #[test]
    fn a_registration_report_is_not_taken_as_evidence() {
        let mut ledger = RegistrationLedger::new();
        assert!(!ledger.observe(&CalibrationEvent::RegistrationApplied {
            sensor: SensorId(2),
            offset_enu: [1.0, 2.0, 3.0],
            residual_spread_m: 0.0,
            references: 4,
            at: MissionTime(0.0),
        }));
        assert_eq!(ledger.reference_count(SensorId(2)), 0);
    }
}
