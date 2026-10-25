// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The ML-04 anomaly rows against the committed test-track sets (GAP-079,
//! `docs/ml/data-pipeline.md` §2; `docs/ml/use-cases.md` ML-04).

use gungnir_interop::dataset::anomaly_rows_schema;
use gungnir_ml::anomaly::{extract_anomaly_set, AnomalyDataset, AnomalyRow, SubjectKind};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn sample(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/tracks/samples")
        .join(format!("{id}-sample"))
}

fn set(id: &str) -> AnomalyDataset {
    extract_anomaly_set(&sample(id)).unwrap_or_else(|e| panic!("{id} extracts: {e}"))
}

fn source<'a>(d: &'a AnomalyDataset, sensor: &str) -> Vec<&'a AnomalyRow> {
    d.rows
        .iter()
        .filter(|r| r.subject_kind == SubjectKind::Source && r.subject_id == sensor)
        .collect()
}

fn track<'a>(d: &'a AnomalyDataset, entity: &str) -> Vec<&'a AnomalyRow> {
    d.rows
        .iter()
        .filter(|r| r.subject_kind == SubjectKind::Track && r.subject_id == entity)
        .collect()
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let v: Vec<f32> = values.collect();
    assert!(!v.is_empty(), "nothing to average");
    #[allow(clippy::cast_precision_loss)]
    let n = v.len() as f32;
    v.iter().sum::<f32>() / n
}

/// **The labels are the planted anomalies and nothing else**, in every committed set:
/// TT-05's two (the scenario's own `expected.anomalies` is 2), TT-07's skewed, degraded
/// and lost sensors, TT-08's intermittent transponder, and no label anywhere else. A
/// label on a subject nobody planted an anomaly on would teach a detector to cry wolf,
/// which ML-04's false-alert target exists to prevent.
#[test]
fn every_set_labels_exactly_its_planted_anomalies() {
    let expected: [(&str, &[(&str, &str)]); 11] = [
        ("TT-01", &[]),
        ("TT-02", &[]),
        ("TT-03", &[]),
        ("TT-04", &[]),
        (
            "TT-05",
            &[
                ("TT05-ais-off-loiterer-001", "cooperative-silent"),
                ("TT05-spoofed-coaster-001", "spoofed-position"),
            ],
        ),
        ("TT-06", &[]),
        (
            "TT-07",
            &[
                ("1", "dropout"),
                ("1", "dropout+sensor-lost"),
                ("2", "dropout"),
                ("3", "clock-skew"),
                ("5", "clock-skew"),
                ("9", "clock-skew"),
            ],
        ),
        (
            "TT-08",
            &[("TT08-light-aircraft-001", "intermittent-transponder")],
        ),
        ("TT-09", &[]),
        ("TT-10", &[]),
        ("TT-11", &[]),
    ];
    for (id, planted) in expected {
        let d = set(id);
        let flagged: BTreeSet<(String, String)> = d
            .rows
            .iter()
            .filter(|r| r.label_anomaly())
            .map(|r| (r.subject_id.clone(), r.label_kind.clone()))
            .collect();
        let planted: BTreeSet<(String, String)> = planted
            .iter()
            .map(|(s, k)| ((*s).to_owned(), (*k).to_owned()))
            .collect();
        assert_eq!(flagged, planted, "{id}");
        assert!(
            d.rows_by_label["none"] > d.rows.len() / 2,
            "{id}: an anomaly set is mostly normal, and the counts say so"
        );
    }
}

/// The clock-skew estimate is the one a live node can compute -- apparent latency less
/// the configured mean -- and it finds TT-07's planted 2 s lag on sensors 3, 5 and 9 and
/// nothing like it on the others.
#[test]
fn the_clock_skew_estimate_finds_tt07_s_lagging_sensors() {
    let d = set("TT-07");
    for sensor in ["1", "2", "3", "4", "5", "6", "9"] {
        let skew = mean(source(&d, sensor).iter().filter_map(|r| r.clock_skew_s));
        if ["3", "5", "9"].contains(&sensor) {
            assert!(
                (skew - 2.0).abs() < 0.5,
                "sensor {sensor} lags 2 s and reads {skew}"
            );
        } else {
            assert!(
                skew.abs() < 0.5,
                "sensor {sensor} keeps time and reads {skew}"
            );
        }
    }
}

