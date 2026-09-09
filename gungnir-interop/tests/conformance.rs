// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The interface conformance suite (GAP-063; `docs/verification-capability-table.md` §2,
//! the system-of-systems interop row).
//!
//! The row asks four things of the schema catalogue: every catalogued schema version
//! round-trips its corpus with zero loss; an unsupported version is refused with the
//! catalogue's compatibility error; public-specification samples decode at least
//! partially with the unsupported fields reported; and nothing panics on the fuzz
//! corpus. This file walks the catalogue **entry by entry**, so an entry added without a
//! corpus fails here rather than being assumed covered, and reads its corpora from the
//! repository: the committed test-track detections (`testdata/tracks/samples/`), the
//! public ASTERIX capture (`testdata/asterix/`), and the gpsd AIS captures
//! (`testdata/ais/`). The fuzz corpus itself lives with `gungnir-ingest`, whose
//! `tests/fuzz_corpus.rs` keeps every seed decoding; the truncation and corruption
//! property is here for the two binary codecs.
//!
//! What is deliberately not claimed: STANAG 4676 has no corpus and no decoder, and its
//! entry is checked to say so (`NotImplemented`) rather than to pass.
//!
//! # In memory here, and over a real wire elsewhere (GAP-063)
//!
//! Everything in this file is checked **in one process**. That is the whole suite for a
//! format read from a file or a feed the deployment owns, and it is not the whole suite
//! for a schema two deployments speak to each other: a type can round-trip perfectly
//! through `serde_json` here and still lose something crossing a transport that
//! re-encodes, filters or truncates it. GAP-063's remaining action asked for the suite
//! "against a peer over the wire once GAP-065 has one", and GAP-065 now has the
//! mutual-TLS machine link.
//!
//! The wire half lives in `gungnir-remote/tests/wire_conformance.rs`, because a test that
//! stands two nodes up needs the transport and the client, and this crate depends on
//! neither -- `ARCHITECTURE.md` §7.1 draws `gungnir-interop` to `gungnir-model` alone.
//! What stays here is the **declaration**: [`wire_coverage`] says, entry by entry,
//! whether that entry's zero-loss criterion is checked across a real transport and, when
//! it is not, why not. A catalogue entry added without a declaration fails
//! `every_catalogue_entry_says_whether_it_is_checked_over_the_wire`, for the same reason
//! an entry added without a corpus check fails the walk above: an unexamined entry must
//! not be assumed covered.

use std::path::{Path, PathBuf};

use gungnir_interop::ais::{AisCodec, AisMessage};
use gungnir_interop::asterix::{cat034, cat048, cat129, cat205, data_blocks};
use gungnir_interop::{
    detections_to_record_batch, record_batch_to_detections, DetectionCodec, InteropError,
    SchemaCatalog, SchemaKind, Stanag4676Codec, UasIdentificationCodec,
};
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::{DetectionView, MissionTime, PlanView, TrackView};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every detection line of every committed sample set.
fn detection_corpus() -> Vec<DetectionView> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root().join("testdata/tracks/samples")).expect("samples") {
        let dir = entry.expect("entry").path();
        let Ok(text) = std::fs::read_to_string(dir.join("detections.jsonl")) else {
            continue;
        };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str::<DetectionView>(line).expect("a detection line"));
        }
    }
    assert!(out.len() > 1000, "the corpus has {} detections", out.len());
    out
}

fn json_round_trip<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
>(
    items: &[T],
    what: &str,
) {
    for (i, item) in items.iter().enumerate() {
        let text = serde_json::to_string(item).expect("serialises");
        let back: T = serde_json::from_str(&text).expect("parses");
        assert!(*item == back, "{what} #{i} did not round-trip: {item:?}");
    }
}

/// A track built from a detection, so the track corpus is as wide as the detection one.
fn track_from(i: u64, d: &DetectionView) -> TrackView {
    TrackView {
        id: gungnir_model::TrackId(i),
        status: gungnir_model::TrackStatus::Confirmed,
        state: {
            // DN-27 made `Measurement` an enum; a track is built only from the
            // detections in this corpus that carry a position, and `expect` is
            // permitted here because this is a test.
            let enu = d
                .measurement
                .position_enu()
                .expect("the corpus detection carries a position");
            nalgebra::SVector::<f64, 6>::new(enu[0], enu[1], enu[2], 1.0, -2.0, 0.5)
        },
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 4.0,
        classification: gungnir_model::Classification::Unknown,
        provenance: d.provenance.clone(),
        quality: gungnir_model::Quality::default(),
        mission_time: d.source_time,
        releasability: gungnir_model::Releasability::default(),
    }
}

