// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Two bearings crossing to a position, with the covariance the geometry gives it
//! (docs/design/DN-27-bearing-only-detections.md §5 rule 2 and §6; GAP-001's acoustic,
//! passive-RF and spotter halves).
//!
//! Row: "`coord` Bearing crossing" in `docs/verification-capability-table.md` §1.
//! Method: two bearings of known geometry, and a pair below the minimum crossing angle.
//! Criterion: the crossing position within 1e-6 m of the closed form; the shallow pair
//! refused, not returned with a large covariance.
//!
//! # Why the conversion lives here and not in an adapter
//!
//! `gungnir-coord` owns every other frame transformation, and DN-27 §6 puts this one
//! here for a second reason: **a conversion that loses an error term is the kind of
//! defect that is found years later in a gate that had always been green.** An angular
//! error is a constant; the cross-range error it implies is not. One degree at 1 km is
//! 17 m across and the same degree at 30 km is 520 m, so a conversion that produced a
//! position with a fixed positional variance would be wrong at every range but one.
//!
//! # The geometry, and why the answer is an information matrix
//!
//! Each bearing constrains the component of the target's horizontal position that is
//! **perpendicular** to its own line of sight, and says nothing at all about the
//! component along it. For a bearing measured at range `r` with azimuth standard
//! deviation `σ_az`, that perpendicular constraint has standard deviation `r · σ_az`
//! for small angles -- DN-27 §6's `σ_cross`. Writing `n_i` for the unit vector
//! perpendicular to bearing `i`, the two constraints combine as
//!
//! ```text
//! J = Σ_i  n_i n_iᵀ / (r_i σ_i)²          (the Fisher information of the pair)
//! P = J⁻¹                                 (the position covariance)
//! ```
//!
//! `det J = sin²θ / (r₁σ₁ r₂σ₂)²` for a crossing angle `θ`, so `P` grows without bound
//! as the crossing closes up, which is exactly DN-27 §6's "`σ_along` from the crossing
//! angle, and unbounded as that angle goes to zero". The ellipse this produces is long
//! along the bisector and short across it, and that elongation is information rather
//! than an artefact to be hidden (§7).
//!
//! # What is refused rather than returned
//!
//! **A crossing below [`CrossingSettings::minimum_angle_rad`] is an error, not a
//! wide answer.** DN-27 §5 rule 2: "a track whose position uncertainty is tens of
//! kilometres long is not a track, and drawing it as one is the same error as §2's" --
//! §2 being the invented range this whole note exists to forbid. A caller that wants to
//! know how bad a shallow crossing would have been can read the angle off the error.
//!
//! A crossing **behind** either sensor is refused for the same reason: the rays cross
//! but the bearings do not, and the intersection of the two infinite lines is a place
//! neither sensor was looking at.
//!
//! # What this module deliberately does not do
//!
//! * **It does not decide which bearing crosses which.** With `n` bearings from two
//!   sensors there are `n²` candidate crossings and at most `n` are real; the rest are
//!   ghosts and are indistinguishable from real ones on geometry alone. That is the
//!   JPDA-shaped problem `gungnir-association` owns, and DN-27 §9 names it as an open
//!   row rather than specifying a rule that would not work.
//! * **It does not fit three or more bearings.** Three rarely meet at a point and the
//!   least-squares fit that resolves them has a different error model (DN-27 §9).
//! * **It never intersects a bearing with the terrain** (DN-27 §2).
//!
//! Deterministic mathematics: no logging here (agentic-coding-standards.md §2.8).

use crate::Enu;

/// A bearing measured from a known place, in the local ENU frame.
///
/// Azimuth is `atan2(east, north)`: a compass bearing, zero at north and increasing to
/// the east, the convention `gungnir_model::Measurement` and
/// `gungnir_filters::RangeAzimuthElevation` both state. Elevation is measured up from
/// the horizontal plane and is `None` when the sensor reported none -- a missing
/// elevation is not a zero one (DN-27 §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BearingRay {
    /// Where the bearing was measured from, local ENU metres.
    pub sensor_enu: Enu,
    pub azimuth_rad: f64,
    pub elevation_rad: Option<f64>,
    /// Angular variance of the azimuth, radians squared. It is what sets the
    /// cross-range error at the crossing, and it is required for that reason.
    pub azimuth_variance_rad2: f64,
}

/// How shallow a crossing a deployment will accept, and nothing else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossingSettings {
    /// Crossings closing to less than this are refused (DN-27 §5 rule 2), radians.
    pub minimum_angle_rad: f64,
}

