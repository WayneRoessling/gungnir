//! metrics: MOTA/MOTP, purity/fragmentation --
//! verification-capability-table.md `metrics` row. Values within 1e-3 vs. motmetrics.
//! Validated against the dense-swarm scenario, where fragmentation is a meaningful
//! signal rather than a trivial zero.
//!
//! # The scaffold's signature could not express the question
//!
//! `compute_metrics` took two flat `&[Track]` slices. MOTA, MOTP and fragmentation are
//! **sequence** metrics: they count misses, false positives and identity switches
//! *across frames*, and an identity switch is not even definable on a single snapshot.
//! Two flat slices carry no time axis, so the scaffold's signature could not have been
//! implemented as written.
//!
//! It now takes a frame per cycle on each side, keeping the two named parameters, plus
//! a [`MetricsConfig`] carrying the gate distance -- which is a required input to the
//! CLEAR MOT definition and had nowhere to live before. It returns a `Result`, because
//! two sequences of different lengths describe different runs and silently truncating
//! one would report a metric for a sequence the caller never asked about.
//!
//! Truth objects are carried as [`Track`] values, as the scaffold intended: a truth
//! object is an id and a position, which is what a `Track` already is. Only the
//! position block is read.

pub mod clear;

use gungnir_track::Track;

pub use clear::point_track;

/// Inputs to the CLEAR MOT definition that are not the data itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricsConfig {
    /// A hypothesis further than this from a truth object cannot be matched to it.
    ///
    /// There is no default worth having: the right gate is a property of the sensor
    /// and the scenario, and a wrong one silently turns misses into false positives
    /// and back. `motmetrics` requires the caller to supply it too, by handing in a
    /// distance matrix with the out-of-gate entries already removed.
    pub max_distance_m: f64,
}

/// Why the metrics could not be computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MetricsError {
    /// The tracker and truth sequences cover a different number of frames.
    #[error("sequence length mismatch: {tracker} tracker frames against {truth} truth frames")]
    FrameCountMismatch { tracker: usize, truth: usize },
}

/// CLEAR MOT metrics over a whole sequence.
///
/// Any of the ratios is `NaN` when its denominator is zero, which is what
/// `motmetrics`' `quiet_divide` produces: an empty sequence has an undefined accuracy,
/// not a perfect one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrackingMetrics {
    /// Multiple-object tracking accuracy: `1 - (misses + switches + false positives) / objects`.
    pub mota: f64,
    /// Multiple-object tracking precision: mean distance over matched pairs.
    pub motp: f64,
    /// Detection-weighted mean track purity. See [`clear`] for its definition and for
    /// why it is the one field not compared against a metric `motmetrics` publishes.
    pub purity: f64,
    /// Times a truth object went from tracked to not tracked within its tracked span.
    pub fragmentation: f64,
    /// Truth objects summed over frames: matches + switches + misses.
    pub num_objects: u64,
    pub num_matches: u64,
    pub num_switches: u64,
    pub num_misses: u64,
    pub num_false_positives: u64,
}

