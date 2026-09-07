//! Differential test for the verification-capability-table.md §1 row
//! "`metrics` | MOTA/MOTP, purity/fragmentation, track-to-truth assignment".
//!
//! Oracle `py-motmetrics` 1.4.0. Criterion: **metric values within 1e-3**.
//!
//! The fixture carries the scenario, not the oracle's matching: every truth object and
//! every hypothesis at every frame. This side runs its own three-stage CLEAR MOT
//! matching over that data and the two answers are compared. Handing over the
//! matching would reduce this to checking arithmetic, when the matching is the part
//! that is actually hard.
//!
//! # Not all four metrics are equally strong evidence
//!
//! Stated plainly, because the fixture's own header does too:
//!
//! * `mota`, `motp` and `fragmentation` are computed by `motmetrics` itself, so those
//!   three are a comparison against the named oracle.
//! * `purity` is **not** a `motmetrics` metric -- it publishes none -- so the fixture
//!   computes it from `motmetrics`' event dataframe by the definition
//!   `gungnir-metrics` documents. That makes the purity comparison a check of the
//!   aggregation given an agreed matching. The matching itself is what the other three
//!   already pin, so it is worth having, but it is not the same claim.
//!
//! MATLAB's track-metric functions are named by the row and were **not** run: MATLAB
//! is not installed; `testdata/oracles/README.md` records that.

use gungnir_metrics::{compute_metrics, point_track, MetricsConfig, TrackingMetrics};
use gungnir_track::Track;

/// The row's tolerance, verbatim.
const TOL: f64 = 1e-3;

#[derive(serde::Deserialize)]
struct Expected {
    /// `None` where the oracle divided zero by zero; the Rust side yields NaN.
    mota: Option<f64>,
    motp: Option<f64>,
    purity: Option<f64>,
    fragmentation: f64,
    num_objects: u64,
    num_matches: u64,
    num_switches: u64,
    num_misses: u64,
    num_false_positives: u64,
}

#[derive(serde::Deserialize)]
struct Frame {
    truth: Vec<Vec<f64>>,
    tracker: Vec<Vec<f64>>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    max_distance_m: f64,
    frames: Vec<Frame>,
    expected: Expected,
}

#[derive(serde::Deserialize)]
struct Fixture {
    row: String,
    oracle: String,
    cases: Vec<Case>,
}

fn load() -> Fixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/oracles/metrics/clear_mot.json"
    );
    let raw = std::fs::read_to_string(path)
        .expect("oracle fixture present; see testdata/oracles/README.md");
    serde_json::from_str(&raw).expect("oracle fixture parses")
}

/// `[id, x, y, z]` from the fixture into a `Track` carrying only position.
fn to_tracks(rows: &[Vec<f64>], case: &str) -> Vec<Track> {
    rows.iter()
        .map(|row| {
            assert_eq!(row.len(), 4, "{case}: expected [id, x, y, z]");
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let id = row[0] as u64;
            point_track(id, [row[1], row[2], row[3]])
        })
        .collect()
}

/// Compare one metric, treating "both undefined" as agreement.
///
/// A NaN on our side against a `null` from the oracle is the two implementations
/// agreeing that the quantity is undefined, which is a real answer for an empty
/// sequence. A NaN against a number, or a number against a null, is a disagreement.
fn agree(got: f64, expected: Option<f64>, metric: &str, case: &str) -> f64 {
    match expected {
        None => {
            assert!(
                got.is_nan(),
                "{case}: {metric} is {got} but the oracle says it is undefined"
            );
            0.0
        }
        Some(want) => {
            assert!(
                !got.is_nan(),
                "{case}: {metric} is undefined but the oracle says {want}"
            );
            let d = (got - want).abs();
            assert!(
                d < TOL,
                "{case}: {metric} is {got}, oracle says {want}, difference {d:e} \
                 exceeds {TOL:e}"
            );
            d
        }
    }
}

