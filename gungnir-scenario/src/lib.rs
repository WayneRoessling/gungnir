// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! scenario: five-scenario ground-truth + sensor simulation generator.
//! See scenario-crate-narrative.md for why these five and no more. Test/bench
//! dependency only -- never a normal dependency of a production crate
//! (agentic-coding-standards.md §1.1). Every stochastic component takes an explicit
//! `Rng` (§2.4) so tests seed a fixed generator and get deterministic output.
//!
//! # What this generator is, and is not
//!
//! It produces the five *engineering* scenarios of `docs/scenario-crate-narrative.md`,
//! each shaped to force a specific set of `docs/verification-capability-table.md` §1
//! rows to run honestly. Truth comes from the `gungnir-core` motion models
//! ([`truth`]); observations come from the numbered procedure in
//! `docs/test-tracks/sensor-models.md` ([`sensor`]); both are written in the units and
//! frame of `docs/test-tracks/data-format.md` §2, so a generated timeline and a
//! plan-07 TT set are the same shape of data.
//!
//! It is **not** a port of `docs/test-tracks/tools/gen_tracks.py`. That generator
//! composes the TT-01 to TT-10 mission library out of four YAML files -- catalogue,
//! classes, sensors, scenarios -- and reproducing its byte-identical output for a
//! given seed additionally requires Python's Mersenne Twister and its `uniform` and
//! `gauss` derivations. Neither the YAML composition engine nor the Python RNG is
//! implemented here. GAP-016 in `docs/mission/gap-analysis/gap-register.md` records
//! which half is done. Since 2026-09-06 the four YAML files load as typed data
//! ([`library`], D-31) and the Python RNG is reproduced ([`python_random`]); the
//! composition engine over them is what remains.
//!
//! # Determinism
//!
//! Given the same [`Scenario`] and the same seeded `Rng`, [`ScenarioGenerator::generate`]
//! returns an identical [`GeneratedTimeline`]. Draws happen in a fixed order: sensors
//! are phase-offset first, then entities are instantiated in declaration order, then
//! observation runs sensor by sensor over opportunity times in increasing order, and
//! within one opportunity every entity in declaration order before that opportunity's
//! false alarms. Adding an out-of-range entity consumes no randomness, so extending a
//! scenario's geography does not reshuffle its detections.

pub mod library;
pub mod pynum;
pub mod python_random;
pub mod sensor;
pub mod tracks;
pub mod truth;

pub use library::TrackLibrary;
pub use python_random::PythonRandom;

use gungnir_coord::{CoordTransform, Ecef, Enu, Geodetic, Wgs84};
use gungnir_fusion_async::Detection;
use nalgebra::{SVector, Vector3};

pub use sensor::SensorModel;
pub use truth::{Entity, MotionKind, PhaseSegment, Side, TruthRecord};

/// One variant per scenario in scenario-crate-narrative.md; the numbers are cited
/// from other crates' doc comments and must not change.
#[derive(Debug, Clone, PartialEq)]
pub enum Scenario {
    /// Scenario 1: single maneuvering aircraft, CV -> CT -> CA. Baseline for EKF/UKF/IMM.
    ManeuveringAircraft,
    /// Scenario 2: maritime clutter, configurable Pd/clutter, land-mask dropout.
    MaritimeClutter { pd: f64, clutter_rate: f64 },
    /// Scenario 3: urban multi-sensor convoy, deliberately mismatched sensor rates
    /// and out-of-order arrivals, injected registration bias.
    UrbanConvoy {
        injected_bias_m: nalgebra::Vector3<f64>,
    },
    /// Scenario 4: dense swarm/debris field, tens-hundreds of closely-spaced targets.
    DenseSwarm { target_count: usize },
    /// Scenario 5: adversarial geometry (pole/antimeridian) + multi-day soak.
    AdversarialGeometrySoak { cycles: u64 },
}

impl Scenario {
    /// The scenario number as used in scenario-crate-narrative.md.
    pub fn number(&self) -> u8 {
        match self {
            Scenario::ManeuveringAircraft => 1,
            Scenario::MaritimeClutter { .. } => 2,
            Scenario::UrbanConvoy { .. } => 3,
            Scenario::DenseSwarm { .. } => 4,
            Scenario::AdversarialGeometrySoak { .. } => 5,
        }
    }
}