/// The default minimum crossing angle: 15 degrees.
///
/// **DN-27 §5 rule 2 requires a stated minimum and deliberately does not state a
/// value; this is that value, chosen here, and this is the reason.** At a crossing
/// angle `θ` the along-bisector standard deviation exceeds the cross-range one by
/// roughly `1 / sin θ`, so 15 degrees is where a crossing stops being a position with a
/// stretched error and starts being a direction with a guess of range attached: the
/// long axis is already near four times the short one, and it doubles again by 7
/// degrees. A deployment whose sensors are sited to do better should raise it; nothing
/// here reads a lower one as safe.
pub const DEFAULT_MINIMUM_CROSSING_ANGLE_RAD: f64 = 15.0 * std::f64::consts::PI / 180.0;

impl Default for CrossingSettings {
    fn default() -> Self {
        Self {
            minimum_angle_rad: DEFAULT_MINIMUM_CROSSING_ANGLE_RAD,
        }
    }
}

/// Where two bearings cross, and how well.
///
/// The horizontal position is separated from the height on purpose. Two azimuths
/// determine a place on the ground plane; they determine a **height** only if both
/// carried an elevation, and packing an unknown height into the same triple as a zero
/// one would put the crossing on the horizon, which is the mistake DN-27 §4 makes an
/// `Option` to avoid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BearingCrossing {
    pub east_m: f64,
    pub north_m: f64,
    /// Horizontal position covariance in `[[ee, en], [ne, nn]]` order, metres squared.
    /// Long along the bisector and short across it, by construction.
    pub covariance_en_m2: [[f64; 2]; 2],
    /// Height of the crossing, metres, present **only** when both bearings carried an
    /// elevation. `None` means no height was measured, not a height of zero.
    pub up_m: Option<f64>,
    /// Variance of [`BearingCrossing::up_m`], metres squared; `Some` exactly when it is.
    pub up_variance_m2: Option<f64>,
    /// The angle the two bearings cross at, radians, in `(0, pi/2]`. Reported because
    /// it is the number that says how much to believe the covariance.
    pub crossing_angle_rad: f64,
    /// Horizontal range from each sensor to the crossing, metres, in the order the
    /// bearings were given.
    pub range_m: [f64; 2],
}

/// Why two bearings did not become a position.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CrossingError {
    /// The bearings cross too shallowly to localise. **Refused rather than returned
    /// with a very large covariance** (DN-27 §5 rule 2).
    #[error(
        "the bearings cross at {crossing_angle_rad} rad, below the stated minimum \
         {minimum_rad} rad; a crossing this shallow determines a direction and not a \
         position, and is refused rather than initiated with a huge covariance"
    )]
    TooShallow {
        crossing_angle_rad: f64,
        minimum_rad: f64,
    },
    /// The lines cross, but behind one of the sensors: neither was looking there.
    #[error("the lines meet {behind_m} m behind sensor {sensor}, which was not looking there")]
    BehindSensor { sensor: usize, behind_m: f64 },
    /// The two sensors are at the same place horizontally, so there is no baseline and
    /// no crossing at any angle.
    #[error("the two bearings share a sensor position, so there is no baseline to cross over")]
    NoBaseline,
    /// A bearing carried a value that is not a finite number, or a non-positive
    /// azimuth variance, which would make the information matrix meaningless.
    #[error("a bearing is not usable: {what}")]
    Unusable { what: &'static str },
}

