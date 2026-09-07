// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Row: "`coord` Bearing crossing" in `docs/verification-capability-table.md` §1
//! (docs/design/DN-27-bearing-only-detections.md §10, second row).
//!
//! Method: two bearings of known geometry, and a pair below the minimum crossing angle.
//! Criterion: the crossing position within 1e-6 m of the closed form; **the shallow pair
//! is refused, not returned with a large covariance**.
//!
//! The closed form is the test's own, derived per case from the geometry rather than
//! read back from the implementation, so agreement is evidence and not a tautology.
//! Where a case is symmetric the closed form is exact and the comparison is at 1e-6 m;
//! the elongation cases compare against `cot²` of the half-angle, which is the
//! analytic ratio of the two axes of the error ellipse for a symmetric pair.

use gungnir_coord::{
    cross_bearings, BearingRay, CrossingError, CrossingSettings, Enu,
    DEFAULT_MINIMUM_CROSSING_ANGLE_RAD,
};

fn ray(east_m: f64, north_m: f64, azimuth_deg: f64, sigma_deg: f64) -> BearingRay {
    let sigma = sigma_deg.to_radians();
    BearingRay {
        sensor_enu: Enu {
            e_m: east_m,
            n_m: north_m,
            u_m: 0.0,
        },
        azimuth_rad: azimuth_deg.to_radians(),
        elevation_rad: None,
        azimuth_variance_rad2: sigma * sigma,
    }
}

/// Two sensors on a 2 km east-west baseline, each bearing `half_angle` inward of north,
/// cross on the perpendicular bisector at `1000 / tan(half_angle)` metres north of the
/// baseline, at slant range `1000 / sin(half_angle)`.
///
/// That is the closed form this test compares against, and it is derived here from the
/// isosceles triangle rather than taken from the implementation.
fn symmetric_case(half_angle_deg: f64) -> (f64, f64, f64) {
    let half = half_angle_deg.to_radians();
    (0.0, 1_000.0 / half.tan(), 1_000.0 / half.sin())
}

/// Six symmetric geometries across the admissible range, each within 1e-6 m of the
/// closed form in both axes and in both ranges.
#[test]
fn the_crossing_lands_on_the_closed_form() {
    for half_angle_deg in [45.0, 35.0, 25.0, 15.0, 10.0, 8.0] {
        let (want_e, want_n, want_r) = symmetric_case(half_angle_deg);
        let crossing = cross_bearings(
            ray(-1_000.0, 0.0, half_angle_deg, 1.0),
            ray(1_000.0, 0.0, -half_angle_deg, 1.0),
            CrossingSettings::default(),
        )
        .unwrap_or_else(|e| panic!("{half_angle_deg} degrees should cross: {e}"));

        assert!(
            (crossing.east_m - want_e).abs() < 1e-6,
            "{half_angle_deg} deg: east {} against {want_e}",
            crossing.east_m
        );
        assert!(
            (crossing.north_m - want_n).abs() < 1e-6,
            "{half_angle_deg} deg: north {} against {want_n}",
            crossing.north_m
        );
        for (i, r) in crossing.range_m.iter().enumerate() {
            assert!(
                (r - want_r).abs() < 1e-6,
                "{half_angle_deg} deg: range {i} is {r} against {want_r}"
            );
        }
        assert!(
            (crossing.crossing_angle_rad - 2.0 * half_angle_deg.to_radians()).abs() < 1e-12,
            "{half_angle_deg} deg: crossing angle {}",
            crossing.crossing_angle_rad
        );
    }
}