/// Each catalogue entry has a check, and the suite refuses to pass an entry it does
/// not know how to check.
#[test]
#[allow(clippy::too_many_lines)]
fn every_catalogue_entry_is_checked_against_a_corpus() {
    let catalogue = SchemaCatalog::builtin();
    let detections = detection_corpus();
    let mut checked = Vec::new();
    for entry in catalogue.entries() {
        match (&entry.kind, entry.name.as_str()) {
            (SchemaKind::GungnirJson, "gungnir.DetectionView") => {
                json_round_trip(&detections, &entry.name);
            }
            (SchemaKind::GungnirJson, "gungnir.TrackView") => {
                let tracks: Vec<TrackView> = detections
                    .iter()
                    .enumerate()
                    .map(|(i, d)| track_from(i as u64, d))
                    .collect();
                json_round_trip(&tracks, &entry.name);
            }
            (SchemaKind::GungnirJson, "gungnir.PlanView") => {
                json_round_trip(&[PlanView::default()], &entry.name);
            }
            (SchemaKind::GungnirJson, "gungnir.Envelope") => {
                let envelopes: Vec<gungnir_eventing::Envelope> = detections
                    .iter()
                    .take(200)
                    .enumerate()
                    .map(|(i, d)| gungnir_eventing::Envelope {
                        seq: i as u64 + 1,
                        mission_time: d.receipt_time,
                        event: gungnir_eventing::Event::Tracking(
                            gungnir_model::events::TrackingEvent::TrackInitiated(track_from(
                                i as u64, d,
                            )),
                        ),
                    })
                    .collect();
                json_round_trip(&envelopes, &entry.name);
            }
            (SchemaKind::Arrow, gungnir_interop::dataset::CLASSIFICATION_ROWS) => {
                // GAP-079: an empty batch in the documented shape builds; the rows
                // themselves are gungnir-ml's, checked there against a committed set.
                let schema = gungnir_interop::dataset::classification_rows_schema();
                assert_eq!(schema.fields().len(), 22);
                assert_eq!(entry.version, gungnir_interop::dataset::DATASET_VERSION);
                let empty =
                    arrow::record_batch::RecordBatch::new_empty(std::sync::Arc::new(schema));
                assert_eq!(empty.num_rows(), 0);
            }
            (SchemaKind::Arrow, _) => {
                let batch = detections_to_record_batch(&detections).expect("to arrow");
                let back = record_batch_to_detections(&batch).expect("from arrow");
                assert_eq!(back.len(), detections.len());
                for (a, b) in detections.iter().zip(&back) {
                    assert_eq!(a.sensor, b.sensor);
                    assert_eq!(a.source_time, b.source_time);
                    assert_eq!(a.receipt_time, b.receipt_time);
                    assert_eq!(a.measurement, b.measurement);
                    assert_eq!(a.provenance, b.provenance, "arrow lost provenance");
                }
            }
            (SchemaKind::TextualIdentity { uuid_version, .. }, _) => {
                // Version-7 identities as `gungnir-identity` mints them (GAP-069).
                for text in [
                    "018f6f1e-2c3a-7b4d-9e8f-0a1b2c3d4e5f",
                    "01912345-6789-7abc-8def-0123456789ab",
                    "0190aaaa-bbbb-7ccc-9ddd-eeeeffff0000",
                ] {
                    let id = text.parse::<GlobalEntityId>().expect("parses");
                    assert_eq!(id.version().map(|v| v as u8), Some(*uuid_version));
                    assert_eq!(id.to_string(), text);
                    assert_eq!(GlobalEntityId::parse(text).expect("parses"), id);
                }
            }
            (SchemaKind::AsterixCategory(48), _) => asterix_048_partially_decodes(),
            (SchemaKind::AsterixCategory(34), _) => asterix_034_decodes(),
            (SchemaKind::AsterixCategory(205), _) => asterix_205_decodes_and_maps(),
            (SchemaKind::AsterixCategory(129), _) => asterix_129_decodes_and_maps(),
            (SchemaKind::Stanag(4676), _) => {
                assert!(matches!(
                    Stanag4676Codec.decode(b"", MissionTime(0.0)),
                    Err(InteropError::NotImplemented(_))
                ));
            }
            (SchemaKind::Ais { edition }, _) => {
                assert_eq!(*edition, 6);
                assert!(AisCodec::EDITION.contains("M.1371-6"));
                ais_captures_decode_and_report_what_they_skip();
            }
            (
                SchemaKind::Adsb1090Es {
                    normative_source_pinned,
                },
                _,
            ) => {
                // GAP-010. The flag must stay false while no ADS-B specification is
                // both free to obtain and permissively licensed, because a peer
                // negotiating this schema reads it as the answer to "is a decode of
                // this conformant?". Changing it is a change to
                // `docs/design/external-standards.md` §4 in the same commit.
                assert!(
                    !normative_source_pinned,
                    "no ADS-B normative source is pinned; the entry must not claim one"
                );
                assert!(gungnir_interop::adsb::SPECIFICATION.contains("none pinned"));
                adsb_capture_decodes_and_reports_what_it_carries();
            }
            (kind, name) => panic!("no conformance check for {name} ({kind:?})"),
        }
        checked.push(entry.name.clone());
    }
    assert!(checked.len() >= 10, "{checked:?}");
}

