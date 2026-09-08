// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Data standards & interoperability, per docs/gungnir-capabilities.md §5.6 and
//! the "Scenario/interop schema (JSON/Arrow, ASTERIX/STANAG)" row of
//! verification-capability-table.md §1. Owns the schema catalog (what formats and
//! versions this build speaks), the Arrow columnar form of detections, and the
//! codec boundary that industry-format adapters implement. Per
//! agentic-coding-standards.md §2.7, the Arrow schema is defined once here and
//! shared by writer and reader; ASTERIX/STANAG mapping is isolated from it.

pub mod adsb;
pub mod ais;
pub mod asterix;
pub mod dataset;
/// MISB ST 0601's UAS Datalink Local Set (GAP-099;
/// `docs/design/external-standards.md` §8 and §8.2). Unlike `asterix` and `ais`, no
/// primary specification text was obtainable -- the module doc comment says exactly
/// what is and is not pinned.
pub mod misb0601;

pub use adsb::AdsbCodec;
pub use ais::AisCodec;
pub use asterix::cat034::{AsterixCat034Codec, RadarServiceReport, ServiceEvent};
pub use asterix::cat048::AsterixCat048Codec;
pub use asterix::cat205::{AsterixCat205Codec, DfSite};
pub use asterix::RadarSite;

