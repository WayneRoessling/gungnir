// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The property gate for the `docs/verification-capability-table.md` §1 row
//! "`rfs` | GLMB / LMB filter": the **label continuity** half of its criterion.
//!
//! `lmb_diff.rs` gates this filter against its oracle. Agreeing with an oracle is not by
//! itself a reason for this row to exist, because the PHD and CPHD rows already agree
//! with theirs and are cheaper. What earns the row is the one thing those filters
//! explicitly cannot do: say that the track in front of you now is the *same target*
//! that was there last scan. These tests are about that and nothing else.
//!
//! Each of them is written against `extract_tracks`, not against the internal Bernoulli
//! list, because a consumer's exposure to this property is exactly the [`TrackId`] it
//! gets back. `PhdFilter::extract_tracks` and `CphdFilter::extract_tracks` both mint
//! fresh identifiers every call and document that they do;
//! [`the_phd_filters_identifiers_carry_no_identity_on_the_same_scenario`] drives the PHD
//! over the *same* detections and shows the difference is real rather than asserted, so
//! this row's separate existence rests on a measurement.

use gungnir_rfs::{GaussianComponent, LmbBirth, LmbFilter, LmbSettings, PhdFilter, PhdSettings};
use gungnir_track::{ConstantVelocity, Track, TrackId};
use nalgebra::{SMatrix, SVector};

fn position_h() -> SMatrix<f64, 3, 6> {
    let mut h = SMatrix::<f64, 3, 6>::zeros();
    for axis in 0..3 {
        h[(axis, axis)] = 1.0;
    }
    h
}

fn measurement_cov() -> SMatrix<f64, 3, 3> {
    SMatrix::<f64, 3, 3>::identity() * 25.0
}

fn birth_cov() -> SMatrix<f64, 6, 6> {
    SMatrix::<f64, 6, 6>::from_diagonal(&SVector::<f64, 6>::from_column_slice(&[
        100.0, 100.0, 100.0, 400.0, 400.0, 400.0,
    ]))
}

fn lmb() -> LmbFilter {
    LmbFilter::new(LmbSettings::default(), position_h(), measurement_cov()).expect("valid settings")
}

fn birth(position: [f64; 3], velocity: [f64; 3]) -> LmbBirth {
    let mut mean = SVector::<f64, 6>::zeros();
    for axis in 0..3 {
        mean[axis] = position[axis];
        mean[3 + axis] = velocity[axis];
    }
    LmbBirth {
        existence: 0.4,
        mean,
        cov: birth_cov(),
    }
}

fn detection(position: [f64; 3]) -> SVector<f64, 3> {
    SVector::<f64, 3>::from_column_slice(&position)
}

/// The same `SplitMix64` the crate's own trial-based test uses, restated here because it
/// is private to the crate: a deterministic stream so a failure is reproducible rather
/// than a coin toss.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[-1, 1)`.
    fn next_signed(&mut self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let unit = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        unit * 2.0 - 1.0
    }
}

/// Which truth target each extracted track is nearest to, in position.
///
/// This is the mapping the whole file is about: a filter that carries identity keeps it
/// constant across scans, and one that does not lets it permute. Ties are impossible in
/// these scenarios because the targets are hundreds of metres apart.
fn nearest_truth(tracks: &[Track], truth: &[[f64; 3]]) -> Vec<(TrackId, usize)> {
    let mut out: Vec<(TrackId, usize)> = tracks
        .iter()
        .map(|track| {
            let (index, _) = truth
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let d = (track.state[0] - p[0]).powi(2)
                        + (track.state[1] - p[1]).powi(2)
                        + (track.state[2] - p[2]).powi(2);
                    (i, d)
                })
                .fold((0_usize, f64::INFINITY), |(bi, bd), (i, d)| {
                    if d < bd {
                        (i, d)
                    } else {
                        (bi, bd)
                    }
                });
            (track.id, index)
        })
        .collect();
    out.sort_by_key(|(id, _)| id.0);
    out
}