/// An asymmetric geometry, so the test is not only exercising the case where the
/// answer sits on an axis of symmetry. Two sensors on the east axis; the first bears
/// due north-east, the second due north. They meet where the 45-degree line from the
/// origin reaches east 3000, which is (3000, 3000).
#[test]
fn an_asymmetric_crossing_lands_on_the_closed_form() {
    let crossing = cross_bearings(
        ray(0.0, 0.0, 45.0, 0.5),
        ray(3_000.0, 0.0, 0.0, 2.0),
        CrossingSettings::default(),
    )
    .expect("a 45 degree crossing");
    assert!((crossing.east_m - 3_000.0).abs() < 1e-6, "{crossing:?}");
    assert!((crossing.north_m - 3_000.0).abs() < 1e-6, "{crossing:?}");
    assert!((crossing.range_m[0] - 3_000.0 * std::f64::consts::SQRT_2).abs() < 1e-6);
    assert!((crossing.range_m[1] - 3_000.0).abs() < 1e-6);
    // The covariance is a real covariance: symmetric and positive definite.
    let c = crossing.covariance_en_m2;
    assert!((c[0][1] - c[1][0]).abs() < 1e-9, "not symmetric: {c:?}");
    assert!(c[0][0] > 0.0 && c[1][1] > 0.0, "{c:?}");
    assert!(
        c[0][0] * c[1][1] - c[0][1] * c[1][0] > 0.0,
        "not positive definite: {c:?}"
    );
}

/// **The criterion's second half.** A pair below the stated minimum is refused with the
/// angle named, not returned with a very large covariance -- DN-27 §5 rule 2: "a track
/// whose position uncertainty is tens of kilometres long is not a track, and drawing it
/// as one is the same error as §2's".
#[test]
fn a_crossing_below_the_minimum_is_refused_and_not_widened() {
    let settings = CrossingSettings::default();
    for half_angle_deg in [7.0, 5.0, 2.0, 0.5, 0.01] {
        let result = cross_bearings(
            ray(-1_000.0, 0.0, half_angle_deg, 1.0),
            ray(1_000.0, 0.0, -half_angle_deg, 1.0),
            settings,
        );
        match result {
            Err(CrossingError::TooShallow {
                crossing_angle_rad,
                minimum_rad,
            }) => {
                assert!(
                    (crossing_angle_rad - 2.0 * half_angle_deg.to_radians()).abs() < 1e-12,
                    "the refusal must name the angle it refused: {crossing_angle_rad}"
                );
                assert!((minimum_rad - DEFAULT_MINIMUM_CROSSING_ANGLE_RAD).abs() < 1e-12);
            }
            other => panic!("{half_angle_deg} deg half-angle was not refused: {other:?}"),
        }
    }
}

/// The boundary is where it says it is: a crossing a hair inside the minimum is
/// refused and one a hair outside it is accepted, so the rule is the stated number and
/// not an approximation of it.
#[test]
fn the_minimum_is_exactly_the_stated_one() {
    let settings = CrossingSettings::default();
    let half = DEFAULT_MINIMUM_CROSSING_ANGLE_RAD / 2.0;
    let at = |half_rad: f64| {
        cross_bearings(
            BearingRay {
                azimuth_rad: half_rad,
                ..ray(-1_000.0, 0.0, 0.0, 1.0)
            },
            BearingRay {
                azimuth_rad: -half_rad,
                ..ray(1_000.0, 0.0, 0.0, 1.0)
            },
            settings,
        )
    };
    assert!(at(half * (1.0 - 1e-9)).is_err(), "just inside must refuse");
    assert!(at(half * (1.0 + 1e-9)).is_ok(), "just outside must cross");
}

/// The covariance is the geometry's and not a constant: the same angular error at ten
/// times the range gives a hundred times the variance (DN-27 §6). A conversion that
/// kept a fixed positional variance would be wrong at every range but one, and this is
/// the measurement of that.
#[test]
fn the_cross_range_variance_grows_with_the_square_of_the_range() {
    let settings = CrossingSettings::default();
    let variance_at = |baseline_m: f64| {
        cross_bearings(
            ray(-baseline_m / 2.0, 0.0, 45.0, 1.0),
            ray(baseline_m / 2.0, 0.0, -45.0, 1.0),
            settings,
        )
        .expect("a right-angled crossing")
        .covariance_en_m2[0][0]
    };
    let near = variance_at(2_000.0);
    let far = variance_at(20_000.0);
    let ratio = far / near;
    assert!(
        (ratio - 100.0).abs() < 1e-6,
        "ten times the range should be a hundred times the variance, got {ratio}"
    );

    // And the absolute value is the closed form. For a symmetric pair at half-angle θ
    // the information along east is `2 cos²θ / σ_cross²`, so the east variance is
    // `σ_cross² / (2 cos²θ)`; at θ = 45 degrees that is exactly `σ_cross²`, with
    // `σ_cross = r σ_azimuth` and `r` the slant range (DN-27 §6).
    let sigma = 1.0_f64.to_radians();
    let r = 1_000.0 * std::f64::consts::SQRT_2;
    let want = (r * sigma).powi(2);
    assert!(
        (near - want).abs() < 1e-6 * want,
        "{near} against the closed form {want}"
    );
}

