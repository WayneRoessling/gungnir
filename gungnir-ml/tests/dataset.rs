//! The dataset extraction against a committed test-track set (GAP-079,
//! `docs/ml/data-pipeline.md` §2, §3, §5).

use gungnir_ml::dataset::{extract_set, split_for};
use gungnir_ml::features::FEATURE_NAMES;
use std::path::{Path, PathBuf};

fn sample(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/tracks/samples")
        .join(format!("{id}-sample"))
}

#[test]
fn tt01_extracts_labelled_rows_in_the_catalogue_form_with_a_stable_hash() {
    let dataset = extract_set(&sample("TT-01")).expect("TT-01 extracts");
    assert_eq!(dataset.split, "train");
    assert_eq!(dataset.provenance.scenario, "TT-01");
    assert_eq!(dataset.provenance.seed, 1701);
    assert!(!dataset.rows.is_empty());
    // Every row is labelled from the truth, never guessed.
    assert!(dataset
        .rows
        .iter()
        .all(|r| r.label_class == "air.owa-prop" && r.label_side == "red"));
    assert_eq!(dataset.rows_by_class["air.owa-prop"], dataset.rows.len());
    // The sensor columns come from the detections the entity caused: TT-01's drones
    // are seen, so some rows carry a sensor.
    assert!(dataset.rows.iter().any(|r| r.features[8] > 0.0));

    let batch = dataset.to_record_batch().expect("arrow");
    assert_eq!(batch.num_rows(), dataset.rows.len());
    let schema = batch.schema();
    let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    for feature in FEATURE_NAMES {
        assert!(names.contains(&feature), "{feature} is a column");
    }
    for label in ["label_class", "label_side", "split", "dataset_version"] {
        assert!(names.contains(&label), "{label} is a column");
    }

    let again = extract_set(&sample("TT-01")).expect("TT-01 extracts again");
    assert_eq!(dataset.content_hash(), again.content_hash());
    assert_eq!(dataset.content_hash().len(), 64);
}

#[test]
fn the_split_is_by_scenario_as_the_pipeline_says() {
    assert_eq!(split_for("TT-01"), "train");
    assert_eq!(split_for("TT-02"), "validation");
    assert_eq!(split_for("TT-05"), "test");
    assert_eq!(split_for("TT-99"), "unassigned");
    let held_out = extract_set(&sample("TT-05")).expect("TT-05 extracts");
    assert_eq!(held_out.split, "test");
    assert_ne!(
        held_out.content_hash(),
        extract_set(&sample("TT-01")).expect("TT-01").content_hash()
    );
}
