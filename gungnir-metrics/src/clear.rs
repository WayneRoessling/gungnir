//! CLEAR MOT accounting: the verification-capability-table.md §1 row
//! "MOTA/MOTP, purity/fragmentation, track-to-truth assignment".
//!
//! Oracle `py-motmetrics` 1.4.0. Criterion: **metric values within 1e-3**.
//!
//! # The matching, and why it is not just an assignment
//!
//! Almost all the difficulty in these metrics is in deciding which hypothesis
//! corresponds to which truth object on each frame, because an identity *switch* is
//! only meaningful relative to what was matched before. `motmetrics`'
//! `MOTAccumulator.update` does it in three ordered stages, and this reproduces that
//! order exactly, because a different order changes the switch count and therefore
//! MOTA:
//!
//! 1. **Carry forward.** Any truth object already matched to a hypothesis keeps it, if
//!    that hypothesis is present this frame and still within the gate. This is what
//!    stops a tracker being charged a switch merely because two hypotheses are
//!    equidistant.
//! 2. **Assign the rest** with a minimum-cost assignment over what is left. A pair
//!    that comes out of this stage for a truth object that was previously matched to a
//!    *different* hypothesis is an identity switch.
//! 3. **Account for the remainder**: unmatched truth is a miss, unmatched hypothesis
//!    is a false positive.
//!
//! Stage 2 uses [`gungnir_association::solve_assignment`], which is the same
//! Jonker-Volgenant optimum `scipy.optimize.linear_sum_assignment` computes and is
//! itself gated against scipy. Pairs beyond the gate are made unassignable using
//! `motmetrics`' own large-constant substitution -- reproduced in [`add_expensive_edges`]
//! with its derivation -- so that the assignment chosen is the same one.
//!
//! # Division by zero
//!
//! `motmetrics` divides through `quiet_divide`, which yields NaN for `0 / 0` rather
//! than raising. An empty sequence therefore has a NaN MOTA, not a MOTA of 1.0 or 0.0.
//! This reproduces that, because a metric that silently reported perfect accuracy for
//! a sequence containing nothing would be worse than one that reports "undefined".
//! The differential test compares NaN to NaN as agreement.

use crate::{MetricsConfig, MetricsError, TrackingMetrics};
use gungnir_association::solve_assignment;
use gungnir_track::{Track, TrackId};
use nalgebra::DMatrix;
use std::collections::HashMap;

/// What happened to one truth object or one hypothesis on one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Match,
    Switch,
    Miss,
    FalsePositive,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    frame: usize,
    kind: EventKind,
    object: Option<u64>,
    hypothesis: Option<u64>,
    distance: f64,
}

/// Replace unassignable pairs with a constant large enough that the optimum never
/// prefers one, reproducing `motmetrics.lap.add_expensive_edges`.
///
/// The constant is not arbitrary. With every valid cost inside `[-c, c]`, choosing one
/// invalid edge plus the `r - 1` best possible edges must be worse than choosing the
/// `r` worst possible valid ones: `l + (r - 1)(-c) > r c`, so `l > (2r - 1) c`. Taking
/// `l = 2 r c + 1` satisfies it with room to spare. Any assignment that still lands on
/// such an edge is discarded afterwards, which is how "no match was possible" is
/// expressed.
fn add_expensive_edges(costs: &[Vec<Option<f64>>], rows: usize, cols: usize) -> DMatrix<f64> {
    let mut max_valid: Option<f64> = None;
    let mut any_valid = false;
    for row in costs.iter().take(rows) {
        for entry in row.iter().take(cols).flatten() {
            any_valid = true;
            max_valid = Some(max_valid.map_or(entry.abs(), |m: f64| m.max(entry.abs())));
        }
    }
    if !any_valid {
        // Nothing can be matched; the assignment is arbitrary and every pair is
        // discarded, so a zero matrix is as good as any and avoids a spurious scale.
        return DMatrix::zeros(rows, cols);
    }
    let c = max_valid.unwrap_or(0.0) + 1.0;
    #[allow(clippy::cast_precision_loss)]
    let r = rows.min(cols) as f64;
    let large = 2.0 * r * c + 1.0;
    DMatrix::from_fn(rows, cols, |i, j| costs[i][j].unwrap_or(large))
}

