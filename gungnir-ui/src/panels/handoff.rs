//! The handoff rows PN-06 and PN-20 both draw (GAP-040; DN-07 §5 and §7;
//! `docs/ux/ux-to-code-map.md`, PN-06 and PN-20).
//!
//! Two panels ask two different questions of one record. PN-06 asks *what is still on
//! somebody's plate*, and answers it while the operator is still at the console. PN-20
//! asks *what was handed off, to whom, when, and what came back*, after the fact. If the
//! wording, the colour and the age arithmetic were written twice, the operator and the
//! administrator would describe the same handoff in two different ways and the
//! after-action account would not match what anybody saw at the time. So they are
//! written once here, and each panel decides only which rows it draws.
//!
//! # "Needs attention" and "stays visible" are different questions
//!
//! `DeliveryState::needs_attention` is about distress: something has gone wrong that a
//! person must chase. It answers false for [`DeliveryState::Manual`], and that is right
//! -- a deployment with no integrated effector is a working deployment, not a broken
//! one, and flagging every radio call as an incident would teach the operator to ignore
//! the flag on the day it means something.
//!
//! [`stays_visible`] is about completion: is this decision finished from the operator's
//! side? A manual handoff is not. Nothing has reached an effector, somebody still has to
//! key the radio, and the approval queue that held the item emptied the instant the
//! decision was recorded. Driving visibility off `needs_attention` would make the honest
//! default -- the arrangement most deployments actually run -- the one case that
//! silently vanished from the screen. The rule is therefore delivery, not distress.
//!
//! # What is not here
//!
//! An effector that reports back is owner decision D-08 and real hardware. The inbound
//! route exists (`POST /v2/handoffs/{decision_id}/report`, applied by
//! `gungnir-app`'s `handoffs::apply_report`), so an empty "reported back" column is not
//! a missing feature in this desktop; it is the absence of an effector that speaks it.
//! [`NO_REPORTS_YET`] is the sentence that keeps those two apart on screen.

use crate::theme;
use egui::Color32;
use gungnir_model::handoff::{DeliveryState, EffectorReport};
use gungnir_model::MissionTime;

/// Why the "reported back" column can be empty for every row (GAP-040, D-08).
///
/// An administrator reading a blank column has to be able to tell "this desktop cannot
/// receive a report" from "nothing has sent one", and only the second is true. Stating
/// the first would understate the build; letting the blank speak for itself would let an
/// administrator conclude the effectors are silent when none was ever fielded.
pub const NO_REPORTS_YET: &str = "No effector has reported on this desktop. The route \
    that carries a report is built; whether a deployment fields an effector that speaks \
    it is D-08, an owner decision, and not a claim this panel can make.";

/// One issued handoff, as both panels read it.
///
/// Everything is borrowed from `gungnir-app`'s handoff record for the frame, which is
/// what keeps this crate free of a second copy of mission state
/// (`rust-ui-architecture-coding-standards.md` §2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandoffRow<'a> {
    pub decision: u64,
    pub plan: u64,
    /// `None` is the honest default rather than a missing value: no endpoint is
    /// configured for the tasked resource, so delivery is a radio call.
    pub endpoint: Option<&'a str>,
    /// Who decided, in the words the record holds. `Handoff::from_decision` is handed
    /// "nobody signed in" when nobody was, so this is never an invented name (DN-23 §5).
    pub operator: &'a str,
    /// The role that was selected when the decision was recorded.
    pub role: &'a str,
    pub issued: MissionTime,
    pub delivery: &'a DeliveryState,
    /// What the effector reported back, oldest first. Empty until one does.
    pub reports: &'a [EffectorReport],
}

/// Whether a decided handoff must still be in front of the operator.
///
/// This is deliberately **not** `DeliveryState::needs_attention`, and the difference is
/// the whole of PN-06's rule; the module documentation above says why the two questions
/// come apart on [`DeliveryState::Manual`]. Everything stays until the effector has it.
#[must_use]
pub fn stays_visible(delivery: &DeliveryState) -> bool {
    !delivery.is_delivered()
}

/// The one-word state, for a column somebody scans rather than reads.
///
/// "By voice" rather than "manual" or, worse, "failed": the word names what the operator
/// does, and no reading of it suggests something went wrong.
#[must_use]
pub fn delivery_label(delivery: &DeliveryState) -> &'static str {
    match delivery {
        DeliveryState::Manual => "by voice",
        DeliveryState::Delivered { .. } => "delivered",
        DeliveryState::Undelivered { .. } => "undelivered",
        DeliveryState::Refused { .. } => "refused",
    }
}

