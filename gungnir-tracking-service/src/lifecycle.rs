// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The track lifecycle as events: what changed in the picture since the last look
//! (GAP-160).
//!
//! **Why this exists.** `gungnir_model::events::TrackingEvent` has had its variants since
//! the event model was written, and five readers consume them: `gungnir-remote`'s link
//! projection (a linked desktop's picture between snapshots), `gungnir-api`'s partner
//! stream (DN-18 §6), `gungnir-reporting` and `gungnir-replay` over a journal, and the
//! node's cross-session entity fold. Until GAP-160 **nothing produced one**: the pipeline
//! answers with whole snapshots and neither binary turned a snapshot into the events the
//! rest of the system was written against. A desktop linked to a node therefore kept the
//! picture its sign-in snapshot held, for as long as the link stayed up.
//!
//! A snapshot is the pipeline's answer and an event is a change, so the one thing that
//! can derive the second from the first is a comparison with the picture last announced.
//! That comparison is here, beside [`crate::project_track`], so a host announces tracks
//! the way the service projects them and no host writes a second opinion of what "a
//! track changed" means.

use gungnir_model::events::TrackingEvent;
use gungnir_model::{TrackId, TrackView};

/// The picture last announced, and the events that move it to the next one.
///
/// Held by a host for the life of its tracker. Each call to [`TrackLifecycle::changes`]
/// compares the tracker's current `tracks()` with what the previous call announced.
#[derive(Debug, Default, Clone)]
pub struct TrackLifecycle {
    announced: Vec<TrackView>,
}

impl TrackLifecycle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The events that take a reader holding the last announced picture to `tracks`.
    ///
    /// In this order, which is the order a reader applying them needs:
    ///
    /// 1. [`TrackingEvent::TrackDeleted`] for every announced track no longer in
    ///    `tracks`, in the order they were announced;
    /// 2. then, in `tracks`' own order, [`TrackingEvent::TrackInitiated`] for a track
    ///    never announced and [`TrackingEvent::TrackUpdated`] for one whose view differs
    ///    in any field from the view last announced for it.
    ///
    /// **A whole view, compared field for field.** An update is announced when anything a
    /// panel could draw has changed -- the state, the covariance, the status, the estimate
    /// time, the staleness mark -- and not otherwise, so a tracker polled every frame
    /// with nothing new announces nothing. Deletions come first so that a reader appending
    /// initiations to the end of its list ends with the same order `tracks` has: the
    /// pipeline reports live tracks in the order they were created, and a reader that
    /// applied an initiation before the deletion it followed would hold the same tracks
    /// in a different order from the host that announced them.
    ///
    /// `TrackCoasting` is not produced: a coasting track's status is on its view, which an
    /// update carries, and a second event saying the same thing would be read twice by
    /// every consumer that counts.
    pub fn changes(&mut self, tracks: &[TrackView]) -> Vec<TrackingEvent> {
        let mut events = Vec::new();
        let now_live: std::collections::HashSet<TrackId> = tracks.iter().map(|t| t.id).collect();
        for gone in self.announced.iter().filter(|t| !now_live.contains(&t.id)) {
            events.push(TrackingEvent::TrackDeleted(gone.id));
        }
        let before: std::collections::HashMap<TrackId, &TrackView> =
            self.announced.iter().map(|t| (t.id, t)).collect();
        for track in tracks {
            match before.get(&track.id) {
                None => events.push(TrackingEvent::TrackInitiated(track.clone())),
                Some(last) if *last != track => {
                    events.push(TrackingEvent::TrackUpdated(track.clone()));
                }
                Some(_) => {}
            }
        }
        if !events.is_empty() {
            self.announced = tracks.to_vec();
        }
        events
    }

    /// The picture as last announced.
    #[must_use]
    pub fn announced(&self) -> &[TrackView] {
        &self.announced
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{
        Classification, MissionTime, Provenance, Quality, Releasability, TrackStatus,
    };

    fn track(id: u64, x: f64) -> TrackView {
        let mut state = nalgebra::SVector::<f64, 6>::zeros();
        state[0] = x;
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Tentative,
            state,
            covariance: nalgebra::SMatrix::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(x),
            releasability: Releasability::default(),
        }
    }

    /// Applies events the way `gungnir-remote`'s link does, so the tests below check
    /// what a reader ends up holding and not only which events were emitted.
    fn apply(held: &mut Vec<TrackView>, events: &[TrackingEvent]) {
        for event in events {
            match event {
                TrackingEvent::TrackInitiated(t) | TrackingEvent::TrackUpdated(t) => {
                    match held.iter_mut().find(|h| h.id == t.id) {
                        Some(h) => *h = t.clone(),
                        None => held.push(t.clone()),
                    }
                }
                TrackingEvent::TrackDeleted(id) => held.retain(|h| h.id != *id),
                TrackingEvent::TrackCoasting(_) => {}
            }
        }
    }

    #[test]
    fn a_new_track_is_initiated_once_and_an_unchanged_one_is_not_repeated() {
        let mut lifecycle = TrackLifecycle::new();
        let picture = vec![track(0, 1.0)];
        assert_eq!(
            lifecycle.changes(&picture),
            vec![TrackingEvent::TrackInitiated(track(0, 1.0))]
        );
        assert!(
            lifecycle.changes(&picture).is_empty(),
            "a tracker polled with nothing new announced something"
        );
    }

    #[test]
    fn any_changed_field_is_an_update_and_a_vanished_track_is_deleted() {
        let mut lifecycle = TrackLifecycle::new();
        let _ = lifecycle.changes(&[track(0, 1.0), track(1, 2.0)]);
        let mut stale = track(1, 2.0);
        stale.quality.is_stale = true;
        let events = lifecycle.changes(&[stale.clone()]);
        assert_eq!(
            events,
            vec![
                TrackingEvent::TrackDeleted(TrackId(0)),
                TrackingEvent::TrackUpdated(stale),
            ],
            "the staleness mark is drawn, so it is a change"
        );
    }

    /// The reader's order follows the host's through a deletion and an initiation in the
    /// same step, which is what the deletions-first rule is for.
    #[test]
    fn a_reader_applying_the_events_holds_the_picture_in_the_hosts_order() {
        let mut lifecycle = TrackLifecycle::new();
        let mut held = Vec::new();
        let pictures = [
            vec![track(0, 1.0), track(1, 1.0)],
            vec![track(1, 2.0), track(2, 2.0)],
            vec![track(1, 3.0), track(2, 2.0), track(3, 3.0)],
            vec![],
            vec![track(4, 5.0)],
        ];
        for picture in &pictures {
            let events = lifecycle.changes(picture);
            apply(&mut held, &events);
            assert_eq!(&held, picture);
            assert_eq!(lifecycle.announced(), picture.as_slice());
        }
    }
}