/// Accumulates CLEAR MOT events frame by frame.
struct Accumulator {
    /// Truth object to the hypothesis it is currently matched with.
    matched: HashMap<u64, u64>,
    events: Vec<Event>,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            matched: HashMap::new(),
            events: Vec::new(),
        }
    }

    fn update(&mut self, frame: usize, truth: &[Track], hypotheses: &[Track], max_distance_m: f64) {
        let (no, nh) = (truth.len(), hypotheses.len());
        let mut object_masked = vec![false; no];
        let mut hypothesis_masked = vec![false; nh];

        // Gated distance matrix: `None` means the pair is beyond the gate and cannot
        // be matched at all, which is `motmetrics`' NaN.
        let mut dists: Vec<Vec<Option<f64>>> = vec![vec![None; nh]; no];
        for (i, t) in truth.iter().enumerate() {
            for (j, h) in hypotheses.iter().enumerate() {
                let d = position_distance(t, h);
                dists[i][j] = (d.is_finite() && d <= max_distance_m).then_some(d);
            }
        }

        if no > 0 && nh > 0 {
            // Stage 1: carry forward existing correspondences.
            for (i, t) in truth.iter().enumerate() {
                let Some(previous) = self.matched.get(&t.id.0).copied() else {
                    continue;
                };
                let Some(j) = hypotheses.iter().position(|h| h.id.0 == previous) else {
                    continue;
                };
                if hypothesis_masked[j] {
                    continue;
                }
                let Some(d) = dists[i][j] else { continue };
                object_masked[i] = true;
                hypothesis_masked[j] = true;
                self.matched.insert(t.id.0, hypotheses[j].id.0);
                self.events.push(Event {
                    frame,
                    kind: EventKind::Match,
                    object: Some(t.id.0),
                    hypothesis: Some(hypotheses[j].id.0),
                    distance: d,
                });
            }

            // Stage 2: assign what is left. Masked rows and columns are made
            // unassignable rather than removed, so the large constant is derived from
            // the same matrix `motmetrics` derives it from.
            for i in 0..no {
                for j in 0..nh {
                    if object_masked[i] || hypothesis_masked[j] {
                        dists[i][j] = None;
                    }
                }
            }
            let cost = add_expensive_edges(&dists, no, nh);
            // The matrix is finite by construction, so the solver cannot reject it.
            if let Ok(assignment) = solve_assignment(&cost) {
                for (i, col) in assignment.row_to_col.iter().enumerate() {
                    let Some(j) = *col else { continue };
                    let Some(d) = dists[i][j] else { continue };
                    let o = truth[i].id.0;
                    let h = hypotheses[j].id.0;
                    let is_switch = self.matched.get(&o).is_some_and(|prev| *prev != h);
                    self.events.push(Event {
                        frame,
                        kind: if is_switch {
                            EventKind::Switch
                        } else {
                            EventKind::Match
                        },
                        object: Some(o),
                        hypothesis: Some(h),
                        distance: d,
                    });
                    object_masked[i] = true;
                    hypothesis_masked[j] = true;
                    self.matched.insert(o, h);
                }
            }
        }

        // Stage 3: everything unaccounted for.
        for (i, t) in truth.iter().enumerate() {
            if !object_masked[i] {
                self.events.push(Event {
                    frame,
                    kind: EventKind::Miss,
                    object: Some(t.id.0),
                    hypothesis: None,
                    distance: f64::NAN,
                });
            }
        }
        for (j, h) in hypotheses.iter().enumerate() {
            if !hypothesis_masked[j] {
                self.events.push(Event {
                    frame,
                    kind: EventKind::FalsePositive,
                    object: None,
                    hypothesis: Some(h.id.0),
                    distance: f64::NAN,
                });
            }
        }
    }

    /// `motmetrics`' `quiet_divide`: `0 / 0` is NaN rather than an error or a zero.
    fn quiet_divide(numerator: f64, denominator: f64) -> f64 {
        if denominator == 0.0 {
            f64::NAN
        } else {
            numerator / denominator
        }
    }

    /// `mota` and `motp` differ by one letter and clippy says so. They are the
    /// published names of the two CLEAR MOT metrics, used verbatim by every paper and
    /// by the oracle; renaming them here to satisfy the lint would make this code
    /// harder to check against the literature, which is the whole point of it.
    #[allow(clippy::similar_names)]
    fn finish(&self) -> TrackingMetrics {
        let count = |k: EventKind| self.events.iter().filter(|e| e.kind == k).count() as u64;
        let num_matches = count(EventKind::Match);
        let num_switches = count(EventKind::Switch);
        let num_misses = count(EventKind::Miss);
        let num_false_positives = count(EventKind::FalsePositive);
        // Every truth object present on a frame produces exactly one of match, switch
        // or miss, which is what makes this the ground-truth count.
        let num_objects = num_matches + num_switches + num_misses;
        let num_detections = num_matches + num_switches;

        let distance_sum: f64 = self
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Match | EventKind::Switch))
            .map(|e| e.distance)
            .sum();

        #[allow(clippy::cast_precision_loss)]
        let mota = 1.0
            - Self::quiet_divide(
                (num_misses + num_switches + num_false_positives) as f64,
                num_objects as f64,
            );
        #[allow(clippy::cast_precision_loss)]
        let motp = Self::quiet_divide(distance_sum, num_detections as f64);

        TrackingMetrics {
            mota,
            motp,
            purity: self.purity(),
            fragmentation: self.fragmentation(),
            num_objects,
            num_matches,
            num_switches,
            num_misses,
            num_false_positives,
        }
    }

    /// Detection-weighted mean track purity: of everything a hypothesis was matched
    /// to, what fraction went to the truth object it was matched to most.
    ///
    /// `motmetrics` publishes no `purity` metric, so unlike the other three this is
    /// not a comparison against a number the oracle computes. It is computed from the
    /// oracle's own matching events in the fixture and from ours here, which makes it
    /// a check on the aggregation given an agreed matching -- and the matching is
    /// exactly what MOTA, MOTP and fragmentation already pin. The fixture and the
    /// differential test both say so rather than implying a motmetrics metric exists.
    fn purity(&self) -> f64 {
        let mut per_hypothesis: HashMap<u64, HashMap<u64, u64>> = HashMap::new();
        for event in &self.events {
            if !matches!(event.kind, EventKind::Match | EventKind::Switch) {
                continue;
            }
            let (Some(h), Some(o)) = (event.hypothesis, event.object) else {
                continue;
            };
            *per_hypothesis.entry(h).or_default().entry(o).or_default() += 1;
        }

        let mut dominant = 0_u64;
        let mut total = 0_u64;
        for counts in per_hypothesis.values() {
            let sum: u64 = counts.values().sum();
            let best = counts.values().copied().max().unwrap_or(0);
            dominant += best;
            total += sum;
        }
        #[allow(clippy::cast_precision_loss)]
        Self::quiet_divide(dominant as f64, total as f64)
    }

    /// Number of times a truth object went from tracked to not tracked, counted only
    /// within the span between its first and last tracked frame.
    ///
    /// Leading and trailing misses are excluded deliberately: a track that is never
    /// picked up, or that ends, has not been *fragmented*. This is `motmetrics`'
    /// `num_fragmentations`, which takes the span from the first non-miss to the last
    /// and counts transitions into miss inside it.
    #[allow(clippy::cast_precision_loss)]
    fn fragmentation(&self) -> f64 {
        let mut per_object: HashMap<u64, Vec<(usize, bool)>> = HashMap::new();
        for event in &self.events {
            let Some(o) = event.object else { continue };
            let is_miss = event.kind == EventKind::Miss;
            per_object
                .entry(o)
                .or_default()
                .push((event.frame, is_miss));
        }

        let mut fragments = 0_u64;
        for series in per_object.values_mut() {
            series.sort_by_key(|(frame, _)| *frame);
            let Some(first) = series.iter().position(|(_, miss)| !*miss) else {
                continue; // never tracked at all
            };
            let Some(last) = series.iter().rposition(|(_, miss)| !*miss) else {
                continue;
            };
            let mut previous_was_miss = false;
            for (index, (_, is_miss)) in series.iter().enumerate() {
                if index < first || index > last {
                    continue;
                }
                if *is_miss && !previous_was_miss {
                    fragments += 1;
                }
                previous_was_miss = *is_miss;
            }
        }
        fragments as f64
    }
}

