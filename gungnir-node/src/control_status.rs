// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Live weapons-control-status changes on the node, for the desktops linked to it
//! (GAP-203; `docs/design/DN-09-authority-and-control-status.md` §10; D-140).
//!
//! # What this is
//!
//! DN-09 §6 makes a status change a recorded decision: who, as which role, why, from what,
//! under which baseline revision. Until GAP-203 the node had no door for one: the status
//! it decided by was the baseline it opened with, and a supervisor at a linked desktop
//! could change nothing the node would honour. `POST /v3/policy/control-status` is that
//! door (`gungnir_api::transport`), and this module is what the node loop does with what
//! the route hands over, in the order it arrived.
//!
//! **The settings in force are the loop's `ConfigBaseline`**, mutated in place: the
//! change is `config.policy.control_status.set(layer, status)`, and from that tick every
//! plan the allocator proposes and every item the queue re-judges reads the new status
//! through the same `cx.config` the chain always read. There is no second copy of the
//! status to drift from the first. `gungnir_approval::ApprovalDesk::set_control_status`
//! then puts the change on the journal and the audit record and withdraws every queued
//! item the new status refuses (`gungnir_approval::control_status`, which says why the
//! control-status engine alone re-judges).
//!
//! # The fold at start
//!
//! A change is journaled as `PolicyEvent::ControlStatusSet` and nothing else, so a node
//! that restarts reads its baseline and then folds every change made under that
//! baseline's revision back over it ([`NodeControlStatus::recover`]), oldest first. A
//! change made under another revision is skipped: a baseline that rewrites the status is a
//! change request under phase H, and it supersedes any live change made under the
//! revision before it (`gungnir_model::ControlStatusChange::applies_to`). What was folded
//! is said at info, and a session that could not be read is said at warning, exactly as
//! the gap acceptances say it: a fold that stopped early would leave a layer at the
//! baseline's status when a person had put it at hold.
//!
//! # The two-person rule
//!
//! D-140's rule is the baseline's `policy.two_person.control_status`, read here at the
//! moment the change is applied, so a baseline turning it on applies to the next change.
//! The route has already verified the second person against the account store; what the
//! loop checks is only that one came where the rule says one must.

use crate::approval::{self, Frame, NodeApproval};
use gungnir_api::transport::{ControlStatusAnswer, NodeApi, PendingControlStatus};
use gungnir_api::v3::{self, ControlStatusRefused};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Event, InProcessBus};
use gungnir_model::events::PolicyEvent;
use gungnir_model::{
    ConfirmingOperator, ControlStatusChange, EffectorLayer, MissionTime, ResourceView, TrackView,
    WeaponsControlStatus,
};
use gungnir_remote::endpoint::EndpointClient;
use gungnir_security::actions::SET_CONTROL_STATUS;
use gungnir_security::AuditEntry;
use gungnir_store::EventJournal;
use std::collections::BTreeMap;

/// How many changes the node keeps in memory: the snapshot's own bound
/// (`gungnir_api::v3::CONTROL_STATUS_CHANGES_CARRIED`). The journal holds every change;
/// this list is for the snapshot and for answering a retry.
const CHANGES_KEPT: usize = v3::CONTROL_STATUS_CHANGES_CARRIED;

/// The live changes this node holds (module documentation).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeControlStatus {
    /// The changes folded at start and recorded this run, oldest first, at most
    /// [`CHANGES_KEPT`].
    changes: Vec<ControlStatusChange>,
    /// Why the fold at start read less than the whole journal, when it did.
    unreadable: Option<String>,
}

