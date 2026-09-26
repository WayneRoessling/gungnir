// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Source lineage per docs/gungnir-capabilities.md §5.2 ("Data Quality,
//! Provenance & Confidence Governance").

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    /// Sensors whose observations contributed to this track or observation.
    pub source_sensor_ids: Vec<u32>,
    /// `gungnir-sensor-management` calibration baseline in force when this was produced.
    pub calibration_baseline_version: Option<String>,
    /// Version of the algorithm/configuration that produced this (`gungnir-modelops`).
    pub algorithm_version: String,
    /// Where a peer-sourced item came from and how far behind it is
    /// (docs/design/DN-16-peer-sources.md). `None` for locally observed data.
    ///
    /// A track fused from local and peer detections carries both this and the local
    /// sensor list, so the operator can see the difference.
    #[serde(default)]
    pub peer: Option<crate::PeerOrigin>,
    /// What a format conversion lost on the way in, when anything did
    /// (docs/design/DN-18-coalition-exchange.md).
    ///
    /// Conversion to and from the industry formats is lossy in both directions, and
    /// the loss is recorded here rather than assumed away.
    #[serde(default)]
    pub conversion_loss: Option<String>,
    /// How strongly the source of this observation was authenticated when the gateway
    /// admitted it (GAP-002). An accepted detection from an allow-list and one from a
    /// credential-backed source are not the same claim, and a reader of the record can
    /// tell them apart. Stamped by the gateway, never by an adapter.
    #[serde(default)]
    pub authentication: SourceAuthentication,
    /// Set on a detection a laydown rehearsal re-observed from a recording, and on
    /// nothing else (docs/design/DN-32-re-observation-for-a-laydown.md §6, mechanism 1):
    /// which recording, which laydown, which seed. A live `gungnir_ingest::IngestGateway`
    /// refuses and counts any detection carrying it; only a rehearsal's own gateway,
    /// built with `IngestGateway::for_rehearsal`, admits one (mechanism 2).
    ///
    /// Defaulted when absent, and left out when `None`, so every record, journal and
    /// fixture written before the field reads, and is written, exactly as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rehearsal: Option<RehearsalOrigin>,
}

/// Where a re-observed detection came from (DN-32 §6, mechanism 1).
///
/// `gungnir-sensor-sim` marks every observation it produces and `gungnir-app`'s laydown
/// rehearsal carries that mark here; the simulation crate sits beneath this one in the
/// dependency graph and cannot name this type, so the conversion is the app's, and it is
/// total.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RehearsalOrigin {
    /// The recording re-observed.
    pub scenario: crate::TestTrackNumber,
    /// The laydown whose sensors re-observed it.
    pub laydown: crate::LaydownId,
    /// The seed every random stream of the run derived from.
    pub seed: u64,
}

impl std::fmt::Display for RehearsalOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "re-observed from {} under laydown {} (seed {})",
            self.scenario.label(),
            self.laydown,
            self.seed
        )
    }
}

/// The strength of a source's authentication at admission (GAP-002, D-02).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceAuthentication {
    /// Nothing checked the source: replay, development, or an adapter that admits all.
    #[default]
    Unauthenticated,
    /// The sensor identifier was on the applied baseline's allow-list. Says the
    /// deployment expected this sensor; says nothing about who sent the bytes.
    AllowList,
    /// A machine identity was verified (D-02: mutual TLS). Stamped only by
    /// `gungnir_ingest::MachineIdentityAuthenticator`, for the node's machine-submission
    /// adapter; raw sensor feeds authenticate by allow-list.
    MachineIdentity,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DN-32 §11: every record written before the field reads unchanged, and a record
    /// that is not a rehearsal's is written exactly as it was before the field existed.
    #[test]
    fn the_rehearsal_mark_changes_no_record_that_does_not_carry_one() {
        let before = r#"{"source_sensor_ids":[3],"calibration_baseline_version":"cb-2026-09","algorithm_version":"tt-gen 0.1.0"}"#;
        let read: Provenance = serde_json::from_str(before).expect("a pre-field record reads");
        assert_eq!(read.rehearsal, None);
        let written = serde_json::to_string(&read).expect("writes");
        assert!(!written.contains("rehearsal"), "{written}");
    }

    #[test]
    fn a_rehearsal_mark_survives_a_round_trip_and_says_what_it_is() {
        let p = Provenance {
            rehearsal: Some(RehearsalOrigin {
                scenario: crate::TestTrackNumber(1),
                laydown: crate::LaydownId("c".into()),
                seed: 1701,
            }),
            ..Provenance::default()
        };
        let back: Provenance =
            serde_json::from_str(&serde_json::to_string(&p).expect("writes")).expect("reads");
        assert_eq!(back, p);
        assert_eq!(
            p.rehearsal.map(|r| r.to_string()).as_deref(),
            Some("re-observed from TT-01 under laydown c (seed 1701)")
        );
    }
}
