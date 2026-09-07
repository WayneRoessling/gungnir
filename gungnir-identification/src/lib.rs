//! Classification & identification, per docs/gungnir-capabilities.md §5.3.
//! Every track in gungnir-model is purely kinematic by default -- this crate is
//! what actually sets `Classification` from evidence, feeding both the dashboard
//! and gungnir-intercept-service's candidate list (which otherwise recommends
//! resources against tracks indiscriminately).

use gungnir_model::{Classification, IdentificationSettings, TrackId};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct IdentificationEvidence {
    pub track_id: TrackId,
    /// e.g. "IFF interrogation", "kinematic profile", "operator-designated".
    pub source: String,
    pub suggested: Classification,
    /// 0.0 .. 1.0
    pub confidence: f32,
}

pub trait IdentificationEngine: Send + Sync {
    fn submit_evidence(&mut self, evidence: IdentificationEvidence);
    /// Fused classification for a track from all evidence received so far.
    fn classification(&self, track_id: TrackId) -> Classification;
}

/// What the engine concluded for a track, with what it could not conclude kept apart.
///
/// `Unknown` is not a class the evidence chose; it is the absence of a conclusion, and
/// the reason is on the variant so PN-04 can say it (DN-08 §5).
#[derive(Debug, Clone, PartialEq)]
pub enum IdentificationDecision {
    /// The evidence cleared the class's own threshold and led by the margin.
    Declared(Classification),
    /// The leading class needs a person: it is on the confirm list, or no threshold is
    /// configured for it and none is guessed. `confidence` is the fused value the person
    /// is shown.
    NeedsOperator {
        class: Classification,
        confidence: f32,
    },
    /// No evidence, or the leading class is below its threshold or within the margin of
    /// the runner-up.
    Unknown { reason: String },
}

impl IdentificationDecision {
    /// The classification the picture may carry: declared or nothing.
    #[must_use]
    pub fn classification(&self) -> Classification {
        match self {
            IdentificationDecision::Declared(c) => *c,
            IdentificationDecision::NeedsOperator { .. }
            | IdentificationDecision::Unknown { .. } => Classification::Unknown,
        }
    }
}

/// The class names the policy configuration spells (DN-08 §6: the same words the
/// thresholds table uses).
fn class_name(class: Classification) -> &'static str {
    match class {
        Classification::Friendly => "friendly",
        Classification::Hostile => "hostile",
        Classification::Neutral => "neutral",
        Classification::Unknown => "unknown",
    }
}

/// Fuses evidence per class and declares the leader when it clears the bar.
///
/// Two bars, and which one applies is a configuration decision:
///
/// - **With settings** (`with_settings`, GAP-018): confidence per class is the noisy-OR
///   of the evidence for it (`1 - Π(1 - c)`), so ten weak reports agree into a strong
///   one without exceeding 1.0; the leader is declared when it clears **its own**
///   threshold from `IdentificationSettings::thresholds` and leads the runner-up by
///   `minimum_margin`; a class on the confirm list, or with no threshold configured, is
///   handed to a person instead (`requires_operator`). A threshold is never guessed.
/// - **Without settings** (`new`, the original rule): the sums of confidence per class,
///   the largest declared when it exceeds the runner-up by `decision_margin`.
#[derive(Debug)]
pub struct EvidenceFusionEngine {
    pub decision_margin: f32,
    settings: Option<IdentificationSettings>,
    evidence: HashMap<TrackId, Vec<IdentificationEvidence>>,
}

impl Default for EvidenceFusionEngine {
    /// The original rule with its original margin, for an engine built without settings.
    fn default() -> Self {
        Self::new(0.25)
    }
}

impl EvidenceFusionEngine {
    pub fn new(decision_margin: f32) -> Self {
        Self {
            decision_margin,
            settings: None,
            evidence: HashMap::new(),
        }
    }

    /// The engine as the policy configuration governs it (GAP-018).
    #[must_use]
    pub fn with_settings(settings: IdentificationSettings) -> Self {
        Self {
            #[allow(clippy::cast_possible_truncation)]
            decision_margin: settings.minimum_margin as f32,
            settings: Some(settings),
            evidence: HashMap::new(),
        }
    }

    pub fn evidence_for(&self, track_id: TrackId) -> &[IdentificationEvidence] {
        self.evidence.get(&track_id).map_or(&[], Vec::as_slice)
    }