impl NodeControlStatus {
    /// Fold every session of `journal`, oldest first, applying each change made under
    /// `config.revision` to `config.policy.control_status` and keeping it; a change under
    /// another revision is superseded by the baseline and skipped.
    ///
    /// A session that cannot be read stops the fold and is said ([`Self::unreadable`]),
    /// as the gap acceptances' fold does: the changes after it would otherwise be lost
    /// silently, and a layer a person put at hold would come back at the baseline's
    /// status.
    #[must_use]
    pub fn recover(journal: &dyn EventJournal, config: &mut ConfigBaseline) -> Self {
        let mut out = Self::default();
        let mut sessions = match journal.sessions() {
            Ok(s) => s,
            Err(err) => {
                out.unreadable = Some(format!("the session list could not be read: {err}"));
                return out;
            }
        };
        sessions.sort_unstable_by_key(|s| s.0);
        let mut skipped = 0usize;
        for session in sessions {
            let envelopes = match journal.read_session(session) {
                Ok(envelopes) => envelopes,
                Err(err) => {
                    out.unreadable = Some(format!("session {}: {err}", session.0));
                    break;
                }
            };
            for envelope in envelopes {
                let Event::Policy(PolicyEvent::ControlStatusSet(change)) = envelope.event else {
                    continue;
                };
                if !change.applies_to(config.revision) {
                    skipped += 1;
                    continue;
                }
                config
                    .policy
                    .control_status
                    .set(change.layer, change.status);
                out.keep(change);
            }
        }
        tracing::info!(
            folded = out.changes.len(),
            superseded = skipped,
            revision = config.revision,
            in_force = ?config.policy.control_status.by_layer,
            "weapons control status folded from the journal"
        );
        if let Some(reason) = &out.unreadable {
            tracing::warn!(
                %reason,
                "the control status was folded from an incomplete journal; a layer may be \
                 at the baseline's status where a person had changed it"
            );
        }
        out
    }

    /// Why the fold at start read less than the whole journal, if it did.
    #[must_use]
    pub fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    /// The changes held, oldest first.
    #[must_use]
    pub fn changes(&self) -> &[ControlStatusChange] {
        &self.changes
    }

    /// The status in force per layer, for the snapshot: the baseline's with every change
    /// folded over it, which is what `config` holds.
    #[must_use]
    pub fn in_force(
        &self,
        config: &ConfigBaseline,
    ) -> BTreeMap<EffectorLayer, WeaponsControlStatus> {
        config.policy.control_status.by_layer.clone()
    }

    /// The change recorded under `request`, if one was: what makes a retry the same
    /// request rather than a second change.
    fn recorded_under(&self, request: &gungnir_model::RequestId) -> Option<&ControlStatusChange> {
        self.changes
            .iter()
            .find(|c| c.request.as_ref() == Some(request))
    }

    fn keep(&mut self, change: ControlStatusChange) {
        if self.changes.len() >= CHANGES_KEPT {
            self.changes.remove(0);
        }
        self.changes.push(change);
    }
}

/// What one tick's control-status work needs from the loop: everything a
/// [`Frame`] carries except the baseline, which this step mutates and so takes
/// separately.
pub struct Tick<'a> {
    pub now: MissionTime,
    pub tracks: &'a [TrackView],
    pub resources: &'a [ResourceView],
    pub geofences: &'a dyn gungnir_policy::GeoService,
    pub bus: &'a InProcessBus,
    pub endpoint_client: Option<&'a EndpointClient>,
    pub api: &'a NodeApi,
}

/// Apply the changes the route handed over, in arrival order, and answer each (module
/// documentation).
///
/// **Before the loop builds the frame the rest of the tick judges by**, because this
/// mutates `config` and a `Frame` borrows it: everything after this step, the plan the
/// allocator proposes included, reads the status these changes put in force. Each
/// refusal is audited under `weapons.control_status` naming the requester; each change
/// is journaled, audited and its withdrawals published by the desk, so what this records
/// is on the journal before the tick's append.
pub fn tick(
    held: &mut NodeControlStatus,
    approval: &mut NodeApproval,
    requests: Vec<PendingControlStatus>,
    config: &mut ConfigBaseline,
    t: &Tick<'_>,
) {
    for pending in requests {
        let PendingControlStatus {
            request,
            layer,
            status,
            reason,
            operator,
            role,
            party,
            confirmed_by,
            reply,
        } = pending;
        let asked = Asked {
            request: &request,
            layer,
            status,
            reason: &reason,
            operator,
            role,
            party: party.as_deref(),
            confirmed_by,
        };
        let answer = answer_one(held, approval, asked, config, t);
        // A caller that stopped waiting has its answer on the record and in the next
        // snapshot; there is nobody left to tell.
        let _ = reply.send(answer);
    }
}

