// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What this console says about its own publishing to coalition exchange, and what it
//! publishes again when its link comes back (GAP-146, GAP-145;
//! `docs/design/DN-18-coalition-exchange.md` §12 and §13; D-69, D-75, D-76).
//!
//! **The link decides what to do with an answer; this says it.** `gungnir-remote`'s link
//! reads every publish answer for what it means -- delivered, not now, not you, not that
//! -- and stops offering when the node has refused the caller. What is left here is the
//! one sentence PN-09 draws, because only this binary can name what would change a
//! refusal: which roles the authority matrix lets publish.

use crate::state::AppState;
use gungnir_model::ExchangeItem;
use gungnir_remote::link::{ExchangeStanding as LinkStanding, PublishRefusal};
use gungnir_ui::panels::sensor_health::ExchangeStanding;
use std::fmt::Write as _;

/// PN-09's line about this console's exchange publishing, owned so the view can borrow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedExchangeLine {
    pub standing: ExchangeStanding,
    pub text: String,
}

/// The line PN-09 draws, or `None` with no node linked: nothing is published from an
/// embedded console, and a section saying so would imply it was published somewhere.
#[must_use]
pub fn exchange_line(state: &AppState) -> Option<OwnedExchangeLine> {
    let link = state.link.as_ref()?;
    Some(match link.exchange_standing() {
        Some(standing) => line_for(&standing),
        // The link task panicked holding its lock. Said, not drawn as "nothing waiting".
        None => OwnedExchangeLine {
            standing: ExchangeStanding::Waiting,
            text: "Coalition exchange: this console cannot read its link's state, so \
                   whether anything it holds has reached the node is unknown."
                .to_owned(),
        },
    })
}

/// The sentence for one standing (GAP-146, DN-18 §13).
///
/// **One line whatever happened**, so a refusal is said once rather than once per
/// handoff or per attempt, and its counts grow in place.
#[must_use]
pub fn line_for(standing: &LinkStanding) -> OwnedExchangeLine {
    let publishing = &standing.publishing;
    let (held, are) = items_in_words(&standing.waiting);
    let (mut kind, mut text) = if let Some(refused) = &publishing.refused {
        (
            ExchangeStanding::Refused,
            refused_sentence(refused, &held, are),
        )
    } else if let (Some(retrying), false) = (&publishing.retrying, standing.waiting.is_empty()) {
        (
            ExchangeStanding::Waiting,
            format!(
                "Coalition exchange: {held} {are} waiting for the node, which has not taken \
                 {them} after {attempts} attempt{s} (the last: {failure}). Offered again at \
                 a growing interval, never less often than every {ceiling} s.",
                them = if standing.waiting.len() == 1 {
                    "it"
                } else {
                    "them"
                },
                attempts = retrying.attempts,
                s = if retrying.attempts == 1 { "" } else { "s" },
                failure = retrying.last_failure,
                ceiling = gungnir_remote::link::PUBLISH_RETRY_CEILING.as_secs(),
            ),
        )
    } else if standing.waiting.is_empty() {
        (
            ExchangeStanding::Current,
            match publishing.delivered {
                0 => "Coalition exchange: nothing published from this console yet.".to_owned(),
                n => format!(
                    "Coalition exchange: {n} set{s} published from this console; nothing \
                     waiting.",
                    s = if n == 1 { "" } else { "s" }
                ),
            },
        )
    } else {
        (
            ExchangeStanding::Current,
            format!("Coalition exchange: sending {held} to the node."),
        )
    };
    if publishing.superseded > 0 {
        let one = publishing.superseded == 1;
        // Writing into a `String` cannot fail.
        let _ = write!(
            text,
            " {n} older set{s} {were} replaced by a newer one before {they} could be sent; \
             only the newest set of each is held.",
            n = publishing.superseded,
            s = if one { "" } else { "s" },
            were = if one { "was" } else { "were" },
            they = if one { "it" } else { "they" },
        );
    }
    if let Some(rejection) = &publishing.last_rejection {
        let _ = write!(
            text,
            " The node rejected {n} set{s} as sent, the last of {what} ({status}: {reason}); \
             each was dropped, and the next set for that item replaces it.",
            n = publishing.rejected,
            s = if publishing.rejected == 1 { "" } else { "s" },
            what = item_in_words(rejection.item),
            status = rejection.status,
            reason = rejection.reason,
        );
        if kind == ExchangeStanding::Current {
            kind = ExchangeStanding::Waiting;
        }
    }
    OwnedExchangeLine {
        standing: kind,
        text,
    }
}

