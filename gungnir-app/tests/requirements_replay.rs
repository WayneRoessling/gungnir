// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The MT-08 replay: collection requirements worked while TT-08 plays through the
//! desktop (GAP-067 walk, 2026-09-16).
//!
//! The `gungnir-workflow` Collection requirements and tasking concurrence (CAP-2.12) row
//! of `docs/verification-capability-table.md` §2, whose method is "MT-08 replay" and whose
//! data source is TT-08. Its criterion, agreed 2026-09-05 and unchanged: "A requirement
//! moves from stated to tasked only with a concurrence carrying an operator; satisfaction
//! always references evidence; a requirement past its needed-by time lapses rather than
//! remaining open".
//!
//! # What is replayed and what is scripted
//!
//! The detections are TT-08-sample's own, released by the recorded-feed adapter into the
//! ingest gateway as a stepped mission clock reaches them: the mechanism
//! `gungnir_app::laydown_rehearsal` uses. The operator actions are TT-08's scripted events
//! (`docs/test-tracks/data-format.md` §6), read from the sample's `events.jsonl` and
//! performed at their times: an intelligence analyst states two requirements, a signed-in
//! sensor manager tasks a commandable sensor for both, the analyst answers one with
//! evidence, and the other passes its needed-by time while tasked. **Nothing about the
//! script is written into this file.** Which requirement has to lapse, which has to be
//! answered, by whom and with what, all follow from the script's own fields and times, so
//! the expected outcomes come from the scenario and the criterion rather than from the
//! code under test.
//!
//! One step is this test's own rather than the script's. At the first scripted tasking,
//! before the sensor manager signs in, the tasking is tried with nobody signed in, which
//! the GAP-067 walk made a refusal: the requirement has to stay stated, no command may be
//! issued, and nothing may be published.
//!
//! # What is read back
//!
//! The session is saved and the desktop dropped, and the criterion is checked on the
//! journal: each requirement event through `gungnir_replay::ReplaySession`, the playback a
//! review uses, and the requirement list through `requirements::recover`, which is what a
//! restarted desktop shows. A property that held only in memory is not one a replay could
//! ever check.
//!
//! # Bounded in time
//!
//! One frame per mission second over the sample's 900 s, where a live desktop draws thirty.
//! Every scripted time is a whole second, the gateway tolerates a receipt up to 5 s ahead,
//! and a lapse is decided on the tick, so the coarser step loses nothing the criterion
//! reads and costs 901 frames instead of 27 000.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gungnir_app::requirements::{self, Recovered, RequirementError};
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{
    AssetConfig, AuthenticationConfig, AuthenticationProvider, ConfigBaseline, EndpointConfig,
    SecurityConfig, SensorConfig,
};
use gungnir_coord::{CoordTransform, Enu, Geodetic, Wgs84};
use gungnir_eventing::Event;
use gungnir_ingest::adapters::recorded::RecordedFeedAdapter;
use gungnir_model::events::{IngestEvent, RequirementEvent};
use gungnir_model::{
    AssetPriority, CollectionRequirement, MissionTime, RequirementId, RequirementState,
};
use gungnir_replay::ReplaySession;
use gungnir_security::{hash_passphrase, Account, LocalAccountAuthority, OperatorId, Role};
use gungnir_sensor_management::SensorControl;
use gungnir_store::{FileEventJournal, SessionId};
use gungnir_time::ReplayClockAuthority;

const PASSPHRASE: &str = "correct horse battery staple";

/// One frame per mission second (see "Bounded in time" above).
const STEP_S: f64 = 1.0;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn sample() -> PathBuf {
    workspace().join("testdata/tracks/samples/TT-08-sample")
}

/// The parts of `metadata.json` this replay reads (`data-format.md` §7).
#[derive(serde::Deserialize)]
struct Metadata {
    duration_s: f64,
    origin: Origin,
}

