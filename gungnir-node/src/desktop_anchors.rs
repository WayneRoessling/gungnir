// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The heads of linked desktops' audit logs, held on the node (GAP-208, D-142;
//! `docs/design/DN-23-operator-authentication.md` §16).
//!
//! A desktop's audit head is kept in its own journal, which its operator's keystore
//! unlocks, so on its own it proves nothing against that operator. Forwarded here while
//! the desktop is linked, it is held where the operator cannot reach, and it is judged:
//!
//! - **by its signature**, against the baseline's audit-signing public key: signed by
//!   it, unsigned, signed by something else, or nothing configured to check with;
//! - **against what is already held for that segment**: the same line count with another
//!   hash is a rewrite, and fewer lines than a head already held is a cut. Both are found
//!   without the file, because a head names a line and the line's hash.
//!
//! Each head goes on the node's journal as `AuditEvent::Forwarded` with its verdict and
//! any finding, so the node's own record says what each desktop's record was. A head the
//! node already holds exactly is not journaled again: a desktop forwards its whole
//! inventory on every reconnect.

use std::collections::BTreeMap;

use gungnir_api::transport::{NodeApi, PendingAnchors};
use gungnir_eventing::{Event, EventBus, EventingError};
use gungnir_model::events::{AnchorVerdict, AuditEvent, AuditHead};
use gungnir_model::MissionTime;
use gungnir_security::audit::signing::{AuditVerifyingKey, SignatureCheck};
use gungnir_store::EventJournal;

/// Every head each desktop has forwarded, by party and then segment, in the order held.
#[derive(Debug, Default)]
pub struct DesktopAnchors {
    held: BTreeMap<String, BTreeMap<String, Vec<AuditHead>>>,
    /// A journal session that could not be read at start, so heads in it are not held.
    unreadable: Vec<String>,
}

impl DesktopAnchors {
    /// What the journal says desktops forwarded, every session, oldest first.
    #[must_use]
    pub fn recover(journal: &dyn EventJournal) -> Self {
        let mut this = Self::default();
        let mut sessions = match journal.sessions() {
            Ok(sessions) => sessions,
            Err(e) => {
                this.unreadable
                    .push(format!("the journal's sessions could not be listed: {e}"));
                return this;
            }
        };
        sessions.sort_unstable_by_key(|s| s.0);
        for session in sessions {
            match journal.read_session(session) {
                Ok(envelopes) => {
                    for envelope in envelopes {
                        if let Event::Audit(AuditEvent::Forwarded { party, head, .. }) =
                            envelope.event
                        {
                            this.hold(&party, head);
                        }
                    }
                }
                Err(e) => this.unreadable.push(format!("session {}: {e}", session.0)),
            }
        }
        this
    }

    /// Sessions that could not be read when this was recovered.
    #[must_use]
    pub fn unreadable(&self) -> &[String] {
        &self.unreadable
    }

    fn hold(&mut self, party: &str, head: AuditHead) {
        self.held
            .entry(party.to_owned())
            .or_default()
            .entry(head.segment.clone())
            .or_default()
            .push(head);
    }

    /// For each desktop, the head with the most entries per segment: what a desktop is
    /// served to check its file against.
    #[must_use]
    pub fn latest(&self) -> BTreeMap<String, Vec<AuditHead>> {
        self.held
            .iter()
            .map(|(party, segments)| {
                let heads = segments
                    .values()
                    .filter_map(|heads| heads.iter().max_by_key(|h| h.entries).cloned())
                    .collect();
                (party.clone(), heads)
            })
            .collect()
    }

    /// What `head` contradicts in what is held for its segment, in words, or `None`.
    fn finding(&self, party: &str, head: &AuditHead) -> Option<String> {
        let segment = self.held.get(party)?.get(&head.segment)?;
        if let Some(other) = segment
            .iter()
            .find(|h| h.entries == head.entries && h.hash != head.hash)
        {
            return Some(format!(
                "{} at entry {} has hash {}, and the node holds {} there from this desktop \
                 earlier: the segment was rewritten",
                head.segment, head.entries, head.hash, other.hash
            ));
        }
        let most = segment.iter().map(|h| h.entries).max()?;
        (head.entries < most).then(|| {
            format!(
                "{} reaches {} entries, and the node holds a head of {most} from this \
                 desktop: {} entries were cut",
                head.segment,
                head.entries,
                most - head.entries
            )
        })
    }