/// A lost sensor says nothing, and says so: its windows after the loss have no
/// detections, the features that need one are null rather than zero, and the gap since
/// its last receipt grows window by window.
#[test]
fn a_lost_sensor_reports_nothing_and_its_receipt_gap_grows() {
    let d = set("TT-07");
    let lost: Vec<&AnomalyRow> = source(&d, "1")
        .into_iter()
        .filter(|r| r.window_start >= 1200.0)
        .collect();
    assert!(!lost.is_empty());
    let mut previous = 0.0;
    for r in &lost {
        assert!(r.label_kind.contains("sensor-lost"), "{r:?}");
        assert_eq!(r.detections, 0, "{r:?}");
        assert_eq!(r.detections_per_scan, Some(0.0));
        assert_eq!(r.unassociated_fraction, None, "no detection, no fraction");
        assert_eq!(r.residual_rms_m, None);
        assert_eq!(r.clock_skew_s, None);
        let gap = r.receipt_gap_s.expect("a source always has a gap");
        assert!(gap > previous, "the gap grows: {gap} after {previous}");
        assert!(f64::from(gap) >= r.window_end - 1200.0 - 5.0, "{r:?}");
        previous = gap;
    }
}

/// TT-05's silent loiterer is seen by radar and never reported cooperatively, while the
/// AIS vessels the same coverage holds are -- the contradiction ML-04 is for. Its spoofed
/// coaster stays on the northern lane outside every sensor's coverage in this sample, so
/// its rows carry the planted label and no detection at all: a label is the planted
/// truth, and the dataset does not pretend the anomaly was seen.
#[test]
fn tt05_s_silent_loiterer_is_seen_and_unreported_and_its_spoofer_is_never_seen() {
    let d = set("TT-05");
    let loiterer = track(&d, "TT05-ais-off-loiterer-001");
    assert!(loiterer.iter().any(|r| r.detections > 0), "radar sees it");
    assert!(loiterer
        .iter()
        .all(|r| r.cooperative_present == Some(false) && r.cooperative_gap_s.is_none()));

    let reporting = track(&d, "TT05-shipping-s-001");
    assert!(reporting
        .iter()
        .any(|r| r.cooperative_present == Some(true)));
    let residual = mean(reporting.iter().filter_map(|r| r.cooperative_residual_m));
    assert!(
        residual < 100.0,
        "an honest AIS report is close: {residual} m"
    );

    let spoofer = track(&d, "TT05-spoofed-coaster-001");
    assert!(!spoofer.is_empty());
    assert!(spoofer
        .iter()
        .all(|r| r.label_kind == "spoofed-position" && r.detections == 0));
}

/// A feature that does not apply to a row's kind is null, never zero: a source has no
/// speed, a track no detection rate against an update period.
#[test]
fn a_feature_that_does_not_apply_is_null() {
    for id in ["TT-05", "TT-07", "TT-08"] {
        for r in &set(id).rows {
            let source_only = [
                r.detections_per_scan,
                r.clock_skew_s.filter(|_| r.detections > 0),
                r.receipt_gap_s,
            ];
            let track_only = [r.speed_mps, r.cooperative_residual_m, r.cooperative_gap_s];
            match r.subject_kind {
                SubjectKind::Source => {
                    assert!(r.detections_per_scan.is_some() && r.receipt_gap_s.is_some());
                    assert!(track_only.iter().all(Option::is_none), "{id}: {r:?}");
                    assert_eq!(r.cooperative_present, None, "{id}: {r:?}");
                }
                SubjectKind::Track => {
                    assert!(r.speed_mps.is_some() && r.cooperative_present.is_some());
                    assert!(
                        source_only.iter().all(Option::is_none)
                            && r.unassociated_fraction.is_none(),
                        "{id}: {r:?}"
                    );
                }
            }
        }
    }
}

/// The rows reach the catalogue's Arrow form, the split is the scenario's, and the same
/// set through the same extractor gives the same hash.
#[test]
fn the_rows_are_in_the_catalogue_form_with_a_stable_hash() {
    let d = set("TT-07");
    assert_eq!(d.split, "validation");
    assert_eq!(d.provenance.scenario, "TT-07");
    assert!((d.window_s - 20.0).abs() < 1e-9, "ten two-second ticks");

    let batch = d.to_record_batch().expect("arrow");
    assert_eq!(batch.num_rows(), d.rows.len());
    assert_eq!(batch.schema().as_ref(), &anomaly_rows_schema());

    let again = set("TT-07");
    assert_eq!(d.content_hash(), again.content_hash());
    assert_eq!(d.content_hash().len(), 64);
    assert_ne!(d.content_hash(), set("TT-05").content_hash());
    assert_eq!(set("TT-05").split, "test");
}