/// One emitted observation: the detection the tracking core consumes, plus the two
/// things it deliberately does not carry -- when the observation arrived, and which
/// entity (if any) caused it.
///
/// `gungnir_fusion_async::Detection` has a source time and no receipt time, because
/// out-of-sequence handling reads arrival *order* from the channel and source time
/// from the detection. The generator keeps the receipt time alongside so that a
/// timeline can be exported to the recorded-feed format of
/// `docs/test-tracks/data-format.md` §3, which records both.
///
/// `truth_entity` is `None` for a false alarm. It is the association ground truth of
/// `detections-truth.jsonl`, and it is never visible to a tracker.
///
/// Deliberately not `serde`: `Detection` belongs to `gungnir-fusion-async`, which is
/// a concurrency crate and carries no serialization. An [`Observation`] is exported by
/// building the `gungnir_model::DetectionView` that `data-format.md` §3 specifies, in a
/// crate that owns that type -- not by serializing this one.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub detection: Detection,
    /// Mission seconds at which the system received this observation.
    pub receipt_time_s: f64,
    /// The entity that caused it, or `None` for a false alarm.
    pub truth_entity: Option<String>,
    /// The calibration baseline of the observing sensor, or `None` if uncalibrated.
    pub calibration: Option<String>,
}

/// Ground truth (mission time, geodetic position) plus the detections a sensor
/// model produced from it, in arrival order (which is not source-time order for
/// Scenario 3).
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedTimeline {
    /// The scenario this came from.
    pub scenario_number: u8,
    /// Geodetic position of the ENU origin every position in this timeline is
    /// relative to (`data-format.md` §2).
    pub origin: Geodetic,
    /// Mission seconds covered, from zero.
    pub duration_s: f64,
    /// Seconds between truth records.
    pub truth_tick_s: f64,
    /// The entities, in declaration order.
    pub entities: Vec<Entity>,
    /// The sensor set, in declaration order.
    pub sensors: Vec<SensorModel>,
    /// One record per alive entity per truth tick, ordered by time then by entity
    /// declaration order.
    pub truth: Vec<TruthRecord>,
    /// Every observation, **in receipt order**: the order a live system would see
    /// them. Source times are therefore not monotonic wherever a sensor is late,
    /// which is the condition `gungnir-fusion-async` exists to handle.
    pub observations: Vec<Observation>,
    /// The single-target geodetic truth track, kept because the `coord` row's
    /// adversarial geometry is stated in geodetic terms: the ENU truth of the first
    /// entity projected through `gungnir-coord` about [`GeneratedTimeline::origin`].
    pub ground_truth: Vec<(f64, Geodetic)>,
}

impl GeneratedTimeline {
    /// The detection stream a tracker consumes, in receipt order.
    pub fn detections(&self) -> impl Iterator<Item = &Detection> {
        self.observations.iter().map(|o| &o.detection)
    }

    /// How many observations were false alarms.
    #[must_use]
    pub fn false_alarm_count(&self) -> usize {
        self.observations
            .iter()
            .filter(|o| o.truth_entity.is_none())
            .count()
    }

    /// How many observations came from a real entity.
    #[must_use]
    pub fn true_detection_count(&self) -> usize {
        self.observations.len() - self.false_alarm_count()
    }

