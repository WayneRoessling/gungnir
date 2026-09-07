//! The `sensor_ingestion_parser` fuzz corpus is real input (GAP-076).
//!
//! `gungnir-fuzz` is excluded from the workspace, so nothing in the ordinary gate compiles
//! or runs its targets. That makes its corpus the kind of thing that rots quietly: a seed
//! the decoder stopped accepting is a seed the fuzzer wastes its budget on, and nobody
//! finds out until somebody reads a nightly report.
//!
//! **A corpus of inputs the decoder rejects is worse than no corpus**: the fuzzer starts
//! from the shape of a rejection rather than the shape of a detection, and mutating a
//! rejected input explores the parser's error path rather than its accept path.
//!
//! # The seeding rule, and why the rule is a test
//!
//! Every seed is a line lifted verbatim out of a committed plan-07 sample set
//! (`testdata/tracks/samples/*/detections.jsonl`), chosen by the rule
//! [`required_lines`] states. The 2026-09-05 seeding covered **identity**: the first
//! line of every (sample set, sensor) pair plus the last line of every set, so every
//! sensor modality the ten sets produce is represented. It covered no **value**, and
//! the rules the decoded detection then has to pass are written about values --
//! `gungnir_ingest::gateway::validate_detection` bounds the measurement magnitude and
//! the distance between source and receipt time. A corpus whose measurements all sit
//! in the middle of the range starts the fuzzer a long mutation away from either
//! bound. The three extremal lines per set added for GAP-076 are those ends: the
//! widest source-to-receipt gap the set contains (the generator's 4.9 s cap, in the
//! sets that reach it), and the largest and smallest measurement magnitude.
//!
//! A seed is its bytes and not its filename, so [`required_lines`] is checked by
//! content. TT-10's extremal lines are byte-identical to TT-09's and are in the corpus
//! under TT-09's names; writing them twice would be one seed with two names, which is
//! what libFuzzer's own deduplication would collapse anyway.

use gungnir_ingest::gateway::decode_json_line;
use gungnir_ingest::DetectionView;
use std::path::{Path, PathBuf};

fn corpus_dir() -> std::path::PathBuf {
    std::path::Path::new("..").join("gungnir-fuzz/corpus/sensor_ingestion_parser")
}

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("testdata")
        .join("tracks")
        .join("samples")
}

/// The committed sample sets, in a stable order so a failure names the same set twice.
fn sample_sets() -> Vec<PathBuf> {
    let mut sets: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("testdata/tracks/samples exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("detections.jsonl").is_file())
        .collect();
    sets.sort();
    sets
}

/// How far from the frame's origin a detection's position is, metres, and zero for a
/// measurement that is not a position.
///
/// The two seeds below bound the magnitude rule the gateway applies, and that rule
/// applies to a place: a bearing has no magnitude, and giving it one would mean giving
/// it a range (docs/design/DN-27-bearing-only-detections.md §2). Every line in every
/// sample set is a position, so nothing in this corpus takes the zero.
fn position_magnitude_m(d: &DetectionView) -> f64 {
    d.measurement.position_enu().map_or(0.0, |enu| enu.norm())
}

/// The index of the line whose `key` is greatest; the first of any tie, so the choice
/// is a function of the file rather than of a sort's stability.
fn argmax(decoded: &[DetectionView], key: impl Fn(&DetectionView) -> f64) -> usize {
    let mut best = 0;
    for (i, detection) in decoded.iter().enumerate() {
        if key(detection) > key(&decoded[best]) {
            best = i;
        }
    }
    best
}

/// The lines of one sample set the corpus has to hold, each with the name it carries
/// in the corpus, so a failure says which line went missing rather than only how many.
///
/// This is the seeding rule itself rather than a list of filenames, which is the point:
/// regenerating the sample sets (`gungnir-scenario::tracks`, GAP-016) moves these lines,
/// and a corpus still holding the old ones is a corpus of inputs that no longer describe
/// the data the gateway is fed.
fn required_lines(set: &Path) -> Vec<(String, Vec<u8>)> {
    let name = set
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("a sample set directory has a name");
    let text = std::fs::read_to_string(set.join("detections.jsonl")).expect("detections readable");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty(), "{name}: no detection lines");
    let decoded: Vec<DetectionView> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            decode_json_line(line.as_bytes())
                .unwrap_or_else(|e| panic!("{name} line {}: {e}", i + 1))
        })
        .collect();

    let seed =
        |suffix: String, i: usize| (format!("{name}-{suffix}"), lines[i].as_bytes().to_vec());
    let mut required = Vec::new();

    // Identity: the first line each sensor produces, and the recording's last line.
    let mut sensors_seen = std::collections::BTreeSet::new();
    for (i, detection) in decoded.iter().enumerate() {
        if sensors_seen.insert(detection.sensor.0) {
            required.push(seed(detection.sensor.0.to_string(), i));
        }
    }
    required.push(seed("last".to_owned(), lines.len() - 1));

    // Value: the ends of the two ranges the gateway's own rules bound.
    required.push(seed(
        "latency-max".to_owned(),
        argmax(&decoded, |d| d.receipt_time.0 - d.source_time.0),
    ));
    required.push(seed(
        "magnitude-max".to_owned(),
        argmax(&decoded, position_magnitude_m),
    ));
    required.push(seed(
        "magnitude-min".to_owned(),
        argmax(&decoded, |d| -position_magnitude_m(d)),
    ));
    required
}

