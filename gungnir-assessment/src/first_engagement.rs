// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! An approach corridor's first-engagement range over a rehearsal (GAP-020).
//!
//! Design: docs/design/DN-02-prediction-and-approach.md §9 (amendment 1), which takes
//! D-45's rule and D-107's and D-108's details. DN-02 §7 says PN-16's approach corridors
//! are "the aggregate of predictions over a rehearsal"; this module is that aggregate and
//! nothing else. It is a pure function over what a run recorded, so the rule can be
//! tested without a run and a run's record can be re-read against the approaches a
//! deployment declares today.
//!
//! # The rule
//!
//! - **A prediction** is one recorded target's *first engagement* in the run: the first
//!   pairing the planner proposed for a track that is that target whose intercept point
//!   could be predicted -- DN-04 §9's earliest constant-velocity intercept, which places
//!   the track where the pipeline's own constant-velocity model says it will be when the
//!   effector reaches it (D-107). Which tracks are a target's is the rehearsal's to say,
//!   from the recording's truth; a track of clutter is no target and no prediction.
//! - **It belongs to the approach whose corridor the target was in** when that pairing
//!   was proposed -- where the recording says the target was, not where its track said
//!   it was, because which approach a target came down is a fact of the recording and an
//!   early track can be kilometres out: within the approach's declared corridor
//!   half-width of its axis, on the ground, nearest axis first. An approach that declares
//!   no corridor width takes no prediction, because nothing could say which targets came
//!   down it (D-108).
//! - **Its range is the ground distance from the predicted intercept point to the
//!   approach's inner end** -- the last point the approach declares, which is where it
//!   leads (D-107).
//! - **The approach's first-engagement range is the worst case: the minimum over the
//!   run**, labelled as the worst case and carrying the number of predictions behind it
//!   (D-45). Not a mean, a median or a percentile: PN-16 compares laydowns on this
//!   column, and an averaged figure flatters the weaker one.
//! - **No prediction on an approach is not a range of zero.** It is
//!   [`NotComputable`], with the reason, and never a number.

use gungnir_model::{MissionTime, ResourceId, TrackId};

/// Where the planner predicted it would engage a track: DN-04 §9's earliest intercept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictedEngagement {
    /// The predicted intercept point, in the run's local ENU frame, metres.
    pub intercept_enu: [f64; 3],
    /// Seconds from the proposal to the intercept.
    pub time_to_intercept_s: f64,
}

/// One recorded target's first pairing in a run, as the planner proposed it.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstPairing {
    /// The recorded target, by the recording's own identifier for it.
    pub target: String,
    /// The track of it the planner paired.
    pub track: TrackId,
    pub resource: ResourceId,
    /// The run's mission time at which the planner proposed it.
    pub proposed_at: MissionTime,
    /// Where the recording says the target was then, in the same frame as the
    /// intercept point: what places it on an approach (D-108).
    pub target_enu: [f64; 3],
    /// The predicted engagement, or `None` when the pairing carried no intercept point:
    /// the resource has no closing speed, or the track outruns it (DN-04 §9). Such a
    /// pairing is counted and never given a range.
    pub engagement: Option<PredictedEngagement>,
}

/// One declared approach, placed in the run's local ENU frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApproachAxis<'a> {
    pub name: &'a str,
    /// The axis, outer end first and inner end last, as the deployment declares it.
    pub points_enu: &'a [[f64; 3]],
    /// How far either side of the axis, on the ground, a track counts as on this
    /// approach; `None` when the deployment declared none (D-108).
    pub corridor_half_width_m: Option<f64>,
}

