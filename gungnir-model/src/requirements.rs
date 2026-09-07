//! Collection requirements: what somebody needs to know, independent of which
//! sensor answers it.
//!
//! Design: docs/design/DN-11-sensor-control-and-tasking.md. Capability CAP-2.12;
//! mission thread MT-08 collection management.
//!
//! The type lives here because `gungnir-workflow` states requirements,
//! `gungnir-sensor-management` serves them with tasks, and the interface publishes
//! them: three crates, so the lowest one owns it (agentic-coding-standards.md §1.2).

use crate::{AssetExtent, AssetPriority, MissionTime};

/// Identifier of a collection requirement.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct RequirementId(pub u64);

/// Who concurred with tasking a requirement, or declined it.
///
/// Not a bare `Option<String>`. DN-11 §5 and the CAP-2.12 verification row both require
/// a concurrence to carry an operator, and this deployment has no operator session --
/// D-02's signed tokens are GAP-057. An absent name would then be ambiguous between
/// "nobody is signed in" and "we failed to record who", which are different facts about
/// the same record, and only one of them is a defect.
///
/// So the two are separate variants. [`Concurrence::Operator`] is the one the
/// verification criterion asks for; [`Concurrence::UnattributedRole`] is what this build
/// can actually produce, and it says so rather than putting a role name in a field an
/// auditor would read as a person.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "attribution", rename_all = "kebab-case")]
pub enum Concurrence {
    /// A signed-in operator concurred, in a role that permitted it.
    Operator { id: String, role: String },
    /// A role acted with no operator session to attribute it to (GAP-057).
    ///
    /// **Not an anonymous operator.** It records that this deployment could not say who
    /// acted, which is a statement about the deployment rather than about the person.
    UnattributedRole { role: String },
}

impl Concurrence {
    /// The operator, when one was signed in.
    ///
    /// The predicate the CAP-2.12 criterion is written against: a requirement may move
    /// to tasked only with a concurrence carrying an operator, and this is what carrying
    /// one means.
    #[must_use]
    pub fn operator(&self) -> Option<&str> {
        match self {
            Concurrence::Operator { id, .. } => Some(id),
            Concurrence::UnattributedRole { .. } => None,
        }
    }

    /// The role, which is known either way.
    #[must_use]
    pub fn role(&self) -> &str {
        match self {
            Concurrence::Operator { role, .. } | Concurrence::UnattributedRole { role } => role,
        }
    }
}

/// A collection requirement, owned by the intelligence analyst (MT-08).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CollectionRequirement {
    pub id: RequirementId,
    pub title: String,
    /// How much this one matters, so a list of them can be read in order.
    ///
    /// Shares `AssetPriority` with defended assets rather than introducing a parallel
    /// scale: two ranking vocabularies for the same operator to hold in their head is
    /// how "high" comes to mean different things on two screens.
    #[serde(default)]
    pub priority: AssetPriority,
    pub area: AssetExtent,
    pub needed_by: Option<MissionTime>,
    pub state: RequirementState,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum RequirementState {
    /// Stated, not yet matched to a sensor task.
    Stated,
    /// A sensor task exists and the sensor manager concurred.
    ///
    /// **Both halves are required**, and `gungnir_workflow::TaskingCase::concur` refuses
    /// without a task: a requirement marked tasked with nothing serving it would read,
    /// to the analyst who stated it, as work in hand.
    Tasked { by: Concurrence },
    /// Answered, with the evidence referenced.
    ///
    /// **Never automatic.** A task acknowledged is not an answer, and inferring one
    /// would be the same error as inferring effect from a track deletion.
    Satisfied { evidence: String },
    /// The sensor manager declined, with a reason.
    Declined { by: Concurrence, reason: String },
    /// The time passed without an answer.
    Lapsed,
}

impl CollectionRequirement {
    /// True when the requirement can still be worked.
    pub fn is_open(&self) -> bool {
        matches!(
            self.state,
            RequirementState::Stated | RequirementState::Tasked { .. }
        )
    }

