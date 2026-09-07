//! Detector settings for the anomaly rules of docs/design/DN-15-anomaly-detectors.md.
//!
//! Here rather than beside the detectors in `gungnir-analytics`, because the baseline
//! carries them and `gungnir-config` cannot depend on the crate that runs them
//! (GAP-021). **A detector with no settings is off**, and the health panel lists which are
//! running, so an unconfigured detector is visibly absent rather than silently missing.

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct AnomalySettings {
    #[serde(default)]
    pub loitering: Option<LoiteringSettings>,
    #[serde(default)]
    pub kinematics: Option<KinematicEnvelope>,
    #[serde(default)]
    pub feed: Option<FeedSettings>,
    #[serde(default)]
    pub cooperative: Option<CooperativeSettings>,
}

impl AnomalySettings {
    /// The detectors that will actually run, for the health summary.
    #[must_use]
    pub fn enabled(&self) -> Vec<&'static str> {
        let mut on = Vec::new();
        if self.loitering.is_some() {
            on.push("loitering");
        }
        if self.kinematics.is_some() {
            on.push("implausible-kinematics");
        }
        if self.feed.is_some() {
            on.push("feed");
        }
        if self.cooperative.is_some() {
            on.push("cooperative");
        }
        on
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoiteringSettings {
    /// Speed at or below which a track counts as loitering, m/s.
    pub max_speed_mps: f64,
    /// Seconds a track must stay that slow before the detector fires.
    pub min_duration_s: f64,
}

/// The widest envelope any known class occupies. Kinematics outside it belong to no
/// class in the catalogue (docs/test-tracks/).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KinematicEnvelope {
    pub max_speed_mps: f64,
    pub max_climb_rate_mps: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FeedSettings {
    /// Multiple of the expected interval after which silence is anomalous.
    pub silence_factor: f64,
    /// Fractional departure from the baseline rate that counts as implausible.
    pub rate_tolerance: f64,
    /// Rejections in the window above which the feed is implausible.
    pub max_rejected: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CooperativeSettings {
    /// Seconds without a cooperative report after which it counts as lost.
    pub lost_after_s: f64,
    /// Separation, metres, beyond which a report and its track disagree.
    pub max_separation_m: f64,
}