/// Every seed decodes, whichever rule put it there. This is the guarantee the module
/// comment is about, and it covers the seeds added for the value half of the rule on the
/// same terms as the ones the identity half put there in 2026-09-05.
#[test]
fn every_fuzz_seed_is_a_detection_the_decoder_accepts() {
    let dir = corpus_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("the fuzz corpus is missing at {}", dir.display());
    };

    let mut checked = 0;
    let mut rejected = Vec::new();
    for entry in entries.flatten() {
        let bytes = std::fs::read(entry.path()).expect("seed readable");
        if decode_json_line(&bytes).is_err() {
            rejected.push(entry.file_name().to_string_lossy().into_owned());
        }
        checked += 1;
    }

    assert!(
        checked >= 20,
        "only {checked} seeds; the corpus is not being found or was not written"
    );
    assert!(
        rejected.is_empty(),
        "seeds the decoder rejects start the fuzzer on the error path: {rejected:?}"
    );
}

/// The seeds are not all the same line. A corpus of one shape repeated is a corpus of one.
#[test]
fn the_seeds_cover_more_than_one_sensor() {
    let dir = corpus_dir();
    let mut sensors = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("corpus").flatten() {
        let bytes = std::fs::read(entry.path()).expect("seed readable");
        if let Ok(detection) = decode_json_line(&bytes) {
            sensors.insert(detection.sensor.0);
        }
    }
    assert!(
        sensors.len() >= 4,
        "the corpus covers only {} sensor(s): {sensors:?}",
        sensors.len()
    );
}

/// The corpus still holds every line the seeding rule names (GAP-076).
///
/// The rule is re-derived from `testdata/tracks/samples/` on every run rather than
/// compared against a checked-in list, so this fails in the two ways that matter: a
/// seed deleted from the corpus, and a sample set regenerated so that the line a seed
/// was taken from is no longer in it. Either leaves the fuzzer starting from data the
/// gateway is no longer fed, which is the silent rot the module comment describes --
/// and it is silent precisely because `gungnir-fuzz` is outside the workspace and no
/// ordinary `cargo test` compiles it.
#[test]
fn the_corpus_holds_every_sample_line_the_seeding_rule_names() {
    let corpus: std::collections::BTreeSet<Vec<u8>> = std::fs::read_dir(corpus_dir())
        .expect("the fuzz corpus directory")
        .flatten()
        .map(|entry| {
            let mut bytes = std::fs::read(entry.path()).expect("seed readable");
            while bytes.last().is_some_and(u8::is_ascii_whitespace) {
                bytes.pop();
            }
            bytes
        })
        .collect();

    let sets = sample_sets();
    assert!(
        sets.len() >= 10,
        "only {} sample sets under testdata/tracks/samples",
        sets.len()
    );

    let mut required = 0usize;
    let mut missing = Vec::new();
    for set in sets {
        for (name, line) in required_lines(&set) {
            required += 1;
            if !corpus.contains(&line) {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} of {required} seeds the rule names are not in the corpus: {missing:?}",
        missing.len()
    );
}
