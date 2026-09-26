// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A recording's truth re-observed by a set of placed sensors
//! (docs/design/DN-32-re-observation-for-a-laydown.md §8).
//!
//! # On the recording's own tick, never between its samples
//!
//! The generator steps every entity once per truth tick and then runs every scan that
//! fell due since the last tick against the state it just stepped to, so each scan it
//! recorded saw the entity **as the truth record of that tick has it**. [`reobserve`]
//! does exactly that: a scan at `st` sees the records of the first tick at or after `st`.
//! That is the recording's own observation rule, not an interpolation -- no position is
//! ever computed between two records -- and a truth record that is not on the
//! recording's tick grid is refused ([`ReObservationError::NotOnTick`]) rather than
//! placed. DN-32 §12 records why this replaced §5.1's finer truth tick.
//!
//! # Streams: common random numbers
//!
//! Every draw comes from a stream keyed by the recording's seed and **which sensor and
//! which target** it concerns -- one per sensor for its scan phase, one per sensor for
//! its false alarms, one per sensor and target for detection -- and never by the
//! placement (D-74). Two laydowns rehearsed over one recording therefore make the same
//! draws for every sensor-and-target pair: a sensor they place identically produces
//! identical detections, and a difference between them comes from the sensors that
//! moved and nothing else. A single interleaved stream would let one moved sensor
//! reshuffle every other sensor's detections, and a comparison would name sensors
//! nothing had changed.
//!
//! # Whose sensors
//!
//! A [`PlacedSensor`] brings its own model (DN-32 §5.4): a deployment sensor is
//! re-observed with the detection model it names, never with a recording sensor that
//! happens to share its identifier. The recording's own sensor-specific events --
//! losses and electronic attack, which name the recording's sensors -- are therefore
//! applied only when the placed sensors *are* the recording's own
//! ([`SensorEvents::ById`], which the statistical check uses), and otherwise counted and
//! not applied (D-73). The sea state names no sensor and is always applied.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::model::SensorParams;
use crate::observe::{
    false_alarms, observe, Observation, ScanConditions, ScanContext, Signature, SimulationMark,
    TargetState,
};
use crate::pynum::Num;
use crate::vec3::f3;
use crate::PythonRandom;

/// One truth record: `truth.jsonl`, the fields re-observation reads.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TruthRecord {
    pub t: f64,
    pub entity: String,
    pub pos: [Num; 3],
    pub vel: [Num; 3],
    pub alive: bool,
}

/// One entity's observation-relevant facts: an `entities.json` entry (DN-32 §5.2).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct EntityRecord {
    pub id: String,
    pub spawn_s: f64,
    /// Seconds after spawn during which the entity is hidden from every sensor.
    pub occluded_window_s: Option<[Num; 2]>,
    pub signature: Signature,
    pub decoy: bool,
    pub adsb_intermittent: Option<Num>,
    pub ais_spoof_offset_m: Option<Vec<Num>>,
    pub surface: bool,
    /// The tick at which another entity destroyed this one, when one did. Its truth
    /// record for that tick may still read alive (it was written before the strike),
    /// and the recording's own scans at that tick did not see it.
    pub destroyed_at_s: Option<f64>,
}

impl EntityRecord {
    fn is_occluded(&self, t: f64) -> bool {
        match self.occluded_window_s {
            Some([a, b]) => {
                let dt = t - self.spawn_s;
                a.f() <= dt && dt <= b.f()
            }
            None => false,
        }
    }
}

/// `entities.json`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct EntitiesFile {
    pub format: u32,
    pub scenario: String,
    pub entities: Vec<EntityRecord>,
}

/// One change to what the sensors are subject to, at the tick the recording applied it:
/// an `environment.json` entry (DN-32 §5.3).
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvironmentEvent {
    SeaState {
        t: f64,
        value: i64,
    },
    SensorLost {
        t: f64,
        sensor: i64,
    },
    SensorRestored {
        t: f64,
        sensor: i64,
    },
    EaSkew {
        t: f64,
        sensor: i64,
        until_s: Num,
        skew_s: Option<Num>,
    },
    EaDropout {
        t: f64,
        sensor: i64,
        until_s: Num,
        multiplier: Option<Num>,
    },
}

impl EnvironmentEvent {
    fn t(&self) -> f64 {
        match self {
            EnvironmentEvent::SeaState { t, .. }
            | EnvironmentEvent::SensorLost { t, .. }
            | EnvironmentEvent::SensorRestored { t, .. }
            | EnvironmentEvent::EaSkew { t, .. }
            | EnvironmentEvent::EaDropout { t, .. } => *t,
        }
    }
}