/// Euclidean distance between the position blocks of two tracks.
fn position_distance(a: &Track, b: &Track) -> f64 {
    let d = a.state.fixed_rows::<3>(0) - b.state.fixed_rows::<3>(0);
    d.norm()
}

/// Compute the CLEAR MOT metrics for a whole sequence.
///
/// # Errors
/// [`MetricsError::FrameCountMismatch`] if the two sequences are not the same length:
/// a metric computed by silently truncating one of them would be measuring a different
/// sequence than the caller asked about.
pub fn compute(
    tracker_output: &[Vec<Track>],
    ground_truth: &[Vec<Track>],
    config: MetricsConfig,
) -> Result<TrackingMetrics, MetricsError> {
    if tracker_output.len() != ground_truth.len() {
        return Err(MetricsError::FrameCountMismatch {
            tracker: tracker_output.len(),
            truth: ground_truth.len(),
        });
    }
    let mut accumulator = Accumulator::new();
    for (frame, (hyps, truth)) in tracker_output.iter().zip(ground_truth.iter()).enumerate() {
        accumulator.update(frame, truth, hyps, config.max_distance_m);
    }
    Ok(accumulator.finish())
}

/// A truth or hypothesis object at a position, for callers assembling frames.
#[must_use]
pub fn point_track(id: u64, position: [f64; 3]) -> Track {
    let mut state = nalgebra::SVector::<f64, 6>::zeros();
    state[0] = position[0];
    state[1] = position[1];
    state[2] = position[2];
    Track {
        id: TrackId(id),
        status: gungnir_track::TrackStatus::Confirmed,
        state,
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
        misses_since_update: 0,
        hits: 0,
    }
}
