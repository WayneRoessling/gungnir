// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The Arrow form of a training dataset (`docs/ml/data-pipeline.md` §2; GAP-079).
//!
//! Owned here rather than by `gungnir-ml` so the catalogue can name it: a dataset is a
//! document that crosses a boundary (to the training repository), and a reader needs
//! the schema before it needs the extractor. `gungnir-ml` builds rows against this
//! schema; nothing here reads a test-track set.

use arrow::datatypes::{DataType, Field, Schema};

/// The catalogue name of the ML-01 classification rows.
pub const CLASSIFICATION_ROWS: &str = "gungnir.ml.classification-rows";

/// The catalogue name of the ML-04 anomaly rows.
pub const ANOMALY_ROWS: &str = "gungnir.ml.anomaly-rows";

/// The dataset schema version, written into every row.
pub const DATASET_VERSION: u32 = 1;

/// The columns of `docs/ml/data-pipeline.md` §2, in order: the pipeline's versions, the
/// set, the entity and track, the time, the eleven features, the two labels, the split.
#[must_use]
pub fn classification_rows_schema() -> Schema {
    Schema::new(vec![
        Field::new("dataset_version", DataType::UInt32, false),
        Field::new("feature_schema_version", DataType::UInt32, false),
        Field::new("source_set", DataType::Utf8, false),
        Field::new("scenario", DataType::Utf8, false),
        Field::new("seed", DataType::UInt64, false),
        Field::new("entity_id", DataType::Utf8, false),
        Field::new("track_id", DataType::UInt64, false),
        Field::new("mission_time", DataType::Float64, false),
        Field::new("speed_mps", DataType::Float32, false),
        Field::new("altitude_m", DataType::Float32, false),
        Field::new("climb_mps", DataType::Float32, false),
        Field::new("turn_rate_dps", DataType::Float32, false),
        Field::new("speed_var", DataType::Float32, false),
        Field::new("heading_var", DataType::Float32, false),
        Field::new("age_s", DataType::Float32, false),
        Field::new("association_confidence", DataType::Float32, false),
        Field::new("sensor_count", DataType::Float32, false),
        Field::new("sensor_mix", DataType::Utf8, false),
        Field::new("cooperative_present", DataType::Boolean, false),
        Field::new("label_class", DataType::Utf8, false),
        Field::new("label_side", DataType::Utf8, false),
        Field::new("split", DataType::Utf8, false),
    ])
}

/// The ML-04 anomaly rows of `docs/ml/data-pipeline.md` §2: one per source per window and
/// one per track per window, in one schema, told apart by `subject_kind`.
///
/// **A feature that does not apply to a row's kind is null, never zero**: a source has no
/// speed and a track has no detection rate against an update period, and a zero would be
/// a measurement a model could learn from. The same holds for a feature with nothing to
/// measure in its window -- no residual where no detection was associated -- because
/// "nothing seen" is the signal a lost sensor gives, and it must not read as "seen, and
/// perfect". `label_anomaly` and `label_kind` are the labels: `label_kind` is `none` or the
/// planted anomalies active in the window, joined with `+`.
#[must_use]
pub fn anomaly_rows_schema() -> Schema {
    Schema::new(vec![
        Field::new("dataset_version", DataType::UInt32, false),
        Field::new("feature_schema_version", DataType::UInt32, false),
        Field::new("source_set", DataType::Utf8, false),
        Field::new("scenario", DataType::Utf8, false),
        Field::new("seed", DataType::UInt64, false),
        Field::new("subject_kind", DataType::Utf8, false),
        Field::new("subject_id", DataType::Utf8, false),
        Field::new("window_start", DataType::Float64, false),
        Field::new("window_end", DataType::Float64, false),
        Field::new("detections", DataType::UInt32, false),
        Field::new("detections_per_scan", DataType::Float32, true),
        Field::new("unassociated_fraction", DataType::Float32, true),
        Field::new("residual_rms_m", DataType::Float32, true),
        Field::new("clock_skew_s", DataType::Float32, true),
        Field::new("receipt_gap_s", DataType::Float32, true),
        Field::new("speed_mps", DataType::Float32, true),
        Field::new("cooperative_present", DataType::Boolean, true),
        Field::new("cooperative_residual_m", DataType::Float32, true),
        Field::new("cooperative_gap_s", DataType::Float32, true),
        Field::new("label_anomaly", DataType::Boolean, false),
        Field::new("label_kind", DataType::Utf8, false),
        Field::new("split", DataType::Utf8, false),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anomaly_schema_has_the_documented_columns_in_order() {
        let schema = anomaly_rows_schema();
        assert_eq!(schema.fields().len(), 22);
        assert_eq!(schema.field(5).name(), "subject_kind");
        assert_eq!(schema.field(19).name(), "label_anomaly");
        assert_eq!(schema.field(21).name(), "split");
        // Every feature is nullable; nothing that identifies or labels a row is.
        for field in schema.fields() {
            let feature = (10..=18).contains(&schema.index_of(field.name()).expect("a column"));
            assert_eq!(field.is_nullable(), feature, "{}", field.name());
        }
    }

    #[test]
    fn the_schema_has_the_documented_columns_in_order() {
        let schema = classification_rows_schema();
        assert_eq!(schema.fields().len(), 22);
        assert_eq!(schema.field(0).name(), "dataset_version");
        assert_eq!(schema.field(21).name(), "split");
        assert_eq!(schema.field(18).data_type(), &DataType::Boolean);
    }
}