fn run(case: &Case) -> TrackingMetrics {
    let tracker: Vec<Vec<Track>> = case
        .frames
        .iter()
        .map(|f| to_tracks(&f.tracker, &case.name))
        .collect();
    let truth: Vec<Vec<Track>> = case
        .frames
        .iter()
        .map(|f| to_tracks(&f.truth, &case.name))
        .collect();
    compute_metrics(
        &tracker,
        &truth,
        MetricsConfig {
            max_distance_m: case.max_distance_m,
        },
    )
    .unwrap_or_else(|e| panic!("{}: {e}", case.name))
}

#[test]
fn fixture_is_the_expected_row() {
    let f = load();
    assert_eq!(
        f.row,
        "MOTA/MOTP, purity/fragmentation, track-to-truth assignment"
    );
    assert_eq!(f.oracle, "py-motmetrics");
    assert!(!f.cases.is_empty());
    assert!(
        f.cases.iter().any(|c| c.expected.num_switches > 0),
        "no case has an identity switch, so the hardest part of the matching is \
         never exercised"
    );
    assert!(
        f.cases.iter().any(|c| c.expected.fragmentation > 0.0),
        "no case fragments, so that metric is only ever compared against zero"
    );
    assert!(
        f.cases.iter().any(|c| c.expected.num_false_positives > 0),
        "no case has clutter"
    );
    assert!(
        f.cases
            .iter()
            .any(|c| c.frames.iter().any(|f| f.truth.len() > 10)),
        "no case runs at a realistic target count"
    );
}

/// The row itself.
#[test]
fn metrics_match_motmetrics() {
    let fixture = load();
    let mut worst = (0.0_f64, String::new(), String::new());
    let mut compared = 0;

    for case in &fixture.cases {
        let got = run(case);
        for (name, value, want) in [
            ("mota", got.mota, case.expected.mota),
            ("motp", got.motp, case.expected.motp),
            ("purity", got.purity, case.expected.purity),
            (
                "fragmentation",
                got.fragmentation,
                Some(case.expected.fragmentation),
            ),
        ] {
            let d = agree(value, want, name, &case.name);
            compared += 1;
            if d > worst.0 {
                worst = (d, name.to_owned(), case.name.clone());
            }
        }
    }

    println!(
        "metrics: {compared} values compared across {} cases; worst difference \
         {:e} on {} in {} (tolerance {TOL:e})",
        fixture.cases.len(),
        worst.0,
        worst.1,
        worst.2
    );
}

/// The counts underneath must agree too. If they did not, a MOTA that happened to
/// match would be an arithmetic coincidence rather than the same accounting: two
/// wrong counts can sum to the right total.
#[test]
fn the_underlying_counts_match_motmetrics() {
    let fixture = load();
    for case in &fixture.cases {
        let got = run(case);
        let e = &case.expected;
        assert_eq!(got.num_objects, e.num_objects, "{}: num_objects", case.name);
        assert_eq!(got.num_matches, e.num_matches, "{}: num_matches", case.name);
        assert_eq!(
            got.num_switches, e.num_switches,
            "{}: identity switches",
            case.name
        );
        assert_eq!(got.num_misses, e.num_misses, "{}: misses", case.name);
        assert_eq!(
            got.num_false_positives, e.num_false_positives,
            "{}: false positives",
            case.name
        );
    }
}

/// Every truth object present on a frame is accounted for exactly once, as a match,
/// a switch or a miss. This is the identity that makes `num_objects` the ground-truth
/// count rather than a separate tally that could drift from it.
#[test]
fn every_truth_object_is_accounted_for_exactly_once() {
    let fixture = load();
    for case in &fixture.cases {
        let got = run(case);
        let truth_instances: u64 = case.frames.iter().map(|f| f.truth.len() as u64).sum();
        assert_eq!(
            got.num_objects, truth_instances,
            "{}: {} truth instances in the scenario but {} accounted for",
            case.name, truth_instances, got.num_objects
        );
        assert_eq!(
            got.num_matches + got.num_switches + got.num_misses,
            truth_instances,
            "{}: matches + switches + misses does not cover the truth",
            case.name
        );
    }
}