    /// Judge, journal and hold every batch the routes took (GAP-208), then publish what
    /// is held. `key` is the baseline's audit-signing public key, when it names one.
    ///
    /// # Errors
    ///
    /// When the bus refuses an event.
    pub fn tick(
        &mut self,
        batches: Vec<PendingAnchors>,
        api: &NodeApi,
        bus: &dyn EventBus,
        key: Option<&AuditVerifyingKey>,
        now: MissionTime,
    ) -> Result<(), EventingError> {
        if batches.is_empty() {
            return Ok(());
        }
        for batch in batches {
            for head in batch.heads {
                let already = self
                    .held
                    .get(&batch.party)
                    .and_then(|s| s.get(&head.segment))
                    .is_some_and(|heads| heads.iter().any(|h| h == &head));
                if already {
                    continue;
                }
                let verdict = verdict(&head, key);
                let finding = self.finding(&batch.party, &head);
                if let Some(finding) = &finding {
                    tracing::error!(desktop = %batch.party, %finding, "a desktop's audit record contradicts what this node holds");
                }
                bus.publish(
                    now,
                    Event::Audit(AuditEvent::Forwarded {
                        party: batch.party.clone(),
                        head: head.clone(),
                        verdict,
                        finding,
                        at: now,
                    }),
                )?;
                self.hold(&batch.party, head);
            }
            if let Some(findings) = batch.findings {
                for finding in &findings {
                    tracing::error!(desktop = %batch.party, %finding, "a desktop's audit file does not reach the heads this node holds");
                }
                bus.publish(
                    now,
                    Event::Audit(AuditEvent::ForwardedVerification {
                        party: batch.party.clone(),
                        findings,
                        at: now,
                    }),
                )?;
            }
        }
        api.publish_held_anchors(self.latest());
        Ok(())
    }
}

/// One head's signature against the baseline's key.
#[must_use]
pub fn verdict(head: &AuditHead, key: Option<&AuditVerifyingKey>) -> AnchorVerdict {
    let Some(key) = key else {
        return AnchorVerdict::NoKeyConfigured;
    };
    let Some(signature) = &head.signature else {
        return AnchorVerdict::Unsigned;
    };
    let checked = crate::audit_record::from_model(head);
    match key.check(&checked, &signature.der().unwrap_or_default()) {
        SignatureCheck::Valid => AnchorVerdict::Signed {
            key_version: signature.key_version,
        },
        SignatureCheck::Invalid => AnchorVerdict::BadSignature,
    }
}

/// The baseline's audit-signing public key, read once at start; a key the baseline names
/// and that does not read is said, and nothing is then checked against it.
#[must_use]
pub fn verifying_key(config: &gungnir_config::ConfigBaseline) -> Option<AuditVerifyingKey> {
    let pem = &config.security.audit_signing.as_ref()?.public_key_pem;
    match AuditVerifyingKey::from_pem(pem) {
        Ok(key) => Some(key),
        Err(err) => {
            tracing::error!(%err, "the baseline's audit-signing key does not read; forwarded heads are recorded with no key to check them");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_eventing::InProcessBus;

    fn head(entries: u64, hash: &str) -> AuditHead {
        AuditHead {
            segment: "audit-000001.jsonl".into(),
            entries,
            seq: entries,
            hash: hash.into(),
            signature: None,
        }
    }

    fn batch(heads: Vec<AuditHead>) -> PendingAnchors {
        PendingAnchors {
            party: "desktop-a".into(),
            operator: gungnir_security::OperatorId(7),
            heads,
            findings: None,
        }
    }

    fn forwarded(
        rx: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>,
    ) -> Vec<(AnchorVerdict, Option<String>)> {
        rx.try_iter()
            .filter_map(|e| match e.event {
                Event::Audit(AuditEvent::Forwarded {
                    verdict, finding, ..
                }) => Some((verdict, finding)),
                _ => None,
            })
            .collect()
    }

    /// The node finds a rewrite and a cut by the heads alone, holds the longest head per
    /// segment for the desktop to read back, and journals a head it already holds once.
    #[test]
    fn a_rewrite_and_a_cut_are_found_from_the_heads_alone() {
        let bus = InProcessBus::new();
        let rx = bus.subscribe();
        let api = NodeApi::new(gungnir_api::v3::SnapshotResponse::new(
            Vec::new(),
            None,
            gungnir_model::SystemHealth::default(),
            Vec::new(),
        ));
        let mut held = DesktopAnchors::default();
        let now = MissionTime(1.0);
        held.tick(
            vec![batch(vec![head(10, "a"), head(20, "b")])],
            &api,
            &bus,
            None,
            now,
        )
        .expect("published");
        let first = forwarded(&rx);
        assert_eq!(first.len(), 2);
        assert!(first
            .iter()
            .all(|(v, f)| *v == AnchorVerdict::NoKeyConfigured && f.is_none()));
        assert_eq!(api.held_anchors_for("desktop-a"), vec![head(20, "b")]);

        held.tick(
            vec![batch(vec![
                head(20, "b"),
                head(20, "forged"),
                head(15, "c"),
            ])],
            &api,
            &bus,
            None,
            now,
        )
        .expect("published");
        let second = forwarded(&rx);
        assert_eq!(
            second.len(),
            2,
            "the head already held is not journaled again"
        );
        assert!(second[0]
            .1
            .as_deref()
            .is_some_and(|f| f.contains("rewritten")));
        assert!(second[1]
            .1
            .as_deref()
            .is_some_and(|f| f.contains("5 entries were cut")));
        assert!(api.held_anchors_for("desktop-b").is_empty());
    }
}