/// Why an approach has no first-engagement range from a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotComputable {
    /// The approach declares no corridor width, so no prediction can be said to lie on
    /// it (D-108).
    NoCorridor,
    /// No recorded target in this approach's corridor was engaged with a predicted
    /// intercept point. `paired_without_point` counts those in it that the planner
    /// paired but could predict no intercept for.
    NoEngagement { paired_without_point: usize },
    /// The approach is not an axis: fewer than two points, or a point that is not
    /// finite. Configuration validation refuses both; this is here so the function
    /// survives an unvalidated baseline rather than inventing a range.
    NotAnAxis,
}

impl std::fmt::Display for NotComputable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotComputable::NoCorridor => f.write_str(
                "the approach declares no corridor width, so no target can be placed on it",
            ),
            NotComputable::NoEngagement {
                paired_without_point: 0,
            } => f.write_str("no recorded target on this approach was engaged in the run"),
            NotComputable::NoEngagement {
                paired_without_point,
            } => write!(
                f,
                "no recorded target on this approach was engaged with a predicted intercept point; \
                 {paired_without_point} were paired with none (no closing speed, or outrun)"
            ),
            NotComputable::NotAnAxis => f.write_str("the approach is not a finite axis"),
        }
    }
}

/// One approach's first-engagement range over a run, or why it has none.
#[derive(Debug, Clone, PartialEq)]
pub enum ApproachFirstEngagement {
    /// D-45: the minimum over the run, with the number of predictions behind it.
    WorstCase {
        /// Ground distance from the worst prediction's intercept point to the
        /// approach's inner end, metres.
        range_m: f64,
        /// How many predictions -- recorded targets first engaged on this approach --
        /// the minimum is over. Never zero: no prediction is [`NotComputable`].
        predictions: usize,
        /// The recorded target the worst case was predicted for, its track, the
        /// effector, and when in the run.
        target: String,
        track: TrackId,
        resource: ResourceId,
        proposed_at: MissionTime,
    },
    NotComputable(NotComputable),
}

impl ApproachFirstEngagement {
    /// The worst-case range, or `None` when there is none.
    #[must_use]
    pub fn range_m(&self) -> Option<f64> {
        match self {
            ApproachFirstEngagement::WorstCase { range_m, .. } => Some(*range_m),
            ApproachFirstEngagement::NotComputable(_) => None,
        }
    }
}

/// Every declared approach's first-engagement range over one run.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstEngagementSummary {
    /// One entry per approach, in the order the approaches were given.
    pub approaches: Vec<ApproachFirstEngagement>,
    /// Paired targets that were in no declared corridor when first paired. Counted so a
    /// raid down an undeclared axis is seen to be missing from the table rather than
    /// silently dropped.
    pub on_no_corridor: usize,
}

fn ground_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Ground distance from `p` to the segment `a`-`b`.
fn ground_distance_to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length_sq = dx * dx + dy * dy;
    if length_sq <= f64::EPSILON {
        return ground_distance(p, a);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length_sq).clamp(0.0, 1.0);
    ground_distance(p, [a[0] + dx * t, a[1] + dy * t, 0.0])
}

fn is_axis(points: &[[f64; 3]]) -> bool {
    points.len() >= 2 && points.iter().flatten().all(|x| x.is_finite())
}

/// Ground distance from `p` to the axis, or `None` when it is not one.
fn ground_distance_to_axis(p: [f64; 3], points: &[[f64; 3]]) -> Option<f64> {
    if !is_axis(points) {
        return None;
    }
    points
        .windows(2)
        .map(|w| ground_distance_to_segment(p, w[0], w[1]))
        .reduce(f64::min)
}