/// The colour delivery is drawn in.
///
/// **Manual is never an error colour.** It is body text: not a warning, because a radio
/// call is how this deployment is arranged, and not muted, because it is still work
/// somebody owes. Only the two states that mean an effector was expected and did not get
/// it -- silence and refusal -- are drawn as trouble, and they are drawn as different
/// degrees of it, because a refusal is an answer and silence is not.
#[must_use]
pub fn delivery_color(delivery: &DeliveryState) -> Color32 {
    match delivery {
        DeliveryState::Manual => theme::TEXT_PRIMARY,
        DeliveryState::Delivered { .. } => theme::MUTED_TEXT_COLOR,
        DeliveryState::Undelivered { .. } => theme::WARNING_COLOR,
        DeliveryState::Refused { .. } => theme::ALERT_COLOR,
    }
}

/// Where delivery stands, in words, with the age of anything still waiting.
#[must_use]
pub fn delivery_sentence(row: &HandoffRow<'_>, now: MissionTime) -> String {
    let to = row.endpoint.unwrap_or("an unnamed endpoint");
    match row.delivery {
        DeliveryState::Manual => "By voice: no endpoint is configured for the tasked \
             resource, so this handoff is a radio call somebody makes. That is the \
             arrangement and not a fault, and nothing has reached an effector."
            .to_owned(),
        DeliveryState::Delivered { at } => {
            format!("Delivered to {to} at T+{:.0} s.", at.0)
        }
        DeliveryState::Refused { reason, at } => format!(
            "Refused by {to} at T+{:.0} s: {reason}. The decision stands; what failed is \
             delivery.",
            at.0
        ),
        DeliveryState::Undelivered { since } => match age_s(now, *since) {
            Some(age) => format!(
                "Undelivered to {to} for {age:.0} s, since T+{:.0} s. Queued and retried, \
                 never dropped and never shown as delivered.",
                since.0
            ),
            None => format!(
                "Undelivered to {to} since T+{:.0} s, which the mission clock has not \
                 reached, so no age is shown.",
                since.0
            ),
        },
    }
}

/// What came back, in words.
#[must_use]
pub fn report_sentence(report: &EffectorReport) -> String {
    match report {
        EffectorReport::Acknowledged { at } => format!("Acknowledged at T+{:.0} s.", at.0),
        EffectorReport::Executing { at } => format!("Executing at T+{:.0} s.", at.0),
        EffectorReport::Completed {
            at,
            effective,
            detail,
        } => format!(
            "Completed at T+{:.0} s, {}: {detail}",
            at.0,
            if *effective {
                "effective"
            } else {
                "ineffective"
            }
        ),
        EffectorReport::Refused { at, reason } => {
            format!("Refused at T+{:.0} s: {reason}", at.0)
        }
    }
}

/// Why nothing has come back for this row.
///
/// The two silences are different facts. A radio call has no return path in this system
/// at all, so "nothing reported" would imply an endpoint that owes an answer and has not
/// given one. An endpoint that has said nothing may still speak.
#[must_use]
pub fn nothing_reported_sentence(row: &HandoffRow<'_>) -> String {
    match row.endpoint {
        None => "Nothing can come back: a radio call has no return path, and the \
                 operator's word on the net is the record."
            .to_owned(),
        Some(endpoint) => format!("{endpoint} has reported nothing back."),
    }
}

