// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Intercept geometry (GAP-031): where and when an effector meets a track.
//!
//! The model is the least one that gives the geofence policy a point to check and PN-05
//! a time to show (`docs/design/DN-04-effector-model.md` §9): the track continues at its
//! current velocity, the effector departs its position at its closing speed the moment
//! the plan is decided, and the solution is the **earliest** moment the two coincide.
//! No envelope, no turn, no minimum range. A resource without a closing speed has no
//! geometry, and the caller reports that rather than defaulting one.

/// An intercept point in the local ENU frame and the time to reach it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterceptGeometry {
    pub point_enu: [f64; 3],
    pub time_s: f64,
}

/// Why no geometry could be produced for a pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoGeometry {
    /// The resource carries no `intercept_speed_mps` (DN-04 §9).
    NoClosingSpeed,
    /// The track outruns the effector and is not closing on it: no positive-time
    /// solution exists.
    Unreachable,
    /// An input was not finite.
    NotFinite,
}

impl std::fmt::Display for NoGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NoGeometry::NoClosingSpeed => "the resource has no closing speed",
            NoGeometry::Unreachable => "the track cannot be reached at that speed",
            NoGeometry::NotFinite => "an input was not finite",
        })
    }
}

/// Earliest constant-velocity intercept.
///
/// With `d = track - effector` and the track velocity `v`, the meeting time `t` solves
/// `|d + v t| = s t`, that is `(v·v - s²) t² + 2 (d·v) t + d·d = 0`; the earliest
/// positive root is the answer. A track already at the effector's position is
/// intercepted at `t = 0`.
///
/// # Errors
///
/// `NoGeometry` as described on each variant. Never a panic and never a NaN point.
pub fn earliest_intercept(
    track_position_enu: [f64; 3],
    track_velocity_mps: [f64; 3],
    effector_position_enu: [f64; 3],
    closing_speed_mps: Option<f64>,
) -> Result<InterceptGeometry, NoGeometry> {
    let speed = closing_speed_mps.ok_or(NoGeometry::NoClosingSpeed)?;
    let all = track_position_enu
        .iter()
        .chain(track_velocity_mps.iter())
        .chain(effector_position_enu.iter())
        .chain(std::iter::once(&speed));
    if !all.into_iter().all(|x| x.is_finite()) || speed <= 0.0 {
        return Err(NoGeometry::NotFinite);
    }
    let d = [
        track_position_enu[0] - effector_position_enu[0],
        track_position_enu[1] - effector_position_enu[1],
        track_position_enu[2] - effector_position_enu[2],
    ];
    let v = track_velocity_mps;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let a = dot(v, v) - speed * speed;
    let b = 2.0 * dot(d, v);
    let c = dot(d, d);
    if c == 0.0 {
        return Ok(InterceptGeometry {
            point_enu: track_position_enu,
            time_s: 0.0,
        });
    }
    let t = if a.abs() < 1e-9 {
        // Equal speeds: the quadratic degenerates to `b t + c = 0`.
        if b >= 0.0 {
            return Err(NoGeometry::Unreachable);
        }
        -c / b
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return Err(NoGeometry::Unreachable);
        }
        let root = discriminant.sqrt();
        let t1 = (-b - root) / (2.0 * a);
        let t2 = (-b + root) / (2.0 * a);
        match (t1 > 0.0, t2 > 0.0) {
            (true, true) => t1.min(t2),
            (true, false) => t1,
            (false, true) => t2,
            (false, false) => return Err(NoGeometry::Unreachable),
        }
    };
    if !t.is_finite() || t <= 0.0 {
        return Err(NoGeometry::Unreachable);
    }
    Ok(InterceptGeometry {
        point_enu: [
            track_position_enu[0] + v[0] * t,
            track_position_enu[1] + v[1] * t,
            track_position_enu[2] + v[2] * t,
        ],
        time_s: t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn head_on_closed_form() {
        // Track 1000 m east flying west at 50 m/s; effector at the origin closing at
        // 150 m/s: they meet after 1000 / 200 = 5 s, 250 m east.
        let g = earliest_intercept([1000.0, 0.0, 0.0], [-50.0, 0.0, 0.0], [0.0; 3], Some(150.0))
            .expect("reachable");
        assert!(close(g.time_s, 5.0));
        assert!(close(g.point_enu[0], 750.0));
    }

    #[test]
    fn a_crossing_track_meets_the_effector_at_the_analytic_point() {
        // Track at (0, 1000) flying east at 100 m/s; effector at the origin, 200 m/s.
        // |(100 t, 1000)| = 200 t  ->  10000 t² + 1e6 = 40000 t²  ->  t = sqrt(1e6/30000).
        let t = (1.0e6_f64 / 30_000.0).sqrt();
        let g = earliest_intercept([0.0, 1000.0, 0.0], [100.0, 0.0, 0.0], [0.0; 3], Some(200.0))
            .expect("reachable");
        assert!(close(g.time_s, t));
        assert!(close(g.point_enu[0], 100.0 * t));
        assert!(close(g.point_enu[1], 1000.0));
    }

    #[test]
    fn a_faster_receding_track_is_unreachable_and_a_faster_closing_one_is_not() {
        assert_eq!(
            earliest_intercept([1000.0, 0.0, 0.0], [300.0, 0.0, 0.0], [0.0; 3], Some(150.0)),
            Err(NoGeometry::Unreachable)
        );
        // Closing at 300 m/s toward an effector doing 150: they meet at 1000/450 s.
        let g = earliest_intercept(
            [1000.0, 0.0, 0.0],
            [-300.0, 0.0, 0.0],
            [0.0; 3],
            Some(150.0),
        )
        .expect("reachable");
        assert!(close(g.time_s, 1000.0 / 450.0));
    }

    #[test]
    fn equal_speeds_still_solve_when_closing() {
        let g = earliest_intercept(
            [1000.0, 0.0, 0.0],
            [-100.0, 0.0, 0.0],
            [0.0; 3],
            Some(100.0),
        )
        .expect("reachable");
        assert!(close(g.time_s, 5.0));
        assert_eq!(
            earliest_intercept([1000.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0; 3], Some(100.0)),
            Err(NoGeometry::Unreachable)
        );
    }

    #[test]
    fn no_speed_and_bad_inputs_are_named() {
        assert_eq!(
            earliest_intercept([1.0; 3], [0.0; 3], [0.0; 3], None),
            Err(NoGeometry::NoClosingSpeed)
        );
        assert_eq!(
            earliest_intercept([f64::NAN; 3], [0.0; 3], [0.0; 3], Some(10.0)),
            Err(NoGeometry::NotFinite)
        );
        assert_eq!(
            earliest_intercept([1.0; 3], [0.0; 3], [0.0; 3], Some(0.0)),
            Err(NoGeometry::NotFinite)
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn a_track_on_top_of_the_effector_is_intercepted_now() {
        let g = earliest_intercept(
            [5.0, 5.0, 0.0],
            [10.0, 0.0, 0.0],
            [5.0, 5.0, 0.0],
            Some(1.0),
        )
        .expect("here already");
        assert_eq!(g.time_s, 0.0);
    }
}