/// The public Category 048 capture: every block reads, the fields this build carries
/// raw are listed by item name on the record, and every mapped detection says what
/// the conversion lost.
fn asterix_048_partially_decodes() {
    let raw = std::fs::read(root().join("testdata/asterix/cat048.raw")).expect("capture");
    let blocks = data_blocks("conformance", &raw).expect("splits");
    assert!(!blocks.is_empty());
    let mut records = 0;
    let mut with_raw_items = 0;
    for block in blocks.iter().filter(|b| b.category == 48) {
        for record in cat048::decode_block(block).expect("decodes") {
            records += 1;
            if !record.carried_raw.is_empty() {
                with_raw_items += 1;
                for item in &record.carried_raw {
                    assert!(item.item.starts_with("I048/"), "{}", item.item);
                }
            }
        }
    }
    assert!(records > 0);
    eprintln!("cat048: {records} records, {with_raw_items} carrying unsupported items by name");
}

fn asterix_034_decodes() {
    let raw = std::fs::read(root().join("testdata/asterix/cat034.raw")).expect("capture");
    let records = cat034::decode_records(&raw).expect("decodes");
    assert!(!records.is_empty());
}

/// The hand-built Category 205 fixture (GAP-100; no real capture exists --
/// `testdata/asterix/SOURCE.md`'s Category 205 section says so): the record decodes,
/// and maps to a bearing once its site's stated accuracy is configured.
fn asterix_205_decodes_and_maps() {
    let raw = std::fs::read(root().join("testdata/asterix/cat205.raw")).expect("fixture");
    let records = cat205::decode_records(&raw).expect("decodes");
    assert_eq!(records.len(), 1);
    let site = gungnir_interop::DfSite {
        sac: 99,
        sic: 1,
        sensor: gungnir_model::SensorId(1),
        origin_enu_m: [0.0; 3],
        azimuth_sigma_rad: 2.0_f64.to_radians(),
    };
    let codec = gungnir_interop::AsterixCat205Codec::new(vec![site]);
    let dets = codec.decode(&raw, MissionTime(0.0)).expect("maps");
    assert_eq!(dets.len(), 1);
    assert!(matches!(
        dets[0].measurement,
        gungnir_model::Measurement::Bearing { .. }
    ));
}

/// The hand-built Category 129 fixture (GAP-101; no real capture exists --
/// `testdata/asterix/SOURCE.md`'s Category 129 section says so): the record decodes,
/// and maps to a UAS identification report once its gateway's SAC/SIC is configured.
fn asterix_129_decodes_and_maps() {
    let raw = std::fs::read(root().join("testdata/asterix/cat129.raw")).expect("fixture");
    let records = cat129::decode_records(&raw).expect("decodes");
    assert_eq!(records.len(), 1);
    let site = gungnir_interop::UasSite {
        sac: 0,
        sic: 0,
        sensor: gungnir_model::SensorId(1),
    };
    let codec = gungnir_interop::AsterixCat129Codec::new(vec![site]);
    let reports = codec.decode(&raw, MissionTime(0.0)).expect("maps");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].registration_country, "US");
}

