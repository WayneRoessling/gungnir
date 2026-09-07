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
    /// A machine identity was verified (D-02: mutual TLS). Not produced by any
    /// authenticator yet; the variant exists so the record can carry it when one is.
    MachineIdentity,
}
