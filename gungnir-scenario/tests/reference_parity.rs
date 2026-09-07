//! The Rust generator against the reference generator's committed output (GAP-016,
//! GAP-046): every sample set under `testdata/tracks/samples/` is regenerated from the
//! four YAML files and its four data files must be **byte-identical**; the two JSON
//! descriptors must be equal as values (the reference writes them in Python dict order).
//!
//! This is the test that says the port is the reference, not a resemblance of it. A
//! change to the YAML that moves a point or a draw shows up here first.

use std::path::{Path, PathBuf};

use gungnir_scenario::tracks::generate_sample;
use gungnir_scenario::TrackLibrary;

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/test-tracks")
}

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/tracks/samples")
}

/// Numbers compared as floats, keys as sets: the value equality the descriptors need.
fn normalise(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Number(n) => serde_json::Value::from(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(normalise).collect()),
        serde_json::Value::Object(o) => {
            serde_json::Value::Object(o.iter().map(|(k, v)| (k.clone(), normalise(v))).collect())
        }
        other => other.clone(),
    }
}

fn first_difference(a: &str, b: &str) -> String {
    for (n, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            return format!("line {}:\n  ours:      {la}\n  reference: {lb}", n + 1);
        }
    }
    format!(
        "line counts differ: ours {}, reference {}",
        a.lines().count(),
        b.lines().count()
    )
}

#[test]
fn every_committed_sample_set_is_regenerated_byte_for_byte() {
    let lib = TrackLibrary::load(&docs()).expect("library");
    let mut checked = 0;
    for entry in std::fs::read_dir(samples()).expect("samples directory") {
        let dir = entry.expect("entry").path();
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix("-sample") else {
            continue;
        };
        let set = generate_sample(&lib, id).unwrap_or_else(|e| panic!("{id}: {e}"));
        for (file, ours) in [
            ("truth.jsonl", &set.truth),
            ("detections.jsonl", &set.detections),
            ("detections-truth.jsonl", &set.detections_truth),
            ("events.jsonl", &set.events),
        ] {
            let reference = std::fs::read_to_string(dir.join(file)).expect(file);
            assert!(
                *ours == reference,
                "{id}/{file} differs from the reference: {}",
                first_difference(ours, &reference)
            );
        }
        for (file, ours) in [
            ("sensors.json", &set.sensors_json),
            ("metadata.json", &set.metadata_json),
        ] {
            let reference: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join(file)).expect(file))
                    .expect("json");
            let (mut ours, mut reference) = (normalise(ours), normalise(&reference));
            // `validation` is written by the validator after generation, not by the
            // generator; the reference's copy carries the validator's verdict.
            for v in [&mut ours, &mut reference] {
                if let Some(o) = v.as_object_mut() {
                    o.remove("validation");
                }
            }
            if let (Some(a), Some(b)) = (ours.as_object(), reference.as_object()) {
                for key in a.keys().chain(b.keys()) {
                    assert_eq!(a.get(key), b.get(key), "{id}/{file} key {key}");
                }
            }
            assert_eq!(ours, reference, "{id}/{file}");
        }
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} sample sets found");
}

/// A set written to disk has the reference's six files.
#[test]
fn a_set_writes_its_six_files() {
    let lib = TrackLibrary::load(&docs()).expect("library");
    let set = generate_sample(&lib, "TT-03").expect("TT-03");
    let dir = std::env::temp_dir().join(format!("gungnir-tracks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    set.write_to(&dir).expect("written");
    for file in [
        "truth.jsonl",
        "detections.jsonl",
        "detections-truth.jsonl",
        "events.jsonl",
        "sensors.json",
        "metadata.json",
    ] {
        assert!(dir.join(file).is_file(), "{file}");
    }
    assert!(set.counts.truth_records > 0 && set.counts.sensors > 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// The full-size set of a scenario is generated (not compared: none is committed) and
/// its metadata says so.
#[test]
fn a_full_size_set_generates_under_its_own_seed() {
    let lib = TrackLibrary::load(&docs()).expect("library");
    let set = gungnir_scenario::tracks::generate(&lib, "TT-03", true, None, None).expect("full");
    assert_eq!(set.variant, "full");
    assert_eq!(set.metadata_json["variant"], "full");
    assert!(set.counts.truth_records > 0);
}