/// The approach whose corridor `p` is in: nearest axis first, the earlier-declared on
/// a tie. `None` when it is in no declared corridor.
fn corridor_of(p: [f64; 3], approaches: &[ApproachAxis<'_>]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (index, approach) in approaches.iter().enumerate() {
        let Some(half_width) = approach.corridor_half_width_m else {
            continue;
        };
        let Some(distance) = ground_distance_to_axis(p, approach.points_enu) else {
            continue;
        };
        if distance <= half_width && best.is_none_or(|(_, d)| distance < d) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

/// DN-02 §9's aggregate: each approach's worst-case first-engagement range over the
/// first pairings a run recorded, or why it has none.
///
/// `pairings` holds at most one entry per recorded target -- its first pairing with a
/// predicted intercept point, else its first pairing -- and every position in it and in
/// `approaches` is in the same local ENU frame.
#[must_use]
pub fn first_engagement_ranges(
    pairings: &[FirstPairing],
    approaches: &[ApproachAxis<'_>],
) -> FirstEngagementSummary {
    struct Tally {
        worst: Option<(f64, usize)>,
        predictions: usize,
        without_point: usize,
    }
    let mut tallies: Vec<Tally> = approaches
        .iter()
        .map(|_| Tally {
            worst: None,
            predictions: 0,
            without_point: 0,
        })
        .collect();
    let mut on_no_corridor = 0;

    for (index, pairing) in pairings.iter().enumerate() {
        let Some(approach) = corridor_of(pairing.target_enu, approaches) else {
            on_no_corridor += 1;
            continue;
        };
        let tally = &mut tallies[approach];
        let inner_end = approaches[approach].points_enu[approaches[approach].points_enu.len() - 1];
        let range = pairing
            .engagement
            .map(|e| ground_distance(e.intercept_enu, inner_end))
            .filter(|r| r.is_finite());
        match range {
            Some(range_m) => {
                tally.predictions += 1;
                // Strictly less: on a tie the earlier prediction stands, so the answer
                // does not depend on anything but the order the run proposed them in.
                if tally.worst.is_none_or(|(w, _)| range_m < w) {
                    tally.worst = Some((range_m, index));
                }
            }
            None => tally.without_point += 1,
        }
    }

    let approaches = approaches
        .iter()
        .zip(tallies)
        .map(|(approach, tally)| {
            if !is_axis(approach.points_enu) {
                return ApproachFirstEngagement::NotComputable(NotComputable::NotAnAxis);
            }
            if approach.corridor_half_width_m.is_none() {
                return ApproachFirstEngagement::NotComputable(NotComputable::NoCorridor);
            }
            match tally.worst {
                Some((range_m, index)) => {
                    let worst = &pairings[index];
                    ApproachFirstEngagement::WorstCase {
                        range_m,
                        predictions: tally.predictions,
                        target: worst.target.clone(),
                        track: worst.track,
                        resource: worst.resource,
                        proposed_at: worst.proposed_at,
                    }
                }
                None => ApproachFirstEngagement::NotComputable(NotComputable::NoEngagement {
                    paired_without_point: tally.without_point,
                }),
            }
        })
        .collect();
    FirstEngagementSummary {
        approaches,
        on_no_corridor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An approach along the east axis, from 20 km out to the origin.
    const WEST_TO_EAST: [[f64; 3]; 2] = [[20_000.0, 0.0, 300.0], [0.0, 0.0, 300.0]];
    /// Another from the north.
    const NORTH: [[f64; 3]; 2] = [[0.0, 20_000.0, 300.0], [0.0, 0.0, 300.0]];

    fn axis(points: &[[f64; 3]], half_width: Option<f64>) -> ApproachAxis<'_> {
        ApproachAxis {
            name: "axis",
            points_enu: points,
            corridor_half_width_m: half_width,
        }
    }

    fn pairing(
        track: u64,
        at_s: f64,
        target_enu: [f64; 3],
        intercept: Option<[f64; 3]>,
    ) -> FirstPairing {
        FirstPairing {
            target: format!("R-{track:03}"),
            track: TrackId(track),
            resource: ResourceId(1),
            proposed_at: MissionTime(at_s),
            target_enu,
            engagement: intercept.map(|intercept_enu| PredictedEngagement {
                intercept_enu,
                time_to_intercept_s: 30.0,
            }),
        }
    }

    /// D-45: the minimum over the run, not the mean, labelled with how many
    /// predictions it is over and which one it was.
    #[test]
    fn the_range_is_the_worst_case_over_the_run_with_its_count() {
        let approaches = [axis(&WEST_TO_EAST, Some(2_000.0))];
        let pairings = [
            pairing(
                1,
                10.0,
                [19_000.0, 100.0, 300.0],
                Some([12_000.0, 0.0, 300.0]),
            ),
            pairing(
                2,
                20.0,
                [18_000.0, -300.0, 300.0],
                Some([7_500.0, 0.0, 300.0]),
            ),
            pairing(
                3,
                30.0,
                [17_000.0, 0.0, 300.0],
                Some([15_000.0, 0.0, 300.0]),
            ),
        ];
        let summary = first_engagement_ranges(&pairings, &approaches);
        assert_eq!(
            summary.approaches[0],
            ApproachFirstEngagement::WorstCase {
                range_m: 7_500.0,
                predictions: 3,
                target: "R-002".into(),
                track: TrackId(2),
                resource: ResourceId(1),
                proposed_at: MissionTime(20.0),
            }
        );
        assert_eq!(summary.on_no_corridor, 0);
    }

    /// The range is measured on the ground to the approach's inner end, its last point,
    /// whatever altitude either is at.
    #[test]
    fn the_range_is_ground_distance_to_the_inner_end() {
        let approaches = [axis(&WEST_TO_EAST, Some(2_000.0))];
        let pairings = [pairing(
            1,
            0.0,
            [19_000.0, 0.0, 300.0],
            Some([3_000.0, 4_000.0, 5_000.0]),
        )];
        let summary = first_engagement_ranges(&pairings, &approaches);
        let range = summary.approaches[0].range_m().expect("a prediction on it");
        assert!((range - 5_000.0).abs() < 1e-9, "{range}");
    }

    /// No prediction on an approach is a stated reason, never a range of zero.
    #[test]
    fn an_approach_nothing_was_engaged_on_is_not_computable_rather_than_zero() {
        let approaches = [
            axis(&WEST_TO_EAST, Some(2_000.0)),
            axis(&NORTH, Some(2_000.0)),
        ];
        let pairings = [
            pairing(1, 0.0, [19_000.0, 0.0, 300.0], Some([9_000.0, 0.0, 300.0])),
            // In the northern corridor, paired, and no intercept point predicted.
            pairing(2, 0.0, [0.0, 15_000.0, 300.0], None),
        ];
        let summary = first_engagement_ranges(&pairings, &approaches);
        assert!(summary.approaches[0].range_m().is_some());
        assert_eq!(
            summary.approaches[1],
            ApproachFirstEngagement::NotComputable(NotComputable::NoEngagement {
                paired_without_point: 1
            })
        );
        assert_eq!(
            first_engagement_ranges(&[], &approaches).approaches,
            vec![
                ApproachFirstEngagement::NotComputable(NotComputable::NoEngagement {
                    paired_without_point: 0
                });
                2
            ],
            "a run with no pairing at all has no range on any approach"
        );
    }

    /// D-108: without a corridor width nothing can be placed on an approach, so it is
    /// not computable even with a track flying straight down its axis; and the track
    /// is counted as on no corridor rather than dropped.
    #[test]
    fn an_approach_with_no_corridor_width_takes_no_prediction() {
        let approaches = [axis(&WEST_TO_EAST, None)];
        let pairings = [pairing(
            1,
            0.0,
            [10_000.0, 0.0, 300.0],
            Some([5_000.0, 0.0, 300.0]),
        )];
        let summary = first_engagement_ranges(&pairings, &approaches);
        assert_eq!(
            summary.approaches[0],
            ApproachFirstEngagement::NotComputable(NotComputable::NoCorridor)
        );
        assert_eq!(summary.on_no_corridor, 1);
    }

    /// A track outside every corridor is counted, and changes no approach's range.
    #[test]
    fn a_track_off_every_corridor_is_counted_and_moves_no_range() {
        let approaches = [axis(&WEST_TO_EAST, Some(1_000.0))];
        let pairings = [
            pairing(
                1,
                0.0,
                [10_000.0, 500.0, 300.0],
                Some([8_000.0, 0.0, 300.0]),
            ),
            // 5 km off the axis, engaged very close in: not this approach's.
            pairing(
                2,
                0.0,
                [10_000.0, 5_000.0, 300.0],
                Some([100.0, 0.0, 300.0]),
            ),
        ];
        let summary = first_engagement_ranges(&pairings, &approaches);
        let range = summary.approaches[0].range_m().expect("track 1 is on it");
        assert!((range - 8_000.0).abs() < 1e-9);
        assert_eq!(summary.on_no_corridor, 1);
    }

    /// Where two corridors overlap, the nearer axis takes the track.
    #[test]
    fn overlapping_corridors_give_a_track_to_the_nearer_axis() {
        let approaches = [
            axis(&WEST_TO_EAST, Some(5_000.0)),
            axis(&NORTH, Some(5_000.0)),
        ];
        // 1 km from the northern axis, 3 km from the eastern one.
        let pairings = [pairing(
            1,
            0.0,
            [1_000.0, 3_000.0, 300.0],
            Some([0.0, 2_000.0, 300.0]),
        )];
        let summary = first_engagement_ranges(&pairings, &approaches);
        assert!(summary.approaches[0].range_m().is_none());
        assert!((summary.approaches[1].range_m().expect("north") - 2_000.0).abs() < 1e-9);
    }

    /// The corridor is measured to the whole polyline, bends included, and not past
    /// its ends.
    #[test]
    fn the_corridor_follows_a_bent_axis_and_stops_at_its_ends() {
        let bent = [
            [20_000.0, 10_000.0, 0.0],
            [10_000.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let approaches = [axis(&bent, Some(1_000.0))];
        let on_the_first_leg = pairing(1, 0.0, [15_000.0, 5_000.0, 0.0], Some([2_000.0, 0.0, 0.0]));
        let beyond_the_outer_end =
            pairing(2, 0.0, [25_000.0, 15_000.0, 0.0], Some([1_000.0, 0.0, 0.0]));
        let summary =
            first_engagement_ranges(&[on_the_first_leg, beyond_the_outer_end], &approaches);
        let range = summary.approaches[0].range_m().expect("on the first leg");
        assert!((range - 2_000.0).abs() < 1e-9);
        assert_eq!(summary.on_no_corridor, 1);
    }

    /// A malformed approach -- one validation refuses -- is said to be one rather than
    /// given a range or a panic.
    #[test]
    fn an_approach_that_is_not_an_axis_is_said_to_be_one() {
        let one_point = [[0.0, 0.0, 0.0]];
        let not_finite = [[f64::NAN, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let approaches = [
            axis(&one_point, Some(1_000.0)),
            axis(&not_finite, Some(1_000.0)),
        ];
        let pairings = [pairing(1, 0.0, [0.0, 0.0, 0.0], Some([0.0, 0.0, 0.0]))];
        let summary = first_engagement_ranges(&pairings, &approaches);
        for a in &summary.approaches {
            assert_eq!(
                *a,
                ApproachFirstEngagement::NotComputable(NotComputable::NotAnAxis)
            );
        }
    }

    #[test]
    fn a_not_computable_reason_reads_as_a_sentence() {
        assert!(NotComputable::NoCorridor
            .to_string()
            .contains("corridor width"));
        assert!(NotComputable::NoEngagement {
            paired_without_point: 2
        }
        .to_string()
        .contains("2 were paired with none"));
        assert_eq!(
            NotComputable::NoEngagement {
                paired_without_point: 0
            }
            .to_string(),
            "no recorded target on this approach was engaged in the run"
        );
    }
}