/// The scenario's fictional origin, in degrees (`data-format.md` §2).
#[derive(serde::Deserialize)]
struct Origin {
    lat: f64,
    lon: f64,
    alt_m: f64,
}

/// The parts of `sensors.json` this replay reads (`data-format.md` §5).
#[derive(serde::Deserialize)]
struct SensorsFile {
    sensors: Vec<SampleSensor>,
}

#[derive(serde::Deserialize)]
struct SampleSensor {
    id: u32,
    #[serde(rename = "type")]
    kind: String,
    /// ENU metres from the scenario origin.
    pos: [f64; 3],
    params: SensorParams,
}

#[derive(serde::Deserialize)]
struct SensorParams {
    range_m: BTreeMap<String, f64>,
}

/// One scripted operator action, as `data-format.md` §6 defines the three kinds.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind")]
enum Step {
    #[serde(rename = "requirement_stated")]
    Stated {
        t: f64,
        requirement: String,
        by: String,
        question: String,
        area: String,
        radius_m: f64,
        priority: String,
        needed_by: f64,
    },
    #[serde(rename = "requirement_tasked")]
    Tasked {
        t: f64,
        requirement: String,
        by: String,
        sensor: u32,
    },
    #[serde(rename = "requirement_satisfied")]
    Satisfied {
        t: f64,
        requirement: String,
        by: String,
        evidence: String,
        sensors: Vec<u32>,
    },
}

impl Step {
    fn t(&self) -> f64 {
        match self {
            Step::Stated { t, .. } | Step::Tasked { t, .. } | Step::Satisfied { t, .. } => *t,
        }
    }