    /// True when its needed-by time has passed with it still open.
    pub fn has_lapsed(&self, now: MissionTime) -> bool {
        self.is_open() && self.needed_by.is_some_and(|by| now > by)
    }
}

/// Lapses every open requirement past its needed-by time.
pub fn lapse_overdue(
    requirements: &mut [CollectionRequirement],
    now: MissionTime,
) -> Vec<RequirementId> {
    let mut lapsed = Vec::new();
    for r in requirements.iter_mut().filter(|r| r.has_lapsed(now)) {
        r.state = RequirementState::Lapsed;
        lapsed.push(r.id);
    }
    lapsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Geodetic;

    fn requirement(id: u64, needed_by: Option<f64>) -> CollectionRequirement {
        CollectionRequirement {
            id: RequirementId(id),
            title: "identify the contact".into(),
            priority: AssetPriority::Medium,
            area: AssetExtent::Point {
                position: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
            },
            needed_by: needed_by.map(MissionTime),
            state: RequirementState::Stated,
        }
    }

    #[test]
    fn a_requirement_is_never_satisfied_automatically() {
        // A task acknowledged is not an answer. Satisfaction always names evidence.
        let mut r = requirement(1, None);
        assert!(r.is_open());
        r.state = RequirementState::Tasked {
            by: Concurrence::Operator {
                id: "j.okonkwo".into(),
                role: "SensorManager".into(),
            },
        };
        assert!(r.is_open(), "tasked is not answered");
        r.state = RequirementState::Satisfied {
            evidence: "track 42 identified from imagery".into(),
        };
        assert!(!r.is_open());
        match &r.state {
            RequirementState::Satisfied { evidence } => assert!(!evidence.is_empty()),
            other => panic!("expected satisfaction with evidence, got {other:?}"),
        }
    }

    #[test]
    fn an_overdue_requirement_lapses_rather_than_remaining_open() {
        let mut requirements = vec![requirement(1, Some(200.0)), requirement(2, None)];
        assert!(lapse_overdue(&mut requirements, MissionTime(150.0)).is_empty());

        let lapsed = lapse_overdue(&mut requirements, MissionTime(250.0));
        assert_eq!(lapsed, vec![RequirementId(1)]);
        assert_eq!(requirements[0].state, RequirementState::Lapsed);
        assert!(
            requirements[1].is_open(),
            "one with no needed-by time never lapses"
        );
    }

    /// The distinction the `Concurrence` type exists for. A role that acted with
    /// nobody signed in is not an operator, and the CAP-2.12 criterion is written
    /// against operators -- so the predicate has to be able to tell them apart rather
    /// than reading a role name out of a field an auditor would take for a person.
    #[test]
    fn an_unattributed_role_does_not_count_as_carrying_an_operator() {
        let signed_in = Concurrence::Operator {
            id: "j.okonkwo".into(),
            role: "SensorManager".into(),
        };
        let nobody = Concurrence::UnattributedRole {
            role: "SensorManager".into(),
        };
        assert_eq!(signed_in.operator(), Some("j.okonkwo"));
        assert_eq!(
            nobody.operator(),
            None,
            "a role with no operator session was counted as an operator"
        );
        // The role is known either way, which is what makes the second variant worth
        // recording at all rather than refusing the act.
        assert_eq!(signed_in.role(), nobody.role());
    }

    #[test]
    fn a_declined_requirement_carries_who_declined_it_and_why() {
        let mut r = requirement(1, None);
        r.state = RequirementState::Declined {
            by: Concurrence::UnattributedRole {
                role: "SensorManager".into(),
            },
            reason: "no sensor can reach that area".into(),
        };
        assert!(!r.is_open());
        assert!(
            !r.has_lapsed(MissionTime(1_000_000.0)),
            "a closed one cannot lapse"
        );
    }
}
