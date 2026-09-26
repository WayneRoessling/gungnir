// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node applies journal retention at start (GAP-122, decision D-78).
//!
//! The real binary, over a data directory holding sessions older and newer than the
//! baseline's limit and one old session under a hand-placed hold: once the node has
//! opened its live session, the old unheld session is gone with its mission record, the
//! rest read back exactly, and the removal is on the live session's record.
//!
//! **Both platforms.** Nothing here depends on how the node is stopped: the purge runs
//! before the loop, and the child is killed outright once its journal says the purge
//! happened, because this test is about retention and `headless_loop.rs` is the one
//! about shutdown.

use gungnir_config::{ConfigBaseline, NodeConfig};
use gungnir_eventing::{Envelope, Event};
use gungnir_mission::{JournalMissionManager, MissionManager, MissionState};
use gungnir_model::events::RetentionEvent;
use gungnir_model::{MissionTime, SessionId, TrackId};
use gungnir_store::retention::RetentionPolicy;
use gungnir_store::{journal, DurabilityPolicy, EventJournal, FileEventJournal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const DAY: Duration = Duration::from_hours(24);

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-node-retention-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn envelopes(session: SessionId) -> Vec<Envelope> {
    (0..3)
        .map(|i| Envelope {
            seq: i,
            mission_time: MissionTime(1.0 / 3.0 + f64::from(u32::try_from(i).unwrap_or(0))),
            event: Event::Tracking(gungnir_model::events::TrackingEvent::TrackDeleted(TrackId(
                session.0 * 10 + i,
            ))),
        })
        .collect()
}

fn earlier_session(dir: &Path, idle: Duration) -> SessionId {
    let mut journal =
        FileEventJournal::open_with_policy(dir, DurabilityPolicy::node()).expect("journal");
    let session = {
        let mut missions = JournalMissionManager::open(dir, &journal).expect("missions");
        let mut mission = missions.create(ConfigBaseline::default()).expect("created");
        missions
            .transition(&mut mission, MissionState::Live)
            .expect("live");
        let session = mission.session;
        missions.close(mission).expect("closed");
        session
    };
    for envelope in envelopes(session) {
        journal.append(session, &envelope).expect("appended");
    }
    drop(journal);
    let at = SystemTime::now() - idle;
    for path in [
        dir.join(journal::session_file_name(session)),
        dir.join(format!("{}.mission.json", session.0)),
    ] {
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("file")
            .set_modified(at)
            .expect("mtime");
    }
    session
}

/// What the live session has recorded about retention so far.
fn purges_recorded(data: &Path, live: SessionId) -> Vec<SessionId> {
    let Ok(journal) = FileEventJournal::open(data) else {
        return Vec::new();
    };
    journal
        .read_session(live)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| match e.event {
            Event::Retention(RetentionEvent::Purged { session, .. }) => Some(session),
            _ => None,
        })
        .collect()
}

#[test]
fn the_node_purges_at_start_and_journals_what_it_removed() {
    let dir = scratch();
    let data = dir.join("journal");
    std::fs::create_dir_all(&data).expect("data dir");
    let old = earlier_session(&data, DAY * 45);
    let held = earlier_session(&data, DAY * 45);
    let recent = earlier_session(&data, DAY * 5);
    FileEventJournal::open(&data)
        .expect("journal")
        .hold(held, "kept for an investigation")
        .expect("held");

    let config = ConfigBaseline {
        node: Some(NodeConfig {
            // Port 0: the operating system picks one, so a parallel run cannot collide.
            bind_addr: "127.0.0.1:0".into(),
            data_dir: data.to_string_lossy().into_owned(),
        }),
        retention: Some(RetentionPolicy {
            max_session_age_days: 30,
            max_audit_log_age_days: 365,
        }),
        ..ConfigBaseline::default()
    };
    let config_path = dir.join("baseline.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("serialised"),
    )
    .expect("config written");

    let mut child = Command::new(env!("CARGO_BIN_EXE_gungnir-node"))
        .arg(&config_path)
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("gungnir-node spawns");

    // The live session is the next identifier after the three written above; the node
    // fsyncs every envelope (D-04), so what it journaled can be read while it runs.
    let live = SessionId(recent.0 + 1);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut recorded = Vec::new();
    while Instant::now() < deadline {
        recorded = purges_recorded(&data, live);
        if !recorded.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        recorded,
        vec![old],
        "the live session's record of the purge"
    );
    let journal = FileEventJournal::open(&data).expect("journal");
    let listed = journal.sessions().expect("listed");
    assert!(!listed.contains(&old));
    assert!(!data.join(format!("{}.mission.json", old.0)).exists());
    for kept in [held, recent] {
        assert_eq!(
            journal.read_session(kept).expect("kept"),
            envelopes(kept),
            "session {} changed",
            kept.0
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