    /// Fused confidence per class: noisy-OR under settings, plain sums without.
    fn fused(&self, items: &[IdentificationEvidence]) -> [(Classification, f32); 3] {
        let mut totals: [(Classification, f32); 3] = [
            (Classification::Friendly, 0.0),
            (Classification::Hostile, 0.0),
            (Classification::Neutral, 0.0),
        ];
        let noisy_or = self.settings.is_some();
        if noisy_or {
            for slot in &mut totals {
                slot.1 = 1.0;
            }
        }
        for e in items {
            if let Some(slot) = totals.iter_mut().find(|(c, _)| *c == e.suggested) {
                let c = e.confidence.clamp(0.0, 1.0);
                if noisy_or {
                    slot.1 *= 1.0 - c;
                } else {
                    slot.1 += c;
                }
            }
        }
        if noisy_or {
            for slot in &mut totals {
                slot.1 = 1.0 - slot.1;
            }
        }
        totals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        totals
    }

    /// The conclusion for a track, with the reason when there is none.
    #[must_use]
    pub fn decide(&self, track_id: TrackId) -> IdentificationDecision {
        let Some(items) = self.evidence.get(&track_id) else {
            return IdentificationDecision::Unknown {
                reason: "no evidence".into(),
            };
        };
        let totals = self.fused(items);
        let (best, best_total) = totals[0];
        let runner_up = totals[1].1;
        if best_total <= 0.0 {
            return IdentificationDecision::Unknown {
                reason: "no evidence".into(),
            };
        }
        let Some(settings) = &self.settings else {
            return if best_total - runner_up >= self.decision_margin {
                IdentificationDecision::Declared(best)
            } else {
                IdentificationDecision::Unknown {
                    reason: format!(
                        "{} leads {} by {:.2}, under the margin {:.2}",
                        class_name(best),
                        class_name(totals[1].0),
                        best_total - runner_up,
                        self.decision_margin
                    ),
                }
            };
        };
        let name = class_name(best);
        if settings.requires_operator(name) {
            return IdentificationDecision::NeedsOperator {
                class: best,
                confidence: best_total,
            };
        }
        // `requires_operator` was false, so a threshold is configured.
        let threshold = settings.threshold_for(name).unwrap_or(1.0);
        #[allow(clippy::cast_possible_truncation)]
        let threshold = threshold as f32;
        if best_total < threshold {
            return IdentificationDecision::Unknown {
                reason: format!("{name} at {best_total:.2} is under its threshold {threshold:.2}"),
            };
        }
        if best_total - runner_up < self.decision_margin {
            return IdentificationDecision::Unknown {
                reason: format!(
                    "{name} leads {} by {:.2}, under the margin {:.2}",
                    class_name(totals[1].0),
                    best_total - runner_up,
                    self.decision_margin
                ),
            };
        }
        IdentificationDecision::Declared(best)
    }
}

impl IdentificationEngine for EvidenceFusionEngine {
    fn submit_evidence(&mut self, evidence: IdentificationEvidence) {
        self.evidence
            .entry(evidence.track_id)
            .or_default()
            .push(evidence);
    }

    fn classification(&self, track_id: TrackId) -> Classification {
        self.decide(track_id).classification()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(track: u64, class: Classification, conf: f32) -> IdentificationEvidence {
        IdentificationEvidence {
            track_id: TrackId(track),
            source: "test".into(),
            suggested: class,
            confidence: conf,
        }
    }

    #[test]
    fn no_evidence_is_unknown() {
        assert_eq!(
            EvidenceFusionEngine::default().classification(TrackId(1)),
            Classification::Unknown
        );
    }

    #[test]
    fn strongest_class_wins_when_margin_is_clear() {
        let mut e = EvidenceFusionEngine::default();
        e.submit_evidence(ev(1, Classification::Hostile, 0.6));
        e.submit_evidence(ev(1, Classification::Hostile, 0.3));
        e.submit_evidence(ev(1, Classification::Friendly, 0.2));
        assert_eq!(e.classification(TrackId(1)), Classification::Hostile);
    }

    #[test]
    fn conflicting_evidence_stays_unknown() {
        let mut e = EvidenceFusionEngine::default();
        e.submit_evidence(ev(1, Classification::Hostile, 0.5));
        e.submit_evidence(ev(1, Classification::Friendly, 0.45));
        assert_eq!(e.classification(TrackId(1)), Classification::Unknown);
    }
}

#[cfg(test)]
mod per_class_tests {
    use super::*;