/// The refusal, in the node's own words, with what is held and what would change it.
fn refused_sentence(refused: &PublishRefusal, held: &str, are: &str) -> String {
    let (whom, what_changes) = if refused.status == 401 {
        (
            "no longer accepts this link's session",
            "sent when the link signs in again".to_owned(),
        )
    } else {
        (
            "refused this console as a publisher",
            format!(
                "nothing more is sent until this console's link signs in as a role that may \
                 publish ({})",
                roles_that_may_publish()
            ),
        )
    };
    format!(
        "Not publishing to coalition exchange: the node {whom} ({status}: {reason}). \
         {held} {are} held here, the newest set of each, and {what_changes}. Partners are \
         not told about them until then.",
        status = refused.status,
        reason = refused.reason,
        held = capitalised(held),
    )
}

/// The roles the authority matrix lets publish to exchange, as a sentence.
///
/// Read from `gungnir-security`'s own table, the one the node checks the same action
/// against, rather than written out here where it would drift from it.
#[must_use]
pub fn roles_that_may_publish() -> String {
    let roles: Vec<String> = gungnir_security::Role::ALL
        .iter()
        .filter(|role| {
            gungnir_security::authz::role_permits(
                **role,
                gungnir_security::actions::PUBLISH_EXCHANGE,
            )
        })
        .map(|role| format!("{role:?}"))
        .collect();
    join(&roles, "or")
}

fn item_in_words(item: ExchangeItem) -> &'static str {
    match item {
        ExchangeItem::Handoffs => "handoffs",
        ExchangeItem::Warnings => "launch warnings",
        ExchangeItem::Reports => "mission report",
        ExchangeItem::Tracks => "tracks",
        ExchangeItem::Health => "health",
    }
}

/// The held items as a noun phrase, and the verb that agrees with it: "this console's
/// handoffs and launch warnings", "are"; "this console's mission report", "is".
fn items_in_words(items: &[ExchangeItem]) -> (String, &'static str) {
    let words: Vec<String> = items.iter().map(|i| item_in_words(*i).to_owned()).collect();
    if words.is_empty() {
        return ("nothing".to_owned(), "is");
    }
    let singular = matches!(items, [ExchangeItem::Reports | ExchangeItem::Health]);
    (
        format!("this console's {}", join(&words, "and")),
        if singular { "is" } else { "are" },
    )
}