    fn requirement(&self) -> &str {
        match self {
            Step::Stated { requirement, .. }
            | Step::Tasked { requirement, .. }
            | Step::Satisfied { requirement, .. } => requirement,
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The collection-requirement steps of the sample's `events.jsonl`, in time order.
///
/// Every other kind is the generator's business and is left alone; a `requirement_` kind
/// this replay does not know fails loudly rather than being skipped, because a skipped
/// step would make the replay pass on less of the script than it claims.
fn script() -> Vec<Step> {
    let path = sample().join("events.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut steps: Vec<Step> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
            let scripted = value
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind.starts_with("requirement_"));
            scripted
                .then(|| serde_json::from_value(value).unwrap_or_else(|e| panic!("{line}: {e}")))
        })
        .collect();
    steps.sort_by(|a, b| a.t().total_cmp(&b.t()));
    steps
}

/// The account a scripted role acts through. One per role the script names, each with a
/// real passphrase hash in the desktop's local account file.
fn account(by: &str) -> (OperatorId, Role) {
    match by {
        "intelligence-analyst" => (OperatorId(21), Role::IntelligenceAnalyst),
        "sensor-manager" => (OperatorId(31), Role::SensorManager),
        other => panic!("the script acts as {other}, and this replay holds no account for it"),
    }
}

fn write_accounts(dir: &Path) {
    let accounts: Vec<Account> = ["intelligence-analyst", "sensor-manager"]
        .into_iter()
        .map(|by| {
            let (operator, role) = account(by);
            Account {
                operator,
                role,
                phc: hash_passphrase(PASSPHRASE).expect("hashed"),
            }
        })
        .collect();
    std::fs::write(
        dir.join("accounts.json"),
        serde_json::to_string(&accounts).expect("json"),
    )
    .expect("the account file is written");
}

/// ENU metres about `origin`, as the geodetic `[lat_rad, lon_rad, alt_m]` a baseline takes.
fn geodetic_of(enu_m: [f64; 3], origin: Geodetic) -> [f64; 3] {
    let enu = Enu {
        e_m: enu_m[0],
        n_m: enu_m[1],
        u_m: enu_m[2],
    };
    let g = Wgs84::ecef_to_geodetic(Wgs84::enu_to_ecef(enu, origin));
    [g.lat_rad, g.lon_rad, g.alt_m]
}

/// One defended asset per stated requirement, over the named point and radius the script
/// gives, in script order: a requirement can only be stated over an asset the baseline
/// declares (`requirements::state_requirement`).
fn assets(steps: &[Step], origin: Geodetic) -> Vec<AssetConfig> {
    let library = gungnir_scenario::TrackLibrary::load(&workspace().join("docs/test-tracks"))
        .expect("the test-track library loads");
    steps
        .iter()
        .filter_map(|step| match step {
            Step::Stated {
                area,
                radius_m,
                priority,
                ..
            } => Some((area, *radius_m, priority)),
            _ => None,
        })
        .zip(1u32..)
        .map(|((area, radius_m, priority), id)| {
            let point = library
                .scenarios
                .points
                .get(area.as_str())
                .unwrap_or_else(|| {
                    panic!("the script names {area}, which scenarios.yaml does not")
                });
            AssetConfig {
                id,
                name: area.clone(),
                position: geodetic_of([point[0].f(), point[1].f(), point[2].f()], origin),
                radius_m: Some(radius_m),
                priority: priority.clone(),
                warning_lead_time_s: None,
                warning_channel: None,
                warning_within_m: None,
                note: None,
            }
        })
        .collect()
}

/// The desktop the replay runs on.
///
/// The sample's own sensors where the sample put them, so its detections pass the
/// gateway's allow-list; the sensors the script tasks declared commandable, the way
/// `tests/requirements.rs` declares one; the requirement areas from [`assets`]; and a
/// local account file.
fn baseline(dir: &Path, steps: &[Step], origin: Geodetic) -> ConfigBaseline {
    let sensors: SensorsFile = read_json(&sample().join("sensors.json"));
    let tasked: BTreeSet<u32> = steps
        .iter()
        .filter_map(|step| match step {
            Step::Tasked { sensor, .. } => Some(*sensor),
            _ => None,
        })
        .collect();
    let control = |id: u32| format!("sensor-{id}-control");

    ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some([origin.lat_rad, origin.lon_rad, origin.alt_m]),
        sensors: sensors
            .sensors
            .iter()
            .map(|s| SensorConfig {
                id: s.id,
                modality: s.kind.clone(),
                position: geodetic_of(s.pos, origin),
                max_range_m: s.params.range_m.values().copied().fold(0.0, f64::max),
                control_endpoint: tasked.contains(&s.id).then(|| control(s.id)),
                maintenance: Vec::new(),
                detection_model: None,
                azimuth_sector: None,
            })
            .collect(),
        endpoints: tasked
            .iter()
            .map(|id| EndpointConfig {
                name: control(*id),
                kind: "sensor-control".into(),
                address: format!("tcp://127.0.0.1:{}", 9100 + id),
            })
            .collect(),
        assets: assets(steps, origin),
        security: SecurityConfig {
            authentication: AuthenticationConfig {
                provider: AuthenticationProvider::LocalAccounts {
                    accounts_path: "accounts.json".into(),
                },
                session_lifetime_s: None,
            },
            ..SecurityConfig::default()
        },
        ..ConfigBaseline::default()
    }
}

/// The desktop under replay, and what the script's labels became on it.
struct Desktop {
    state: AppState,
    /// The requirement each script label was stated as.
    ids: BTreeMap<String, RequirementId>,
    /// The asset the next stated requirement is over; [`assets`] declares them in order.
    next_area: usize,
    refusal_tried: bool,
}

impl Desktop {
    fn id(&self, label: &str) -> RequirementId {
        *self
            .ids
            .get(label)
            .unwrap_or_else(|| panic!("the script acts on {label} before stating it"))
    }

    fn standing(&self, id: RequirementId) -> RequirementState {
        self.state
            .requirements
            .iter()
            .find(|r| r.id == id)
            .expect("stated")
            .state
            .clone()
    }

