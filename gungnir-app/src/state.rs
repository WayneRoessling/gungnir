// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Single source of truth for rendering (rust-ui-architecture-coding-standards.md
//! §2) -- but per docs/gungnir-capabilities.md §9 ("`AppState` must not become the
//! system of record"), this struct is a local, render-friendly *projection* of
//! durable mission state, not the authoritative store itself. The durable record is
//! the `gungnir-store` journal: this desktop's own in the disconnected profile, the
//! service node's in the connected profiles (ARCHITECTURE.md §8).
//!
//! Backend selection: `gungnir-config`'s `BackendConfig` picks the embedded services
//! (`LiveTrackingService`, `DpInterceptService`) or the `gungnir-remote` clients. If
//! a remote endpoint cannot be reached the desktop falls back to embedded and raises
//! an alert, which is the required degraded-connectivity behaviour (§8.4).

use crate::decisions::{DenialHistory, QueueOutcomeCounts};
use gungnir_command::InMemoryApprovalWorkflow;
use gungnir_config::{
    BackendConfig, ConfigBaseline, ConfigError, ConfigStore, FileConfigStore, KnownVocabulary,
};
use gungnir_data::DataStore;
use gungnir_eventing::{Envelope, EventBus, InProcessBus, Receiver};
use gungnir_ingest::{AllowListAuthenticator, IngestGateway};
use gungnir_intercept_service::{DpInterceptService, InterceptService};
use gungnir_mission::{JournalMissionManager, Mission, MissionManager, MissionState};
use gungnir_model::{
    CollectionRequirement, PlanView, ResourceView, SensorId, SystemHealth, TrackId,
};
use gungnir_security::{InMemoryAuditLog, Role};
use gungnir_sensor_management::InMemorySensorRegistry;
use gungnir_store::{DurabilityPolicy, EventJournal, FileEventJournal, SessionId, StoreError};
use gungnir_time::{TimeAuthority, WallClockAuthority};
use gungnir_tracking_service::{LiveTrackingService, TrackingService};
use gungnir_viewport3d::ViewportState;
use gungnir_workflow::WorkspaceLayout;
use std::collections::HashSet;