/// The headline property, under conditions that would break a filter relying on
/// ordering: three targets, measurement noise on every return, and one target dropping
/// out on every third scan so the strengths of the three keep reordering.
///
/// The assertion is not "the labels are stable" in the weak sense of the same *set*
/// coming back -- it is that the map from label to *which physical target* is constant
/// for the whole run. A filter that swapped two labels would keep the set and fail this.
#[test]
fn a_tracked_target_keeps_the_same_label_for_the_whole_run() {
    let mut filter = lmb();
    let motion = ConstantVelocity { sigma_a_sq: 1.0 };
    let truth = [[0.0, 0.0, 100.0], [250.0, 0.0, 100.0], [0.0, 320.0, 100.0]];
    let mut rng = SplitMix64(0x10BC_0117_0001);
    let mut established: Option<Vec<(TrackId, usize)>> = None;

    for scan in 0..40 {
        let births = if scan == 0 {
            truth
                .iter()
                .map(|p| birth(*p, [0.0, 0.0, 0.0]))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        filter.predict(&motion, 1.0, &births).expect("finite");

        let detections: Vec<_> = truth
            .iter()
            .enumerate()
            // Target 1 is unreported on every third scan.
            .filter(|(i, _)| !(*i == 1 && scan % 3 == 2))
            .map(|(_, p)| {
                detection([
                    p[0] + rng.next_signed() * 6.0,
                    p[1] + rng.next_signed() * 6.0,
                    p[2] + rng.next_signed() * 6.0,
                ])
            })
            .collect();
        filter.update(&detections).expect("valid");

        let tracks = filter.extract_tracks().expect("valid");
        // Let the three establish before pinning the map; a target born at r = 0.4 is
        // below the 0.5 extraction threshold until it has been seen.
        if scan < 3 {
            continue;
        }
        assert_eq!(
            tracks.len(),
            3,
            "scan {scan}: three targets, {} tracks extracted",
            tracks.len()
        );
        let mapping = nearest_truth(&tracks, &truth);
        match &established {
            None => established = Some(mapping),
            Some(first) => assert_eq!(
                &mapping, first,
                "scan {scan}: the label-to-target map changed. A label that moves to a \
                 different target is worse than no label at all, because a consumer \
                 cannot tell it happened"
            ),
        }
    }
    assert!(
        established.is_some(),
        "the run never established three tracks"
    );
}

/// The other half of the promise: a target that genuinely appears must get an identifier
/// that has never been used, and the targets already tracked must not have theirs
/// disturbed by its arrival.
#[test]
fn a_new_target_gets_a_new_label_and_the_existing_ones_keep_theirs() {
    let mut filter = lmb();
    let motion = ConstantVelocity { sigma_a_sq: 1.0 };
    let original = [[0.0, 0.0, 100.0], [250.0, 0.0, 100.0]];
    let newcomer = [0.0, 400.0, 100.0];
    let born_at = 8;

    let mut before: Option<Vec<TrackId>> = None;
    let mut every_label_ever: Vec<TrackId> = Vec::new();

    for scan in 0..20 {
        let births = if scan == 0 {
            original
                .iter()
                .map(|p| birth(*p, [0.0, 0.0, 0.0]))
                .collect::<Vec<_>>()
        } else if scan == born_at {
            vec![birth(newcomer, [0.0, 0.0, 0.0])]
        } else {
            Vec::new()
        };
        filter.predict(&motion, 1.0, &births).expect("finite");

        let mut detections: Vec<_> = original.iter().map(|p| detection(*p)).collect();
        if scan >= born_at {
            detections.push(detection(newcomer));
        }
        filter.update(&detections).expect("valid");

        let tracks = filter.extract_tracks().expect("valid");
        let ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        for id in &ids {
            if !every_label_ever.contains(id) {
                every_label_ever.push(*id);
            }
        }
        if scan == born_at - 1 {
            before = Some(ids.clone());
            assert_eq!(ids.len(), 2, "two targets before the third arrives");
        }
        if scan > born_at {
            assert_eq!(ids.len(), 3, "scan {scan}: the newcomer was not tracked");
            let previous = before.as_ref().expect("captured above");
            for id in previous {
                assert!(
                    ids.contains(id),
                    "scan {scan}: label {id:?} was lost when a new target arrived"
                );
            }
            let fresh: Vec<&TrackId> = ids.iter().filter(|id| !previous.contains(id)).collect();
            assert_eq!(
                fresh.len(),
                1,
                "scan {scan}: exactly one new label should have appeared, got {fresh:?}"
            );
        }
    }
    assert_eq!(
        every_label_ever.len(),
        3,
        "three births over the run should have issued exactly three distinct labels, \
         issued {every_label_ever:?}"
    );
}

/// Identity through the hardest moment this scenario has: two targets at **the same
/// point at the same scan**, where the measurement model -- position only -- genuinely
/// cannot tell the two returns apart.
///
/// Two things must hold, and the second is the one that matters. The filter must not
/// invent certainty at the tie: its association marginals go to an even split, which the
/// crate's own unit test pins. And once the targets separate again, each label must be
/// back on the target it started on -- which it can be only because the state carries
/// velocity through the moment when position could not distinguish them.
#[test]
fn labels_survive_two_targets_passing_through_the_same_point() {
    let mut filter = lmb();
    let motion = ConstantVelocity { sigma_a_sq: 1.0 };
    let speed = 20.0;
    filter
        .predict(
            &motion,
            1.0,
            &[
                birth([-160.0, 0.0, 100.0], [speed, 0.0, 0.0]),
                birth([160.0, 0.0, 100.0], [-speed, 0.0, 0.0]),
            ],
        )
        .expect("finite");

    let mut eastbound: Option<TrackId> = None;
    let mut westbound: Option<TrackId> = None;
    let mut saw_the_tie = false;

    for scan in 0..17 {
        if scan > 0 {
            filter.predict(&motion, 1.0, &[]).expect("finite");
        }
        let t = f64::from(scan);
        let a = [-160.0 + speed * t, 0.0, 100.0];
        let b = [160.0 - speed * t, 0.0, 100.0];
        filter.update(&[detection(a), detection(b)]).expect("valid");

        if filter
            .last_association()
            .iter()
            .any(|(_, row)| row.len() >= 4 && (row[2] - row[3]).abs() < 1e-9 && row[2] > 0.4)
        {
            saw_the_tie = true;
            assert_eq!(scan, 8, "the tie should be exactly at the coincident scan");
        }

        let tracks = filter.extract_tracks().expect("valid");
        if scan < 2 {
            continue;
        }
        assert_eq!(
            tracks.len(),
            2,
            "scan {scan}: two targets, {} tracks",
            tracks.len()
        );
        // Which track is on which target, by velocity -- position cannot say at the tie,
        // and asking by position either side of it is what would let a swap through.
        let mut by_velocity: Vec<(TrackId, f64)> =
            tracks.iter().map(|t| (t.id, t.state[3])).collect();
        by_velocity.sort_by(|x, y| x.1.total_cmp(&y.1));
        let (west, east) = (by_velocity[0].0, by_velocity[1].0);
        match (eastbound, westbound) {
            (None, None) => {
                eastbound = Some(east);
                westbound = Some(west);
            }
            (Some(e), Some(w)) => {
                assert_eq!(
                    east, e,
                    "scan {scan}: the eastbound target's label changed across the pass"
                );
                assert_eq!(
                    west, w,
                    "scan {scan}: the westbound target's label changed across the pass"
                );
            }
            _ => unreachable!("both are set together"),
        }
    }
    assert!(
        saw_the_tie,
        "the two targets never actually became indistinguishable, so this test proved \
         nothing about identity through ambiguity"
    );

    // And they really did swap sides: without this the test would pass on two targets
    // that never went anywhere near each other.
    let tracks = filter.extract_tracks().expect("valid");
    let mut ends: Vec<(TrackId, f64)> = tracks.iter().map(|t| (t.id, t.state[0])).collect();
    ends.sort_by_key(|(id, _)| id.0);
    assert!(
        ends[0].1 > 100.0 && ends[1].1 < -100.0,
        "the targets did not cross: they ended at {ends:?}"
    );
}

/// The contrast that justifies this row existing at all, driven rather than asserted.
///
/// The same detections through `PhdFilter`: its `extract_tracks` mints identifiers in
/// component order, and components are sorted by weight, so when one target's weight
/// dips below another's the identifiers permute across a scan boundary and a consumer
/// holding one is silently handed a different target. That is not a defect in the PHD
/// filter -- a PHD intensity has no identity to carry, and its documentation says so --
/// it is the reason a labelled filter is a separate row.
///
/// This test fails if the PHD's identifiers ever *stop* permuting on this scenario,
/// which would mean the justification recorded here had gone stale and the row's
/// separate existence needed re-arguing.
#[test]
fn the_phd_filters_identifiers_carry_no_identity_on_the_same_scenario() {
    let truth = [[0.0, 0.0, 100.0], [250.0, 0.0, 100.0], [0.0, 320.0, 100.0]];
    let motion = ConstantVelocity { sigma_a_sq: 1.0 };

    // One detection stream, replayed identically through both filters.
    let mut rng = SplitMix64(0xC011_7A57_0002);
    let mut stream: Vec<Vec<SVector<f64, 3>>> = Vec::new();
    for scan in 0..40 {
        stream.push(
            truth
                .iter()
                .enumerate()
                .filter(|(i, _)| !(*i == 1 && scan % 3 == 2))
                .map(|(_, p)| {
                    detection([
                        p[0] + rng.next_signed() * 6.0,
                        p[1] + rng.next_signed() * 6.0,
                        p[2] + rng.next_signed() * 6.0,
                    ])
                })
                .collect(),
        );
    }

    let mut phd =
        PhdFilter::new(PhdSettings::default(), position_h(), measurement_cov()).expect("valid");
    let mut lmb_filter = lmb();
    let mut phd_maps: Vec<Vec<(TrackId, usize)>> = Vec::new();
    let mut lmb_maps: Vec<Vec<(TrackId, usize)>> = Vec::new();

    for (scan, detections) in stream.iter().enumerate() {
        let phd_births: Vec<GaussianComponent> = if scan == 0 {
            truth
                .iter()
                .map(|p| {
                    let mut mean = SVector::<f64, 6>::zeros();
                    for axis in 0..3 {
                        mean[axis] = p[axis];
                    }
                    GaussianComponent {
                        weight: 0.4,
                        mean,
                        cov: birth_cov(),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let lmb_births: Vec<LmbBirth> = if scan == 0 {
            truth.iter().map(|p| birth(*p, [0.0, 0.0, 0.0])).collect()
        } else {
            Vec::new()
        };

        phd.predict(&motion, 1.0, &phd_births).expect("finite");
        phd.update(detections).expect("valid");
        lmb_filter
            .predict(&motion, 1.0, &lmb_births)
            .expect("finite");
        lmb_filter.update(detections).expect("valid");

        if scan < 3 {
            continue;
        }
        let phd_tracks = phd.extract_tracks().expect("valid");
        if phd_tracks.len() == 3 {
            phd_maps.push(nearest_truth(&phd_tracks, &truth));
        }
        let lmb_tracks = lmb_filter.extract_tracks().expect("valid");
        if lmb_tracks.len() == 3 {
            lmb_maps.push(nearest_truth(&lmb_tracks, &truth));
        }
    }

    assert!(
        phd_maps.len() > 20 && lmb_maps.len() > 20,
        "both filters must actually track the scene for the comparison to mean \
         anything: PHD {} scans, LMB {} scans",
        phd_maps.len(),
        lmb_maps.len()
    );

    let phd_permutations = phd_maps.windows(2).filter(|w| w[0] != w[1]).count();
    let lmb_permutations = lmb_maps.windows(2).filter(|w| w[0] != w[1]).count();
    println!(
        "identifier-to-target map changed on {phd_permutations} of {} PHD scan \
         boundaries, and {lmb_permutations} of {} LMB ones",
        phd_maps.len() - 1,
        lmb_maps.len() - 1
    );

    assert!(
        phd_permutations > 0,
        "the PHD filter's identifiers did not permute even once on a scenario built to \
         make them, so the contrast this row rests on is no longer demonstrated here \
         and needs re-arguing rather than quietly assuming"
    );
    assert_eq!(
        lmb_permutations, 0,
        "the labelled filter's identifiers permuted, which is the one thing it exists \
         not to do"
    );
}
