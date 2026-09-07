// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Ground-truth entities and their motion.
//!
//! Motion is produced by the `gungnir-core` motion models rather than by a second
//! set of trajectory formulas written here: the generator and the filters under test
//! then share one definition of what a constant-velocity or coordinated-turn
//! trajectory is, and a disagreement between them is a filter bug rather than a
//! disagreement between two hand-written kinematics.
//!
//! Truth is **noiseless**. Every stochastic choice in a scenario -- spawn time,
//! lateral offset, phase parameters -- is drawn once when the entity is
//! instantiated, and the trajectory that follows is exact. Noise belongs to the
//! sensor model ([`crate::sensor`]), which is what the trackers are scored against.
//! That split is what makes truth usable as truth (`docs/test-tracks/data-format.md`
//! §4).

use gungnir_core::{ConstantAcceleration, ConstantVelocity, CoordinatedTurn, MotionModel};
use nalgebra::{SMatrix, SVector, Vector3};

/// Which side an entity belongs to, per `docs/test-tracks/data-format.md` §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Red,
    Blue,
    Civil,
}

/// The motion model in force for a phase. One variant per `gungnir-core` model; the
/// generator holds no kinematics of its own.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MotionKind {
    /// Straight and level.
    ConstantVelocity,
    /// Constant-rate turn about the local vertical, radians/second, signed.
    CoordinatedTurn { omega_rad_s: f64 },
    /// Constant acceleration, m/s² in the ENU frame, held for the phase.
    ConstantAcceleration { accel_m_s2: Vector3<f64> },
}

impl MotionKind {
    /// The name written into the truth record's `phase` field.
    #[must_use]
    pub fn phase_name(self) -> &'static str {
        match self {
            MotionKind::ConstantVelocity => "cruise",
            MotionKind::CoordinatedTurn { .. } => "turn",
            MotionKind::ConstantAcceleration { .. } => "terminal",
        }
    }

    /// The 9-state transition for this phase over `dt`, in the block ordering
    /// `[e, n, u, ve, vn, vu, ae, an, au]`.
    ///
    /// The constant-velocity and coordinated-turn models are six-state, so they are
    /// embedded in the top-left block and the acceleration block is left at zero:
    /// entering a non-accelerating phase drops the acceleration, which is what
    /// "the entity is now flying straight" means.
    #[must_use]
    pub fn transition(self, dt: f64) -> SMatrix<f64, 9, 9> {
        let mut f = SMatrix::<f64, 9, 9>::zeros();
        match self {
            MotionKind::ConstantVelocity => {
                embed6(&ConstantVelocity { sigma_a_sq: 0.0 }.f(dt), &mut f);
            }
            MotionKind::CoordinatedTurn { omega_rad_s } => {
                embed6(
                    &CoordinatedTurn {
                        omega: omega_rad_s,
                        sigma_a_sq: 0.0,
                    }
                    .f(dt),
                    &mut f,
                );
            }
            MotionKind::ConstantAcceleration { .. } => {
                f = ConstantAcceleration { sigma_j_sq: 0.0 }.f(dt);
            }
        }
        f
    }
}

/// Copy a six-state transition into the top-left of a nine-state one, leaving the
/// acceleration block at zero.
fn embed6(src: &SMatrix<f64, 6, 6>, dst: &mut SMatrix<f64, 9, 9>) {
    for i in 0..6 {
        for j in 0..6 {
            dst[(i, j)] = src[(i, j)];
        }
    }
}

/// One leg of an entity's route: a motion model held until `until_s` mission time.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhaseSegment {
    pub until_s: f64,
    pub motion: MotionKind,
}

/// A ground-truth entity: identity, life, and the phases it flies.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    /// Synthetic id, stable for the entity's life (`data-format.md` §4).
    pub id: String,
    /// Catalogue class id, e.g. `air.owa-prop`. Truth for identification scoring;
    /// never present in the observation stream.
    pub class: String,
    /// Catalogue platform id.
    pub platform: String,
    pub side: Side,
    /// Mission time the entity enters the scenario.
    pub spawn_s: f64,
    /// Mission time the entity leaves it; records stop after this.
    pub despawn_s: f64,
    /// `[e, n, u, ve, vn, vu, ae, an, au]` at [`Entity::spawn_s`].
    pub initial_state: SVector<f64, 9>,
    /// Phases in force order; the last one runs to [`Entity::despawn_s`].
    pub phases: Vec<PhaseSegment>,
    /// Window `(from, to)` in which the entity is hidden from every sensor -- the
    /// land mask of Scenario 2, which is what makes a track-manager coast state
    /// reachable at all.
    pub occlusion: Option<(f64, f64)>,
}

impl Entity {
    /// The phase in force at `t`, or the last one once every `until_s` has passed.
    #[must_use]
    pub fn phase_at(&self, t: f64) -> MotionKind {
        for segment in &self.phases {
            if t < segment.until_s {
                return segment.motion;
            }
        }
        self.phases
            .last()
            .map_or(MotionKind::ConstantVelocity, |s| s.motion)
    }

    #[must_use]
    pub fn is_alive_at(&self, t: f64) -> bool {
        t >= self.spawn_s && t <= self.despawn_s
    }

    #[must_use]
    pub fn is_occluded_at(&self, t: f64) -> bool {
        self.occlusion
            .is_some_and(|(from, to)| t >= from && t <= to)
    }
}

/// One truth record: one entity at one truth tick (`data-format.md` §4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TruthRecord {
    /// Mission seconds from the scenario start.
    pub t: f64,
    pub entity: String,
    pub class: String,
    pub platform: String,
    pub side: Side,
    pub phase: String,
    /// ENU metres relative to the scenario origin.
    pub pos: Vector3<f64>,
    /// ENU metres per second.
    pub vel: Vector3<f64>,
    pub alive: bool,
}