/// Score a tracker's output against ground truth, frame by frame.
///
/// `tracker_output[i]` and `ground_truth[i]` are the same cycle. Only the position
/// block of each [`Track`] is read; ids carry identity.
///
/// # Errors
/// [`MetricsError::FrameCountMismatch`] if the two sequences differ in length.
pub fn compute_metrics(
    tracker_output: &[Vec<Track>],
    ground_truth: &[Vec<Track>],
    config: MetricsConfig,
) -> Result<TrackingMetrics, MetricsError> {
    clear::compute(tracker_output, ground_truth, config)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn config() -> MetricsConfig {
        MetricsConfig {
            max_distance_m: 1.0,
        }
    }

    /// A tracker that reports the truth exactly scores perfectly, and reports no
    /// switches, misses or false positives.
    #[test]
    fn a_perfect_tracker_scores_one() {
        let frames: Vec<Vec<Track>> = (0..5)
            .map(|k| vec![point_track(1, [f64::from(k), 0.0, 0.0])])
            .collect();
        let m = compute_metrics(&frames, &frames, config()).expect("same length");
        assert_eq!(m.mota, 1.0);
        assert_eq!(m.motp, 0.0);
        assert_eq!(m.purity, 1.0);
        assert_eq!(m.fragmentation, 0.0);
        assert_eq!(m.num_matches, 5);
        assert_eq!(m.num_switches, 0);
        assert_eq!(m.num_misses, 0);
        assert_eq!(m.num_false_positives, 0);
    }

    /// A tracker that reports nothing misses everything: MOTA is zero, and MOTP is
    /// undefined because there is no matched pair to average over.
    #[test]
    fn a_silent_tracker_scores_zero_and_has_no_precision() {
        let truth: Vec<Vec<Track>> = (0..4).map(|_| vec![point_track(1, [0.0; 3])]).collect();
        let empty: Vec<Vec<Track>> = vec![Vec::new(); 4];
        let m = compute_metrics(&empty, &truth, config()).expect("same length");
        assert_eq!(m.mota, 0.0);
        assert!(m.motp.is_nan(), "no matches, so precision is undefined");
        assert_eq!(m.num_misses, 4);
        assert_eq!(m.fragmentation, 0.0, "never tracked is not fragmented");
    }

    /// An empty sequence has an undefined accuracy, not a perfect one.
    #[test]
    fn an_empty_sequence_is_undefined_not_perfect() {
        let m = compute_metrics(&[], &[], config()).expect("same length");
        assert!(m.mota.is_nan());
        assert!(m.motp.is_nan());
        assert_eq!(m.num_objects, 0);
    }

    /// Swapping which hypothesis covers a truth object is an identity switch, and it
    /// costs MOTA even though the object was tracked on every frame.
    #[test]
    fn an_identity_swap_is_charged_as_a_switch() {
        let truth: Vec<Vec<Track>> = (0..4).map(|_| vec![point_track(1, [0.0; 3])]).collect();
        let tracker: Vec<Vec<Track>> = (0..4)
            .map(|k| vec![point_track(if k < 2 { 10 } else { 20 }, [0.0; 3])])
            .collect();
        let m = compute_metrics(&tracker, &truth, config()).expect("same length");
        assert_eq!(m.num_switches, 1, "one change of identity");
        assert_eq!(m.num_matches, 3);
        assert_eq!(m.num_misses, 0);
        assert_eq!(m.mota, 1.0 - 1.0 / 4.0);
    }

    /// A gap in the middle of a tracked span is a fragmentation; a gap at the end is
    /// not, because the track simply ended.
    #[test]
    fn only_interior_gaps_are_fragmentations() {
        let truth: Vec<Vec<Track>> = (0..5).map(|_| vec![point_track(1, [0.0; 3])]).collect();
        // Tracked, gap, tracked: one fragmentation.
        let interior: Vec<Vec<Track>> = (0..5)
            .map(|k| {
                if k == 2 {
                    Vec::new()
                } else {
                    vec![point_track(10, [0.0; 3])]
                }
            })
            .collect();
        let m = compute_metrics(&interior, &truth, config()).expect("same length");
        assert_eq!(m.fragmentation, 1.0);

        // Tracked then dropped for good: no fragmentation.
        let trailing: Vec<Vec<Track>> = (0..5)
            .map(|k| {
                if k >= 3 {
                    Vec::new()
                } else {
                    vec![point_track(10, [0.0; 3])]
                }
            })
            .collect();
        let m = compute_metrics(&trailing, &truth, config()).expect("same length");
        assert_eq!(m.fragmentation, 0.0);
    }

    /// A hypothesis beyond the gate is a false positive and the truth object is a
    /// miss: the gate is what separates "wrong" from "absent".
    #[test]
    fn a_hypothesis_beyond_the_gate_matches_nothing() {
        let truth = vec![vec![point_track(1, [0.0; 3])]];
        let far = vec![vec![point_track(10, [100.0, 0.0, 0.0])]];
        let m = compute_metrics(&far, &truth, config()).expect("same length");
        assert_eq!(m.num_matches, 0);
        assert_eq!(m.num_misses, 1);
        assert_eq!(m.num_false_positives, 1);
        assert_eq!(m.mota, 1.0 - 2.0 / 1.0, "both errors are charged");
    }

    /// MOTP is the mean distance over matched pairs, and is unaffected by misses.
    #[test]
    fn motp_is_the_mean_matched_distance() {
        let truth = vec![
            vec![point_track(1, [0.0; 3])],
            vec![point_track(1, [0.0; 3])],
        ];
        let tracker = vec![
            vec![point_track(10, [0.2, 0.0, 0.0])],
            vec![point_track(10, [0.4, 0.0, 0.0])],
        ];
        let m = compute_metrics(&tracker, &truth, config()).expect("same length");
        assert!((m.motp - 0.3).abs() < 1e-12, "motp was {}", m.motp);
    }

    /// One hypothesis covering two different truth objects over time is impure.
    #[test]
    fn purity_falls_when_one_hypothesis_covers_two_objects() {
        let truth: Vec<Vec<Track>> = (0..4)
            .map(|k| vec![point_track(if k < 3 { 1 } else { 2 }, [0.0; 3])])
            .collect();
        let tracker: Vec<Vec<Track>> = (0..4).map(|_| vec![point_track(10, [0.0; 3])]).collect();
        let m = compute_metrics(&tracker, &truth, config()).expect("same length");
        assert!(
            (m.purity - 0.75).abs() < 1e-12,
            "three of four matches went to the dominant object; purity was {}",
            m.purity
        );
    }

    #[test]
    fn a_length_mismatch_is_an_error() {
        let one = vec![vec![point_track(1, [0.0; 3])]];
        let two = vec![Vec::new(), Vec::new()];
        assert_eq!(
            compute_metrics(&one, &two, config()),
            Err(MetricsError::FrameCountMismatch {
                tracker: 1,
                truth: 2
            })
        );
    }
}