use arrow::array::{Array, ArrayRef, Float64Array, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use gungnir_model::{DetectionView, MissionTime, Provenance, SensorId, TrackView, SCHEMA_VERSION};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum InteropError {
    #[error("codec {0} is not implemented in this build")]
    NotImplemented(&'static str),
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("record batch does not match the detection schema: {0}")]
    SchemaMismatch(String),
    /// The catalogue's compatibility error (GAP-063): a peer offered a version of a
    /// schema this build does not speak. Exact match is the rule until a second version
    /// exists (`docs/gungnir-api-v1.md`).
    #[error("schema {name} version {offered} is not the version {spoken} this build speaks")]
    IncompatibleSchema {
        name: String,
        offered: u32,
        spoken: u32,
    },
    #[error("schema {0} is not in this build's catalogue")]
    UnknownSchema(String),
    /// The input is not well-formed for the codec: the offset is the byte the
    /// decoder could not get past, the reason names the item and what was wrong.
    #[error("{codec}: malformed input at octet {offset}: {reason}")]
    Malformed {
        codec: &'static str,
        offset: usize,
        reason: String,
    },
    /// A data block of a category this codec does not decode.
    #[error("{codec}: data block at octet {offset} is category {found}, expected {expected}")]
    WrongCategory {
        codec: &'static str,
        expected: u8,
        found: u8,
        offset: usize,
    },
    /// A report from a site the codec was not configured with (a radar's `RadarSite`
    /// or, since Category 205, a direction finder's `DfSite`). The report is not
    /// attributed to a guessed sensor.
    #[error("{codec}: report from SAC {sac} SIC {sic}, which no configured site matches")]
    UnknownRadar {
        codec: &'static str,
        sac: u8,
        sic: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SchemaKind {
    /// A `gungnir-model` type as JSON.
    GungnirJson,
    Arrow,
    AsterixCategory(u16),
    Stanag(u32),
    /// A textual identity form, named by its RFC (GAP-069, D-11).
    ///
    /// Its own kind rather than a JSON schema, because it is **not a document**: it is how
    /// a single value is written when it crosses a boundary, and a peer needs to know the
    /// format before it can read any document containing one.
    TextualIdentity {
        rfc: u32,
        uuid_version: u8,
    },
    /// AIS payloads in NMEA sentences, decoded to the named edition of ITU-R M.1371
    /// (D-32, `docs/design/external-standards.md` §3).
    Ais {
        edition: u8,
    },
    /// Mode S 1090 MHz extended squitter (GAP-010,
    /// `docs/design/external-standards.md` §4).
    ///
    /// The flag exists because it is **`false`**, and it is the only schema in this
    /// catalogue for which it is. No ADS-B specification is both free to obtain and
    /// permissively licensed, so `adsb` is gated against two open-source decoders
    /// rather than against a normative document. A peer negotiating this schema is
    /// entitled to know that before it treats a decode as conformant.
    Adsb1090Es {
        normative_source_pinned: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemaEntry {
    pub name: String,
    pub version: u32,
    pub kind: SchemaKind,
}

/// Every schema this build can read or write, with the version it speaks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemaCatalog {
    entries: Vec<SchemaEntry>,
}

impl SchemaCatalog {
    pub fn builtin() -> Self {
        let json = |name: &str| SchemaEntry {
            name: name.into(),
            version: SCHEMA_VERSION,
            kind: SchemaKind::GungnirJson,
        };
        Self {
            entries: vec![
                json("gungnir.DetectionView"),
                json("gungnir.TrackView"),
                json("gungnir.PlanView"),
                json("gungnir.Envelope"),
                SchemaEntry {
                    name: "gungnir.detections.arrow".into(),
                    version: SCHEMA_VERSION,
                    kind: SchemaKind::Arrow,
                },
                // GAP-079: the ML-01 training rows (`docs/ml/data-pipeline.md` §2), so a
                // dataset and the wire format share one definition. Versioned on its own:
                // a dataset's columns change with the feature schema, not the model's.
                SchemaEntry {
                    name: dataset::CLASSIFICATION_ROWS.into(),
                    version: dataset::DATASET_VERSION,
                    kind: SchemaKind::Arrow,
                },
                // GAP-069: identities are UUID v7 (RFC 9562, which supersedes RFC 4122).
                // Registered so a peer knows how to read one before it reads a document
                // that carries one.
                SchemaEntry {
                    name: "gungnir.GlobalEntityId".into(),
                    version: SCHEMA_VERSION,
                    kind: SchemaKind::TextualIdentity {
                        rfc: 9562,
                        uuid_version: 7,
                    },
                },
                // Editions pinned 2026-09-06 in docs/design/external-standards.md §1.6
                // and §1.7. Category 048 decodes (`asterix::cat048`, edition 1.32) and
                // Category 034 decodes (`asterix::cat034`, edition 1.29); neither encodes.
                SchemaEntry {
                    name: "asterix.cat048".into(),
                    version: 1,
                    kind: SchemaKind::AsterixCategory(48),
                },
                SchemaEntry {
                    name: "asterix.cat034".into(),
                    version: 1,
                    kind: SchemaKind::AsterixCategory(34),
                },
                // Pinned 2026-09-08 in docs/design/external-standards.md §9 (GAP-100):
                // Category 205, Radio Direction Finder Reports, edition 1.0. Decodes
                // (`asterix::cat205`); does not encode, for the same reason 048 does not.
                SchemaEntry {
                    name: "asterix.cat205".into(),
                    version: 1,
                    kind: SchemaKind::AsterixCategory(205),
                },
                SchemaEntry {
                    name: "stanag.4676".into(),
                    version: 1,
                    kind: SchemaKind::Stanag(4676),
                },
                // D-32, 2026-09-06: M.1371-6 pinned and decoded (`ais`); the entry
                // version is this build's, the edition is the Recommendation's.
                SchemaEntry {
                    name: "ais.m1371".into(),
                    version: 1,
                    kind: SchemaKind::Ais { edition: 6 },
                },
                // GAP-010, 2026-09-06: the extended squitter decodes (`adsb`), and the
                // entry says in its own kind that no normative source stands behind it.
                SchemaEntry {
                    name: adsb::CODEC_NAME.into(),
                    version: 1,
                    kind: SchemaKind::Adsb1090Es {
                        normative_source_pinned: false,
                    },
                },
            ],
        }
    }

    pub fn entries(&self) -> &[SchemaEntry] {
        &self.entries
    }

    pub fn lookup(&self, name: &str) -> Option<&SchemaEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Version negotiation: a peer's version is compatible if it matches exactly.
    /// (Minor-version tolerance is a policy decision recorded in
    /// docs/gungnir-api-v1.md once there is a second version.)
    pub fn is_compatible(&self, name: &str, version: u32) -> bool {
        self.lookup(name).is_some_and(|e| e.version == version)
    }

    /// [`Self::is_compatible`] as the refusal the verification table asks for
    /// (GAP-063): an unknown schema and a wrong version are each named.
    ///
    /// # Errors
    ///
    /// [`InteropError::UnknownSchema`] or [`InteropError::IncompatibleSchema`].
    pub fn check(&self, name: &str, version: u32) -> Result<(), InteropError> {
        let entry = self
            .lookup(name)
            .ok_or_else(|| InteropError::UnknownSchema(name.to_string()))?;
        if entry.version == version {
            Ok(())
        } else {
            Err(InteropError::IncompatibleSchema {
                name: name.to_string(),
                offered: version,
                spoken: entry.version,
            })
        }
    }
}

/// The boundary for formats that carry a sensor's own service messages rather
/// than observations: sector timing, north crossings, operational status. A
/// service message is not a detection, so it does not go through
/// [`DetectionCodec`] (`docs/design/external-standards.md` §1.7).
pub trait ServiceMessageCodec: Send + Sync {
    fn name(&self) -> &'static str;
    fn decode(
        &self,
        bytes: &[u8],
        receipt_time: MissionTime,
    ) -> Result<Vec<RadarServiceReport>, InteropError>;
}

/// The boundary industry-format adapters implement.
pub trait DetectionCodec: Send + Sync {
    fn name(&self) -> &'static str;
    fn decode(
        &self,
        bytes: &[u8],
        receipt_time: MissionTime,
    ) -> Result<Vec<DetectionView>, InteropError>;
    fn encode(&self, tracks: &[TrackView]) -> Result<Vec<u8>, InteropError>;
}

/// NATO STANAG 4676 (ISR tracking standard).
///
/// The governing document is AEDP-12; where it is and why it has not been obtained:
/// `docs/design/external-standards.md` §2 (GAP-064).
#[derive(Debug, Default, Clone, Copy)]
pub struct Stanag4676Codec;

impl DetectionCodec for Stanag4676Codec {
    fn name(&self) -> &'static str {
        "stanag.4676"
    }
    fn decode(
        &self,
        _bytes: &[u8],
        _receipt_time: MissionTime,
    ) -> Result<Vec<DetectionView>, InteropError> {
        Err(InteropError::NotImplemented(self.name()))
    }
    fn encode(&self, _tracks: &[TrackView]) -> Result<Vec<u8>, InteropError> {
        Err(InteropError::NotImplemented(self.name()))
    }
}

/// The one Arrow schema for detections, shared by writer and reader.
///
/// **Lossless on provenance since 2026-09-06 (GAP-063).** The first schema carried the
/// sensor, the times, the measurement and the algorithm version, and the conformance
/// suite found it dropped the calibration baseline, the contributing sensor list, the
/// peer origin, the conversion loss and the authentication strength. Each has its column
/// now; the two that are not scalars (the sensor list and the peer origin) are carried
/// as JSON text, which is honest about what Arrow is being asked to hold.
///
/// **The three `e_m`/`n_m`/`u_m` columns became one `measurement_json` column on
/// 2026-09-06** (docs/design/DN-27-bearing-only-detections.md §4 and §8). A measurement
/// is no longer three floats: it is a position, a native range/azimuth/elevation
/// report, or a bearing with no range at all, and each carries its own error. Three
/// non-nullable float columns could hold a bearing only by inventing a range for it,
/// which is the one thing DN-27 exists to forbid, and could hold a position's variance
/// nowhere at all. `gungnir_model::SCHEMA_VERSION` went from 2 to 3 in the same
/// change, so a peer holding the old shape is refused rather than misreading this one.
pub fn detection_arrow_schema() -> Schema {
    Schema::new(vec![
        Field::new("sensor_id", DataType::UInt32, false),
        Field::new("source_time_s", DataType::Float64, false),
        Field::new("receipt_time_s", DataType::Float64, false),
        Field::new("measurement_json", DataType::Utf8, false),
        Field::new("algorithm_version", DataType::Utf8, false),
        Field::new("source_sensor_ids_json", DataType::Utf8, false),
        Field::new("calibration_baseline_version", DataType::Utf8, true),
        Field::new("authentication", DataType::Utf8, false),
        Field::new("peer_json", DataType::Utf8, true),
        Field::new("conversion_loss", DataType::Utf8, true),
    ])
}

fn authentication_name(a: gungnir_model::SourceAuthentication) -> &'static str {
    match a {
        gungnir_model::SourceAuthentication::Unauthenticated => "unauthenticated",
        gungnir_model::SourceAuthentication::AllowList => "allow-list",
        gungnir_model::SourceAuthentication::MachineIdentity => "machine-identity",
    }
}

fn authentication_from(name: &str) -> Result<gungnir_model::SourceAuthentication, InteropError> {
    match name {
        "unauthenticated" => Ok(gungnir_model::SourceAuthentication::Unauthenticated),
        "allow-list" => Ok(gungnir_model::SourceAuthentication::AllowList),
        "machine-identity" => Ok(gungnir_model::SourceAuthentication::MachineIdentity),
        other => Err(InteropError::SchemaMismatch(format!(
            "authentication {other:?} is not a strength this build knows"
        ))),
    }
}

/// # Errors
///
/// An Arrow error building the batch, or a peer origin that does not serialise.
pub fn detections_to_record_batch(
    detections: &[DetectionView],
) -> Result<RecordBatch, InteropError> {
    let json = |v: &dyn erased::Json| v.to_json();
    let source_ids: Vec<String> = detections
        .iter()
        .map(|d| json(&d.provenance.source_sensor_ids))
        .collect();
    let peers: Vec<Option<String>> = detections
        .iter()
        .map(|d| d.provenance.peer.as_ref().map(|p| json(p)))
        .collect();
    // The measurement is carried whole rather than flattened (DN-27 §4): a bearing has
    // no `e_m`, and a position's per-axis variance had nowhere to go in the old shape.
    let measurements: Vec<String> = detections
        .iter()
        .map(|d| {
            serde_json::to_string(&d.measurement).map_err(|e| {
                InteropError::SchemaMismatch(format!("measurement does not serialise: {e}"))
            })
        })
        .collect::<Result<_, _>>()?;
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(
            detections.iter().map(|d| d.sensor.0),
        )),
        Arc::new(Float64Array::from_iter_values(
            detections.iter().map(|d| d.source_time.0),
        )),
        Arc::new(Float64Array::from_iter_values(
            detections.iter().map(|d| d.receipt_time.0),
        )),
        Arc::new(StringArray::from_iter_values(
            measurements.iter().map(String::as_str),
        )),
        Arc::new(StringArray::from_iter_values(
            detections
                .iter()
                .map(|d| d.provenance.algorithm_version.as_str()),
        )),
        Arc::new(StringArray::from_iter_values(
            source_ids.iter().map(String::as_str),
        )),
        Arc::new(StringArray::from_iter(
            detections
                .iter()
                .map(|d| d.provenance.calibration_baseline_version.as_deref()),
        )),
        Arc::new(StringArray::from_iter_values(
            detections
                .iter()
                .map(|d| authentication_name(d.provenance.authentication)),
        )),
        Arc::new(StringArray::from_iter(peers.iter().map(Option::as_deref))),
        Arc::new(StringArray::from_iter(
            detections
                .iter()
                .map(|d| d.provenance.conversion_loss.as_deref()),
        )),
    ];
    Ok(RecordBatch::try_new(
        Arc::new(detection_arrow_schema()),
        columns,
    )?)
}