/// Advance an entity's state from its spawn to `t`.
///
/// The march steps **only at phase boundaries**, never on a fixed grid. Within one
/// phase the transition is a matrix exponential, so `F(a) F(b) = F(a + b)` exactly
/// (`gungnir-core`'s own `transition_composes` test pins this); subdividing a phase
/// would therefore cost time and change nothing. Stepping at boundaries and nowhere
/// else has two consequences that matter:
///
/// * the trajectory does not depend on the truth tick, the sensor rates, or which
///   times a caller happens to ask about -- two consumers of the same scenario see
///   the same truth; and
/// * evaluating a position is O(phases), not O(elapsed / tick), which is what makes
///   a 200-entity swarm or a long soak generate in reasonable time.
#[must_use]
pub fn state_at(entity: &Entity, t: f64) -> SVector<f64, 9> {
    let mut state = entity.initial_state;
    if t <= entity.spawn_s {
        return state;
    }
    let mut now = entity.spawn_s;
    while now < t {
        let motion = entity.phase_at(now);
        // The end of the phase in force, or the requested time, whichever is first.
        let boundary = entity
            .phases
            .iter()
            .map(|p| p.until_s)
            .filter(|until| *until > now)
            .fold(f64::INFINITY, f64::min);
        let step_end = boundary.min(t);
        // A constant-acceleration phase carries its acceleration in the state, which
        // the phase sets on entry; every other phase has already zeroed it.
        if let MotionKind::ConstantAcceleration { accel_m_s2 } = motion {
            state[6] = accel_m_s2.x;
            state[7] = accel_m_s2.y;
            state[8] = accel_m_s2.z;
        }
        state = motion.transition(step_end - now) * state;
        // A phase list whose boundaries do not advance would loop forever; the guard
        // turns a malformed entity into a terminated march rather than a hang.
        if step_end <= now {
            break;
        }
        now = step_end;
    }
    state
}

/// Position and velocity of a nine-state truth vector.
#[must_use]
pub fn split(state: &SVector<f64, 9>) -> (Vector3<f64>, Vector3<f64>) {
    (
        Vector3::new(state[0], state[1], state[2]),
        Vector3::new(state[3], state[4], state[5]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight_flyer() -> Entity {
        Entity {
            id: "T-001".into(),
            class: "air.test".into(),
            platform: "test".into(),
            side: Side::Red,
            spawn_s: 0.0,
            despawn_s: 100.0,
            initial_state: SVector::<f64, 9>::from_column_slice(&[
                0.0, 0.0, 1000.0, 100.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ]),
            phases: vec![PhaseSegment {
                until_s: 100.0,
                motion: MotionKind::ConstantVelocity,
            }],
            occlusion: None,
        }
    }

    #[test]
    fn constant_velocity_is_exact() {
        let e = straight_flyer();
        let (pos, vel) = split(&state_at(&e, 10.0));
        assert!((pos.x - 1000.0).abs() < 1e-9, "east {}", pos.x);
        assert!((pos.z - 1000.0).abs() < 1e-9);
        assert!((vel.x - 100.0).abs() < 1e-12);
    }

    fn turning_flyer() -> Entity {
        let mut e = straight_flyer();
        e.phases = vec![
            PhaseSegment {
                until_s: 30.0,
                motion: MotionKind::ConstantVelocity,
            },
            PhaseSegment {
                until_s: 100.0,
                motion: MotionKind::CoordinatedTurn { omega_rad_s: 0.04 },
            },
        ];
        e
    }

    /// Evaluating an instant must not depend on which instants were evaluated before
    /// it, or two consumers of the same scenario see two different truths.
    #[test]
    fn evaluation_is_path_independent() {
        let e = turning_flyer();
        let direct = state_at(&e, 60.0);
        for probe in [0.0, 5.0, 29.999, 30.0, 30.001, 45.0, 59.5] {
            let _ = state_at(&e, probe);
        }
        assert!((direct - state_at(&e, 60.0)).abs().max() < 1e-12);
    }

    /// A phase boundary is honoured exactly: the turn starts at 30 s, not at whatever
    /// grid point happens to be near it.
    #[test]
    fn phase_boundary_is_exact() {
        let e = turning_flyer();
        let (_, before) = split(&state_at(&e, 30.0));
        assert!(before.y.abs() < 1e-12, "turned early: vn = {}", before.y);
        let (_, after) = split(&state_at(&e, 30.1));
        assert!(after.y.abs() > 0.1, "did not turn: vn = {}", after.y);
    }

    /// A coordinated turn holds speed: this is the property a wrong `F` breaks first.
    #[test]
    fn coordinated_turn_preserves_speed() {
        let mut e = straight_flyer();
        e.phases = vec![PhaseSegment {
            until_s: 100.0,
            motion: MotionKind::CoordinatedTurn { omega_rad_s: 0.05 },
        }];
        for t in [0.0, 7.0, 31.0, 60.0, 100.0] {
            let (_, vel) = split(&state_at(&e, t));
            assert!(
                (vel.norm() - 100.0).abs() < 1e-8,
                "speed {} at t={t}",
                vel.norm()
            );
        }
    }

    #[test]
    fn occlusion_window_is_inclusive() {
        let mut e = straight_flyer();
        e.occlusion = Some((10.0, 20.0));
        assert!(!e.is_occluded_at(9.9));
        assert!(e.is_occluded_at(10.0));
        assert!(e.is_occluded_at(20.0));
        assert!(!e.is_occluded_at(20.1));
    }
}