    fn ev(track: u64, class: Classification, conf: f32) -> IdentificationEvidence {
        IdentificationEvidence {
            track_id: TrackId(track),
            source: "fixture".into(),
            suggested: class,
            confidence: conf,
        }
    }

    fn settings() -> IdentificationSettings {
        let mut s = IdentificationSettings::default();
        // A hostile call needs more than a friendly one (DN-08 §5): the cost of being
        // wrong differs by class, and so does the bar.
        s.thresholds.insert("hostile".into(), 0.9);
        s.thresholds.insert("friendly".into(), 0.6);
        s.minimum_margin = 0.2;
        s.operator_confirms.push("neutral".into());
        s
    }

    /// The fixture: evidence sets and what each must conclude.
    #[test]
    fn fixture_sets_conclude_as_expected() {
        let mut engine = EvidenceFusionEngine::with_settings(settings());
        // Track 1: two independent hostile reports, 0.8 each -> noisy-OR 0.96, over 0.9.
        engine.submit_evidence(ev(1, Classification::Hostile, 0.8));
        engine.submit_evidence(ev(1, Classification::Hostile, 0.8));
        // Track 2: one hostile report at 0.8 -> under the hostile bar.
        engine.submit_evidence(ev(2, Classification::Hostile, 0.8));
        // Track 3: friendly 0.7 (over its bar) but hostile 0.6 within the margin.
        engine.submit_evidence(ev(3, Classification::Friendly, 0.7));
        engine.submit_evidence(ev(3, Classification::Hostile, 0.6));
        // Track 4: neutral 0.99 -> on the confirm list, a person decides.
        engine.submit_evidence(ev(4, Classification::Neutral, 0.99));
        // Track 5: friendly 0.7 alone -> declared.
        engine.submit_evidence(ev(5, Classification::Friendly, 0.7));

        assert_eq!(
            engine.decide(TrackId(1)),
            IdentificationDecision::Declared(Classification::Hostile)
        );
        assert!(matches!(
            engine.decide(TrackId(2)),
            IdentificationDecision::Unknown { reason } if reason.contains("under its threshold 0.90")
        ));
        assert!(matches!(
            engine.decide(TrackId(3)),
            IdentificationDecision::Unknown { reason } if reason.contains("under the margin")
        ));
        assert!(matches!(
            engine.decide(TrackId(4)),
            IdentificationDecision::NeedsOperator {
                class: Classification::Neutral,
                ..
            }
        ));
        assert_eq!(
            engine.decide(TrackId(5)),
            IdentificationDecision::Declared(Classification::Friendly)
        );
        assert_eq!(engine.classification(TrackId(1)), Classification::Hostile);
        assert_eq!(engine.classification(TrackId(4)), Classification::Unknown);
    }

    /// A class with no configured threshold is never declared, however confident the
    /// evidence: no threshold is guessed (DN-08 §5).
    #[test]
    fn a_class_with_no_threshold_goes_to_a_person() {
        let mut s = IdentificationSettings::default();
        s.thresholds.insert("friendly".into(), 0.5);
        let mut engine = EvidenceFusionEngine::with_settings(s);
        engine.submit_evidence(ev(1, Classification::Hostile, 0.99));
        assert!(matches!(
            engine.decide(TrackId(1)),
            IdentificationDecision::NeedsOperator { class: Classification::Hostile, confidence } if confidence > 0.98
        ));
    }

    /// Noisy-OR never exceeds one, so a class cannot be argued past certainty by volume.
    #[test]
    fn fused_confidence_stays_within_one() {
        let mut engine = EvidenceFusionEngine::with_settings(settings());
        for _ in 0..50 {
            engine.submit_evidence(ev(1, Classification::Hostile, 0.5));
        }
        match engine.decide(TrackId(1)) {
            IdentificationDecision::Declared(Classification::Hostile) => {}
            other => panic!("{other:?}"),
        }
        let totals = engine.fused(engine.evidence_for(TrackId(1)));
        assert!(totals[0].1 <= 1.0 && totals[0].1 > 0.999);
    }

    /// The original rule is unchanged for an engine built without settings.
    #[test]
    fn the_margin_only_rule_still_holds_without_settings() {
        let mut engine = EvidenceFusionEngine::new(0.25);
        engine.submit_evidence(ev(1, Classification::Hostile, 0.9));
        engine.submit_evidence(ev(1, Classification::Friendly, 0.5));
        assert_eq!(engine.classification(TrackId(1)), Classification::Hostile);
    }
}