/// `environment.json`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct EnvironmentFile {
    pub format: u32,
    pub scenario: String,
    pub events: Vec<EnvironmentEvent>,
}

/// A recording, as re-observation reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    /// The test-track label (`TT-01`).
    pub scenario: String,
    /// The recording's own seed, from which every stream here derives.
    pub seed: u64,
    pub duration_s: f64,
    /// Seconds between truth records (`metadata.json`'s `truth_tick_s`).
    pub tick_s: f64,
    pub truth: Vec<TruthRecord>,
    pub entities: Vec<EntityRecord>,
    pub environment: Vec<EnvironmentEvent>,
}

/// A sensor, where it stands, and the model it observes with.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSensor {
    pub id: i64,
    pub model: SensorParams,
    /// ENU metres about the recording's origin.
    pub position: [f64; 3],
}

/// Which of the recording's sensor-specific events apply (D-73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorEvents {
    /// The placed sensors are not the recording's: its losses and electronic-attack
    /// windows name sensors that are not here, and are counted rather than applied.
    NotApplied,
    /// The placed sensors are the recording's own, by identifier: every event applies
    /// to the sensor it names, as the generator applied it.
    ById,
}

/// What one placed sensor produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SensorTally {
    pub sensor: i64,
    pub scans: usize,
    /// Detections of a recorded target.
    pub detections: usize,
    pub false_alarms: usize,
}

/// What a re-observation produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ReObservation {
    /// Every observation, in the order the scans ran.
    pub observations: Vec<Observation>,
    /// One tally per placed sensor, in the order they were given.
    pub per_sensor: Vec<SensorTally>,
    /// The recording's sensor-specific events that were not applied, because they name
    /// sensors that are not the ones placed ([`SensorEvents::NotApplied`]).
    pub sensor_events_not_applied: usize,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ReObservationError {
    #[error(
        "the recording's truth tick is {0} s; a re-observation steps on the recording's own \
         tick and needs a positive, finite one"
    )]
    BadTick(f64),
    #[error(
        "truth record for {entity} at {t} s is not on the recording's {tick_s} s tick; a \
         rehearsal re-observes the recording where it put each target and does not \
         interpolate between its records"
    )]
    NotOnTick { entity: String, t: f64, tick_s: f64 },
    #[error("truth names {0}, which the recording's entities.json does not describe")]
    UnknownEntity(String),
    #[error(
        "sensor {sensor} has an update period of {period} s; a detection model needs a \
         positive, finite one"
    )]
    BadUpdatePeriod { sensor: i64, period: f64 },
    #[error("sensor {0} is placed at a non-finite position")]
    NonFinitePosition(i64),
    #[error("sensor {0} is placed twice")]
    DuplicateSensor(i64),
}