/// The ellipse is long along the bisector and short across it, by `cot²` of the
/// half-angle, and it lengthens without bound as the crossing closes. Measured at four
/// angles against the closed form, not against each other.
#[test]
fn the_ellipse_is_elongated_along_the_bisector_by_the_closed_form() {
    for half_angle_deg in [45.0, 30.0, 15.0, 8.0] {
        let crossing = cross_bearings(
            ray(-1_000.0, 0.0, half_angle_deg, 1.0),
            ray(1_000.0, 0.0, -half_angle_deg, 1.0),
            CrossingSettings::default(),
        )
        .expect("crosses");
        // The bisector is north here, so the frame's own axes are the ellipse's.
        assert!(
            crossing.covariance_en_m2[0][1].abs() < 1e-6,
            "a symmetric pair has no cross term: {crossing:?}"
        );
        let elongation = crossing.covariance_en_m2[1][1] / crossing.covariance_en_m2[0][0];
        let want = 1.0 / half_angle_deg.to_radians().tan().powi(2);
        assert!(
            (elongation - want).abs() < 1e-6 * want.max(1.0),
            "{half_angle_deg} deg: elongation {elongation} against cot^2 {want}"
        );
    }
}

/// The height appears only when both bearings measured one. A crossing of two azimuths
/// determines a place on the ground plane and nothing about height, and reporting zero
/// would be the horizon (DN-27 §4).
#[test]
fn a_height_is_reported_only_when_both_bearings_carried_an_elevation() {
    let settings = CrossingSettings::default();
    let mut a = ray(-1_000.0, 0.0, 45.0, 1.0);
    let mut b = ray(1_000.0, 0.0, -45.0, 1.0);
    let crossing = cross_bearings(a, b, settings).expect("crosses");
    assert!(crossing.up_m.is_none());
    assert!(crossing.up_variance_m2.is_none());

    // Both at 30 degrees up, at a horizontal range of 1000*sqrt(2): height is
    // r tan(30) in both, so they agree exactly.
    let elevation = 30.0_f64.to_radians();
    a.elevation_rad = Some(elevation);
    b.elevation_rad = Some(elevation);
    let crossing = cross_bearings(a, b, settings).expect("crosses");
    let want = 1_000.0 * std::f64::consts::SQRT_2 * elevation.tan();
    let got = crossing.up_m.expect("both elevations present");
    assert!((got - want).abs() < 1e-6, "{got} against {want}");
    assert!(crossing.up_variance_m2.is_some_and(|v| v < 1e-12));

    // One of the two missing takes the height away again, rather than halving it.
    b.elevation_rad = None;
    let crossing = cross_bearings(a, b, settings).expect("crosses");
    assert!(crossing.up_m.is_none(), "a missing elevation is not a zero");
}

/// A deployment may raise the minimum; nothing reads a lower one as safe by default.
#[test]
fn a_deployment_may_state_a_stricter_minimum() {
    let strict = CrossingSettings {
        minimum_angle_rad: 40.0_f64.to_radians(),
    };
    // A 30 degree crossing: fine by default, refused by this deployment.
    let a = ray(-1_000.0, 0.0, 15.0, 1.0);
    let b = ray(1_000.0, 0.0, -15.0, 1.0);
    assert!(cross_bearings(a, b, CrossingSettings::default()).is_ok());
    assert!(matches!(
        cross_bearings(a, b, strict),
        Err(CrossingError::TooShallow { .. })
    ));
}
