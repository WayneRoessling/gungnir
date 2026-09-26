// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The risk score against time to impact (GAP-124): the `gungnir-assessment` Risk scoring
//! row of `docs/verification-capability-table.md` §2, "score monotonic in time-to-impact",
//! and MOP-28's "non-increasing in time to impact".
//!
//! The row's earlier fixture moved range at a constant speed, so time to impact and range
//! moved together and a score that read only range passed it. These fixtures move time to
//! impact **independently of range**: one range at two closing speeds, a far fast track
//! against a near slow one, and a generated grid of ranges, speeds and bearings sorted by
//! time to impact. Every track here is confidently closing -- its closing speed is many
//! times its one-sigma -- which is the population the monotonicity holds over exactly
//! (`gungnir_assessment::kinematics`; D-83).

use gungnir_assessment::{AssetAnchor, AssetListAssessor, RiskScore, ThreatAssessor};
use gungnir_model::{
    AssetExtent, AssetId, AssetPriority, Classification, DefendedAsset, Geodetic, MissionTime,
    Provenance, Quality, Releasability, TrackId, TrackStatus, TrackView,
};
use nalgebra::{SMatrix, SVector};

const MAX_RANGE_M: f64 = 10_000.0;

fn anchor(radius_m: f64) -> AssetAnchor {
    let centre = Geodetic {
        lat_rad: 0.0,
        lon_rad: 0.0,
        alt_m: 0.0,
    };
    AssetAnchor {
        asset: DefendedAsset {
            id: AssetId(1),
            name: "port".into(),
            extent: if radius_m > 0.0 {
                AssetExtent::Circle {
                    center: centre,
                    radius_m,
                }
            } else {
                AssetExtent::Point { position: centre }
            },
            priority: AssetPriority::High,
            warning: None,
            note: None,
        },
        center_enu: [0.0; 3],
    }
}

fn assessor(radius_m: f64) -> AssetListAssessor {
    AssetListAssessor::new(1, vec![anchor(radius_m)], MAX_RANGE_M).with_urgency_half_time_s(60.0)
}

