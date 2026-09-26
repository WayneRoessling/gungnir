// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The three re-observation sidecars against the reference generator's committed copies
//! (`docs/design/DN-32-re-observation-for-a-laydown.md` §5): `entities.json` and
//! `environment.json` in every sample set, and the JSON export of `sensors.yaml` at
//! `testdata/tracks/sensor-models.json`. **Byte for byte**, as the four data files are in
//! `reference_parity.rs`, not as equal values the way that test compares the two older
//! descriptors: both generators write the sidecars with sorted keys, so there is no
//! dictionary order to forgive.
//!
//! `reference_parity.rs` is deliberately untouched by DN-32 (§4: the extraction must
//! leave it passing *unchanged*), which is why the sidecars have a test of their own.

use std::path::{Path, PathBuf};

use gungnir_scenario::tracks::{generate_sample, sensor_catalogue_export};
use gungnir_scenario::TrackLibrary;

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/test-tracks")
}

fn tracks() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/tracks")
}

fn first_difference(ours: &str, reference: &str) -> String {
    for (n, (a, b)) in ours.lines().zip(reference.lines()).enumerate() {
        if a != b {
            return format!("line {}:\n  ours:      {a}\n  reference: {b}", n + 1);
        }
    }
    format!(
        "line counts differ: ours {}, reference {}",
        ours.lines().count(),
        reference.lines().count()
    )
}

#[test]
fn every_sample_sets_sidecars_are_regenerated_byte_for_byte() {
    let lib = TrackLibrary::load(&docs()).expect("library");
    let mut checked = 0;
    for entry in std::fs::read_dir(tracks().join("samples")).expect("samples directory") {
        let dir = entry.expect("entry").path();
        let Some(id) = dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix("-sample"))
        else {
            continue;
        };
        let set = generate_sample(&lib, id).unwrap_or_else(|e| panic!("{id}: {e}"));
        for (file, ours) in [
            ("entities.json", &set.entities_json),
            ("environment.json", &set.environment_json),
        ] {
            let reference = std::fs::read_to_string(dir.join(file))
                .unwrap_or_else(|e| panic!("{id}/{file} is committed: {e}"));
            assert!(
                *ours == reference,
                "{id}/{file} differs from the reference: {}",
                first_difference(ours, &reference)
            );
        }
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} sample sets found");
}

#[test]
fn the_sensor_catalogue_export_is_regenerated_byte_for_byte() {
    let lib = TrackLibrary::load(&docs()).expect("library");
    let ours = sensor_catalogue_export(&lib);
    let reference = std::fs::read_to_string(tracks().join("sensor-models.json"))
        .expect("testdata/tracks/sensor-models.json is committed");
    assert!(
        ours == reference,
        "sensor-models.json differs from the reference: {}",
        first_difference(&ours, &reference)
    );
}

/// The export reads back as the model the observation model runs: every type parses,
/// and a type's model is the one the generator builds from the YAML directly.
#[test]
fn every_exported_type_reads_back_as_the_generators_own_model() {
    let lib = TrackLibrary::load(&docs()).expect("library");
    let catalogue: gungnir_sensor_sim::SensorCatalogue =
        serde_json::from_str(&sensor_catalogue_export(&lib)).expect("the export parses");
    assert_eq!(catalogue.sensors_version, lib.sensors.version);
    assert_eq!(catalogue.types.len(), lib.sensors.types.len());
    for t in &lib.sensors.types {
        let exported = catalogue
            .model(&t.id)
            .unwrap_or_else(|| panic!("{} is in the export", t.id));
        let mut direct = t.detection_model(None);
        // A type-level `bias_m` (isr-video declares a zero one) is carried by the export
        // and ignored by the generator, which applies an instance's override only.
        let mut exported = exported.clone();
        exported.bias_m = None;
        direct.bias_m = None;
        assert_eq!(exported, direct, "{}", t.id);
    }
}