/// The request without its reply channel, borrowed, so [`answer_one`] reads it while
/// [`tick`] keeps the channel to answer on.
#[derive(Clone, Copy)]
struct Asked<'r> {
    request: &'r gungnir_model::RequestId,
    layer: EffectorLayer,
    status: WeaponsControlStatus,
    reason: &'r str,
    operator: gungnir_security::OperatorId,
    role: gungnir_security::Role,
    party: Option<&'r str>,
    confirmed_by: Option<(gungnir_security::OperatorId, gungnir_security::Role)>,
}

/// One request, answered: the retry, the two refusals the loop makes, or the change.
fn answer_one(
    held: &mut NodeControlStatus,
    approval: &mut NodeApproval,
    asked: Asked<'_>,
    config: &mut ConfigBaseline,
    t: &Tick<'_>,
) -> ControlStatusAnswer {
    // A request key already recorded is answered with the change it produced, and
    // records nothing: what makes a retry after a `504` safe. Asked first, before the
    // layer is looked at, because the layer is already at the status that change set
    // and "unchanged" would be the wrong answer to the same question asked twice.
    if let Some(change) = held.recorded_under(asked.request) {
        tracing::info!(
            request = %asked.request,
            "a repeated control-status request key was answered with its first outcome"
        );
        return ControlStatusAnswer::Recorded(v3::ControlStatusRecorded {
            change: change.clone(),
            withdrawn: Vec::new(),
        });
    }
    let refused = if config.policy.two_person.control_status && asked.confirmed_by.is_none() {
        Some(ControlStatusRefused::TwoPersonRequired)
    } else if config.policy.control_status.for_layer(asked.layer) == asked.status {
        Some(ControlStatusRefused::Unchanged {
            layer: asked.layer,
            status: asked.status,
        })
    } else {
        None
    };
    if let Some(refused) = refused {
        approval.audit.record(
            AuditEntry::new(
                Some(asked.operator),
                SET_CONTROL_STATUS,
                t.now.0,
                format!(
                    "refused: {} (asked: {:?} to {:?}, {})",
                    refused.sentence(),
                    asked.layer,
                    asked.status,
                    asked.reason
                ),
            )
            .by_party(asked.party.map(str::to_owned)),
        );
        return ControlStatusAnswer::Refused(refused);
    }
    // The host applies the change first (the desk debug-asserts it), and the frame is
    // built after, so `cx.config` carries the new status when the queue is re-judged.
    let previous = config.policy.control_status.set(asked.layer, asked.status);
    let change = ControlStatusChange {
        layer: asked.layer,
        status: asked.status,
        previous,
        reason: asked.reason.to_owned(),
        operator: asked.operator.0.to_string(),
        role: format!("{:?}", asked.role),
        confirmed_by: asked
            .confirmed_by
            .map(|(operator, role)| ConfirmingOperator {
                operator: operator.0.to_string(),
                role: format!("{role:?}"),
            }),
        revision: config.revision,
        // The desktop's certificate name, which is where the change was asked from
        // (D-67); `None` over plaintext, which is a connection this node cannot name.
        origin: asked.party.map(str::to_owned),
        at: t.now,
        request: Some(asked.request.clone()),
    };
    let frame = Frame {
        now: t.now,
        config,
        tracks: t.tracks,
        resources: t.resources,
        geofences: t.geofences,
        bus: t.bus,
        endpoint_client: t.endpoint_client,
        // A status change hands nothing off, so it reads no effector.
        effectors: None,
        api: t.api,
    };
    let withdrawn = approval::with_desk(
        approval,
        &frame,
        Some((asked.operator, asked.role)),
        asked.party,
        asked.role,
        |desk, cx, _policy, host| desk.set_control_status(cx, host, change.clone()),
    );
    tracing::info!(
        layer = ?change.layer,
        status = ?change.status,
        previous = ?change.previous,
        operator = %change.operator,
        withdrawn = withdrawn.len(),
        "weapons control status changed"
    );
    held.keep(change.clone());
    ControlStatusAnswer::Recorded(v3::ControlStatusRecorded {
        change,
        withdrawn: withdrawn
            .into_iter()
            .map(|w| v3::WithdrawnItem {
                item: w.item,
                plan: w.plan,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_eventing::Envelope;
    use gungnir_store::{FileEventJournal, SessionId};

    fn change(
        layer: EffectorLayer,
        status: WeaponsControlStatus,
        revision: u32,
    ) -> ControlStatusChange {
        ControlStatusChange {
            layer,
            status,
            previous: WeaponsControlStatus::Tight,
            reason: "test".into(),
            operator: "7".into(),
            role: "Supervisor".into(),
            confirmed_by: None,
            revision,
            origin: None,
            at: MissionTime(1.0),
            request: None,
        }
    }

    fn envelope(seq: u64, change: ControlStatusChange) -> Envelope {
        Envelope {
            seq,
            mission_time: MissionTime(1.0),
            event: Event::Policy(PolicyEvent::ControlStatusSet(change)),
        }
    }

    /// The fold applies the changes made under the baseline's revision, oldest first so
    /// the last one wins, and skips the ones a later baseline superseded.
    #[test]
    fn the_fold_applies_this_revisions_changes_and_skips_superseded_ones() {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-node-control-status-fold-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut journal = FileEventJournal::open(&dir).expect("a journal");
        journal
            .append(
                SessionId(1),
                &envelope(
                    1,
                    change(EffectorLayer::Area, WeaponsControlStatus::Hold, 2),
                ),
            )
            .expect("appended");
        journal
            .append(
                SessionId(1),
                &envelope(
                    2,
                    change(EffectorLayer::Area, WeaponsControlStatus::Free, 2),
                ),
            )
            .expect("appended");
        journal
            .append(
                SessionId(2),
                &envelope(
                    3,
                    change(EffectorLayer::Point, WeaponsControlStatus::Free, 1),
                ),
            )
            .expect("appended");
        let mut config = ConfigBaseline {
            revision: 2,
            ..ConfigBaseline::default()
        };
        let folded = NodeControlStatus::recover(&journal, &mut config);
        assert_eq!(folded.changes().len(), 2);
        assert_eq!(
            config.policy.control_status.for_layer(EffectorLayer::Area),
            WeaponsControlStatus::Free,
            "the later change wins"
        );
        assert_eq!(
            config.policy.control_status.for_layer(EffectorLayer::Point),
            WeaponsControlStatus::Hold,
            "a change under revision 1 is superseded by baseline revision 2"
        );
        assert!(folded.unreadable().is_none());
        drop(journal);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The in-memory list is bounded at the snapshot's bound, oldest dropped.
    #[test]
    fn the_changes_kept_are_bounded() {
        let mut status = NodeControlStatus::default();
        for i in 0..CHANGES_KEPT + 5 {
            let mut c = change(EffectorLayer::Area, WeaponsControlStatus::Hold, 1);
            c.revision = u32::try_from(i).expect("a small count");
            status.keep(c);
        }
        assert_eq!(status.changes().len(), CHANGES_KEPT);
        assert_eq!(
            status.changes()[0].revision,
            5,
            "the oldest five were dropped"
        );
    }
}