    /// Be the operator the script says acts: whoever else is at the console signs out,
    /// and the scripted role's operator signs in with a real credential.
    fn act_as(&mut self, by: &str) {
        let (operator, role) = account(by);
        if self.state.attributed_operator() != Some(operator) {
            self.state.sign_out();
            self.state
                .sign_in(&LocalAccountAuthority::credential(operator, PASSPHRASE))
                .unwrap_or_else(|e| panic!("operator {} could not sign in: {e}", operator.0));
        }
        self.state.set_role(role);
    }

    fn perform(&mut self, step: &Step, now: MissionTime) {
        match step {
            Step::Stated {
                requirement,
                by,
                question,
                priority,
                needed_by,
                ..
            } => {
                self.act_as(by);
                let priority = AssetPriority::parse(priority)
                    .unwrap_or_else(|| panic!("the script gives priority {priority}"));
                // The desktop takes a window from now; the script states the time.
                let within_minutes = (needed_by - now.0) / 60.0;
                let id = requirements::state_requirement(
                    &mut self.state,
                    question.clone(),
                    self.next_area,
                    priority,
                    Some(within_minutes),
                )
                .unwrap_or_else(|e| panic!("{requirement} was not stated: {e}"));
                self.next_area += 1;
                self.ids.insert(requirement.clone(), id);
            }
            Step::Tasked {
                requirement,
                by,
                sensor,
                ..
            } => {
                let id = self.id(requirement);
                if !self.refusal_tried {
                    self.refusal_tried = true;
                    self.tasking_with_nobody_signed_in_is_refused(id, *sensor, now);
                }
                self.act_as(by);
                requirements::task(&mut self.state, id, *sensor, now)
                    .unwrap_or_else(|e| panic!("{requirement} was not tasked: {e}"));
            }
            Step::Satisfied {
                requirement,
                by,
                evidence,
                ..
            } => {
                self.act_as(by);
                let id = self.id(requirement);
                requirements::satisfy(&mut self.state, id, evidence.clone())
                    .unwrap_or_else(|e| panic!("{requirement} was not answered: {e}"));
            }
        }
    }

    /// The step this test adds to the script: the tasking tried with nobody at the
    /// console. Refused, with the reason; the requirement still stated; no command
    /// issued; nothing published.
    fn tasking_with_nobody_signed_in_is_refused(
        &mut self,
        id: RequirementId,
        sensor: u32,
        now: MissionTime,
    ) {
        self.state.sign_out();
        // A sensor manager's console with nobody signed in: the role that may task,
        // selected, so the refusal under test is the missing operator's. Since GAP-127
        // (2026-09-17) `task` asks the permission first, and whichever role the previous
        // step left selected would otherwise be refused for want of `sensor.task`.
        self.state.set_role(gungnir_security::Role::SensorManager);
        assert_eq!(self.state.attributed_operator(), None);
        let tasks_before = self.state.sensors.tasks().len();
        let seen = self.state.events.subscribe();

        let err = requirements::task(&mut self.state, id, sensor, now)
            .expect_err("a requirement was tasked with nobody signed in");
        let RequirementError::Unattributed { requirement, .. } = &err else {
            panic!("refused for some other reason: {err}");
        };
        assert_eq!(*requirement, id);
        assert!(
            err.to_string().contains("Sign in to task it"),
            "the refusal did not say to sign in: {err}"
        );
        assert_eq!(
            self.standing(id),
            RequirementState::Stated,
            "a refused tasking moved the requirement"
        );
        assert_eq!(
            self.state.sensors.tasks().len(),
            tasks_before,
            "a command was issued for a concurrence that was refused"
        );
        assert!(
            seen.try_iter()
                .all(|envelope| !matches!(envelope.event, Event::Requirement(_))),
            "a refused tasking was published"
        );
    }
}