/// JSON text for the two provenance fields Arrow carries as text.
mod erased {
    pub trait Json {
        fn to_json(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn to_json(&self) -> String {
            serde_json::to_string(self).unwrap_or_default()
        }
    }
}

fn column<'a, T: Array + 'static>(
    batch: &'a RecordBatch,
    index: usize,
    name: &str,
) -> Result<&'a T, InteropError> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| InteropError::SchemaMismatch(format!("column {index} is not {name}")))
}

fn optional(column: &StringArray, i: usize) -> Option<String> {
    if column.is_null(i) {
        None
    } else {
        Some(column.value(i).to_string())
    }
}

/// # Errors
///
/// A batch whose fields are not [`detection_arrow_schema`]'s, or a column whose text
/// does not decode.
pub fn record_batch_to_detections(batch: &RecordBatch) -> Result<Vec<DetectionView>, InteropError> {
    if batch.schema().fields() != detection_arrow_schema().fields() {
        return Err(InteropError::SchemaMismatch("field list differs".into()));
    }
    let sensor = column::<UInt32Array>(batch, 0, "UInt32")?;
    let source = column::<Float64Array>(batch, 1, "Float64")?;
    let receipt = column::<Float64Array>(batch, 2, "Float64")?;
    let measurement = column::<StringArray>(batch, 3, "Utf8")?;
    let algo = column::<StringArray>(batch, 4, "Utf8")?;
    let source_ids = column::<StringArray>(batch, 5, "Utf8")?;
    let calibration = column::<StringArray>(batch, 6, "Utf8")?;
    let authentication = column::<StringArray>(batch, 7, "Utf8")?;
    let peer = column::<StringArray>(batch, 8, "Utf8")?;
    let loss = column::<StringArray>(batch, 9, "Utf8")?;
    (0..batch.num_rows())
        .map(|i| {
            let ids: Vec<u32> = serde_json::from_str(source_ids.value(i)).map_err(|e| {
                InteropError::SchemaMismatch(format!("row {i}: source sensor ids: {e}"))
            })?;
            let peer_origin = match optional(peer, i) {
                Some(text) => Some(serde_json::from_str(&text).map_err(|e| {
                    InteropError::SchemaMismatch(format!("row {i}: peer origin: {e}"))
                })?),
                None => None,
            };
            Ok(DetectionView {
                sensor: SensorId(sensor.value(i)),
                source_time: MissionTime(source.value(i)),
                receipt_time: MissionTime(receipt.value(i)),
                measurement: serde_json::from_str(measurement.value(i)).map_err(|e| {
                    InteropError::SchemaMismatch(format!("row {i}: measurement: {e}"))
                })?,
                provenance: Provenance {
                    source_sensor_ids: ids,
                    calibration_baseline_version: optional(calibration, i),
                    algorithm_version: algo.value(i).to_string(),
                    peer: peer_origin,
                    conversion_loss: optional(loss, i),
                    authentication: authentication_from(authentication.value(i))?,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detection(i: u32) -> DetectionView {
        DetectionView {
            sensor: SensorId(i),
            source_time: MissionTime(f64::from(i)),
            receipt_time: MissionTime(f64::from(i) + 0.5),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(1.0, 2.0, 3.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance {
                source_sensor_ids: vec![i, i + 10],
                calibration_baseline_version: Some(format!("cb-{i}")),
                algorithm_version: "0.1.0".into(),
                peer: Some(gungnir_model::PeerOrigin {
                    peer: "sector-north".into(),
                    remote_track: format!("t-{i}"),
                    peer_time: MissionTime(1.0),
                    receipt_time: MissionTime(2.0),
                    assigned_quality: 0.5,
                }),
                conversion_loss: Some("covariance structure".into()),
                authentication: gungnir_model::SourceAuthentication::AllowList,
            },
        }
    }

    #[test]
    fn arrow_round_trip_is_lossless() {
        let input = vec![detection(1), detection(2)];
        let batch = detections_to_record_batch(&input).expect("to batch");
        assert_eq!(batch.num_rows(), 2);
        let back = record_batch_to_detections(&batch).expect("from batch");
        assert_eq!(back, input);
    }

    #[test]
    fn catalog_negotiates_exact_versions() {
        let c = SchemaCatalog::builtin();
        assert!(c.is_compatible("gungnir.TrackView", SCHEMA_VERSION));
        assert!(!c.is_compatible("gungnir.TrackView", SCHEMA_VERSION + 1));
        assert!(c.lookup("nope").is_none());
    }

    /// external-standards.md §4: the catalogue entry for ADS-B says, in the schema
    /// kind itself, that no normative source is pinned. A green differential gate is
    /// agreement with the open-source consensus and not conformance (GAP-010), and a
    /// peer is told so before it negotiates the schema.
    #[test]
    fn the_adsb_entry_declares_that_no_normative_source_is_pinned() {
        let c = SchemaCatalog::builtin();
        assert_eq!(
            c.lookup(adsb::CODEC_NAME).map(|e| &e.kind),
            Some(&SchemaKind::Adsb1090Es {
                normative_source_pinned: false
            })
        );
        assert!(adsb::SPECIFICATION.contains("none pinned"));
    }

    /// external-standards.md §1.7: the build declares both radar categories it intends
    /// to speak, and there is no codec for 034 to claim it does.
    #[test]
    fn catalog_lists_both_monoradar_categories() {
        let c = SchemaCatalog::builtin();
        assert_eq!(
            c.lookup("asterix.cat048").map(|e| &e.kind),
            Some(&SchemaKind::AsterixCategory(48))
        );
        assert_eq!(
            c.lookup("asterix.cat034").map(|e| &e.kind),
            Some(&SchemaKind::AsterixCategory(34))
        );
    }

    /// GAP-100: the build declares Category 205 too, once it decodes.
    #[test]
    fn catalog_lists_category_205() {
        let c = SchemaCatalog::builtin();
        assert_eq!(
            c.lookup("asterix.cat205").map(|e| &e.kind),
            Some(&SchemaKind::AsterixCategory(205))
        );
    }

    /// What is not built says so: STANAG 4676 both ways, Category 048 encode.
    /// Category 048 decode is built (`asterix::cat048`) and tested there and in
    /// `tests/asterix_fixtures.rs`.
    #[test]
    fn unbuilt_codecs_report_not_implemented() {
        assert!(matches!(
            Stanag4676Codec.decode(b"", MissionTime(0.0)),
            Err(InteropError::NotImplemented(_))
        ));
        assert!(matches!(
            Stanag4676Codec.encode(&[]),
            Err(InteropError::NotImplemented(_))
        ));
        assert!(matches!(
            AsterixCat048Codec::default().encode(&[]),
            Err(InteropError::NotImplemented(_))
        ));
    }
}