/// The age of a waiting handoff, or `None` when the clock has not passed the stamp.
///
/// A negative age means the mission clock is behind the stamp -- a resumed journal, a
/// corrected time source -- and both of the obvious renderings of that are claims
/// nothing supports: `-40 s` reads as a countdown, and clamping to zero reads as a
/// handoff issued this instant. The caller draws the stamp itself instead.
fn age_s(now: MissionTime, since: MissionTime) -> Option<f64> {
    let age = now.0 - since.0;
    (age >= 0.0).then_some(age)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(endpoint: Option<&'a str>, delivery: &'a DeliveryState) -> HandoffRow<'a> {
        HandoffRow {
            decision: 4,
            plan: 9,
            endpoint,
            operator: "nobody signed in",
            role: "Operator",
            issued: MissionTime(100.0),
            delivery,
            reports: &[],
        }
    }

    /// PN-06's rule, and the distinction it turns on. A manual handoff needs nobody's
    /// attention and is not finished either; if the two questions were one question the
    /// radio call would be the case that disappeared.
    #[test]
    fn a_manual_handoff_stays_visible_although_it_needs_no_attention() {
        let manual = DeliveryState::Manual;
        assert!(
            !manual.needs_attention(),
            "a deployment with no integrated effector is not an incident"
        );
        assert!(
            stays_visible(&manual),
            "nothing has reached an effector, so the operator still owes the call"
        );
    }

    /// Delivery is the only thing that takes a handoff off PN-06. Refusal and silence
    /// stay for the obvious reason; manual stays for the reason above.
    #[test]
    fn only_a_delivered_handoff_leaves_the_operators_view() {
        for state in [
            DeliveryState::Manual,
            DeliveryState::Undelivered {
                since: MissionTime(1.0),
            },
            DeliveryState::Refused {
                reason: "no rounds".into(),
                at: MissionTime(2.0),
            },
        ] {
            assert!(stays_visible(&state), "{state:?} left the view undelivered");
        }
        assert!(!stays_visible(&DeliveryState::Delivered {
            at: MissionTime(3.0)
        }));
    }

    /// An operator deciding whether to chase an endpoint needs the wait, not the stamp
    /// alone; a stamp on its own has to be subtracted from the clock in the reader's
    /// head, which is the arithmetic that gets skipped under load.
    #[test]
    fn an_undelivered_handoff_carries_its_age() {
        let delivery = DeliveryState::Undelivered {
            since: MissionTime(100.0),
        };
        let sentence = delivery_sentence(&row(Some("battery-2"), &delivery), MissionTime(142.0));
        assert!(sentence.contains("42 s"), "{sentence}");
        assert!(sentence.contains("battery-2"), "{sentence}");
        assert!(
            sentence.contains("never shown as delivered"),
            "the queued state has to say what it is not: {sentence}"
        );
    }

    /// A clock behind the stamp is not a wait of zero seconds and is not a countdown.
    #[test]
    fn a_clock_behind_the_stamp_reports_no_age_rather_than_a_wrong_one() {
        assert_eq!(age_s(MissionTime(140.0), MissionTime(100.0)), Some(40.0));
        assert_eq!(age_s(MissionTime(90.0), MissionTime(100.0)), None);
        let delivery = DeliveryState::Undelivered {
            since: MissionTime(100.0),
        };
        let sentence = delivery_sentence(&row(Some("battery-2"), &delivery), MissionTime(90.0));
        assert!(sentence.contains("no age is shown"), "{sentence}");
        assert!(
            !sentence.contains("-10 s"),
            "a negative age reached the screen: {sentence}"
        );
    }

    /// The word and the colour both have to read as an arrangement rather than a fault,
    /// because an operator scanning the column reads the colour first and the word
    /// second.
    #[test]
    fn manual_delivery_is_never_drawn_as_an_error() {
        let manual = DeliveryState::Manual;
        assert_eq!(delivery_label(&manual), "by voice");
        assert_ne!(delivery_color(&manual), theme::ALERT_COLOR);
        assert_ne!(delivery_color(&manual), theme::WARNING_COLOR);
        let sentence = delivery_sentence(&row(None, &manual), MissionTime(200.0));
        assert!(sentence.contains("not a fault"), "{sentence}");
        assert!(
            sentence.contains("nothing has reached an effector"),
            "saying it is fine without saying what has not happened is reassurance: \
             {sentence}"
        );
    }

    /// A refusal and a silence are both trouble and are not the same trouble: one is an
    /// answer to act on, the other is a wait to chase.
    #[test]
    fn a_refusal_and_a_silence_are_drawn_apart() {
        let refused = DeliveryState::Refused {
            reason: "unit not ready".into(),
            at: MissionTime(120.0),
        };
        let silent = DeliveryState::Undelivered {
            since: MissionTime(120.0),
        };
        assert_ne!(delivery_color(&refused), delivery_color(&silent));
        let sentence = delivery_sentence(&row(Some("battery-2"), &refused), MissionTime(200.0));
        assert!(sentence.contains("unit not ready"), "{sentence}");
        assert!(
            sentence.contains("The decision stands"),
            "a refused delivery is not a reversed decision: {sentence}"
        );
    }

    /// The empty "reported back" cell means two different things and has to say which.
    #[test]
    fn the_two_silences_after_a_handoff_are_told_apart() {
        let manual = DeliveryState::Manual;
        let queued = DeliveryState::Undelivered {
            since: MissionTime(1.0),
        };
        let by_voice = nothing_reported_sentence(&row(None, &manual));
        let by_wire = nothing_reported_sentence(&row(Some("battery-2"), &queued));
        assert_ne!(by_voice, by_wire);
        assert!(by_voice.contains("no return path"), "{by_voice}");
        assert!(by_wire.contains("battery-2"), "{by_wire}");
        assert!(
            NO_REPORTS_YET.contains("D-08"),
            "the blank column has to name the decision it is waiting on"
        );
    }

    /// Every report variant reaches words rather than a debug-formatted enum, and the
    /// one that matters most -- an ineffective engagement -- says so.
    #[test]
    fn every_report_variant_reaches_words() {
        assert!(report_sentence(&EffectorReport::Acknowledged {
            at: MissionTime(1.0)
        })
        .contains("Acknowledged"));
        assert!(report_sentence(&EffectorReport::Executing {
            at: MissionTime(2.0)
        })
        .contains("Executing"));
        let ineffective = report_sentence(&EffectorReport::Completed {
            at: MissionTime(3.0),
            effective: false,
            detail: "missed".into(),
        });
        assert!(ineffective.contains("ineffective"), "{ineffective}");
        assert!(ineffective.contains("missed"), "{ineffective}");
        assert!(report_sentence(&EffectorReport::Refused {
            at: MissionTime(4.0),
            reason: "no rounds".into()
        })
        .contains("no rounds"));
    }
}