/// Sixty-four bits of FNV-1a: small, fixed, and the same on every platform, which is all
/// a stream key needs.
fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in *part {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // A separator no identifier byte sequence can forge a boundary around.
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The seed of one stream: the recording's seed in the high word, the stream's key in
/// the low word.
fn stream(seed: u64, sensor: i64, key: &str) -> PythonRandom {
    let low = fnv1a(&[&sensor.to_le_bytes(), key.as_bytes()]);
    PythonRandom::new((u128::from(seed) << 64) | u128::from(low))
}

/// A tick's time as the truth file writes it, in whole milliseconds: `round(t, 3)`.
#[allow(clippy::cast_possible_truncation)]
fn millis(t: f64) -> i64 {
    (t * 1000.0).round() as i64
}

/// Re-observe `recording` with `sensors` where they stand (DN-32 §8 step 3).
///
/// `placement` names what placed the sensors, for the [`SimulationMark`] every
/// observation carries.
///
/// # Errors
///
/// A tick that is not positive and finite, a truth record off the tick grid or naming an
/// entity `entities.json` does not describe, a sensor placed twice or at a non-finite
/// position, or a model whose update period is not positive and finite. Each is refused
/// rather than worked around, because each would otherwise produce detections the
/// recording does not support.
#[allow(clippy::too_many_lines)]
pub fn reobserve(
    recording: &Recording,
    sensors: &[PlacedSensor],
    events: SensorEvents,
    placement: &str,
) -> Result<ReObservation, ReObservationError> {
    let dt = recording.tick_s;
    if !(dt.is_finite() && dt > 0.0) {
        return Err(ReObservationError::BadTick(dt));
    }
    let mut placed = HashSet::new();
    for s in sensors {
        if !placed.insert(s.id) {
            return Err(ReObservationError::DuplicateSensor(s.id));
        }
        if s.position.iter().any(|v| !v.is_finite()) {
            return Err(ReObservationError::NonFinitePosition(s.id));
        }
        let period = s.model.update_period_s.f();
        if !(period.is_finite() && period > 0.0) {
            return Err(ReObservationError::BadUpdatePeriod {
                sensor: s.id,
                period,
            });
        }
    }

    // The ticks, accumulated exactly as the generator accumulates them.
    let mut ticks = Vec::new();
    let mut t = 0.0f64;
    while t <= recording.duration_s + 1e-9 {
        ticks.push(t);
        t += dt;
    }
    let tick_index: HashMap<i64, usize> = ticks
        .iter()
        .enumerate()
        .map(|(i, t)| (millis(*t), i))
        .collect();
    let entities: HashMap<&str, &EntityRecord> = recording
        .entities
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    let mut at_tick: Vec<Vec<(&TruthRecord, &EntityRecord)>> = vec![Vec::new(); ticks.len()];
    for r in &recording.truth {
        let Some(&k) = tick_index.get(&millis(r.t)) else {
            return Err(ReObservationError::NotOnTick {
                entity: r.entity.clone(),
                t: r.t,
                tick_s: dt,
            });
        };
        let Some(&e) = entities.get(r.entity.as_str()) else {
            return Err(ReObservationError::UnknownEntity(r.entity.clone()));
        };
        at_tick[k].push((r, e));
    }

    let mark = Arc::new(SimulationMark {
        scenario: recording.scenario.clone(),
        placement: placement.to_owned(),
        seed: recording.seed,
    });
    let seed = recording.seed;
    let positions: Vec<[Num; 3]> = sensors.iter().map(|s| f3(s.position)).collect();
    let mut next_scan: Vec<f64> = sensors
        .iter()
        .map(|s| stream(seed, s.id, "phase").uniform(0.0, s.model.update_period_s.f()))
        .collect();
    let mut fa_streams: Vec<PythonRandom> = sensors
        .iter()
        .map(|s| stream(seed, s.id, "false-alarms"))
        .collect();
    let mut pair_streams: HashMap<(i64, &str), PythonRandom> = HashMap::new();
    let mut tallies: Vec<SensorTally> = sensors
        .iter()
        .map(|s| SensorTally {
            sensor: s.id,
            ..SensorTally::default()
        })
        .collect();

    let mut environment: Vec<&EnvironmentEvent> = recording.environment.iter().collect();
    // Stable: events at one tick keep the order the recording applied them in.
    environment.sort_by(|a, b| {
        a.t()
            .partial_cmp(&b.t())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut next_event = 0usize;
    let mut sea_state: i64 = 0;
    let mut lost: HashSet<i64> = HashSet::new();
    // Per sensor, per kind: the stated value and the window's end.
    let mut skew: HashMap<i64, (Option<Num>, Num)> = HashMap::new();
    let mut dropout: HashMap<i64, (Option<Num>, Num)> = HashMap::new();
    let mut not_applied = 0usize;
    let mut last_seen: HashMap<&str, f64> = HashMap::new();
    let mut observations = Vec::new();

    for (k, &t) in ticks.iter().enumerate() {
        while let Some(ev) = environment.get(next_event) {
            if ev.t() > t + 1e-6 {
                break;
            }
            next_event += 1;
            match (**ev, events) {
                (EnvironmentEvent::SeaState { value, .. }, _) => sea_state = value,
                (_, SensorEvents::NotApplied) => not_applied += 1,
                (EnvironmentEvent::SensorLost { sensor, .. }, SensorEvents::ById) => {
                    lost.insert(sensor);
                }
                (EnvironmentEvent::SensorRestored { sensor, .. }, SensorEvents::ById) => {
                    lost.remove(&sensor);
                }
                (
                    EnvironmentEvent::EaSkew {
                        sensor,
                        until_s,
                        skew_s,
                        ..
                    },
                    SensorEvents::ById,
                ) => {
                    skew.insert(sensor, (skew_s, until_s));
                }
                (
                    EnvironmentEvent::EaDropout {
                        sensor,
                        until_s,
                        multiplier,
                        ..
                    },
                    SensorEvents::ById,
                ) => {
                    dropout.insert(sensor, (multiplier, until_s));
                }
            }
        }
        for (i, s) in sensors.iter().enumerate() {
            if lost.contains(&s.id) {
                continue;
            }
            let period = s.model.update_period_s.f();
            while next_scan[i] <= t {
                let st = next_scan[i];
                next_scan[i] = st + period;
                let mut conditions = ScanConditions::calm(sea_state);
                if let Some((stated, until)) = skew.get(&s.id) {
                    if st <= until.f() {
                        conditions = conditions.with_skew(&s.model, *stated);
                    }
                }
                if let Some((stated, until)) = dropout.get(&s.id) {
                    if st <= until.f() {
                        conditions = conditions.with_dropout(&s.model, *stated);
                    }
                }
                let scan = ScanContext::new(s.id, &s.model, st, conditions, Arc::clone(&mark));
                tallies[i].scans += 1;
                for (record, entity) in &at_tick[k] {
                    if entity.destroyed_at_s.is_some_and(|d| d <= t + 1e-9) {
                        continue;
                    }
                    let target = TargetState {
                        id: &entity.id,
                        position: record.pos,
                        velocity: record.vel,
                        alive: record.alive,
                        occluded: entity.is_occluded(st),
                        signature: &entity.signature,
                        decoy: entity.decoy,
                        adsb_intermittent: entity.adsb_intermittent,
                        ais_spoof_offset_m: entity.ais_spoof_offset_m.as_deref(),
                        surface: entity.surface,
                        last_seen_s: last_seen.get(entity.id.as_str()).copied(),
                    };
                    let rng = pair_streams
                        .entry((s.id, entity.id.as_str()))
                        .or_insert_with(|| stream(seed, s.id, &entity.id));
                    if let Some(o) = observe(&s.model, positions[i], &scan, &target, rng) {
                        tallies[i].detections += 1;
                        if !s.model.cued {
                            last_seen.insert(entity.id.as_str(), st);
                        }
                        observations.push(o);
                    }
                }
                let fa = false_alarms(&s.model, positions[i], &scan, &mut fa_streams[i]);
                tallies[i].false_alarms += fa.len();
                observations.extend(fa);
            }
        }
    }
    Ok(ReObservation {
        observations,
        per_sensor: tallies,
        sensor_events_not_applied: not_applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ElectronicAttack, Latency, Noise};
    use std::collections::BTreeMap;

    fn radar(period: f64) -> SensorParams {
        SensorParams {
            signature_key: "rcs".into(),
            range_m: BTreeMap::from([("small".to_owned(), Num::Int(10_000))]),
            pd_in_range: Num::Float(1.0),
            update_period_s: Num::Float(period),
            noise: Noise {
                range_m: Num::Int(10),
                cross_m: Num::Int(10),
                height_m: Num::Int(10),
            },
            field_of_regard_deg: [Num::Int(0), Num::Int(360)],
            altitude_m: [Num::Int(0), Num::Int(10_000)],
            horizon: false,
            latency_s: Latency {
                mean: Num::Float(0.1),
                jitter: Num::Float(0.0),
            },
            dropout: Num::Float(0.0),
            out_of_order: Num::Float(0.0),
            false_alarms_per_scan: Num::Float(0.0),
            ea: ElectronicAttack {
                skew_s: Num::Int(0),
                dropout_multiplier: Num::Float(1.0),
                fa_multiplier: Num::Float(1.0),
            },
            cued: false,
            moving_only: false,
            sea_state_dropout: BTreeMap::from([(4, Num::Float(1.0))]),
            bias_m: None,
        }
    }

    fn entity(id: &str) -> EntityRecord {
        EntityRecord {
            id: id.to_owned(),
            spawn_s: 0.0,
            occluded_window_s: None,
            signature: Signature {
                rcs: Some("small".into()),
                ..Signature::default()
            },
            decoy: false,
            adsb_intermittent: None,
            ais_spoof_offset_m: None,
            surface: false,
            destroyed_at_s: None,
        }
    }

    fn record(t: f64, entity: &str) -> TruthRecord {
        TruthRecord {
            t,
            entity: entity.to_owned(),
            pos: [Num::Float(1000.0), Num::Float(0.0), Num::Float(100.0)],
            vel: [Num::Float(0.0); 3],
            alive: true,
        }
    }

    fn recording(truth: Vec<TruthRecord>) -> Recording {
        Recording {
            scenario: "TT-99".into(),
            seed: 1,
            duration_s: 10.0,
            tick_s: 2.0,
            truth,
            entities: vec![entity("E")],
            environment: Vec::new(),
        }
    }

    fn placed(id: i64) -> PlacedSensor {
        PlacedSensor {
            id,
            model: radar(1.0),
            position: [0.0, 0.0, 0.0],
        }
    }

    fn every_tick() -> Vec<TruthRecord> {
        (0..=5).map(|k| record(f64::from(k) * 2.0, "E")).collect()
    }

    #[test]
    fn a_truth_record_between_ticks_is_refused_rather_than_interpolated() {
        let mut truth = every_tick();
        truth.push(record(3.0, "E"));
        let err = reobserve(
            &recording(truth),
            &[placed(1)],
            SensorEvents::NotApplied,
            "x",
        )
        .expect_err("3 s is not on a 2 s tick");
        assert!(matches!(err, ReObservationError::NotOnTick { .. }), "{err}");
        assert!(err.to_string().contains("does not interpolate"), "{err}");
    }

    #[test]
    fn truth_naming_an_undescribed_entity_is_refused() {
        let err = reobserve(
            &recording(vec![record(0.0, "ghost")]),
            &[placed(1)],
            SensorEvents::NotApplied,
            "x",
        )
        .expect_err("ghost is not in entities.json");
        assert_eq!(err, ReObservationError::UnknownEntity("ghost".into()));
    }

    #[test]
    fn a_sensor_placed_twice_or_nowhere_or_with_no_period_is_refused() {
        let rec = recording(every_tick());
        assert_eq!(
            reobserve(&rec, &[placed(1), placed(1)], SensorEvents::NotApplied, "x"),
            Err(ReObservationError::DuplicateSensor(1))
        );
        let mut nowhere = placed(2);
        nowhere.position[0] = f64::NAN;
        assert_eq!(
            reobserve(&rec, &[nowhere], SensorEvents::NotApplied, "x"),
            Err(ReObservationError::NonFinitePosition(2))
        );
        let mut stuck = placed(3);
        stuck.model.update_period_s = Num::Int(0);
        assert!(matches!(
            reobserve(&rec, &[stuck], SensorEvents::NotApplied, "x"),
            Err(ReObservationError::BadUpdatePeriod { sensor: 3, .. })
        ));
        let mut bad = rec;
        bad.tick_s = 0.0;
        assert_eq!(
            reobserve(&bad, &[placed(1)], SensorEvents::NotApplied, "x"),
            Err(ReObservationError::BadTick(0.0))
        );
    }

    #[test]
    fn every_observation_is_marked_with_the_recording_the_placement_and_the_seed() {
        let run = reobserve(
            &recording(every_tick()),
            &[placed(1)],
            SensorEvents::NotApplied,
            "laydown-c",
        )
        .expect("runs");
        assert!(!run.observations.is_empty());
        for o in &run.observations {
            assert_eq!(
                o.mark(),
                &SimulationMark {
                    scenario: "TT-99".into(),
                    placement: "laydown-c".into(),
                    seed: 1,
                }
            );
        }
    }

    #[test]
    fn a_target_destroyed_at_a_tick_is_not_seen_at_that_tick_though_its_record_reads_alive() {
        let mut rec = recording(every_tick());
        rec.entities[0].destroyed_at_s = Some(6.0);
        let run = reobserve(&rec, &[placed(1)], SensorEvents::NotApplied, "x").expect("runs");
        // Scans run while the scan time is at or before the tick; the tick at 6 s is the
        // first the destroyed target is gone from, and nothing after it is recorded.
        for o in &run.observations {
            assert!(o.scan_time_s() <= 4.0, "seen at {}", o.scan_time_s());
        }
        assert!(run.per_sensor[0].detections > 0);
    }

    #[test]
    fn a_deployments_sensors_meet_the_sea_state_and_not_the_recordings_sensor_events() {
        let mut rec = recording(every_tick());
        rec.environment = vec![
            EnvironmentEvent::SensorLost { t: 0.0, sensor: 1 },
            EnvironmentEvent::EaDropout {
                t: 0.0,
                sensor: 1,
                until_s: Num::Int(10),
                multiplier: Some(Num::Float(100.0)),
            },
        ];
        let calm = reobserve(&rec, &[placed(1)], SensorEvents::NotApplied, "x").expect("runs");
        assert_eq!(calm.sensor_events_not_applied, 2);
        assert!(
            calm.per_sensor[0].detections > 0,
            "the loss names the recording's sensor 1"
        );
        let own = reobserve(&rec, &[placed(1)], SensorEvents::ById, "x").expect("runs");
        assert_eq!(own.sensor_events_not_applied, 0);
        assert_eq!(
            own.per_sensor[0].scans, 0,
            "the recording's own sensor 1 was lost"
        );

        // Sea state 4 raises this model's dropout to the 0.95 cap, whoever placed it.
        rec.environment = vec![EnvironmentEvent::SeaState { t: 0.0, value: 4 }];
        let rough = reobserve(&rec, &[placed(1)], SensorEvents::NotApplied, "x").expect("runs");
        assert_eq!(rough.sensor_events_not_applied, 0);
        assert!(rough.per_sensor[0].detections < calm.per_sensor[0].detections);
    }
}