/// The gpsd captures: every sentence of an in-scope type decodes, and every other type
/// is reported as unsupported with its header rather than silently dropped.
fn ais_captures_decode_and_report_what_they_skip() {
    let mut decoded = 0;
    let mut unsupported = 0;
    for name in ["ais-nmea.log", "ais-18-27.log", "ais-raw-messages.log"] {
        let text =
            std::fs::read_to_string(root().join("testdata/ais").join(name)).expect("capture");
        let mut codec = AisCodec::default();
        for line in text.lines() {
            match codec.decode_line(line) {
                Ok(Some(AisMessage::Unsupported { header, .. })) => {
                    unsupported += 1;
                    assert!(header.mmsi > 0 || header.message_type > 0);
                }
                Ok(Some(_)) => decoded += 1,
                Ok(None) | Err(gungnir_interop::ais::AisError::NotAisSentence) => {}
                Err(e) => panic!("{name}: {line}: {e}"),
            }
        }
    }
    assert!(
        decoded > 300 && unsupported > 0,
        "{decoded} decoded, {unsupported} unsupported"
    );
}

/// The vendored ADS-B capture through the codec: every extended squitter decodes, and
/// every frame that does not is **counted under a name** rather than dropped
/// (`testdata/adsb/SOURCE.md`, GAP-010).
///
/// What this does not check is agreement with anything: that is
/// `tests/adsb_fixtures.rs` against two open-source decoders, and it is agreement with
/// the open-source consensus rather than conformance. What is checked here is the
/// catalogue's own promise -- that an entry this build declares is an entry it can read
/// a corpus with, and that nothing in the corpus goes unaccounted for.
fn adsb_capture_decodes_and_reports_what_it_carries() {
    let text = std::fs::read_to_string(root().join("testdata/adsb/lax-messages-first40000.txt"))
        .expect("the vendored AVR capture");
    let mut codec = gungnir_interop::AdsbCodec::default();
    let mut decoded = 0usize;
    let mut carried = 0usize;
    for line in text.lines() {
        let Ok(octets) = gungnir_interop::adsb::parse_avr(line) else {
            panic!("{line}: the capture is AVR frames throughout");
        };
        match codec.decode_frame(&octets) {
            Ok(frame) => match frame.message() {
                Some(gungnir_interop::adsb::MeMessage::Carried { name, .. }) => {
                    assert!(!name.is_empty(), "a carried type code must be named");
                    carried += 1;
                }
                Some(_) => decoded += 1,
                None => {}
            },
            Err(e) => panic!("{line}: {e}"),
        }
    }
    let stats = codec.stats();
    assert_eq!(stats.frames, 40_000);
    assert!(
        decoded > 10_000 && carried > 1_000,
        "{decoded} decoded, {carried} carried"
    );
    let counted: u64 = stats.decoded.values().sum::<u64>() + stats.not_decoded();
    assert_eq!(
        counted, stats.frames,
        "every frame is either decoded or counted under a reason"
    );
}

/// An unsupported version is refused with the catalogue's own error, and an unknown
/// schema is named.
#[test]
fn an_unsupported_version_is_refused_with_the_catalogues_error() {
    let catalogue = SchemaCatalog::builtin();
    for entry in catalogue.entries() {
        catalogue
            .check(&entry.name, entry.version)
            .expect("the spoken version");
        match catalogue.check(&entry.name, entry.version + 1) {
            Err(InteropError::IncompatibleSchema {
                name,
                offered,
                spoken,
            }) => {
                assert_eq!(name, entry.name);
                assert_eq!((offered, spoken), (entry.version + 1, entry.version));
            }
            other => panic!("{}: {other:?}", entry.name),
        }
        assert!(!catalogue.is_compatible(&entry.name, entry.version + 1));
    }
    assert!(matches!(
        catalogue.check("gungnir.Nothing", 1),
        Err(InteropError::UnknownSchema(_))
    ));
}

/// Truncations and single-octet corruptions of the binary corpora never panic: they
/// decode or they return an error.
#[test]
fn truncated_and_corrupted_inputs_never_panic() {
    let raw = std::fs::read(root().join("testdata/asterix/cat048.raw")).expect("capture");
    for cut in (0..raw.len().min(600)).step_by(7) {
        let _ = cat048::decode_records(&raw[..cut]);
        let _ = cat034::decode_records(&raw[..cut]);
    }
    let mut corrupted = raw[..raw.len().min(400)].to_vec();
    for i in (0..corrupted.len()).step_by(5) {
        corrupted[i] ^= 0xFF;
        let _ = cat048::decode_records(&corrupted);
        corrupted[i] ^= 0xFF;
    }
    // AIS: every prefix of a real sentence, and every armour byte flipped.
    let sentence = "!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5C";
    for cut in 0..sentence.len() {
        let mut codec = AisCodec::default();
        let _ = codec.decode_line(&sentence[..cut]);
    }
    let bytes = sentence.as_bytes();
    for i in 0..bytes.len() {
        let mut b = bytes.to_vec();
        b[i] = b[i].wrapping_add(1);
        let mut codec = AisCodec::default();
        let _ = codec.decode_line(&String::from_utf8_lossy(&b));
    }
}