/// Cross two bearings from separated sensors into a position and the covariance the
/// geometry gives it (DN-27 §5 rule 2, §6).
///
/// The crossing is solved in the horizontal plane, where the azimuths live. When both
/// bearings carry an elevation the height is the inverse-variance-weighted mean of the
/// two each implies at its own horizontal range; when either does not, the result says
/// so with `None` rather than reporting a height of zero.
///
/// # Errors
///
/// [`CrossingError::TooShallow`] below `settings.minimum_angle_rad`, which is the rule
/// this function exists to enforce; [`CrossingError::BehindSensor`] when the lines meet
/// behind a sensor; [`CrossingError::NoBaseline`] for co-located sensors; and
/// [`CrossingError::Unusable`] for a non-finite input or a non-positive variance.
pub fn cross_bearings(
    a: BearingRay,
    b: BearingRay,
    settings: CrossingSettings,
) -> Result<BearingCrossing, CrossingError> {
    for ray in [&a, &b] {
        if !(ray.azimuth_rad.is_finite()
            && ray.sensor_enu.e_m.is_finite()
            && ray.sensor_enu.n_m.is_finite()
            && ray.sensor_enu.u_m.is_finite())
        {
            return Err(CrossingError::Unusable {
                what: "a bearing or a sensor position is not a finite number",
            });
        }
        if !(ray.azimuth_variance_rad2.is_finite() && ray.azimuth_variance_rad2 > 0.0) {
            return Err(CrossingError::Unusable {
                what: "an azimuth variance is not finite and positive, so the crossing \
                       would have no stated error",
            });
        }
        if ray.elevation_rad.is_some_and(|e| !e.is_finite()) {
            return Err(CrossingError::Unusable {
                what: "an elevation is not a finite number",
            });
        }
    }

    // Unit direction of each bearing in (east, north). Azimuth is atan2(east, north),
    // so east is the sine and north the cosine, and not the other way round.
    let (a_e, a_n) = (a.azimuth_rad.sin(), a.azimuth_rad.cos());
    let (b_e, b_n) = (b.azimuth_rad.sin(), b.azimuth_rad.cos());

    let baseline_e = b.sensor_enu.e_m - a.sensor_enu.e_m;
    let baseline_n = b.sensor_enu.n_m - a.sensor_enu.n_m;
    if baseline_e == 0.0 && baseline_n == 0.0 {
        return Err(CrossingError::NoBaseline);
    }

    // The crossing angle first, so a shallow pair is refused before anything is
    // computed from a nearly singular determinant.
    let cross = a_e * b_n - a_n * b_e;
    let dot = a_e * b_e + a_n * b_n;
    // Folded onto (0, pi/2]: two bearings pointing at each other cross just as well as
    // two pointing the same way, and the geometry cares about the line and not the ray.
    let crossing_angle_rad = cross.abs().atan2(dot.abs());
    if crossing_angle_rad < settings.minimum_angle_rad {
        return Err(CrossingError::TooShallow {
            crossing_angle_rad,
            minimum_rad: settings.minimum_angle_rad,
        });
    }

    // p_a + t_a d_a = p_b + t_b d_b, solved by Cramer's rule. `cross` cannot be zero
    // here: the angle test above already refused everything near it.
    let t_a = (baseline_e * b_n - baseline_n * b_e) / cross;
    let t_b = (baseline_e * a_n - baseline_n * a_e) / cross;
    for (sensor, t) in [(0usize, t_a), (1usize, t_b)] {
        if t <= 0.0 {
            return Err(CrossingError::BehindSensor {
                sensor,
                behind_m: -t,
            });
        }
    }

    let east_m = a.sensor_enu.e_m + t_a * a_e;
    let north_m = a.sensor_enu.n_m + t_a * a_n;

    // The cross-range standard deviation each bearing carries at its own range: DN-27
    // §6's `σ_cross ≈ r · σ_azimuth`. It is a function of range, which is the whole
    // reason a constant positional variance would be wrong.
    let sigma_a = t_a * a.azimuth_variance_rad2.sqrt();
    let sigma_b = t_b * b.azimuth_variance_rad2.sqrt();

    // J = Σ n_i n_iᵀ / σ_i², with n_i perpendicular to bearing i in the ground plane,
    // written `[east, north]`.
    let perpendicular = [[a_n, -a_e], [b_n, -b_e]];
    let weight = [1.0 / (sigma_a * sigma_a), 1.0 / (sigma_b * sigma_b)];
    let mut information = [[0.0_f64; 2]; 2];
    for (n, w) in perpendicular.iter().zip(weight) {
        for row in 0..2 {
            for col in 0..2 {
                information[row][col] += w * n[row] * n[col];
            }
        }
    }
    let det = information[0][0] * information[1][1] - information[0][1] * information[1][0];
    if !det.is_finite() || det <= 0.0 {
        // Unreachable for a crossing that passed the angle test with positive
        // variances; refused rather than inverted, because a covariance built from a
        // non-positive determinant is a confident answer with no information in it.
        return Err(CrossingError::Unusable {
            what: "the crossing's information matrix is singular",
        });
    }
    let covariance_en_m2 = [
        [information[1][1] / det, -information[0][1] / det],
        [-information[1][0] / det, information[0][0] / det],
    ];

    // Height, only if both bearings measured one. Each implies `u = u_sensor + r tanθ`
    // at its own horizontal range.
    let (up_m, up_variance_m2) = match (a.elevation_rad, b.elevation_rad) {
        (Some(el_a), Some(el_b)) => {
            let u_a = a.sensor_enu.u_m + t_a * el_a.tan();
            let u_b = b.sensor_enu.u_m + t_b * el_b.tan();
            // The two independent heights are averaged, and the variance reported is
            // the variance of that mean estimated from the two samples themselves:
            // half their squared difference. **This is the disagreement between the two
            // sensors and not a propagated elevation error**, because neither bearing
            // states an elevation variance to propagate -- a `Bearing`'s
            // `elevation_variance_rad2` is optional, and this function takes the shape
            // that is always present. A caller that has both elevation errors and wants
            // them propagated needs a three-dimensional fit, which DN-27 §9 puts
            // outside this note.
            let mean = f64::midpoint(u_a, u_b);
            let half_difference = 0.5 * (u_a - u_b);
            (Some(mean), Some(half_difference * half_difference))
        }
        _ => (None, None),
    };

    Ok(BearingCrossing {
        east_m,
        north_m,
        covariance_en_m2,
        up_m,
        up_variance_m2,
        crossing_angle_rad,
        range_m: [t_a, t_b],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(e: f64, n: f64, azimuth_deg: f64, sigma_deg: f64) -> BearingRay {
        let sigma = sigma_deg.to_radians();
        BearingRay {
            sensor_enu: Enu {
                e_m: e,
                n_m: n,
                u_m: 0.0,
            },
            azimuth_rad: azimuth_deg.to_radians(),
            elevation_rad: None,
            azimuth_variance_rad2: sigma * sigma,
        }
    }

    /// Two sensors 2 km apart on the east axis, each looking 45 degrees inward, cross
    /// at the apex of the isosceles right triangle they make: 1000 m east of the first
    /// and 1000 m north of both.
    #[test]
    fn a_right_angled_crossing_lands_on_the_closed_form() {
        let a = ray(0.0, 0.0, 45.0, 1.0);
        let b = ray(2_000.0, 0.0, -45.0, 1.0);
        let c = cross_bearings(a, b, CrossingSettings::default()).expect("crosses");
        assert!((c.east_m - 1_000.0).abs() < 1e-6, "east {}", c.east_m);
        assert!((c.north_m - 1_000.0).abs() < 1e-6, "north {}", c.north_m);
        assert!(
            (c.crossing_angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
            "angle {}",
            c.crossing_angle_rad
        );
        // Both ranges are the hypotenuse of a 1000 m square.
        let expected = 1_000.0 * std::f64::consts::SQRT_2;
        assert!((c.range_m[0] - expected).abs() < 1e-6);
        assert!((c.range_m[1] - expected).abs() < 1e-6);
        assert!(c.up_m.is_none(), "neither bearing carried an elevation");
    }

    /// The cross-range error scales with range: the same angular error at ten times the
    /// distance gives a hundred times the variance. DN-27 §6 is this test.
    #[test]
    fn the_covariance_grows_with_the_square_of_the_range() {
        let settings = CrossingSettings::default();
        let near = cross_bearings(
            ray(0.0, 0.0, 45.0, 1.0),
            ray(2_000.0, 0.0, -45.0, 1.0),
            settings,
        )
        .expect("crosses");
        let far = cross_bearings(
            ray(0.0, 0.0, 45.0, 1.0),
            ray(20_000.0, 0.0, -45.0, 1.0),
            settings,
        )
        .expect("crosses");
        let ratio = far.covariance_en_m2[0][0] / near.covariance_en_m2[0][0];
        assert!(
            (ratio - 100.0).abs() < 1e-6,
            "ten times the range should be a hundred times the variance, got {ratio}"
        );
    }

    /// A shallow pair is refused, and the error says how shallow. Not returned with a
    /// very large covariance: DN-27 §5 rule 2.
    #[test]
    fn a_shallow_crossing_is_refused_rather_than_widened() {
        // Five degrees of crossing: well inside the default 15 degree minimum.
        let a = ray(0.0, 0.0, 0.0, 1.0);
        let b = ray(2_000.0, 0.0, 5.0, 1.0);
        match cross_bearings(a, b, CrossingSettings::default()) {
            Err(CrossingError::TooShallow {
                crossing_angle_rad,
                minimum_rad,
            }) => {
                assert!((crossing_angle_rad - 5.0_f64.to_radians()).abs() < 1e-12);
                assert!((minimum_rad - DEFAULT_MINIMUM_CROSSING_ANGLE_RAD).abs() < 1e-12);
            }
            other => panic!("a five degree crossing must be refused, got {other:?}"),
        }
    }

    /// The error ellipse is long along the bisector and short across it, and gets
    /// longer as the crossing closes up. Measured at the smallest angle the default
    /// setting admits.
    #[test]
    fn the_ellipse_is_elongated_and_lengthens_as_the_crossing_closes() {
        let elongation = |half_angle_deg: f64| {
            // Two sensors on the east axis looking inward, symmetric about north, so
            // the bisector is north and the ellipse's long axis is the north one.
            let a = ray(-1_000.0, 0.0, half_angle_deg, 1.0);
            let b = ray(1_000.0, 0.0, -half_angle_deg, 1.0);
            let c = cross_bearings(a, b, CrossingSettings::default()).expect("crosses");
            // Off-diagonals vanish by symmetry, so the axes are the frame's own.
            assert!(c.covariance_en_m2[0][1].abs() < 1e-6);
            c.covariance_en_m2[1][1] / c.covariance_en_m2[0][0]
        };
        // A right-angled crossing at equal ranges with equal angular errors is the one
        // case that *is* isotropic, and saying so is worth more than asserting a bound
        // it happens to clear: it is the reference the elongation below is measured
        // against.
        let square = elongation(45.0);
        assert!(
            (square - 1.0).abs() < 1e-9,
            "a symmetric right-angled crossing is a circle, got {square}"
        );
        let narrow = elongation(8.0);
        // The closed form for this symmetric pair is `cot²(half-angle)`: about 50.6 at
        // a 16 degree crossing, against 1 at a right-angled one. That is DN-27 §6's
        // "`σ_along` from the crossing angle, and unbounded as that angle goes to zero"
        // measured at the shallowest crossing the default setting admits.
        assert!(
            (narrow - 1.0 / 8.0_f64.to_radians().tan().powi(2)).abs() < 1e-6,
            "closing the crossing to 16 degrees should lengthen the ellipse to \
             cot^2 of its half-angle, got {narrow}"
        );
        assert!(narrow > square * 50.0, "{square} then {narrow}");
    }

    /// The lines meet behind the sensors when both look away from each other. That is
    /// an intersection of two infinite lines and not a crossing of two bearings.
    #[test]
    fn a_meeting_behind_a_sensor_is_refused() {
        let a = ray(0.0, 0.0, -135.0, 1.0);
        let b = ray(2_000.0, 0.0, 135.0, 1.0);
        assert!(matches!(
            cross_bearings(a, b, CrossingSettings::default()),
            Err(CrossingError::BehindSensor { .. })
        ));
    }

    /// Two bearings from the same place have no baseline, at any angle.
    #[test]
    fn co_located_sensors_have_no_baseline() {
        assert_eq!(
            cross_bearings(
                ray(50.0, 50.0, 10.0, 1.0),
                ray(50.0, 50.0, 80.0, 1.0),
                CrossingSettings::default()
            ),
            Err(CrossingError::NoBaseline)
        );
    }

    /// A bearing with no stated azimuth error cannot produce a covariance, so it is
    /// refused rather than given a default one.
    #[test]
    fn a_bearing_with_no_stated_error_is_refused() {
        let mut a = ray(0.0, 0.0, 45.0, 1.0);
        a.azimuth_variance_rad2 = 0.0;
        assert!(matches!(
            cross_bearings(
                a,
                ray(2_000.0, 0.0, -45.0, 1.0),
                CrossingSettings::default()
            ),
            Err(CrossingError::Unusable { .. })
        ));
    }

    /// Both elevations present gives a height; one missing gives none, and never a
    /// height of zero -- the distinction DN-27 §4 exists for.
    #[test]
    fn a_height_appears_only_when_both_bearings_measured_one() {
        let settings = CrossingSettings::default();
        let mut a = ray(0.0, 0.0, 45.0, 1.0);
        let mut b = ray(2_000.0, 0.0, -45.0, 1.0);
        // 45 degrees up at a horizontal range of 1000*sqrt(2) is that same height.
        a.elevation_rad = Some(std::f64::consts::FRAC_PI_4);
        b.elevation_rad = Some(std::f64::consts::FRAC_PI_4);
        let both = cross_bearings(a, b, settings).expect("crosses");
        let expected = 1_000.0 * std::f64::consts::SQRT_2;
        let height = both.up_m.expect("both elevations present");
        assert!((height - expected).abs() < 1e-6, "height {height}");
        assert!(both.up_variance_m2.is_some_and(|v| v < 1e-12));

        b.elevation_rad = None;
        let one = cross_bearings(a, b, settings).expect("crosses");
        assert!(one.up_m.is_none(), "a missing elevation is not a zero one");
        assert!(one.up_variance_m2.is_none());
    }
}