    /// How many chances a sensor had to report a real entity: every
    /// (sensor, opportunity, entity) triple where the entity was alive, unoccluded,
    /// and inside the sensor's range.
    ///
    /// This is the denominator of the empirical probability of detection that the
    /// `scenario` "Ground-truth & sensor simulation" verification row compares
    /// against the configured value. It is recomputed from the plan rather than
    /// counted during generation on purpose: a bug that made the generator skip
    /// opportunities would otherwise shrink the numerator and the denominator
    /// together and hide itself.
    #[must_use]
    pub fn detection_opportunities(&self) -> usize {
        let mut n = 0;
        for sensor in &self.sensors {
            for opportunity in sensor.opportunities(self.duration_s) {
                for entity in &self.entities {
                    if !entity.is_alive_at(opportunity) || entity.is_occluded_at(opportunity) {
                        continue;
                    }
                    let pos = truth::split(&truth::state_at(entity, opportunity)).0;
                    if (pos - sensor.position).norm() <= sensor.range_m {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// How many scans happened in total: the denominator of the empirical clutter
    /// rate, which is per scan rather than per entity.
    #[must_use]
    pub fn scan_count(&self) -> usize {
        self.sensors
            .iter()
            .map(|s| s.opportunities(self.duration_s).count())
            .sum()
    }

    /// Truth position of `entity` at `t`, if the entity is alive then. Linear search
    /// over the truth records: this is test-support code, not a hot path.
    #[must_use]
    pub fn truth_at(&self, entity: &str, t: f64) -> Option<Vector3<f64>> {
        self.truth
            .iter()
            .find(|r| r.entity == entity && (r.t - t).abs() < 1e-9)
            .map(|r| r.pos)
    }
}

/// Where every scenario is anchored unless it says otherwise: the fictional sector
/// command post of `docs/test-tracks/data-format.md` §2, placed at a mid-latitude
/// coastal position so that ENU is a good local frame.
const DEFAULT_ORIGIN: Geodetic = Geodetic {
    // 54.5 N, 10.2 E in radians: a fictional estuary, no real installation.
    lat_rad: 0.951_412_060_9,
    lon_rad: 0.178_023_583_7,
    alt_m: 0.0,
};

pub struct ScenarioGenerator<R: rand::Rng> {
    pub rng: R,
}

impl<R: rand::Rng> ScenarioGenerator<R> {
    pub fn new(rng: R) -> Self {
        Self { rng }
    }

    /// Build the ground truth and the detections for one scenario.
    ///
    /// Validated by the scenario rows of verification-capability-table.md §1: the
    /// statistical self-check (empirical Pd and clutter rate within 2σ of the
    /// configured values) and round-trip fidelity.
    pub fn generate(&mut self, scenario: &Scenario) -> GeneratedTimeline {
        let mut plan = match scenario {
            Scenario::ManeuveringAircraft => plan_maneuvering_aircraft(),
            Scenario::MaritimeClutter { pd, clutter_rate } => {
                plan_maritime_clutter(*pd, *clutter_rate)
            }
            Scenario::UrbanConvoy { injected_bias_m } => plan_urban_convoy(*injected_bias_m),
            Scenario::DenseSwarm { target_count } => plan_dense_swarm(*target_count),
            Scenario::AdversarialGeometrySoak { cycles } => plan_adversarial_geometry(*cycles),
        };
        plan.scenario_number = scenario.number();

        // Sensors are phase-offset first, before any entity is instantiated, so that
        // changing the entity list cannot move a sensor's scan pattern.
        for s in &mut plan.sensors {
            s.phase_offset_s = self.rng.gen_range(0.0..s.update_period_s);
        }

        let truth = Self::build_truth(&plan);
        let observations = self.observe(&plan);
        let ground_truth = geodetic_track(&truth, plan.origin, plan.entities.first());

        GeneratedTimeline {
            scenario_number: plan.scenario_number,
            origin: plan.origin,
            duration_s: plan.duration_s,
            truth_tick_s: plan.truth_tick_s,
            entities: plan.entities,
            sensors: plan.sensors,
            truth,
            observations,
            ground_truth,
        }
    }

    /// One truth record per alive entity per tick, ordered by time then declaration.
    ///
    /// Truth is noiseless, so this consumes no randomness; it is an associated
    /// function rather than a method to say so in the signature.
    ///
    /// The tick count is a non-negative floor of a finite duration over a positive
    /// tick, so the conversion is exact for every scenario length this generator can
    /// express; the running index is small enough to be exact in an `f64`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn build_truth(plan: &Plan) -> Vec<TruthRecord> {
        let mut out = Vec::new();
        let ticks = (plan.duration_s / plan.truth_tick_s).floor() as u64;
        for k in 0..=ticks {
            let t = k as f64 * plan.truth_tick_s;
            for entity in &plan.entities {
                if !entity.is_alive_at(t) {
                    continue;
                }
                let state = truth::state_at(entity, t);
                let (pos, vel) = truth::split(&state);
                out.push(TruthRecord {
                    t,
                    entity: entity.id.clone(),
                    class: entity.class.clone(),
                    platform: entity.platform.clone(),
                    side: entity.side,
                    phase: entity.phase_at(t).phase_name().to_owned(),
                    pos,
                    vel,
                    alive: true,
                });
            }
        }
        out
    }

    /// Run every sensor over its opportunities, then sort the result into receipt
    /// order, which is the order the file and the live system both see.
    fn observe(&mut self, plan: &Plan) -> Vec<Observation> {
        let mut out = Vec::new();
        for sensor in &plan.sensors {
            let opportunities: Vec<f64> = sensor.opportunities(plan.duration_s).collect();
            for opportunity in opportunities {
                for entity in &plan.entities {
                    if !entity.is_alive_at(opportunity) || entity.is_occluded_at(opportunity) {
                        continue;
                    }
                    let pos = truth::split(&truth::state_at(entity, opportunity)).0;
                    if let Some(obs) = sensor.observe(pos, &entity.id, opportunity, &mut self.rng) {
                        out.push(obs);
                    }
                }
                out.extend(sensor.false_alarms(opportunity, &mut self.rng));
            }
        }
        // Receipt order across sensors. `total_cmp` rather than `partial_cmp`: receipt
        // times are always finite here, and a total order means the sort is stable and
        // reproducible rather than depending on how ties compare.
        out.sort_by(|a, b| a.receipt_time_s.total_cmp(&b.receipt_time_s));
        out
    }
}

/// The first entity's truth track, projected into geodetic coordinates about the
/// scenario origin. Scenario 5's whole point is that this projection is exercised at
/// the pole and across the antimeridian.
fn geodetic_track(
    truth: &[TruthRecord],
    origin: Geodetic,
    first: Option<&Entity>,
) -> Vec<(f64, Geodetic)> {
    let Some(first) = first else {
        return Vec::new();
    };
    truth
        .iter()
        .filter(|r| r.entity == first.id)
        .map(|r| {
            let ecef = Wgs84::enu_to_ecef(
                Enu {
                    e_m: r.pos.x,
                    n_m: r.pos.y,
                    u_m: r.pos.z,
                },
                origin,
            );
            (r.t, Wgs84::ecef_to_geodetic(ecef))
        })
        .collect()
}

/// Everything a scenario decides before any randomness is consumed.
struct Plan {
    scenario_number: u8,
    origin: Geodetic,
    duration_s: f64,
    truth_tick_s: f64,
    entities: Vec<Entity>,
    sensors: Vec<SensorModel>,
}

/// A nine-state truth vector from a position and velocity.
fn state(pos: [f64; 3], vel: [f64; 3]) -> SVector<f64, 9> {
    SVector::<f64, 9>::from_column_slice(&[
        pos[0], pos[1], pos[2], vel[0], vel[1], vel[2], 0.0, 0.0, 0.0,
    ])
}

/// Scenario 1: one aircraft, one radar, a clean CV -> CT -> CA sequence. The
/// baseline the other four build on: the nonlinearity is real but not
/// adversarial, so a relative-error failure means the filter is wrong.
fn plan_maneuvering_aircraft() -> Plan {
    let duration_s = 300.0;
    let entity = Entity {
        id: "S1-air-001".to_owned(),
        class: "air.fast-jet".to_owned(),
        platform: "generic-jet".to_owned(),
        side: Side::Red,
        spawn_s: 0.0,
        despawn_s: duration_s,
        initial_state: state([-40_000.0, 20_000.0, 6_000.0], [220.0, -30.0, 0.0]),
        phases: vec![
            PhaseSegment {
                until_s: 100.0,
                motion: MotionKind::ConstantVelocity,
            },
            PhaseSegment {
                until_s: 200.0,
                motion: MotionKind::CoordinatedTurn { omega_rad_s: 0.035 },
            },
            PhaseSegment {
                until_s: duration_s,
                motion: MotionKind::ConstantAcceleration {
                    accel_m_s2: Vector3::new(-4.0, 2.0, -3.0),
                },
            },
        ],
        occlusion: None,
    };
    Plan {
        scenario_number: 1,
        origin: DEFAULT_ORIGIN,
        duration_s,
        truth_tick_s: 1.0,
        entities: vec![entity],
        sensors: vec![SensorModel::radar_medium(
            1,
            "R1 airfield radar",
            Vector3::new(0.0, 0.0, 120.0),
        )],
    }
}

/// Scenario 2: several vessels, one coastal radar with a configurable Pd and
/// clutter rate, and a land mask that hides each vessel for a stretch. Without
/// that dropout run, a track manager's coast state is never entered.
#[allow(clippy::cast_precision_loss)]
fn plan_maritime_clutter(pd: f64, clutter_rate: f64) -> Plan {
    let duration_s = 600.0;
    let mut entities = Vec::new();
    for i in 0..6_usize {
        let i_f = i as f64;
        entities.push(Entity {
            id: format!("S2-sea-{:03}", i + 1),
            class: "sea.merchant".to_owned(),
            platform: "generic-vessel".to_owned(),
            side: if i % 3 == 0 { Side::Civil } else { Side::Red },
            spawn_s: 0.0,
            despawn_s: duration_s,
            initial_state: state(
                [-18_000.0 + i_f * 2_500.0, -12_000.0 + i_f * 1_400.0, 0.0],
                [6.0 + i_f * 1.1, 4.0 - i_f * 0.6, 0.0],
            ),
            phases: vec![PhaseSegment {
                until_s: duration_s,
                motion: MotionKind::ConstantVelocity,
            }],
            // Staggered land-mask windows: the vessels do not all disappear at
            // once, so association has to keep the others straight meanwhile.
            occlusion: Some((120.0 + i_f * 40.0, 180.0 + i_f * 40.0)),
        });
    }
    let mut radar = SensorModel::radar_coastal(1, "C1 port radar", Vector3::new(0.0, 0.0, 35.0));
    radar.pd_in_range = pd;
    radar.dropout = 0.0;
    radar.false_alarms_per_scan = clutter_rate;
    Plan {
        scenario_number: 2,
        origin: DEFAULT_ORIGIN,
        duration_s,
        truth_tick_s: 1.0,
        entities,
        sensors: vec![radar],
    }
}

/// Scenario 3: a convoy seen by three sensors that genuinely disagree about time
/// -- a 1 Hz radar, a 3 s ISR video feed that is late and often out of order, and
/// an acoustic node that is later still. The video sensor carries the injected
/// registration bias the `track-fusion` rows have to recover.
#[allow(clippy::cast_precision_loss)]
fn plan_urban_convoy(injected_bias_m: Vector3<f64>) -> Plan {
    let duration_s = 480.0;
    let mut entities = Vec::new();
    for i in 0..4_usize {
        let i_f = i as f64;
        entities.push(Entity {
            id: format!("S3-land-{:03}", i + 1),
            class: "land.vehicle".to_owned(),
            platform: "generic-truck".to_owned(),
            side: Side::Red,
            spawn_s: 0.0,
            despawn_s: duration_s,
            // Nose-to-tail spacing along the route, which is what makes the
            // convoy an association problem rather than four separate targets.
            initial_state: state([-3_000.0 - i_f * 120.0, 500.0, 40.0], [11.0, 2.0, 0.0]),
            phases: vec![
                PhaseSegment {
                    until_s: 240.0,
                    motion: MotionKind::ConstantVelocity,
                },
                PhaseSegment {
                    until_s: duration_s,
                    motion: MotionKind::CoordinatedTurn { omega_rad_s: 0.012 },
                },
            ],
            occlusion: None,
        });
    }
    let radar = SensorModel::radar_medium(1, "R1 city radar", Vector3::new(0.0, 0.0, 60.0));
    let mut video = SensorModel::isr_video(2, "V1 ISR sortie", Vector3::new(1_500.0, 900.0, 800.0));
    video.bias_m = injected_bias_m;
    let acoustic = SensorModel::acoustic(3, "A1 acoustic node", Vector3::new(-800.0, 200.0, 15.0));
    Plan {
        scenario_number: 3,
        origin: DEFAULT_ORIGIN,
        duration_s,
        truth_tick_s: 1.0,
        entities,
        sensors: vec![radar, video, acoustic],
    }
}

/// Scenario 4: a dense swarm with births, deaths, and near-simultaneous
/// crossings. Closely spaced on purpose: a naive tracker gets cardinality right
/// by accident when there is nothing to confuse it with.
#[allow(clippy::cast_precision_loss)]
fn plan_dense_swarm(target_count: usize) -> Plan {
    let duration_s = 240.0;
    let mut entities = Vec::with_capacity(target_count);
    for i in 0..target_count {
        let i_f = i as f64;
        // Two interleaved columns on opposing headings, so that roughly halfway
        // through the run the two sets pass through each other.
        let opposing = i % 2 == 1;
        let sign = if opposing { -1.0 } else { 1.0 };
        let lane = (i / 2) as f64;
        entities.push(Entity {
            id: format!("S4-uas-{:03}", i + 1),
            class: "air.owa-prop".to_owned(),
            platform: "generic-owa".to_owned(),
            side: Side::Red,
            // Staggered births across the first third of the run, and a death for
            // every fifth entity, so cardinality has to track a moving number.
            spawn_s: (i_f * 0.7) % (duration_s / 3.0),
            despawn_s: if i % 5 == 4 {
                duration_s * 0.6
            } else {
                duration_s
            },
            initial_state: state(
                [
                    sign * -8_000.0,
                    lane * 180.0 - 900.0,
                    900.0 + (i_f % 7.0) * 60.0,
                ],
                [sign * 62.0, 0.0, 0.0],
            ),
            phases: vec![PhaseSegment {
                until_s: duration_s,
                motion: MotionKind::ConstantVelocity,
            }],
            occlusion: None,
        });
    }
    Plan {
        scenario_number: 4,
        origin: DEFAULT_ORIGIN,
        duration_s,
        truth_tick_s: 1.0,
        entities,
        sensors: vec![SensorModel::radar_medium(
            1,
            "R1 swarm radar",
            Vector3::new(0.0, 0.0, 90.0),
        )],
    }
}

/// Scenario 5: the geometry the other four avoid by being realistic. The origin
/// sits on the antimeridian just short of the north pole, and the target is
/// routed straight over the pole, so every truth record round-trips through
/// `gungnir-coord`'s singular case. `cycles` sets the run length: this is also
/// the soak that the PSD-stability row needs.
#[allow(clippy::cast_precision_loss)]
fn plan_adversarial_geometry(cycles: u64) -> Plan {
    let truth_tick_s = 1.0;
    let duration_s = cycles as f64 * truth_tick_s;
    // 89.9 N on the antimeridian. The target flies north at 200 m/s, which crosses
    // the pole about 55 s in and then descends the far side.
    let origin = Geodetic {
        lat_rad: 89.9_f64.to_radians(),
        lon_rad: std::f64::consts::PI,
        alt_m: 0.0,
    };
    let entity = Entity {
        id: "S5-air-001".to_owned(),
        class: "air.transport".to_owned(),
        platform: "generic-transport".to_owned(),
        side: Side::Civil,
        spawn_s: 0.0,
        despawn_s: duration_s,
        initial_state: state([0.0, -20_000.0, 9_000.0], [0.0, 200.0, 0.0]),
        phases: vec![PhaseSegment {
            until_s: duration_s,
            motion: MotionKind::ConstantVelocity,
        }],
        occlusion: None,
    };
    // Near-collinear sensor geometry: three sites almost in a line with the
    // target's track, which is the ill-conditioning the square-root filters exist
    // to survive.
    let sensors = vec![
        SensorModel::radar_medium(1, "P1 polar radar", Vector3::new(0.0, -400.0, 20.0)),
        SensorModel::radar_medium(2, "P2 polar radar", Vector3::new(0.0, 0.0, 20.0)),
        SensorModel::radar_medium(3, "P3 polar radar", Vector3::new(0.0, 400.0, 20.0)),
    ];
    Plan {
        scenario_number: 5,
        origin,
        duration_s,
        truth_tick_s,
        entities: vec![entity],
        sensors,
    }
}

/// Convert an ENU truth position to ECEF about a scenario origin. Exposed because the
/// `coord` row's adversarial cases are stated in geodetic terms and consumers need the
/// same projection the generator used.
#[must_use]
pub fn enu_to_ecef(pos: Vector3<f64>, origin: Geodetic) -> Ecef {
    Wgs84::enu_to_ecef(
        Enu {
            e_m: pos.x,
            n_m: pos.y,
            u_m: pos.z,
        },
        origin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn generate(scenario: &Scenario, seed: u64) -> GeneratedTimeline {
        ScenarioGenerator::new(StdRng::seed_from_u64(seed)).generate(scenario)
    }

    #[test]
    fn scenario_numbers_are_stable() {
        assert_eq!(Scenario::ManeuveringAircraft.number(), 1);
        assert_eq!(Scenario::AdversarialGeometrySoak { cycles: 1 }.number(), 5);
    }

    /// Every scenario must produce truth and detections, or a verification row that
    /// names it has nothing to run against.
    #[test]
    fn every_scenario_produces_a_timeline() {
        let scenarios = [
            Scenario::ManeuveringAircraft,
            Scenario::MaritimeClutter {
                pd: 0.8,
                clutter_rate: 1.0,
            },
            Scenario::UrbanConvoy {
                injected_bias_m: Vector3::new(40.0, -25.0, 5.0),
            },
            Scenario::DenseSwarm { target_count: 40 },
            Scenario::AdversarialGeometrySoak { cycles: 200 },
        ];
        for s in &scenarios {
            let t = generate(s, 42);
            assert_eq!(t.scenario_number, s.number());
            assert!(!t.truth.is_empty(), "scenario {} has no truth", s.number());
            assert!(
                !t.observations.is_empty(),
                "scenario {} has no observations",
                s.number()
            );
            for o in &t.observations {
                assert!(
                    o.detection.measurement.iter().all(|v| v.is_finite()),
                    "scenario {} emitted a non-finite measurement",
                    s.number()
                );
                assert!(o.receipt_time_s >= o.detection.timestamp_s);
            }
        }
    }

    /// The same seed must give the same timeline, or nothing downstream is
    /// reproducible.
    #[test]
    fn generation_is_deterministic() {
        let s = Scenario::MaritimeClutter {
            pd: 0.75,
            clutter_rate: 1.5,
        };
        assert_eq!(generate(&s, 99), generate(&s, 99));
    }

    /// Different seeds must give different timelines, or the seed is being ignored.
    #[test]
    fn seed_changes_the_timeline() {
        let s = Scenario::MaritimeClutter {
            pd: 0.75,
            clutter_rate: 1.5,
        };
        assert_ne!(generate(&s, 1), generate(&s, 2));
    }

    /// Observations are in receipt order; source times are not monotonic in Scenario
    /// 3, which is the condition `gungnir-fusion-async` exists for. If this ever
    /// stops being true, the out-of-sequence rows are no longer being exercised.
    #[test]
    fn urban_convoy_delivers_out_of_sequence_data() {
        let t = generate(
            &Scenario::UrbanConvoy {
                injected_bias_m: Vector3::new(40.0, -25.0, 5.0),
            },
            7,
        );
        let receipts: Vec<f64> = t.observations.iter().map(|o| o.receipt_time_s).collect();
        assert!(
            receipts.windows(2).all(|w| w[0] <= w[1]),
            "observations are not in receipt order"
        );
        let sources: Vec<f64> = t
            .observations
            .iter()
            .map(|o| o.detection.timestamp_s)
            .collect();
        let inversions = sources.windows(2).filter(|w| w[0] > w[1]).count();
        assert!(
            inversions > 0,
            "no out-of-sequence arrivals in the scenario built to produce them"
        );
    }

    /// Scenario 5 must actually cross the pole, or it is not testing what it claims.
    #[test]
    fn adversarial_geometry_crosses_the_pole() {
        let t = generate(&Scenario::AdversarialGeometrySoak { cycles: 300 }, 3);
        let max_lat = t
            .ground_truth
            .iter()
            .map(|(_, g)| g.lat_rad)
            .fold(f64::MIN, f64::max);
        assert!(
            max_lat > 89.99_f64.to_radians(),
            "highest latitude reached was {} deg",
            max_lat.to_degrees()
        );
        // Crossing the pole flips the longitude by pi; both sides must appear.
        let lons: Vec<f64> = t.ground_truth.iter().map(|(_, g)| g.lon_rad).collect();
        let spread = lons.iter().fold(f64::MIN, |a, b| a.max(*b))
            - lons.iter().fold(f64::MAX, |a, b| a.min(*b));
        assert!(
            spread > 3.0,
            "longitude spread {spread} rad: the pole was not crossed"
        );
        for (_, g) in &t.ground_truth {
            assert!(g.lat_rad.is_finite() && g.lon_rad.is_finite() && g.alt_m.is_finite());
        }
    }

    /// The swarm's cardinality has to change over the run, or the RFS rows are being
    /// handed a constant they can get right by accident.
    #[test]
    fn dense_swarm_cardinality_varies() {
        let t = generate(&Scenario::DenseSwarm { target_count: 50 }, 5);
        let alive_at = |time: f64| t.entities.iter().filter(|e| e.is_alive_at(time)).count();
        assert!(alive_at(0.0) < alive_at(100.0), "no births");
        assert!(alive_at(239.0) < alive_at(100.0), "no deaths");
    }
}