/// Environment variable naming the JSON config baseline to load; default baseline
/// when unset.
pub const CONFIG_ENV_VAR: &str = "GUNGNIR_CONFIG";

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("tokio runtime could not start: {0}")]
    Runtime(#[from] std::io::Error),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The session lifecycle refused to open a mission.
    ///
    /// Returned rather than absorbed: a desktop that could not record which session it
    /// is running would journal envelopes nothing could later attribute, and a mission
    /// fabricated to keep the window open would be exactly the fiction GAP-051 found.
    #[error(transparent)]
    Mission(#[from] gungnir_mission::MissionError),
}

pub struct AppState {
    pub tracking: Box<dyn TrackingService>,
    pub intercept: Box<dyn InterceptService>,
    pub data: DataStore,
    pub resources: Vec<ResourceView>,
    /// Warnings owed to assets, open and closed (GAP-042, DN-03).
    pub warnings: gungnir_workflow::warning::WarningLedger,
    /// The seeded session in progress, if this is a usability rehearsal (GAP-089).
    pub rehearsal: Option<crate::rehearsal::Rehearsal>,
    /// Where the configured terrain stands (GAP-023).
    pub terrain: crate::terrain::TerrainStatus,
    /// The radar feeds' service observations, drained each frame (GAP-064).
    pub service_sinks: Vec<gungnir_ingest::adapters::asterix::ServiceObservationSink>,
    /// Each bound feed's counters, by name, for PN-09 (GAP-001).
    pub feed_stats: Vec<(String, gungnir_ingest::adapters::asterix::FeedStatsSink)>,
    /// The AIS feeds' cooperative reports, drained each frame (GAP-010).
    pub ais_sinks: Vec<gungnir_ingest::adapters::ais::CooperativeSink>,
    /// Each bound AIS feed's counters, by name, for PN-09 (GAP-010).
    pub ais_stats: Vec<(String, gungnir_ingest::adapters::ais::AisStatsSink)>,
    /// The association memory between cooperative reports and tracks (GAP-010).
    pub cooperative: crate::cooperative::CooperativeState,
    /// The ADS-B feeds' cooperative reports, drained each frame (GAP-010).
    pub adsb_sinks: Vec<gungnir_ingest::adapters::adsb::CooperativeSink>,
    /// Each bound ADS-B feed's counters, by name, for PN-09 (GAP-010).
    pub adsb_stats: Vec<(String, gungnir_ingest::adapters::adsb::AdsbStatsSink)>,
    /// (track, ICAO address) pairs already submitted as evidence, same purpose as
    /// `cooperative.submitted` (GAP-010).
    pub adsb_submitted: std::collections::HashSet<(TrackId, u32)>,
    /// The association memory between ADS-B reports and tracks, for the platform class
    /// they declare (GAP-027); same purpose as `cooperative.by_track`, narrower.
    pub adsb_cooperative: std::collections::HashMap<TrackId, crate::adsb::LastAdsbCooperative>,
    /// Each bound SAPIENT feed's counters, by name, for PN-09 (GAP-001).
    pub sapient_stats: Vec<(
        String,
        gungnir_ingest::adapters::sapient::SapientFeedStatsSink,
    )>,
    /// Every bound SAPIENT feed's `TaskAck` sink, drained each frame (GAP-004): the
    /// inbound half of the outbound-tasking round trip `sapient_task.rs` issues.
    pub sapient_task_acks: Vec<gungnir_ingest::adapters::sapient::TaskAckSink>,
    /// The identification engine, fed by cooperative evidence and governed by the
    /// baseline's thresholds (GAP-010, GAP-018, DN-08 §5).
    pub identification: gungnir_identification::EvidenceFusionEngine,
    /// The persistent keystore, once an operator's sign-in has opened it (GAP-084,
    /// DN-22 amendment 3).
    pub keystore: Option<std::sync::Arc<gungnir_security::PersistentKeyProvider>>,
    /// The pipeline settings this desktop's tracker was built with (GAP-053).
    ///
    /// Kept because the prediction has to use the same motion model and the same process
    /// noise the filter runs (GAP-020): a predictor with a different process noise would
    /// draw an uncertainty the tracker never claimed. On a remote backend the tracks come
    /// from the node, which reads the same baseline, so the model is the same one.
    pub pipeline: gungnir_tracking_service::PipelineSettings,
    /// The node link, while one is up (GAP-050): the tick judges its silence.
    pub link: Option<gungnir_remote::link::NodeLink>,
    /// The link the registry's control adapter delivers through (GAP-004); `None`
    /// inside means the adapter refuses with the reason.
    pub link_control: std::sync::Arc<std::sync::Mutex<Option<gungnir_remote::link::NodeLink>>>,
    /// The node's task ids to this desktop's (GAP-004).
    pub node_task_map:
        std::collections::HashMap<gungnir_model::SensorTaskId, gungnir_model::SensorTaskId>,
    /// The peer links bound from the baseline (GAP-009), for PN-09.
    pub peer_links: Vec<crate::peers::BoundPeer>,
    /// The fallback in force after a node went silent, if any (GAP-050).
    pub fallback: Option<crate::failover::Fallback>,
    /// The node's history being fetched for the reconciliation (GAP-050).
    pub pending_history: Option<gungnir_remote::link::PendingHistory>,
    /// The endpoint transport (GAP-040), or `None` with the reason in the alerts.
    pub endpoint_client: Option<gungnir_remote::endpoint::EndpointClient>,
    /// Handoffs posted and not yet answered, or waiting to be posted again.
    pub pending_handoffs: Vec<crate::deliveries::PendingHandoff>,
    /// Warnings posted and not yet answered.
    pub pending_warnings: Vec<crate::deliveries::PendingWarning>,
    /// The loader thread, while a load is in flight.
    pub loader: Option<(
        crossbeam_channel::Sender<gungnir_data::LoadRequest>,
        crossbeam_channel::Receiver<gungnir_data::LoadResult>,
    )>,
    pub last_plan: PlanView,
    /// The id of the last plan `update::tick`'s own live-planner step (3) has already
    /// published and submitted, distinct from [`Self::last_plan`] (GAP-097).
    ///
    /// **Why a separate field.** [`Self::last_plan`] is what PN-04/PN-05 draw as the
    /// current recommendation, and `rehearsal.rs`'s scripted plans legitimately write
    /// it too, so an operator sees whichever plan -- live or scripted -- was proposed
    /// most recently. But that means comparing the live planner's fresh output
    /// against `last_plan` to decide "has this already been announced" answers a
    /// different question than it looks like: right after a scripted plan writes
    /// `last_plan`, the live planner's own unchanged assignment compares unequal to
    /// it and gets resubmitted, even though nothing about the live plan itself
    /// changed. `gungnir_intercept_service::DpInterceptService::fresh_plan` never
    /// reuses a [`gungnir_model::PlanId`] for a different assignment, so comparing
    /// only the id -- tracked here, touched only by the live-planner step -- answers
    /// the actual question without being disturbed by what else wrote `last_plan`.
    pub last_live_plan_id: Option<gungnir_model::PlanId>,
    /// The recommendation and its policy-checked alternatives for [`Self::last_plan`]
    /// (GAP-032), regenerated when the plan changes rather than every frame because each
    /// alternative is another allocator solve.
    ///
    /// Held on the state rather than computed in the panel so PN-05 cannot draw
    /// alternatives for one plan next to a different plan: the two are refreshed in the
    /// same step of the tick, and an empty list means no plan has been proposed yet.
    pub alternatives: Vec<gungnir_decision::CourseOfAction>,
    /// The rehearsal for the selected track being lost (GAP-032), or `None` when nothing
    /// is selected. Committed to nothing: it is produced by `DecisionSupport::what_if`,
    /// which is a `&self` method over shared borrows.
    pub what_if: Option<gungnir_decision::CourseOfAction>,
    /// The selection [`Self::what_if`] was computed for, so the rehearsal is recomputed
    /// when the operator points at a different track and **not once per frame** -- each
    /// one is another allocator solve. Cleared when the plan changes, which is the other
    /// thing that makes the held answer stale.
    pub(crate) what_if_for: Option<TrackId>,
    pub alerts: Vec<String>,
    pub viewport: ViewportState,

    /// Foundational productization layer (docs/gungnir-capabilities.md §5.1 / §7
    /// Increment 1). `mission` is the live session opened at launch; `config` is
    /// the applied baseline; `events` is the bus every subscriber reads from instead
    /// of polling `tracking.tracks()` directly; `journal` is this desktop's local
    /// system of record.
    pub mission: Option<Mission>,
    pub config: ConfigBaseline,
    pub events: Box<dyn EventBus>,
    pub health: SystemHealth,
    pub clock: Box<dyn TimeAuthority>,
    pub backend: BackendConfig,
    pub ingest: IngestGateway,

    /// The role this desktop is signed in as, and the workspace that follows from it
    /// (GAP-055). Held together so they cannot disagree: [`AppState::set_role`] is the
    /// only way to change either.
    ///
    /// Defaults to [`Role::Operator`]. There is no operator session yet -- D-02's
    /// short-lived signed tokens are GAP-057 -- so the role is not yet *authenticated*,
    /// only selected. It decides the layout, never an authorisation: every action still
    /// goes through `gungnir_security::role_permits`.
    role: Role,
    layout: WorkspaceLayout,

    /// The sensors this deployment has, and what each is doing (GAP-003).
    ///
    /// Built from the baseline, and **every sensor starts at Standby**, which is
    /// `SensorRecord::from_config`'s deliberate choice: a registry that came up
    /// claiming every sensor was searching would report coverage nobody had switched
    /// on. So a fresh desktop covers nothing until an operator says otherwise, and the
    /// coverage map says exactly that.
    pub sensors: InMemorySensorRegistry,
    /// The most recent refused mode change, shown by PN-10. A refusal that only
    /// reached the log would leave the operator looking at a row that did not move.
    pub sensor_error: Option<String>,

    /// Whether this deployment's journal is encrypted, and why not when it is not
    /// (GAP-084, DN-22 §5).
    ///
    /// Derived once at start-up from the baseline's provider and whether one could be
    /// built. **Never optimistic**: `Active` only when the journal is actually sealing,
    /// which is what `FileEventJournal::is_sealing` reports. A status strip claiming
    /// encryption a deployment is not performing is the failure DN-22 §5 calls worse than
    /// admitting it.
    pub encryption: gungnir_security::EncryptionStatus,

    /// The static hazard layer the baseline declares, stamped with its version
    /// (DN-14 §5, GAP-017). Built once at start; a static layer that pretended to be
    /// live would be the failure the note names.
    pub hazards: gungnir_geo::HazardLayer,

    /// Who is signed in, and the authority that decides (GAP-057, DN-23).
    ///
    /// **Defaults to a store that is not configured**, so a fresh desktop reports
    /// `SessionState::StoreUnavailable` and attributes nothing. That is DN-23 §5 rule 5:
    /// a console whose account store is missing **starts** and says what it cannot do,
    /// rather than refusing to run. A deployment installs a real store with
    /// [`AppState::set_session_authority`].
    ///
    /// The role remains separate and still decides only the layout. Signing in does not
    /// change the role, and a role selection has never been an authorisation.
    pub session: Box<dyn gungnir_security::SessionAuthority>,
    /// The accounts the store lists, for PN-20 (operator and role, never a hash), or the
    /// reason there is no list.
    pub accounts: Result<Vec<(gungnir_security::OperatorId, gungnir_security::Role)>, String>,
    /// The operator whose expiry the tick has already announced (GAP-057), so the alert
    /// is raised once and the strip carries it from then on.
    pub expiry_announced: Option<gungnir_security::OperatorId>,

    /// The collection requirements this session has stated (GAP-005).
    ///
    /// Only the requirements. The tasks serving them belong to `sensors`, which is what
    /// changes their state, so `gungnir_app::requirements` assembles a
    /// `gungnir_workflow::TaskingCase` on demand rather than storing one. A stored copy
    /// would be stale the first time the tick's timeout sweep ran, and PN-15 would
    /// report a requirement as being worked by a task that had already timed out.
    ///
    /// **Recovered from the journal at start-up** (GAP-005), so an analyst's list
    /// survives a restart. The lifecycle is on the event bus and the journal is what
    /// makes it durable, which is why `RequirementEvent::Stated` carries the whole
    /// requirement rather than its title.
    pub requirements: Vec<CollectionRequirement>,
    /// Where the list came from, so an empty one can say which kind of empty it is.
    pub recovered: crate::requirements::Recovered,
    /// Serial for [`AppState::next_requirement_id`]. Private so the only way to get an
    /// identifier is to take the next one, which is what stops two requirements sharing.
    next_requirement: u64,
    /// The most recent refused requirement action, shown by PN-15.
    pub requirement_error: Option<String>,

    /// Launch warnings this deployment has declared (GAP-009), recovered from the
    /// journal at start-up the same way [`Self::requirements`] is: an append-only
    /// record, since a declared warning is never withdrawn or amended.
    pub issued_launch_warnings: Vec<gungnir_model::LaunchWarningReport>,
    /// Where the list came from, so an empty one can say which kind of empty it is.
    pub launch_warnings_recovered: crate::launch_warning::Recovered,
    /// Serial for [`AppState::next_launch_warning_id`]. Private for the same reason
    /// `next_requirement` is.
    next_launch_warning: u64,

    /// The baseline file this desktop loaded, when it loaded one (GAP-071).
    ///
    /// `None` when it started from the built-in default, which is not the same as a
    /// missing file: there is nothing to write back to, and PN-14 says so rather than
    /// offering an apply button that could not do anything.
    pub config_store: Option<FileConfigStore>,

    /// Configuration and decision actions, append-only (`gungnir-security`). PN-14
    /// shows it, which is what makes an apply visible to the next person.
    pub audit: InMemoryAuditLog,

    /// The approval gate between a proposed plan and anything acting on it
    /// (GAP-038). Every plan the intercept service proposes goes through
    /// `decisions::submit`, which evaluates it against the policy chain and queues it
    /// for a human only if it clears. Nothing acts on a plan without a
    /// `DecisionRecord`, which is contract C-01.
    ///
    /// `denials` is kept so an empty queue can say *why* it is empty. That matters
    /// more here than anywhere else in the desktop: this build's queue is still always
    /// empty -- the pipeline produces tracks as of 2026-09-06, and the allocator that
    /// would turn them into a plan returns `NotImplemented` (GAP-029) -- and a
    /// permanently calm approval queue reads as "nothing needs deciding".
    pub approvals: InMemoryApprovalWorkflow,
    /// Engagements opened on actionable decisions (GAP-043, DN-06). Session state:
    /// the journal carries the events, and a replay rebuilds the outcomes from them.
    pub engagements: Vec<gungnir_intercept_service::engagement::Engagement>,
    /// Handoffs issued from actionable decisions and where their delivery stands
    /// (GAP-040, DN-07). Never delivered from this side until a transport exists.
    pub handoffs: Vec<crate::handoffs::HandoffRecord>,
    /// Decisions whose engaged track has been in the picture since they opened, so a
    /// track's absence can be read as "left" rather than "never arrived".
    pub engaged_seen: std::collections::HashSet<gungnir_model::DecisionId>,
    pub denials: DenialHistory,
    /// The watch's rhythm: how far the scheduler has run and the handover in progress
    /// (GAP-054, DN-21). Small on purpose -- the schedules are in the baseline and the
    /// maintenance windows are on the sensor records.
    pub rhythm: crate::rhythm::RhythmState,
    /// The anomaly detectors' memory between frames (GAP-021, DN-15).
    pub anomaly: crate::anomaly::AnomalyState,
    /// The cross-session identity resolver, folded from the retained sessions at start
    /// (GAP-019, GAP-025, DN-19).
    pub identity: crate::identity::IdentityState,
    /// Per-source clock skew, fed from every accepted detection (GAP-008, MOP-09).
    ///
    /// Held here rather than inside the clock authority because the authority is a trait
    /// object that reports `&self`, and the estimator needs feeding.
    pub clock_skew: gungnir_time::ClockSkewEstimator,
    /// Sources already alerted as out of sync (GAP-008, MOP-09): the flag is raised once
    /// per source, and PN-09 carries the live figure from then on.
    pub skew_alerted: std::collections::HashSet<u32>,
    /// The health last put on the record (GAP-047, MOE-06): a transition is journaled,
    /// a repeat is not.
    pub health_journaled: Option<SystemHealth>,
    /// Resources the last planning call declined to propose (DN-04 §5, GAP-030).
    ///
    /// Copied out of the planner each tick because `intercept` is a trait object and
    /// PN-05 cannot reach the concrete service. Empty for a remote backend: the node
    /// decided, and this desktop does not know what it held back.
    pub withheld: Vec<gungnir_intercept_service::WithheldResource>,
    /// The fires checks for the last submitted plan, every one with its result (GAP-036,
    /// DN-05 §7). Empty when the last plan was not a fires task.
    pub fires_checks: Vec<gungnir_model::DeconflictionCheck>,
    /// Which algorithm configuration this deployment governs (GAP-086, DN-24).
    ///
    /// The first thing in this workspace to construct a `gungnir-modelops` registry. It
    /// governs the record of what the deployment intends to run, not the filtering -- there
    /// is no pipeline to apply it to (GAP-011).
    pub governance: crate::governance::Governance,
    /// Whether this session has journaled what it opened with (GAP-086).
    ///
    /// One event per session, not one per frame: which configuration a session ran under is
    /// a fact about the session, and repeating it every tick would bury the promotions that
    /// are facts about moments in it.
    pub governance_recorded: bool,
    /// Expiries and escalations this session (GAP-034), which PN-17 reports as the
    /// honest measure of whether the queue is keeping up.
    pub queue_outcomes: QueueOutcomeCounts,

    /// The queued item PN-07 is deciding, and the text the operator has entered in it.
    /// Immediate mode has nowhere else to keep the reject reason across frames.
    selected_approval: Option<gungnir_ui::panels::approval_queue::PendingId>,
    pub dialog: gungnir_ui::panels::decision_dialog::DecisionDialogState,

    /// The track PN-04 is showing, if any (GAP-073).
    ///
    /// A track id rather than a `TrackView`, because the view is regenerated every
    /// tick: holding the struct would pin a stale copy on screen while the tracker
    /// moved on, which is the failure the staleness marking exists to prevent. A
    /// selection whose track has since been deleted resolves to `None`, so the card
    /// closes rather than showing the last state of a track that no longer exists.
    selected_track: Option<TrackId>,

    /// Which laydown option PN-16 has selected for PN-11's before-and-after preview
    /// (GAP-087's own remaining item). Session state, not baseline: a comparison an
    /// operator is drawing now, not a fact about the deployment.
    selected_laydown: Option<gungnir_model::LaydownId>,

    /// Which scenario PN-16's rehearsal picker currently offers to run (GAP-045).
    /// Session state, the same as the selection above; `TestTrackNumber(1)` is not a
    /// claim that TT-01 is somehow the default rehearsal, only that a picker needs an
    /// initial value and the first scenario is as good as any to start on.
    rehearsal_scenario: gungnir_model::TestTrackNumber,

    /// The last rehearsal run for each laydown that has one (GAP-045). Session state,
    /// like the selection above: a rehearsal is a real tick loop this desktop actually
    /// ran, not a fact recorded in the baseline, and it is gone the way any other
    /// unsaved comparison is.
    pub rehearsal_records: std::collections::HashMap<
        gungnir_model::LaydownId,
        crate::laydown_rehearsal::RehearsalRecord,
    >,

    pub(crate) journal: FileEventJournal,
    pub(crate) journal_rx: Receiver<Envelope>,
    pub journal_failed: bool,

    // Not yet wired into the update loop (scaffolded as their own crates, ready to
    // be added without touching this struct's existing fields):
    //   gungnir-identity / -identification -- would extend `tracking` output with
    //                                global identity + classification
    //   gungnir-policy / -command -- would sit between `intercept` and `last_plan`
    //                                as an approval gate, per ARCHITECTURE.md §7
    //   gungnir-security          -- would gate `mission`/`config` mutation
    //   gungnir-replay / -reporting -- would read from `journal` rather than from
    //                                live `tracking`/`intercept`
    /// The runtime the embedded services and the node link run on. Reachable so a
    /// sign-in can establish the link (GAP-057) and a sign-out can drop it.
    pub runtime: tokio::runtime::Runtime,
}

impl AppState {
    /// Build the desktop state from the configured baseline
    /// (`GUNGNIR_CONFIG`, or the default when unset).
    pub fn new() -> Result<Self, AppError> {
        let (config, store) = load_config()?;
        Self::with_config_and_store(config, store)
    }

    /// Build the desktop state from an explicit baseline.
    ///
    /// Split out of [`AppState::new`] so a harness or a test can point `data_dir` at
    /// a scratch directory and name its own sensors, rather than depending on
    /// whatever `GUNGNIR_CONFIG` happens to be in the environment. The binary always
    /// goes through [`AppState::new`]; this changes no behaviour for it.
    pub fn with_config(config: ConfigBaseline) -> Result<Self, AppError> {
        // No store: this baseline did not come from a file, so there is nothing to
        // write back to. PN-14 reports that rather than offering an inert apply.
        Self::with_config_and_store(config, None)
    }

    /// Build from a baseline and the file it was loaded from, when there was one.
    ///
    /// The store is passed in rather than rebuilt from the environment so it always
    /// names the file this baseline actually came from. Deriving it from
    /// `GUNGNIR_CONFIG` here would let a caller that supplied its own baseline end up
    /// with an apply button pointed at an unrelated file.
    // Wiring: one line per subsystem, and splitting it would hide the order they come
    // up in, which `update::tick` depends on.
    #[allow(clippy::too_many_lines)]
    pub fn with_config_and_store(
        config: ConfigBaseline,
        config_store: Option<FileConfigStore>,
    ) -> Result<Self, AppError> {
        let runtime = desktop_runtime()?;
        let mut alerts = Vec::new();
        let hazards = crate::hazards::layer_from_config(&config)?;
        // GAP-057: the session authority and the account listing the baseline names,
        // built before the baseline is moved into the state.
        let accounts = account_listing(&config);
        let session = build_session_authority(&config, &mut alerts);
        // GAP-053: the promoted algorithm baseline is read before the backends are
        // built, because it decides how the pipeline behind them filters.
        let governance = crate::governance::Governance::from_config(&config);
        let pipeline = pipeline_settings(&config, governance.in_force(), &mut alerts);
        let (tracking, intercept, backend) =
            build_backends(&config, runtime.handle(), pipeline.clone(), &mut alerts);

        let events = InProcessBus::new();
        let journal_rx = events.subscribe();
        // D-04 (`ARCHITECTURE.md` §10 item 19): the desktop journal is buffered and
        // fsynced on session save and every 5 s. The node takes the other profile and
        // fsyncs every envelope. `update::tick` calls `sync_if_due` each frame, which
        // is what makes the 5 s a wall-clock bound.
        let mut journal =
            FileEventJournal::open_with_policy(&config.data_dir, DurabilityPolicy::desktop())?;
        // GAP-019: what earlier sessions saw, so this one can recognise it.
        let identity =
            crate::identity::IdentityState::recover(&journal, config.reporting.retention_sessions);
        if let Some(reason) = &identity.unreadable {
            alerts.push(format!("cross-session identity is partial: {reason}"));
        }
        // The provider the baseline names, and what actually came of trying.
        // **Reported from `is_sealing`, not from the configuration**: a status derived
        // from what was asked for would say "encrypted" about a journal that is not.
        let (encryption, keystore) = build_encryption(&config, &mut journal, &mut alerts);

        let (requirements, recovered, next_requirement) =
            recover_requirements_or_alert(&journal, &mut alerts);
        let (issued_launch_warnings, launch_warnings_recovered, next_launch_warning) =
            recover_launch_warnings_or_alert(&journal, &mut alerts);

        // GAP-086: the registry the baseline describes, built through the real promotion
        // state machine. A deployment whose promoted candidate fails the gate does not stop
        // the desktop; it governs nothing and PN-14 says why.
        let governance = crate::governance::Governance::from_config(&config);
        if let Some(reason) = governance.unavailable() {
            alerts.push(format!(
                "No algorithm configuration is in force ({reason}); this deployment is not governing which filter it runs"
            ));
        }

        let clock = WallClockAuthority::default();
        // **The session is created through the lifecycle, not fabricated here.** This
        // used to mint an identifier from the wall clock and declare the mission `Live`
        // with nothing on disk saying so, which is why a desktop killed mid-session left
        // a journal no record explained. `JournalMissionManager` writes the record beside
        // the journal, allocates past every identifier either store already holds, and
        // reports a session it finds still marked live as interrupted rather than
        // resumable.
        let mission = {
            let mut missions = JournalMissionManager::open(journal.root(), &journal)?;
            // What the last run left behind, before this one takes an identifier. An
            // interrupted session is not an error -- the process stopped, which happens --
            // but the operator is told, because the hole in that record is not in this one
            // and an after-action review reading them together must not miss it.
            report_interrupted_sessions(&mut missions, &mut alerts);

            let mut mission = missions.create(config.clone())?;
            missions.transition(&mut mission, MissionState::Live)?;
            mission
        };
        // The desktop starts under a baseline that does not validate on purpose -- a
        // console that refuses to open protects nobody -- but it does not start quietly.
        // The objection is in the mission record for the review, and on screen for the
        // operator who is about to act under it.
        if let Some(objection) = &mission.baseline_objection {
            alerts.push(format!(
                "This session is running under a baseline that did not validate ({objection}); decisions taken now carry that objection"
            ));
        }
        tracing::info!(session = mission.session.0, journal = %journal.root().display(), "opened live session");

        let (ingest, feeds, ais, adsb, sapient, endpoint_client, peers) =
            build_ingest(&config, runtime.handle(), &mut alerts);
        let identification_settings = config.policy.identification.clone();

        // The calibration baseline every record is stamped with. One string for the
        // whole registry until `gungnir-modelops` tracks them per sensor: a version
        // that varied per sensor without anything setting it would be fiction.
        let calibration_version = format!("baseline-v{}", config.version);
        // Built before `config` moves into the struct, for the same reason as the
        // decision settings below.
        let sensors = InMemorySensorRegistry::from_config(&config.sensors, &calibration_version);

        // Cloned before `config` moves into the struct: the workflow is timed by the
        // baseline in force, so the two cannot disagree about when an item expires.
        let decision_settings = config.policy.decisions.clone();

        Ok(Self {
            tracking,
            intercept,
            data: DataStore::default(),
            warnings: gungnir_workflow::warning::WarningLedger::default(),
            rehearsal: None,
            terrain: crate::terrain::TerrainStatus::NotConfigured,
            service_sinks: feeds.observations,
            feed_stats: feeds.stats,
            ais_sinks: ais.reports,
            ais_stats: ais.stats,
            cooperative: crate::cooperative::CooperativeState::default(),
            adsb_sinks: adsb.reports,
            adsb_stats: adsb.stats,
            adsb_submitted: std::collections::HashSet::new(),
            adsb_cooperative: std::collections::HashMap::new(),
            sapient_stats: sapient.stats,
            sapient_task_acks: sapient.task_acks,
            identification: gungnir_identification::EvidenceFusionEngine::with_settings(
                identification_settings,
            ),
            keystore,
            expiry_announced: None,
            pipeline,
            link: None,
            link_control: std::sync::Arc::new(std::sync::Mutex::new(None)),
            node_task_map: std::collections::HashMap::new(),
            peer_links: peers,
            fallback: None,
            pending_history: None,
            endpoint_client,
            pending_handoffs: Vec::new(),
            pending_warnings: Vec::new(),
            loader: None,
            resources: config.resource_views(),
            last_plan: PlanView::default(),
            // `PlanId::default()` is `PlanId(0)`, which `DpInterceptService::next_plan_id`
            // (starting at 1) never mints -- it is the id of `PlanView::default()` alone,
            // the same starting plan `last_plan` above is seeded with. Seeding this field
            // to match rather than to `None` keeps the very first empty solve from
            // comparing as "new": both fields start at the same point the empty solve
            // itself produces, exactly as the single `last_plan` field did before this one
            // existed.
            last_live_plan_id: Some(gungnir_model::PlanId::default()),
            alternatives: Vec::new(),
            what_if: None,
            what_if_for: None,
            alerts,
            viewport: ViewportState::new(),
            mission: Some(mission),
            config,
            events: Box::new(events),
            health: SystemHealth::default(),
            clock: Box::new(clock),
            backend,
            ingest,
            rhythm: crate::rhythm::RhythmState::default(),
            governance,
            governance_recorded: false,
            withheld: Vec::new(),
            fires_checks: Vec::new(),
            engagements: Vec::new(),
            handoffs: Vec::new(),
            engaged_seen: HashSet::new(),
            clock_skew: gungnir_time::ClockSkewEstimator::new(),
            skew_alerted: HashSet::new(),
            health_journaled: None,
            anomaly: crate::anomaly::AnomalyState::default(),
            identity,
            role: Role::Operator,
            layout: WorkspaceLayout::for_role(Role::Operator),
            // Timed by the baseline in force: a layer with no configured expiry never
            // expires, which is DN-08's default read the way DN-10 requires.
            sensors,
            sensor_error: None,
            encryption,
            hazards,
            accounts,
            session,
            requirements,
            recovered,
            next_requirement,
            requirement_error: None,
            issued_launch_warnings,
            launch_warnings_recovered,
            next_launch_warning,
            config_store,
            audit: InMemoryAuditLog::new(),
            approvals: InMemoryApprovalWorkflow::with_settings(decision_settings),
            denials: DenialHistory::default(),
            queue_outcomes: QueueOutcomeCounts::default(),
            selected_approval: None,
            dialog: gungnir_ui::panels::decision_dialog::DecisionDialogState::default(),
            selected_track: None,
            selected_laydown: None,
            rehearsal_scenario: gungnir_model::TestTrackNumber(1),
            rehearsal_records: std::collections::HashMap::new(),
            journal,
            journal_rx,
            journal_failed: false,
            runtime,
        })
    }

    pub fn session(&self) -> Option<SessionId> {
        self.mission.as_ref().map(|m| m.session)
    }

    /// Install the deployment's session authority (GAP-057).
    ///
    /// Separate from construction because custody of accounts belongs to the host, the
    /// way DN-22 puts key custody there: the binary decides where accounts come from.
    pub fn set_session_authority(
        &mut self,
        authority: Box<dyn gungnir_security::SessionAuthority>,
    ) {
        self.session = authority;
    }

    /// Who is signed in, as of now.
    #[must_use]
    pub fn session_state(&self) -> gungnir_security::SessionState {
        self.session.state(self.clock.now().0)
    }

    /// The operator to attribute an act to, or `None`.
    ///
    /// **The only place attribution is obtained.** Everything that records who did
    /// something reads this, so there is one answer rather than several: an expired
    /// session and a missing store both yield `None`, and neither is mistaken for a
    /// person.
    #[must_use]
    pub fn attributed_operator(&self) -> Option<gungnir_security::OperatorId> {
        self.session_state().operator()
    }

    /// Sign in, recording the attempt in the audit log whether it worked or not.
    ///
    /// # Errors
    ///
    /// The failure DN-23 §5 defines, which never says which half of the credential was
    /// wrong.
    pub fn sign_in(
        &mut self,
        credential: &[u8],
    ) -> Result<gungnir_security::OperatorSession, gungnir_security::AuthFailure> {
        let now = self.clock.now().0;
        let outcome = self.session.sign_in(credential, now);
        gungnir_security::audit_attempt(
            &mut self.audit,
            outcome.as_ref().ok().map(|s| s.operator),
            outcome.as_ref().map(|_| ()).map_err(|e| *e),
            now,
        );
        outcome
    }

    /// Sign out, recording it.
    pub fn sign_out(&mut self) {
        let now = self.clock.now().0;
        if let Some(operator) = self.attributed_operator() {
            gungnir_security::audit_sign_out(&mut self.audit, operator, now);
        }
        self.session.sign_out(now);
    }

    /// Take the next requirement identifier (GAP-005).
    pub fn next_requirement_id(&mut self) -> gungnir_model::RequirementId {
        self.next_requirement += 1;
        gungnir_model::RequirementId(self.next_requirement)
    }

    /// Take the next launch-warning identifier (GAP-009). A plain per-deployment
    /// serial, the same shape [`Self::next_requirement_id`] is, since a
    /// `LaunchWarningReport.id` needs only local uniqueness -- a peer disambiguates by
    /// its own name alongside it (`PeerLaunchWarning::peer`), not by this string alone.
    pub fn next_launch_warning_id(&mut self) -> String {
        self.next_launch_warning += 1;
        format!("launch-warning-{}", self.next_launch_warning)
    }

    /// The role this desktop is signed in as.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// The workspace for the current role.
    #[must_use]
    pub fn layout(&self) -> &WorkspaceLayout {
        &self.layout
    }

    /// Force every accepted envelope onto the disk.
    ///
    /// This is the "fsync on session save" half of D-04 (`ARCHITECTURE.md` §10 item
    /// 19). `update::tick` calls `sync_if_due`, which bounds loss at the 5 s interval
    /// while the desktop runs; this is what a host calls when the session is saved or
    /// closed, and without it a clean exit could drop up to 5 s of envelopes from a
    /// session someone had just deliberately finished.
    ///
    /// The error is returned rather than logged: a caller that ignores it is claiming
    /// a durability it does not have.
    pub fn save_session(&mut self) -> Result<(), StoreError> {
        // **Drain first.** This used to sync only, so anything published after the last
        // frame -- a requirement stated and then the window closed -- reached the bus,
        // never reached the journal, and was gone. An fsync of a file the envelope was
        // not written to is a durable record of nothing, which is worse than an obvious
        // failure because it looks like it worked.
        crate::update::journal_pending(self);
        self.journal.sync()
    }

    /// Save, and then record that this session ended deliberately.
    ///
    /// **The order is the point.** Envelopes reach the disk first; only then is the
    /// record moved to `Closed`. If the second step fails the record still says `Live`,
    /// the next launch reports the session as interrupted, and the operator is told about
    /// a session that was in fact finished -- which is the harmless direction. The other
    /// order would mark a session cleanly closed and then fail to write its last
    /// envelopes, and nothing afterwards would know the record was short.
    ///
    /// # Errors
    ///
    /// The save error if the journal did not reach the disk, or the lifecycle error if
    /// the mission record could not be closed.
    pub fn close_session(&mut self) -> Result<(), AppError> {
        self.save_session()?;
        let Some(mission) = self.mission.take() else {
            return Ok(());
        };
        let mut missions = JournalMissionManager::open(self.journal.root(), &self.journal)?;
        let session = mission.session;
        let result = missions.close(mission);
        tracing::info!(session = session.0, "closed session");
        Ok(result?)
    }

    /// Resources the planner would not propose, with the reason each (GAP-030).
    #[must_use]
    pub fn withheld_resources(&self) -> &[gungnir_intercept_service::WithheldResource] {
        &self.withheld
    }

    /// The queued item PN-07 is deciding.
    #[must_use]
    pub fn selected_approval(&self) -> Option<gungnir_ui::panels::approval_queue::PendingId> {
        self.selected_approval
    }

    /// Open PN-07 on a queued item, or close it by re-clicking the open one.
    ///
    /// Changing which item is being decided clears the dialog: a reject reason typed
    /// against one plan must never be recorded against another.
    pub fn select_approval(&mut self, id: gungnir_ui::panels::approval_queue::PendingId) {
        self.selected_approval = if self.selected_approval == Some(id) {
            None
        } else {
            Some(id)
        };
        self.dialog = gungnir_ui::panels::decision_dialog::DecisionDialogState::default();
    }

    /// Close PN-07, discarding what was typed in it.
    pub fn clear_selected_approval(&mut self) {
        self.selected_approval = None;
        self.dialog = gungnir_ui::panels::decision_dialog::DecisionDialogState::default();
    }

    /// The track PN-04 is showing.
    #[must_use]
    pub fn selected_track(&self) -> Option<TrackId> {
        self.selected_track
    }

    /// Select a track, or clear the selection by re-clicking the selected row.
    pub fn select_track(&mut self, id: TrackId) {
        self.selected_track = if self.selected_track == Some(id) {
            None
        } else {
            Some(id)
        };
    }

    /// The laydown option PN-11 is drawing a before-and-after preview of.
    #[must_use]
    pub fn selected_laydown(&self) -> Option<&gungnir_model::LaydownId> {
        self.selected_laydown.as_ref()
    }

    /// Select a laydown option, or clear the selection by re-clicking the selected row.
    pub fn select_laydown(&mut self, id: gungnir_model::LaydownId) {
        self.selected_laydown = if self.selected_laydown.as_ref() == Some(&id) {
            None
        } else {
            Some(id)
        };
    }

    #[must_use]
    pub fn rehearsal_scenario(&self) -> gungnir_model::TestTrackNumber {
        self.rehearsal_scenario
    }

    pub fn pick_rehearsal_scenario(&mut self, scenario: gungnir_model::TestTrackNumber) {
        self.rehearsal_scenario = scenario;
    }

    /// PN-16's rehearsal section for whichever laydown is selected right now (GAP-045).
    #[must_use]
    pub fn rehearsal_section(&self) -> gungnir_ui::panels::planning::RehearsalSection {
        use gungnir_ui::panels::planning::{RehearsalSection, RehearsalSummary};
        let Some(id) = self.selected_laydown() else {
            return RehearsalSection::NothingSelected;
        };
        match self.rehearsal_records.get(id) {
            Some(record) => RehearsalSection::Ran(RehearsalSummary {
                scenario: record.scenario,
                tracks_formed: record.tracks_formed,
                decisions_raised: record.decisions_raised,
                decisions_expired: record.decisions_expired,
            }),
            None => RehearsalSection::NotYetRun,
        }
    }

    /// Run a rehearsal of `laydown` against `scenario` and record it, or alert why not
    /// (GAP-045). `laydown` must be one this baseline declares; an id naming none is an
    /// alert, not a panic -- the panel can only ever offer a laydown that already
    /// exists, but state should not assume its own caller got that right.
    ///
    /// `testdata_root` follows the same convention as this desktop's journal: relative
    /// to the working directory the binary was launched from
    /// (`docs/agentic-workflow.md`'s "both journal to `./gungnir-journal`"), here
    /// `./testdata`. **Not yet addressed**: whether a packaged release bundles
    /// `testdata/tracks/samples/` beside the binary, which is a release-packaging
    /// question this change does not answer.
    pub fn run_rehearsal(
        &mut self,
        scenario: gungnir_model::TestTrackNumber,
        laydown_id: &gungnir_model::LaydownId,
    ) {
        let Some(laydown) = self
            .config
            .laydowns
            .iter()
            .find(|l| &l.id == laydown_id)
            .cloned()
        else {
            self.alerts.push(format!(
                "rehearsal not run: {laydown_id} is not a laydown this baseline declares"
            ));
            return;
        };
        match crate::laydown_rehearsal::run(
            std::path::Path::new("testdata"),
            scenario,
            &laydown,
            &self.config.resources,
        ) {
            Ok(record) => {
                self.alerts.push(format!(
                    "rehearsal of {} under {}: {} track(s) formed, {} decision(s) raised \
                     ({} expired)",
                    laydown_id,
                    scenario.label(),
                    record.tracks_formed,
                    record.decisions_raised,
                    record.decisions_expired
                ));
                self.rehearsal_records.insert(laydown_id.clone(), record);
            }
            Err(err) => self
                .alerts
                .push(format!("rehearsal of {laydown_id} did not run: {err}")),
        }
    }

    /// Switch role, rebuilding the workspace with it.
    ///
    /// This is a *display* change and nothing more. It grants no authority: a role
    /// selected here still has exactly the permissions `role_permits` gives it, and
    /// GAP-057 will make the role authenticated rather than chosen.
    pub fn set_role(&mut self, role: Role) {
        self.role = role;
        self.layout = WorkspaceLayout::for_role(role);
    }
}

/// Tell the operator about sessions the last run did not close.
///
/// Reported as an alert, not an error: a process that stopped is a fact about yesterday,
/// not a reason to refuse to start today. What must not happen is silence -- an
/// interrupted record has a hole in it, and whoever reads it after the fact needs to know
/// which session that was.
fn report_interrupted_sessions(missions: &mut dyn MissionManager, alerts: &mut Vec<String>) {
    let recorded = match missions.missions() {
        Ok(recorded) => recorded,
        Err(err) => {
            alerts.push(format!(
                "Earlier sessions could not be listed ({err}); whether any were left \
                 unclosed is unknown"
            ));
            return;
        }
    };
    for session in recorded {
        match missions.load(session) {
            Ok(mission) if mission.state == MissionState::Interrupted => alerts.push(format!(
                "Session {} was not closed; its record ends where the process stopped",
                session.0
            )),
            Ok(_) => {}
            Err(err) => alerts.push(format!("Session {} could not be read ({err})", session.0)),
        }
    }
}

/// The action and role names this build knows, for checking authority rules
/// (DN-08 §6 rule 3, GAP-052).
///
/// Derived from `gungnir-security` rather than listed here, for the same reason
/// `decisions::escalation_ladder` is: a second list would drift from the authorization it
/// is supposed to describe, and the drift would show up as rules that silently grant
/// nothing. `gungnir-config` cannot read these itself -- it may not depend on
/// `gungnir-security` -- so the binary that can see both hands them over.
///
/// `gungnir-node` builds the same vocabulary from the same two constants.
#[must_use]
pub fn known_vocabulary() -> KnownVocabulary {
    KnownVocabulary::new(
        gungnir_security::actions::ALL.iter().copied(),
        gungnir_security::Role::ALL.iter().map(|r| format!("{r:?}")),
    )
}

/// The baseline in force, and the file it came from when it came from one.
fn load_config() -> Result<(ConfigBaseline, Option<FileConfigStore>), ConfigError> {
    match std::env::var(CONFIG_ENV_VAR) {
        Ok(path) if !path.trim().is_empty() => {
            let store = FileConfigStore::new(&path, known_vocabulary());
            let baseline = store.load()?;
            store.validate(&baseline)?;
            tracing::info!(%path, "loaded config baseline");
            Ok((baseline, Some(store)))
        }
        _ => {
            tracing::info!("no {CONFIG_ENV_VAR} set; using the default config baseline");
            Ok((ConfigBaseline::default(), None))
        }
    }
}

/// Set up journal encryption from the baseline, and report what happened (GAP-084).
///
/// DN-22 §5's disconnected fallback in one function: **the desktop starts either way**.
/// A console that refused to run because a keystore was missing would be a worse failure
/// than one that runs and says what it cannot do, and one that claimed encryption it was
/// not performing would be worse than both.
///
/// Returns the persistent provider alongside the status when one was actually opened
/// (D-39's OS-keystore path, unlike `Ephemeral`'s `InProcessKeyProvider`, produces the
/// same `PersistentKeyProvider` type `AppState.keystore` holds), so the caller can wire
/// it in for later use the same way a passphrase sign-in already does.
fn build_encryption(
    config: &ConfigBaseline,
    journal: &mut FileEventJournal,
    alerts: &mut Vec<String>,
) -> (
    gungnir_security::EncryptionStatus,
    Option<std::sync::Arc<gungnir_security::PersistentKeyProvider>>,
) {
    use gungnir_config::KeyProviderConfig;
    use gungnir_security::{EncryptionStatus, KeyPurpose};

    match &config.security.key_provider {
        KeyProviderConfig::None => (EncryptionStatus::NotConfigured, None),

        // DN-22 amendment 3: the keystore opens at sign-in, not at start. Until then the
        // journal is plaintext and the strip says so, which is §5's fallback rule.
        KeyProviderConfig::PassphraseSealedFile => (
            EncryptionStatus::UnavailableWritingPlaintext {
                reason: "the keystore is sealed under the operator's passphrase and opens at \
                         sign-in; nobody has signed in"
                    .into(),
            },
            None,
        ),

        KeyProviderConfig::Ephemeral => {
            let mut provider = gungnir_security::InProcessKeyProvider::new();
            let key = provider.generate(KeyPurpose::JournalAtRest);
            journal.seal_with(Box::new(EphemeralSealer {
                provider: std::sync::Arc::new(provider),
                key,
            }));
            // Said out loud, because the failure mode is silent: the journal is real
            // ciphertext that nothing will ever be able to read again.
            alerts.push(
                "Journal encryption is using an ephemeral key: this session's journal \
                 cannot be read after the application closes."
                    .into(),
            );
            (
                EncryptionStatus::Active {
                    provider: "ephemeral".into(),
                },
                None,
            )
        }

        // D-39: unlocked at operator login rather than typed at sign-in, so unlike
        // `PassphraseSealedFile` this opens right here at start.
        KeyProviderConfig::OperatingSystemKeystore { account } => {
            let escrow = crate::keystore::escrow_from_config(config, alerts);
            let dir = std::path::PathBuf::from(&config.data_dir);
            let provider =
                match gungnir_security::PersistentKeyProvider::open_or_create_via_os_keystore(
                    &dir,
                    gungnir_security::DESKTOP_KEYSTORE_SERVICE,
                    account,
                    escrow,
                ) {
                    Ok(p) => std::sync::Arc::new(p),
                    Err(err) => {
                        let reason = format!("the operating-system keystore did not open: {err}");
                        alerts.push(format!("Journal encryption is off: {reason}"));
                        return (
                            EncryptionStatus::UnavailableWritingPlaintext { reason },
                            None,
                        );
                    }
                };
            let key = match provider.active_or_generate(KeyPurpose::JournalAtRest) {
                Ok(key) => key,
                Err(err) => {
                    let reason = format!("no journal key: {err}");
                    alerts.push(format!("Journal encryption is off: {reason}"));
                    return (
                        EncryptionStatus::UnavailableWritingPlaintext { reason },
                        None,
                    );
                }
            };
            crate::keystore::write_escrow_record(&provider, key, &dir, alerts);
            journal.seal_with(Box::new(crate::keystore::KeystoreSealer {
                provider: std::sync::Arc::clone(&provider),
                key,
            }));
            (
                EncryptionStatus::Active {
                    provider: "os-keystore".into(),
                },
                Some(provider),
            )
        }

        // Validation refuses this, so reaching here means a baseline bypassed it. Say
        // so and write in the clear rather than pretending either way.
        other @ KeyProviderConfig::ManagedService { .. } => {
            let reason = format!(
                "the configured key provider is designed and not built ({})",
                other.owning_gap().unwrap_or("GAP-084")
            );
            alerts.push(format!("Journal encryption is off: {reason}"));
            (
                EncryptionStatus::UnavailableWritingPlaintext { reason },
                None,
            )
        }
    }
}

/// Wires a `KeyProvider` to the journal's own narrow trait (DN-22 §4).
struct EphemeralSealer {
    provider: std::sync::Arc<gungnir_security::InProcessKeyProvider>,
    key: gungnir_security::KeyId,
}

impl gungnir_store::sealing::JournalSealer for EphemeralSealer {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, gungnir_store::StoreError> {
        use gungnir_security::KeyProvider;
        self.provider
            .seal(&self.key, plaintext)
            .map_err(|e| gungnir_store::StoreError::Sealing(e.to_string()))
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, gungnir_store::StoreError> {
        use gungnir_security::KeyProvider;
        self.provider
            .unseal(&self.key, sealed)
            .map_err(|e| gungnir_store::StoreError::Sealing(e.to_string()))
    }
}

/// The accounts PN-20 lists, read the same way the authority reads them.
fn account_listing(
    config: &ConfigBaseline,
) -> Result<Vec<(gungnir_security::OperatorId, gungnir_security::Role)>, String> {
    use gungnir_config::AuthenticationProvider;
    match &config.security.authentication.provider {
        AuthenticationProvider::None => Err("no account store is configured".into()),
        AuthenticationProvider::LocalAccounts { accounts_path } => {
            let path = std::path::Path::new(&config.data_dir).join(accounts_path);
            gungnir_security::FileAccountStore::open(&path)
                .map(|s| s.listing())
                .map_err(|e| e.to_string())
        }
        AuthenticationProvider::OsKeystoreAccounts { .. } => Err(
            "the operating-system-keystore account provider is for gungnir-node; the \
             desktop uses local-accounts or no accounts (DN-23 §5)"
                .into(),
        ),
    }
}

/// The session authority the baseline names (GAP-057, DN-23 §5).
///
/// **The desktop starts either way**: a store that cannot be read is an unavailable one,
/// reported as `SessionState::StoreUnavailable`, never a refusal to run (rule 5). No
/// provider means nobody can sign in, and PN-20 says so.
fn build_session_authority(
    config: &ConfigBaseline,
    alerts: &mut Vec<String>,
) -> Box<dyn gungnir_security::SessionAuthority> {
    use gungnir_config::AuthenticationProvider;
    use gungnir_security::{FileAccountStore, InMemoryAccountStore, LocalAccountAuthority};
    let store: Box<dyn gungnir_security::AccountStore> =
        match &config.security.authentication.provider {
            AuthenticationProvider::None => Box::new(InMemoryAccountStore::unavailable(
                "no account store is configured for this deployment",
            )),
            AuthenticationProvider::LocalAccounts { accounts_path } => {
                let path = std::path::Path::new(&config.data_dir).join(accounts_path);
                match FileAccountStore::open(&path) {
                    Ok(store) => Box::new(store),
                    Err(err) => {
                        alerts.push(format!(
                            "The account store could not be read; nobody can sign in: {err}"
                        ));
                        Box::new(InMemoryAccountStore::unavailable(err.to_string()))
                    }
                }
            }
            AuthenticationProvider::OsKeystoreAccounts { .. } => {
                alerts.push(
                    "The operating-system-keystore account provider is for gungnir-node; \
                     nobody can sign in to this desktop until local-accounts is configured \
                     instead (DN-23 §5)."
                        .into(),
                );
                Box::new(InMemoryAccountStore::unavailable(
                    "the operating-system-keystore account provider is for gungnir-node",
                ))
            }
        };
    // GAP-057: the lifetime the baseline names is enforced here; absent means the
    // disconnected profile's non-expiring session (DN-23 §5).
    Box::new(
        LocalAccountAuthority::new(store)
            .with_lifetime(config.security.authentication.session_lifetime_s),
    )
}

/// The runtime the embedded services run on.
fn desktop_runtime() -> Result<tokio::runtime::Runtime, AppError> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

/// Requirements outlive a session: an analyst who states one on Monday expects it on
/// Tuesday. Recovered before anything else uses the journal, and the serial is continued
/// past what came back so a new requirement cannot collide with a recovered one -- the
/// identifier is per-deployment now, not per-session. A journal that cannot be read for
/// them is an alert, because whether any are outstanding is then unknown.
fn recover_requirements_or_alert(
    journal: &FileEventJournal,
    alerts: &mut Vec<String>,
) -> (
    Vec<gungnir_model::CollectionRequirement>,
    crate::requirements::Recovered,
    u64,
) {
    let (requirements, recovered, next_requirement) = recover_requirements(journal);
    if let crate::requirements::Recovered::Unreadable { reason } = &recovered {
        alerts.push(format!(
            "Collection requirements could not be recovered ({reason}); whether any \
             are outstanding is unknown"
        ));
    }
    (requirements, recovered, next_requirement)
}

/// Requirements recovered from the journal, and the serial to continue from: past the
/// highest recovered identifier, so a new requirement cannot collide with an old one.
fn recover_requirements(
    journal: &FileEventJournal,
) -> (
    Vec<gungnir_model::CollectionRequirement>,
    crate::requirements::Recovered,
    u64,
) {
    let (requirements, recovered) = crate::requirements::recover(journal);
    let next = requirements.iter().map(|r| r.id.0).max().unwrap_or(0);
    (requirements, recovered, next)
}

/// Launch warnings declared in earlier sessions (GAP-009): recovered the same way
/// requirements are, and for the same reason -- a warning declared on Monday is still
/// on the record on Tuesday. A journal that cannot be read for them is an alert.
fn recover_launch_warnings_or_alert(
    journal: &FileEventJournal,
    alerts: &mut Vec<String>,
) -> (
    Vec<gungnir_model::LaunchWarningReport>,
    crate::launch_warning::Recovered,
    u64,
) {
    let (issued, recovered) = crate::launch_warning::recover(journal);
    if let crate::launch_warning::Recovered::Unreadable { reason } = &recovered {
        alerts.push(format!(
            "Issued launch warnings could not be recovered ({reason}); what this \
             deployment has already declared is unknown"
        ));
    }
    // Append-only and never updated by id (unlike a requirement), so the count
    // recovered is exactly the serial to continue from.
    #[allow(clippy::cast_possible_truncation)]
    let next = issued.len() as u64;
    (issued, recovered, next)
}

/// The gateway with its allow-list and radar adapters (GAP-001), the radar feeds' service
/// sinks (GAP-064), and the endpoint transport (GAP-040) trusting what the baseline pins
/// (GAP-060). Split from the constructor for length; every alert it pushes says which.
/// The sensor positions the tracker needs to place an angular report (GAP-001, DN-27 §4).
///
/// Built from the baseline's own sensor list, which is where a deployment states where
/// each sensor is. Without it every bearing and every range-azimuth-elevation report is
/// refused, which is what happened until 2026-09-07.
fn sensor_positions(config: &ConfigBaseline) -> gungnir_tracking_service::SensorPositions {
    gungnir_tracking_service::SensorPositions::from_sensors(
        config.sensors.iter().map(|s| (s.id, s.position)),
    )
}

fn build_ingest(
    config: &ConfigBaseline,
    runtime: &tokio::runtime::Handle,
    alerts: &mut Vec<String>,
) -> (
    IngestGateway,
    crate::radar::BoundFeeds,
    crate::cooperative::BoundAisFeeds,
    crate::adsb::BoundAdsbFeeds,
    crate::sapient::BoundSapientFeeds,
    Option<gungnir_remote::endpoint::EndpointClient>,
    Vec<crate::peers::BoundPeer>,
) {
    let mut ingest = IngestGateway::new(Box::new(AllowListAuthenticator {
        // DN-16 §5: a peer is a source and is admitted like one, under its own id.
        allowed: config
            .sensors
            .iter()
            .map(|s| SensorId(s.id))
            .chain(config.peers.iter().map(|p| SensorId(p.source_id)))
            .collect(),
    }));
    // GAP-009: a machine link per peer whose endpoint is a node, under this desktop's
    // certificate (D-02).
    let tls = crate::session::link_tls_for(config);
    let peers = crate::peers::bind_peers(config, &mut ingest, &tls, runtime, alerts);
    if config.sensors.is_empty() {
        alerts.push("No sensors configured; ingest gateway is idle".into());
    }
    ingest.set_expected_adapters(config.sensors.len());
    // GAP-001: an ASTERIX adapter per configured radar feed, with the observation
    // sink the tick drains into the registry (GAP-064).
    let feeds = crate::radar::bind_feeds(config, &mut ingest, alerts);
    let ais = crate::cooperative::bind_feeds(config, &mut ingest, alerts);
    // GAP-010: an ADS-B adapter per configured feed, on the same shape as AIS above.
    let adsb = crate::adsb::bind_feeds(config, &mut ingest, alerts);
    // GAP-001: a SAPIENT adapter per configured feed (spotter, acoustic, or
    // passive-RF); its detections enter the gateway like a radar's, not like AIS's or
    // ADS-B's cooperative reports.
    let sapient = crate::sapient::bind_feeds(config, &mut ingest, alerts);
    // GAP-040: the endpoint transport, trusting what the baseline pins (GAP-060).
    let endpoint_client = match gungnir_remote::endpoint::EndpointClient::new(
        runtime.clone(),
        &config.security.tls.trust_roots_pem,
        std::time::Duration::from_secs(5),
    ) {
        Ok(client) => Some(client),
        Err(err) => {
            alerts.push(format!(
                "the endpoint client could not be built ({err}); handoffs and warnings \
                 to endpoints will be recorded undelivered"
            ));
            None
        }
    };
    (ingest, feeds, ais, adsb, sapient, endpoint_client, peers)
}

/// The embedded tracker, filtering as the promoted algorithm baseline says (GAP-053,
/// DN-24 §7).
///
/// A baseline naming a filter this build does not implement is **not** silently run as
/// the default: the alert says so and the service keeps
/// `UNGOVERNED_ALGORITHM_VERSION`, so the picture and the governance record disagree
/// visibly rather than quietly.
/// The settings this desktop's pipeline runs, from the promoted algorithm baseline
/// where there is one this build can apply (GAP-053, DN-24 §7).
///
/// A baseline naming a filter this build does not implement is **not** silently run as
/// the default: the alert says so, and [`tracking_service`] leaves the tracker
/// ungoverned, so the picture and the governance record disagree visibly rather than
/// quietly.
/// The `imm-cv-ct` fields `PipelineSettings::from_baseline` needs, from the baseline's
/// own `TrackingConfig` (DN-28 §5). Built here rather than in `gungnir-tracking-service`,
/// which sits below `gungnir-config` and may not depend on it (the same reason
/// `from_baseline` takes primitives at all).
fn imm_fields(
    config: &gungnir_config::TrackingConfig,
) -> gungnir_tracking_service::ImmBaselineFields {
    gungnir_tracking_service::ImmBaselineFields {
        turn_rate_rad_s: config.imm_turn_rate_rad_s,
        mode_transition: config.imm_mode_transition,
        initial_mode_probabilities: config.imm_initial_mode_probabilities,
    }
}

fn pipeline_settings(
    config: &ConfigBaseline,
    in_force: Option<&gungnir_modelops::ModelBaseline>,
    alerts: &mut Vec<String>,
) -> gungnir_tracking_service::PipelineSettings {
    let _ = config;
    let Some(baseline) = in_force else {
        return gungnir_tracking_service::PipelineSettings::default();
    };
    match gungnir_tracking_service::PipelineSettings::from_baseline(
        baseline.config.gate_threshold,
        &baseline.config.filter_selection,
        &imm_fields(&baseline.config),
        baseline.config.measurement_noise_var,
    ) {
        Ok(settings) => settings,
        Err(err) => {
            alerts.push(format!(
                "the promoted algorithm baseline {} is not applied: {err}; the tracker \
                 runs its default filter and every track stays marked ungoverned",
                baseline.id
            ));
            gungnir_tracking_service::PipelineSettings::default()
        }
    }
}

/// Whether the promoted baseline is the one the pipeline is running, which is the only
/// condition under which its identifier may be stamped (DN-24 §7).
fn applied_baseline(
    in_force: Option<&gungnir_modelops::ModelBaseline>,
) -> Option<&gungnir_model::AlgorithmBaselineId> {
    let baseline = in_force?;
    gungnir_tracking_service::PipelineSettings::from_baseline(
        baseline.config.gate_threshold,
        &baseline.config.filter_selection,
        &imm_fields(&baseline.config),
        baseline.config.measurement_noise_var,
    )
    .ok()
    .map(|_| &baseline.id)
}

fn tracking_service(
    runtime: &tokio::runtime::Handle,
    config: &ConfigBaseline,
    pipeline: gungnir_tracking_service::PipelineSettings,
    alerts: &mut Vec<String>,
) -> LiveTrackingService {
    let _ = alerts;
    let staleness = config.policy.staleness.clone();
    let service = LiveTrackingService::with_pipeline_settings(runtime, pipeline)
        .with_staleness(staleness)
        .with_sensor_positions(sensor_positions(config));
    match applied_baseline(
        gungnir_modelops::InMemoryModelRegistry::from_baseline(config)
            .ok()
            .as_ref()
            .and_then(|registry| {
                config
                    .operating_profile()
                    .and_then(|p| gungnir_modelops::ModelRegistry::promoted(registry, &p))
            }),
    ) {
        Some(id) => service.with_algorithm_baseline(id),
        None => service,
    }
}

/// Embedded services, or remote clients when configured and reachable. On a remote
/// failure, fall back to embedded and record why (ARCHITECTURE.md §8.4).
fn build_backends(
    config: &ConfigBaseline,
    runtime: &tokio::runtime::Handle,
    pipeline: gungnir_tracking_service::PipelineSettings,
    alerts: &mut Vec<String>,
) -> (
    Box<dyn TrackingService>,
    Box<dyn InterceptService>,
    BackendConfig,
) {
    if let BackendConfig::Remote { endpoint } = &config.backend {
        // **Connecting is an authenticated act as of GAP-057**, and nobody is signed in
        // while this struct is being built. So a remote backend cannot be established at
        // start-up any more: the desktop comes up embedded and says why, rather than
        // connecting with a credential it would have had to invent.
        //
        // Establishing the link after an operator signs in is the remaining half of this
        // wiring; until it exists, a deployment configured for a node runs its own
        // services and the alert says so rather than the status strip implying a link.
        tracing::info!(
            %endpoint,
            "a remote backend needs an operator sign-in; running embedded until one exists"
        );
        alerts.push(format!(
            "Remote backend {endpoint} needs an operator to sign in before it can be reached (GAP-057); running embedded"
        ));
        let _ = runtime;
    }
    (
        // GAP-012: the tracker judges staleness by the baseline's policy. GAP-053: and
        // it filters by the promoted algorithm baseline, and stamps that baseline only
        // because it is applying it (DN-24 §7).
        Box::new(tracking_service(runtime, config, pipeline, alerts)),
        // GAP-031: the planner solves geometry in the deployment's frame, when it has one.
        Box::new(
            DpInterceptService::new(config.allocation_horizon).with_local_frame(config.origin.map(
                |[lat_rad, lon_rad, alt_m]| {
                    gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
                        lat_rad,
                        lon_rad,
                        alt_m,
                    })
                },
            )),
        ),
        BackendConfig::Embedded,
    )
}