/// A track `range_m` from the asset's centre on `bearing_deg`, flying straight at it at
/// `speed_mps`, with a velocity one-sigma of half a metre a second.
fn inbound(id: u64, range_m: f64, bearing_deg: f64, speed_mps: f64) -> TrackView {
    let b = bearing_deg.to_radians();
    let (e, n) = (range_m * b.sin(), range_m * b.cos());
    let mut covariance = SMatrix::<f64, 6, 6>::identity() * 25.0;
    for i in 3..6 {
        covariance[(i, i)] = 0.25;
    }
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: SVector::<f64, 6>::new(e, n, 0.0, -speed_mps * b.sin(), -speed_mps * b.cos(), 0.0),
        covariance,
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn score(assessor: &AssetListAssessor, track: TrackView) -> RiskScore {
    assessor.assess(&[track])[0]
}

fn time_to_impact(s: &RiskScore) -> f32 {
    s.time_to_impact_s
        .expect("every fixture track is closing, so every one has a time to impact")
}

/// One range, two closing speeds: the faster arrives sooner and scores higher.
#[test]
fn at_one_range_the_faster_closer_scores_higher() {
    let a = assessor(0.0);
    for range in [500.0, 3_000.0, 9_000.0] {
        let slow = score(&a, inbound(1, range, 40.0, 30.0));
        let fast = score(&a, inbound(2, range, 40.0, 150.0));
        assert!(time_to_impact(&fast) < time_to_impact(&slow));
        assert!(
            fast.score > slow.score,
            "at {range} m: {} s scores {}, {} s scores {}",
            time_to_impact(&fast),
            fast.score,
            time_to_impact(&slow),
            slow.score
        );
    }
}

/// The defect the row was held on: a far, fast track that arrives sooner scored below a
/// near, slow one because the score read range and not time.
#[test]
fn a_far_fast_track_arriving_sooner_outscores_a_near_slow_one() {
    let a = assessor(0.0);
    for (far, near) in [
        // 8 km at 200 m/s (40 s) against 1 km at 5 m/s (200 s).
        (
            inbound(1, 8_000.0, 90.0, 200.0),
            inbound(2, 1_000.0, 270.0, 5.0),
        ),
        // 9.5 km at 300 m/s (32 s) against 600 m at 10 m/s (60 s).
        (
            inbound(3, 9_500.0, 10.0, 300.0),
            inbound(4, 600.0, 190.0, 10.0),
        ),
        // Nearly tied in time and far apart in range: 6 km at 100 m/s (60 s) against
        // 1.2 km at 19.9 m/s (60.3 s). Time decides, not range.
        (
            inbound(5, 6_000.0, 135.0, 100.0),
            inbound(6, 1_200.0, 315.0, 19.9),
        ),
    ] {
        let (f, n) = (score(&a, far), score(&a, near));
        assert!(time_to_impact(&f) < time_to_impact(&n));
        assert!(
            f.score >= n.score,
            "far-fast {} s scored {}, below near-slow {} s at {}",
            time_to_impact(&f),
            f.score,
            time_to_impact(&n),
            n.score
        );
    }
}

/// A grid of ranges, speeds and bearings, point and area assets, sorted by time to
/// impact: the score never rises as time to impact grows, and falls wherever time to
/// impact differs by more than a percent.
#[test]
fn over_a_generated_grid_the_score_never_rises_with_time_to_impact() {
    for radius in [0.0, 400.0] {
        let a = assessor(radius);
        let mut scored = Vec::new();
        let mut id = 0;
        for range in [
            450.0, 800.0, 1_500.0, 2_500.0, 4_000.0, 6_000.0, 8_000.0, 9_900.0,
        ] {
            for speed in [6.0, 12.0, 25.0, 50.0, 90.0, 160.0, 320.0] {
                for bearing in [0.0, 73.0, 181.0, 299.0] {
                    id += 1;
                    scored.push(score(&a, inbound(id, range, bearing, speed)));
                }
            }
        }
        scored.sort_by(|x, y| time_to_impact(x).total_cmp(&time_to_impact(y)));
        for pair in scored.windows(2) {
            let (sooner, later) = (&pair[0], &pair[1]);
            let (t0, t1) = (time_to_impact(sooner), time_to_impact(later));
            assert!(
                sooner.score >= later.score,
                "radius {radius}: {t0} s scored {} but {t1} s scored {}",
                sooner.score,
                later.score
            );
            if t1 > t0 * 1.01 {
                assert!(
                    sooner.score > later.score,
                    "radius {radius}: {t0} s and {t1} s both scored {}",
                    sooner.score
                );
            }
        }
    }
}

/// One track flown in: as it closes its time to impact falls and its score never does.
#[test]
fn a_track_flying_in_rises_as_its_time_to_impact_falls() {
    let a = assessor(0.0);
    let mut previous: Option<RiskScore> = None;
    for step in 0..95 {
        let range = 9_500.0 - 100.0 * f64::from(step);
        let now = score(&a, inbound(1, range, 225.0, 100.0));
        if let Some(before) = previous {
            assert!(time_to_impact(&now) < time_to_impact(&before));
            assert!(now.score >= before.score, "at {range} m");
        }
        previous = Some(now);
    }
}

/// Time to impact is a closing track's; a track opening or at rest has none and takes no
/// urgency from it. At the same range, any confidently closing track outscores one that
/// is not closing, because it has a time to impact and the other has none.
#[test]
fn a_track_that_is_not_closing_has_no_time_to_impact_and_no_urgency() {
    let a = assessor(0.0);
    let opening = score(&a, inbound(1, 2_000.0, 30.0, -80.0));
    let resting = score(&a, inbound(2, 2_000.0, 30.0, 0.0));
    let slowest_closer = score(&a, inbound(3, 9_900.0, 30.0, 6.0));
    for s in [&opening, &resting] {
        assert!(s.time_to_impact_s.is_none());
        let k = s.kinematics.expect("in range, so scored");
        assert!(k.urgency.abs() < f64::EPSILON);
        assert!(s.score < slowest_closer.score);
    }
}

/// A closing speed the filter cannot tell from noise is not promoted as inbound: the same
/// slow closer scores less the less certain its velocity is, and never more than a
/// confident one.
#[test]
fn an_uncertain_closing_speed_earns_less_than_a_certain_one() {
    let a = assessor(0.0);
    let mut previous = f32::INFINITY;
    for sigma in [0.1, 1.0, 3.0, 10.0] {
        let mut t = inbound(1, 3_000.0, 60.0, 4.0);
        for i in 3..6 {
            t.covariance[(i, i)] = sigma * sigma;
        }
        let s = score(&a, t);
        assert!(s.time_to_impact_s.is_some(), "it is still closing");
        assert!(
            s.score <= previous,
            "sigma {sigma}: {} after {previous}",
            s.score
        );
        previous = s.score;
    }
}

/// A state that is not a number scores zero with no exposure, rather than a NaN that
/// would sort to the top of a triage and into the reward matrix.
#[test]
fn a_non_finite_track_is_not_scored() {
    let a = assessor(0.0);
    let mut t = inbound(1, 2_000.0, 0.0, 50.0);
    t.state[3] = f64::NAN;
    let s = score(&a, t);
    assert!(s.score.abs() < f32::EPSILON);
    assert!(s.exposure.is_none() && s.kinematics.is_none());
}