/// Replay the sample, performing each scripted step at its time, then save the session.
/// Returns the data directory, the session to read, and what each label became.
fn replay(
    steps: &[Step],
    metadata: &Metadata,
) -> (PathBuf, SessionId, BTreeMap<String, RequirementId>) {
    let origin = Geodetic {
        lat_rad: metadata.origin.lat.to_radians(),
        lon_rad: metadata.origin.lon.to_radians(),
        alt_m: metadata.origin.alt_m,
    };
    let dir = std::env::temp_dir().join(format!("gungnir-mt08-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    write_accounts(&dir);
    let config = baseline(&dir, steps, origin);
    gungnir_config::validate(&config).expect("the replay's baseline validates");

    let state = AppState::with_config(config).expect("the desktop starts");
    assert!(
        state.accounts.as_ref().is_ok_and(|a| a.len() == 2),
        "the local account store did not load: {:?}",
        state.accounts
    );
    let mut desktop = Desktop {
        state,
        ids: BTreeMap::new(),
        next_area: 0,
        refusal_tried: false,
    };
    let feed = RecordedFeedAdapter::open(&sample().join("detections.jsonl"))
        .expect("the sample's detections parse");
    desktop.state.ingest.add_adapter(Box::new(feed));

    let mut pending = steps.iter().peekable();
    let mut second = 0.0;
    while second <= metadata.duration_s {
        let now = MissionTime(second);
        desktop.state.clock = Box::new(ReplayClockAuthority { current: now });
        while let Some(step) = pending.next_if(|step| step.t() <= now.0) {
            desktop.perform(step, now);
        }
        update::tick(&mut desktop.state);
        second += STEP_S;
    }
    assert!(
        pending.next().is_none(),
        "a scripted step falls after the sample ends, so the replay never performed it"
    );
    assert!(
        desktop.refusal_tried,
        "the script tasks nothing, so no refusal was tried"
    );

    let Desktop { mut state, ids, .. } = desktop;
    state.save_session().expect("the session saves");
    let session = state.session().expect("the desktop opened a session");
    // Dropped before the journal is reopened: the desktop still holds its files, and on
    // Windows a reader beside an open writer is the case `laydown_rehearsal` found reading
    // stale data.
    drop(state);
    (dir, session, ids)
}

/// What the saved journal says, read back the two ways a review reads it.
struct ReadBack {
    ids: BTreeMap<String, RequirementId>,
    /// Every requirement event, in journal order, with its mission time.
    lifecycle: Vec<(MissionTime, RequirementEvent)>,
    /// Every detection the gateway accepted: when, and from which sensor.
    accepted: Vec<(MissionTime, u32)>,
    /// The requirement list a restarted desktop would recover.
    recovered: Vec<CollectionRequirement>,
}

impl ReadBack {
    fn read(dir: &Path, session: SessionId, ids: BTreeMap<String, RequirementId>) -> Self {
        let journal = FileEventJournal::open(dir).expect("the journal reopens");
        let mut lifecycle = Vec::new();
        let mut accepted = Vec::new();
        let mut playback = ReplaySession::open(&journal, session).expect("the session reads back");
        while let Some(envelope) = playback.step() {
            match &envelope.event {
                Event::Requirement(event) => {
                    lifecycle.push((envelope.mission_time, event.clone()));
                }
                Event::Ingest(IngestEvent::Accepted(d)) => {
                    accepted.push((envelope.mission_time, d.sensor.0));
                }
                _ => {}
            }
        }
        let (recovered, outcome) = requirements::recover(&journal);
        assert_eq!(outcome, Recovered::FromJournal { sessions: 1 });
        Self {
            ids,
            lifecycle,
            accepted,
            recovered,
        }
    }

    fn history(&self, label: &str) -> Vec<(MissionTime, &RequirementEvent)> {
        let id = self.ids[label];
        self.lifecycle
            .iter()
            .filter(|(_, e)| e.requirement() == id)
            .map(|(at, e)| (*at, e))
            .collect()
    }

    fn recovered(&self, label: &str) -> &CollectionRequirement {
        let id = self.ids[label];
        self.recovered
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("{label} did not come back from the journal"))
    }

    fn records(&self, kind: fn(&RequirementEvent) -> bool) -> Vec<&RequirementEvent> {
        self.lifecycle
            .iter()
            .map(|(_, e)| e)
            .filter(|e| kind(e))
            .collect()
    }
}

/// What was stated is what the script stated: the question, at the priority, over the
/// radius, and needed by the time it gave.
fn stated_as_scripted(steps: &[Step], back: &ReadBack) {
    for step in steps {
        let Step::Stated {
            requirement,
            question,
            radius_m,
            priority,
            needed_by,
            ..
        } = step
        else {
            continue;
        };
        let stated = back.recovered(requirement);
        assert_eq!(&stated.title, question);
        assert_eq!(Some(stated.priority), AssetPriority::parse(priority));
        assert!(
            (stated.area.radius_m() - radius_m).abs() < 1e-9,
            "{requirement} is over {} m, not the scripted {radius_m} m",
            stated.area.radius_m()
        );
        let by = stated.needed_by.expect("stated with a needed-by time").0;
        assert!(
            (by - needed_by).abs() < 1e-9,
            "{requirement} is needed by {by}, not the scripted {needed_by}"
        );
    }
}

/// "A requirement moves from stated to tasked only with a concurrence carrying an
/// operator": every tasked record names one, and it is the operator of the role the
/// script says concurred. One record per scripted tasking, so the refused try left none.
fn tasked_only_with_an_operator(steps: &[Step], back: &ReadBack) {
    let scripted: Vec<(&str, &str)> = steps
        .iter()
        .filter_map(|step| match step {
            Step::Tasked {
                requirement, by, ..
            } => Some((requirement.as_str(), by.as_str())),
            _ => None,
        })
        .collect();
    let records = back.records(|e| matches!(e, RequirementEvent::Tasked { .. }));
    assert_eq!(
        records.len(),
        scripted.len(),
        "tasked records do not match the scripted taskings: {records:?}"
    );
    for record in &records {
        let RequirementEvent::Tasked { by, .. } = record else {
            unreachable!("filtered to tasked records")
        };
        assert!(
            by.operator().is_some(),
            "a requirement was recorded tasked on a concurrence naming nobody: {record:?}"
        );
    }
    for (requirement, by) in scripted {
        let (operator, role) = account(by);
        let operator = operator.0.to_string();
        let role = format!("{role:?}");
        let named = back.history(requirement).into_iter().any(|(_, e)| {
            matches!(e, RequirementEvent::Tasked { by: c, .. }
                if c.operator() == Some(operator.as_str()) && c.role() == role)
        });
        assert!(
            named,
            "{requirement}'s tasking does not name operator {operator} as {role}"
        );
    }
}

/// "Satisfaction always references evidence": every answer names evidence, it is the
/// evidence the script gave, and the sensors that evidence rests on had really reported
/// something the gateway accepted by the time it was given.
fn answered_only_with_evidence(steps: &[Step], back: &ReadBack) {
    let records = back.records(|e| matches!(e, RequirementEvent::Satisfied { .. }));
    let scripted = steps
        .iter()
        .filter(|s| matches!(s, Step::Satisfied { .. }))
        .count();
    assert_eq!(records.len(), scripted, "{records:?}");
    for record in &records {
        let RequirementEvent::Satisfied { evidence, .. } = record else {
            unreachable!("filtered to answers")
        };
        assert!(
            !evidence.trim().is_empty(),
            "a requirement was answered with no evidence: {record:?}"
        );
    }
    for step in steps {
        let Step::Satisfied {
            t,
            requirement,
            evidence,
            sensors,
            ..
        } = step
        else {
            continue;
        };
        assert_eq!(
            back.recovered(requirement).state,
            RequirementState::Satisfied {
                evidence: evidence.clone()
            },
            "{requirement} did not come back answered with its evidence"
        );
        for sensor in sensors {
            assert!(
                back.accepted
                    .iter()
                    .any(|(at, s)| s == sensor && at.0 <= *t),
                "{requirement}'s evidence rests on sensor {sensor}, which had reported \
                 nothing the gateway accepted by T+{t} s"
            );
        }
    }
}

/// "A requirement past its needed-by time lapses rather than remaining open": every
/// stated requirement the script does not answer by its time, where that time falls
/// inside the sample, comes back lapsed and closed -- lapsed on the first frame past the
/// time, and lapsed from tasked where the script tasked it first. At least one has to be,
/// or the clause went unchecked.
fn overdue_lapses_rather_than_remaining_open(steps: &[Step], back: &ReadBack, end_s: f64) {
    let mut lapsed_while_tasked = 0;
    for step in steps {
        let Step::Stated {
            requirement,
            needed_by,
            ..
        } = step
        else {
            continue;
        };
        let before_its_time = |kind: fn(&Step) -> bool| {
            steps
                .iter()
                .any(|s| kind(s) && s.requirement() == requirement && s.t() <= *needed_by)
        };
        if before_its_time(|s| matches!(s, Step::Satisfied { .. })) || needed_by + STEP_S > end_s {
            continue;
        }
        let overdue = back.recovered(requirement);
        assert_eq!(
            overdue.state,
            RequirementState::Lapsed,
            "{requirement} is past its needed-by time and did not lapse"
        );
        assert!(
            !overdue.is_open(),
            "{requirement} remained open past its time"
        );
        let history = back.history(requirement);
        let lapsed_at = history
            .iter()
            .find_map(|(at, e)| matches!(e, RequirementEvent::Lapsed { .. }).then_some(at.0))
            .unwrap_or_else(|| panic!("{requirement} lapsed with no lapse on the record"));
        assert!(
            lapsed_at > *needed_by && lapsed_at <= needed_by + STEP_S,
            "{requirement} lapsed at T+{lapsed_at} s against a needed-by of T+{needed_by} s"
        );
        if before_its_time(|s| matches!(s, Step::Tasked { .. })) {
            let kinds: Vec<&str> = history
                .iter()
                .map(|(_, e)| match e {
                    RequirementEvent::Stated { .. } => "stated",
                    RequirementEvent::Tasked { .. } => "tasked",
                    RequirementEvent::Declined { .. } => "declined",
                    RequirementEvent::Satisfied { .. } => "satisfied",
                    RequirementEvent::Lapsed { .. } => "lapsed",
                })
                .collect();
            assert_eq!(
                kinds,
                ["stated", "tasked", "lapsed"],
                "{requirement} did not pass its needed-by time while tasked"
            );
            lapsed_while_tasked += 1;
        }
    }
    assert!(
        lapsed_while_tasked > 0,
        "no scripted requirement passes its needed-by time while tasked, so the lapse clause \
         went unchecked"
    );
}

/// **The MT-08 replay.** See the module documentation for what is replayed, scripted and
/// read back; each helper called below checks one clause of the criterion.
#[test]
fn mt08_replayed_through_tt08_meets_the_requirement_criterion() {
    let steps = script();
    // A script missing any of the three kinds would pass the clause it lacks by having
    // nothing to check.
    for (kind, present) in [
        (
            "stated",
            steps.iter().any(|s| matches!(s, Step::Stated { .. })),
        ),
        (
            "tasked",
            steps.iter().any(|s| matches!(s, Step::Tasked { .. })),
        ),
        (
            "satisfied",
            steps.iter().any(|s| matches!(s, Step::Satisfied { .. })),
        ),
    ] {
        assert!(
            present,
            "TT-08-sample's events.jsonl scripts no requirement being {kind}"
        );
    }

    let metadata: Metadata = read_json(&sample().join("metadata.json"));
    let (dir, session, ids) = replay(&steps, &metadata);
    let back = ReadBack::read(&dir, session, ids);

    stated_as_scripted(&steps, &back);
    tasked_only_with_an_operator(&steps, &back);
    answered_only_with_evidence(&steps, &back);
    overdue_lapses_rather_than_remaining_open(&steps, &back, metadata.duration_s);

    let _ = std::fs::remove_dir_all(dir);
}
