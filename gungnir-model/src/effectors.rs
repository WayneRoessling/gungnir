//! Effector layers, relative cost, and magazines: what the cheapest-adequate rule
//! reads before it recommends anything.
//!
//! Design: docs/design/DN-04-effector-model.md. Capability CAP-3.3.
//!
//! `layer` is mandatory and `cost` is optional because MOE-03 in
//! docs/mission/measures.md is defined by layer and not by money: "fraction of
//! propeller-drone engagements made by the point or self-defense layers rather than
//! area-defense interceptors". A currency figure would put a procurement question in
//! front of every customer before the system could compute its own headline measure.

/// Defence layer, ordered outermost first.
///
/// MOE-03 is defined by this field, so it has no default: a baseline that omits it
/// is rejected rather than guessed at (docs/design/DN-04-effector-model.md §5).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum EffectorLayer {
    Area,
    Point,
    SelfDefence,
    /// Jammers, spoofers, directed effects. Distinct because policy and warning
    /// obligations differ, not because the geometry does.
    NonKinetic,
}

impl EffectorLayer {
    /// Parses the baseline's spelling. `None` for anything unrecognized; the caller
    /// rejects the baseline rather than defaulting, because a wrong layer silently
    /// corrupts MOE-03.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "area" => Some(EffectorLayer::Area),
            "point" => Some(EffectorLayer::Point),
            "self-defence" | "self-defense" => Some(EffectorLayer::SelfDefence),
            "non-kinetic" => Some(EffectorLayer::NonKinetic),
            _ => None,
        }
    }

    /// True for the layers MOE-03 counts as the preferred answer to a propeller
    /// drone: point and self-defence.
    pub fn is_inner_kinetic(self) -> bool {
        matches!(self, EffectorLayer::Point | EffectorLayer::SelfDefence)
    }
}

/// What one round of this resource costs relative to the others in the same
/// deployment. Unitless by design: it only ever breaks ties between resources of
/// the same layer that are both adequate.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct RelativeCost(pub f64);

impl Default for RelativeCost {
    /// 1.0 means "no preference expressed".
    fn default() -> Self {
        RelativeCost(1.0)
    }
}

/// Rounds held and rounds withheld.
///
/// Optional on a resource because a non-kinetic effector and a sensor-cued camera
/// have no rounds, and modelling them with a fictitious count would make
/// [`Magazine::allocatable`] meaningless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Magazine {
    pub rounds_available: u32,
    /// Rounds held back from automatic recommendation. A plan may not propose a
    /// resource whose remaining rounds are at or below this; eating the reserve is
    /// a decision for a person (docs/design/DN-04-effector-model.md §5, rule 4).
    pub reserve: u32,
}

impl Magazine {
    /// Rounds a recommendation may plan against.
    pub fn allocatable(&self) -> u32 {
        self.rounds_available.saturating_sub(self.reserve)
    }

    /// True when the reserve is intact and something remains to allocate.
    pub fn has_allocatable(&self) -> bool {
        self.allocatable() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_are_ordered_outermost_first() {
        assert!(EffectorLayer::Area < EffectorLayer::Point);
        assert!(EffectorLayer::Point < EffectorLayer::SelfDefence);
        assert!(!EffectorLayer::Area.is_inner_kinetic());
        assert!(EffectorLayer::Point.is_inner_kinetic());
        assert!(EffectorLayer::SelfDefence.is_inner_kinetic());
        assert!(!EffectorLayer::NonKinetic.is_inner_kinetic());
    }

    #[test]
    fn an_unknown_layer_does_not_default() {
        assert_eq!(EffectorLayer::parse("point"), Some(EffectorLayer::Point));
        assert_eq!(
            EffectorLayer::parse("self_defense"),
            Some(EffectorLayer::SelfDefence)
        );
        assert_eq!(EffectorLayer::parse("gun"), None);
        assert_eq!(EffectorLayer::parse(""), None);
    }

    #[test]
    fn allocatable_never_eats_the_reserve() {
        let m = Magazine {
            rounds_available: 10,
            reserve: 4,
        };
        assert_eq!(m.allocatable(), 6);
        assert!(m.has_allocatable());

        let at_reserve = Magazine {
            rounds_available: 4,
            reserve: 4,
        };
        assert_eq!(at_reserve.allocatable(), 0);
        assert!(!at_reserve.has_allocatable());

        // Saturating rather than panicking: a baseline with reserve > available is
        // rejected by validation, and this type must not panic if one slips past.
        let inverted = Magazine {
            rounds_available: 1,
            reserve: 4,
        };
        assert_eq!(inverted.allocatable(), 0);
    }

    #[test]
    fn cost_defaults_to_no_preference() {
        assert!((RelativeCost::default().0 - 1.0).abs() < f64::EPSILON);
    }
}
