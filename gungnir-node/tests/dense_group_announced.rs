// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node announces its dense group as it changes (GAP-015): the event a linked desktop
//! reads to keep the group live between snapshots.

use gungnir_eventing::{Event, EventBus, InProcessBus};
use gungnir_model::events::TrackingEvent;
use gungnir_model::{DenseGroupView, MissionTime};
use gungnir_node::picture::Announcer;

fn group(epoch: f64, expected: f64) -> DenseGroupView {
    DenseGroupView {
        epoch: MissionTime(epoch),
        expected_targets: expected,
        most_probable_count: None,
        count_distribution: Vec::new(),
        components: Vec::new(),
        filter: "gm-phd".into(),
        scan_size: 21,
        is_stale: false,
    }
}

/// One event per change, the release included, and none for a tick on which nothing
/// changed -- so the record says when a raid outgrew the tracker and when it stopped,
/// without a copy of the same group on every tick between.
#[test]
fn the_group_is_announced_once_per_change_and_its_release_once() {
    let bus = InProcessBus::new();
    let rx = bus.subscribe();
    let mut announcer = Announcer::new();
    let first = group(10.0, 19.5);
    let second = group(11.0, 20.25);

    announcer
        .dense_group(&bus, MissionTime(10.0), None)
        .expect("published");
    announcer
        .dense_group(&bus, MissionTime(10.0), Some(&first))
        .expect("published");
    announcer
        .dense_group(&bus, MissionTime(10.1), Some(&first))
        .expect("published");
    announcer
        .dense_group(&bus, MissionTime(11.0), Some(&second))
        .expect("published");
    announcer
        .dense_group(&bus, MissionTime(12.0), None)
        .expect("published");
    announcer
        .dense_group(&bus, MissionTime(12.1), None)
        .expect("published");

    let published: Vec<Option<DenseGroupView>> = rx
        .try_iter()
        .map(|envelope| match envelope.event {
            Event::Tracking(TrackingEvent::DenseGroup(group)) => group,
            other => panic!("only dense-group events were published: {other:?}"),
        })
        .collect();
    assert_eq!(published, vec![Some(first), Some(second), None]);
}