/// Whether a catalogue entry's zero-loss criterion is checked across a real transport
/// between two nodes (GAP-063), and when it is not, why not.
///
/// A declaration and not a measurement: the wire checks themselves are in
/// `gungnir-remote/tests/wire_conformance.rs`, which this crate cannot reach. Its value
/// is that the two files must be changed together, and that "nothing checks this over a
/// wire" is written down beside the entry rather than inferred from an absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireCoverage {
    /// Checked over a real wire, in the named test of
    /// `gungnir-remote/tests/wire_conformance.rs`.
    Covered(&'static str),
    /// Not checked over a wire, with the reason. Never "not yet" on its own.
    NotCovered(&'static str),
}

/// The declaration, entry by entry. `None` for a name nobody has decided about, which is
/// what makes a new catalogue entry fail rather than pass by default.
fn wire_coverage(name: &str) -> Option<WireCoverage> {
    Some(match name {
        "gungnir.TrackView" => WireCoverage::Covered(
            "the_track_corpus_crosses_the_snapshot_with_zero_loss",
        ),
        "gungnir.Envelope" => WireCoverage::Covered(
            "the_track_corpus_crosses_the_event_stream_with_zero_loss",
        ),
        "gungnir.DetectionView" => WireCoverage::NotCovered(
            "it crosses `POST /v2/detections`, which is an operator's write path rather than a peer's; `gungnir-remote/tests/transport.rs` checks that a submitted detection reaches the node, and nothing checks byte parity",
        ),
        "gungnir.PlanView" => WireCoverage::NotCovered(
            "a plan is a recommendation for this deployment's own effectors and no exchange item covers it, so `NodeApi::snapshot_for` withholds it from every party (DN-18 §5); it crosses no wire to a peer to be checked on",
        ),
        "gungnir.detections.arrow" | gungnir_interop::dataset::CLASSIFICATION_ROWS => {
            WireCoverage::NotCovered(
                "an Arrow dataset is written to a file for training, not served on the v2 transport; it has no producer on a wire",
            )
        }
        "gungnir.GlobalEntityId" => WireCoverage::NotCovered(
            "an identity is a value inside a document rather than a payload of its own, so it crosses the wire only as part of one that is checked",
        ),
        "asterix.cat048" | "asterix.cat034" | "asterix.cat205" | "asterix.cat129"
        | "ais.m1371" | "adsb.1090es" => {
            WireCoverage::NotCovered(
                "read from a sensor feed rather than from this transport; the decode is checked against the committed captures above, which is where the loss would be",
            )
        }
        "stanag.4676" => WireCoverage::NotCovered(
            "there is no corpus and no decoder (D-09), so there is nothing to send; this is the other half of GAP-063 and is blocked on an owner decision",
        ),
        _ => return None,
    })
}

/// Every catalogue entry says whether its zero-loss criterion is checked over a real
/// wire, and the health payload -- which is an exchange item without a catalogue entry --
/// is checked too.
///
/// The suite refuses to pass an entry nobody has decided about, which is the same rule
/// `every_catalogue_entry_is_checked_against_a_corpus` applies to corpora. Two entries
/// are covered today, `gungnir.TrackView` and `gungnir.Envelope`, which are the schemas
/// behind the only exchange items with a producer: tracks, and health.
#[test]
fn every_catalogue_entry_says_whether_it_is_checked_over_the_wire() {
    let catalogue = SchemaCatalog::builtin();
    let mut covered = Vec::new();
    for entry in catalogue.entries() {
        match wire_coverage(&entry.name) {
            None => panic!(
                "{} has no wire-coverage declaration; say in `wire_coverage` whether it is checked in `gungnir-remote/tests/wire_conformance.rs` and, if not, why",
                entry.name
            ),
            Some(WireCoverage::Covered(test)) => {
                assert!(!test.is_empty(), "{} names no test", entry.name);
                covered.push(entry.name.clone());
            }
            Some(WireCoverage::NotCovered(reason)) => {
                assert!(
                    reason.len() > 40,
                    "{}: a reason this short is an excuse, not a reason: {reason:?}",
                    entry.name
                );
            }
        }
    }
    assert_eq!(
        covered,
        vec!["gungnir.TrackView".to_owned(), "gungnir.Envelope".to_owned()],
        "the set of entries checked over the wire changed; update this file and `gungnir-remote/tests/wire_conformance.rs` together"
    );
}