fn join(words: &[String], conjunction: &str) -> String {
    match words {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} {conjunction} {last}", init.join(", ")),
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Publish every set this console holds in mission state, the tick its link comes back
/// (GAP-145, GAP-146; DN-18 §12, §13).
///
/// **Handoffs, launch warnings and the mission report.** Each is a set held in
/// [`AppState`] and republished whole, so sending it again costs a partner nothing and
/// repairs a node that came back without it -- and since GAP-146 a new link is also what a
/// refused console waits on: a sign-in on a linked desktop builds a new link, whose
/// connection is this edge, and the old link's held sets went with it.
///
/// **The report since GAP-150** (D-97, DN-18 §14): the last one PN-13 generated is kept in
/// [`AppState::exchange_report`] as partners were sent it, and goes out again here with
/// the time it was generated, not the time it was resent. A console that has generated
/// none publishes none, as a console that has issued no handoff publishes no set.
pub fn republish_all(state: &mut AppState) {
    crate::handoffs::republish_to_node(state);
    crate::launch_warning::republish_to_node(state);
    crate::sustainment::publish_to_exchange(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_remote::link::{ExchangePublishing, PublishRetry};

    fn refused(status: u16, reason: &str) -> LinkStanding {
        LinkStanding {
            waiting: vec![ExchangeItem::Handoffs, ExchangeItem::Warnings],
            publishing: ExchangePublishing {
                posts: 1,
                superseded: 12,
                refused: Some(PublishRefusal {
                    item: ExchangeItem::Handoffs,
                    status,
                    reason: reason.into(),
                }),
                ..ExchangePublishing::default()
            },
        }
    }

    /// The sentence names why in the node's words, what is held, and what would change
    /// it -- the roles the matrix lets publish, read from the matrix.
    #[test]
    fn a_refused_console_is_told_why_what_is_held_and_what_would_change_it() {
        let line = line_for(&refused(
            403,
            "role Operator may not publish to exchange (exchange.publish)",
        ));
        assert_eq!(line.standing, ExchangeStanding::Refused);
        for words in [
            "Not publishing to coalition exchange",
            "403: role Operator may not publish to exchange",
            "This console's handoffs and launch warnings are held here",
            "12 older sets were replaced",
        ] {
            assert!(line.text.contains(words), "{words:?} not in {}", line.text);
        }
        // The roles are the matrix's, so the sentence names Supervisor and never the
        // Operator it is refusing.
        let roles = roles_that_may_publish();
        assert!(
            roles.contains("Supervisor") && !roles.contains("Operator,"),
            "{roles}"
        );
        assert!(
            line.text.contains(&format!("may publish ({roles})")),
            "{}",
            line.text
        );
    }

    /// A lapsed session is its own sentence: signing in again is the remedy, not a role.
    #[test]
    fn a_session_the_node_no_longer_accepts_says_to_sign_in_again() {
        let line = line_for(&refused(401, "the token is not valid"));
        assert_eq!(line.standing, ExchangeStanding::Refused);
        assert!(line.text.contains("no longer accepts this link's session"));
        assert!(line.text.contains("sent when the link signs in again"));
        assert!(!line.text.contains("Supervisor"), "{}", line.text);
    }

    /// Not now is not no: a node that has not answered is waiting, not a refusal.
    #[test]
    fn a_set_the_node_has_not_taken_yet_is_waiting() {
        let line = line_for(&LinkStanding {
            waiting: vec![ExchangeItem::Reports],
            publishing: ExchangePublishing {
                retrying: Some(PublishRetry {
                    attempts: 4,
                    last_failure: "507: the register is full".into(),
                }),
                ..ExchangePublishing::default()
            },
        });
        assert_eq!(line.standing, ExchangeStanding::Waiting);
        assert!(
            line.text
                .contains("this console's mission report is waiting for the node"),
            "{}",
            line.text
        );
        assert!(
            line.text.contains("4 attempts (the last: 507"),
            "{}",
            line.text
        );
    }

    /// Quiet is said as quiet, with the count, and a rejected set turns it amber.
    #[test]
    fn a_console_with_nothing_waiting_says_what_it_has_published() {
        let mut standing = LinkStanding::default();
        assert!(line_for(&standing).text.contains("nothing published"));
        standing.publishing.delivered = 3;
        let line = line_for(&standing);
        assert_eq!(line.standing, ExchangeStanding::Current);
        assert!(line.text.contains("3 sets published"), "{}", line.text);
        standing.publishing.rejected = 1;
        standing.publishing.last_rejection = Some(PublishRefusal {
            item: ExchangeItem::Reports,
            status: 400,
            reason: "the products could not be decoded".into(),
        });
        let line = line_for(&standing);
        assert_eq!(line.standing, ExchangeStanding::Waiting);
        assert!(
            line.text
                .contains("rejected 1 set as sent, the last of mission report (400"),
            "{}",
            line.text
        );
    }
}
