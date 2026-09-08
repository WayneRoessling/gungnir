// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Replay, reports and the configuration baseline, wired to the desktop (GAP-071).
//!
//! The three panels PN-12, PN-13 and PN-14 read subsystems `ARCHITECTURE.md` §7.3 listed
//! as unwired: `gungnir-replay` over the journal, `gungnir-reporting` over the same
//! journal, and `gungnir-config`'s `ConfigStore` for validate and apply. As with the
//! approval gate, what lives here is construction and the mapping into view structs; the
//! decisions live in the crates.
//!
//! # Three limitations that are the point rather than caveats
//!
//! **Replay scrubs events; it does not rebuild the picture.** The desktop's `AppState` is
//! built from the service facades, not from the event stream, so moving the replay cursor
//! moves a cursor and nothing else. Reconstituting state at a past moment is GAP-045.
//! An analyst who believed otherwise would read the live picture as the picture at the
//! time they scrubbed to.
//!
//! **A report's counts are not the measures catalogue.** `EventCounts` is what folding
//! the journal yields; MOE and MOP are GAP-047.
//!
//! **Applying a baseline persists it; it does not swap it under the running session.**
//! `AppState` builds its services, journal, ingest gateway and approval deadlines from
//! the baseline at construction, so a live swap would have to rebuild most of this struct
//! to be true. Persisting and saying "on restart" is honest; persisting and leaving the
//! screen implying otherwise would be the worst option on a panel that sets weapons
//! control status.

use crate::state::AppState;
use gungnir_config::ConfigStore;
use gungnir_model::MissionTime;
use gungnir_replay::ReplaySession;
use gungnir_reporting::{JournalReportGenerator, MissionReport, ReportGenerator};
use gungnir_security::authz::role_permits;
use gungnir_security::{actions, AuditEntry, AuditLog};
use gungnir_store::{EventJournal, SessionId};
use gungnir_ui::panels::config_editor::{
    ApplyState, AuditLine, Candidate, ConfigEditorView, ConfigSection, GovernedProfiles,
    ProfileLine, Validation,
};
use gungnir_ui::panels::replay::{OpenReplay, PlayRate, ReplayView, SessionSummary};
use gungnir_ui::panels::reports::{CountLine, ExportState, MetricsLine, ReportsView};
use gungnir_ui::panels::unavailable::Unavailable;
use gungnir_viewport3d::layers::{CoverageCircle, CoverageLayer, NoCoverage};

/// Rebuilding the picture at a past moment: replay through the live pipeline.
const RECONSTRUCTION: Unavailable<'static> = Unavailable {
    owner: "gungnir-replay",
    gap: "GAP-045",
};

/// Tracking metrics need ground truth, which a live session does not have.
const TRUTH: Unavailable<'static> = Unavailable {
    owner: "gungnir-scenario",
    gap: "GAP-045",
};

/// A form over the baseline's sections.
const EDITING: Unavailable<'static> = Unavailable {
    owner: "gungnir-ui",
    gap: "GAP-071",
};

/// Where an exported report is written, under the configured data directory.
pub const REPORT_DIR: &str = "reports";

/// Everything the panels that keep scratch carry across frames.
///
/// Not mission state, which is why it is not in `AppState`: a half-scrubbed replay
/// cursor, the last report generated, an unapplied candidate baseline and a half-typed
/// collection requirement are things a window is doing, not things the mission is.
///
/// Named for the three sustainment panels it began as; PN-15 joined them in GAP-005
/// because it needs the same thing for the same reason.
#[derive(Default)]
pub struct SustainmentState {
    /// PN-17's handover notes, as the outgoing watch types them (GAP-054).
    ///
    /// Session state and not mission state: a half-typed note is this window's business
    /// until somebody acknowledges the handover, which is when it reaches the record.
    pub handover_notes: String,
    pub replay: ReplayState,
    pub reports: ReportState,
    pub config: ConfigEditorState,
    /// What PN-15 has typed but not committed (GAP-005).
    pub requirements: gungnir_ui::panels::requirements::Draft,
    /// Session ids and lengths, refreshed when the replay picker is shown. Reading
    /// every session's length is a disk read per session, so it is not done per frame.
    pub sessions: Vec<SessionSummary>,
    pub sessions_read: bool,
    /// The after-action review in progress (GAP-049, DN-20). Session state: the journal
    /// carries its events, which is the record.
    pub review: Option<gungnir_workflow::ReviewCase>,
    /// What PN-13's reviewer has typed and not committed.
    pub review_draft: gungnir_ui::panels::reports::ReviewDraft,
    /// Finding identifiers are minted per review, from one.
    pub next_finding: u64,
    /// PN-20's sign-in form (GAP-057). The passphrase is cleared on submit.
    pub sign_in: gungnir_ui::panels::audit::SignInDraft,
}

/// The desktop's replay state: which session is open and how fast it is running.
#[derive(Default)]
pub struct ReplayState {
    session: Option<ReplaySession>,
    open_id: Option<SessionId>,
    rate: PlayRate,
    /// Fractional envelopes owed at the current rate, carried between frames so a
    /// slow rate still advances rather than rounding to nothing every frame.
    owed: f32,
    /// One-line description of the envelope the cursor last stepped over.
    cursor_event: Option<String>,
}

impl ReplayState {
    /// Open a session for review. Returns the error to the caller rather than
    /// swallowing it: a replay that silently failed to open would leave the panel
    /// showing the previous session under a new heading.
    pub fn open(
        &mut self,
        state: &AppState,
        id: SessionId,
    ) -> Result<(), gungnir_store::StoreError> {
        let session = ReplaySession::open(&state.journal, id)?;
        self.session = Some(session);
        self.open_id = Some(id);
        self.rate = PlayRate::Paused;
        self.owed = 0.0;
        self.cursor_event = None;
        // MOE-12: a rehearsal leaves an event on the *live* session's record.
        let now = state.clock.now();
        if let Err(err) = state.events.publish(
            now,
            gungnir_eventing::Event::Replay(gungnir_model::events::ReplayEvent::Opened {
                session: id,
                at: now,
            }),
        ) {
            tracing::error!(%err, "replay-opened publish failed");
        }
        Ok(())
    }

    /// Close the replay, recording how far it was stepped (MOE-12).
    pub fn close(&mut self, state: &AppState) {
        if let (Some(session), Some(id)) = (self.session.as_ref(), self.open_id) {
            let now = state.clock.now();
            if let Err(err) = state.events.publish(
                now,
                gungnir_eventing::Event::Replay(gungnir_model::events::ReplayEvent::Closed {
                    session: id,
                    stepped: session.len() - session.remaining(),
                    at: now,
                }),
            ) {
                tracing::error!(%err, "replay-closed publish failed");
            }
        }
        *self = Self::default();
    }

    pub fn set_rate(&mut self, rate: PlayRate) {
        self.rate = rate;
        self.owed = 0.0;
    }

    /// Advance one envelope.
    pub fn step(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.cursor_event = session.step().map(describe);
    }

    /// The session under replay, if one is.
    #[must_use]
    pub fn open_session(&self) -> Option<SessionId> {
        self.session.as_ref().and(self.open_id)
    }

    /// The replay clock: the mission time of the most recently stepped envelope.
    #[must_use]
    pub fn clock(&self) -> Option<MissionTime> {
        self.session.as_ref().map(|s| s.clock().now())
    }

    /// Seek to a mission time (GAP-049: a finding's moment).
    pub fn seek_to(&mut self, at: MissionTime) {
        if let Some(session) = self.session.as_mut() {
            session.seek_to(at);
            self.cursor_event = None;
        }
    }

    /// Move the cursor to a fraction of the session.
    pub fn seek_fraction(&mut self, fraction: f32) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let len = session.len();
        if len == 0 {
            return;
        }
        // Seeking is by mission time, which is what `ReplaySession` indexes on, so the
        // fraction is turned into a time rather than an envelope index: two envelopes
        // at the same instant must not be separable by a scrubber.
        let clamped = fraction.clamp(0.0, 1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let target_index = ((clamped * (len - 1) as f32).round() as usize).min(len - 1);
        let at = session.mission_time_at(target_index).unwrap_or_default();
        session.seek_to(at);
        self.cursor_event = None;
    }

    /// Advance by the play rate. Called once per frame with the frame's duration.
    pub fn advance(&mut self, dt_s: f32) {
        if self.session.is_none() || self.rate == PlayRate::Paused {
            return;
        }
        self.owed += self.rate.envelopes_per_second() * dt_s;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            self.step();
            // Stop at the end rather than spinning: `step` returns nothing and the
            // owed count would otherwise grow without bound.
            if self.session.as_ref().is_some_and(|s| s.remaining() == 0) {
                self.owed = 0.0;
                self.rate = PlayRate::Paused;
                break;
            }
        }
    }

    #[must_use]
    pub fn rate(&self) -> PlayRate {
        self.rate
    }
}

/// A one-line description of an envelope, for the cursor.
fn describe(env: &gungnir_eventing::Envelope) -> String {
    use gungnir_eventing::Event;
    let kind = match &env.event {
        Event::Tracking(e) => format!("tracking {e:?}"),
        Event::Intercept(e) => format!("intercept {e:?}"),
        Event::Ingest(e) => format!("ingest {e:?}"),
        Event::Command(e) => format!("command {e:?}"),
        Event::Sensor(e) => format!("sensor {e:?}"),
        Event::SensorTask(e) => format!("sensor task {e:?}"),
        Event::Requirement(e) => format!("requirement {e:?}"),
        Event::Rhythm(e) => format!("rhythm {e:?}"),
        Event::Governance(e) => format!("governance {e:?}"),
        Event::Engagement(e) => format!("engagement {e:?}"),
        Event::Review(e) => format!("review {e:?}"),
        Event::Health(e) => format!("health {e:?}"),
        Event::Replay(e) => format!("replay {e:?}"),
        Event::Handoff(e) => format!("handoff {e:?}"),
        Event::Warning(e) => format!("warning {e:?}"),
        Event::LaunchWarning(e) => format!("launch warning {e:?}"),
        Event::Identity(e) => format!("identity {e:?}"),
        Event::Rehearsal(e) => format!("rehearsal {e:?}"),
        Event::Link(e) => format!("link {e:?}"),
    };
    let short: String = kind.chars().take(120).collect();
    format!("seq {} at {:.1} s: {short}", env.seq, env.mission_time.0)
}

/// Build PN-12's view.
#[must_use]
pub fn replay_view<'a>(
    state: &AppState,
    replay: &'a ReplayState,
    sessions: &'a [SessionSummary],
) -> ReplayView<'a> {
    let open = replay.session.as_ref().map(|s| OpenReplay {
        session: replay.open_id.map_or(0, |id| id.0),
        position: s.len() - s.remaining(),
        length: s.len(),
        remaining: s.remaining(),
        clock_s: s.clock().now().0,
        cursor_event: replay.cursor_event.as_deref(),
        live: replay.open_id == state.session(),
    });
    ReplayView {
        sessions,
        open,
        rate: replay.rate,
        reconstruction: RECONSTRUCTION,
    }
}

/// The sessions this journal holds, for the picker.
///
/// A session whose length could not be read reports `None` rather than zero: an
/// unreadable session and an empty one are different, and an analyst choosing what to
/// review would act differently on each.
#[must_use]
pub fn session_summaries(state: &AppState) -> Vec<SessionSummary> {
    let Ok(ids) = state.journal.sessions() else {
        return Vec::new();
    };
    ids.into_iter()
        .map(|id| SessionSummary {
            id: id.0,
            envelopes: state.journal.read_session(id).ok().map(|e| e.len()),
            live: state.session() == Some(id),
        })
        .collect()
}

/// The desktop's report state: the last report generated, and the last file written.
#[derive(Default)]
pub struct ReportState {
    report: Option<MissionReport>,
    counts: Vec<CountLine<'static>>,
    /// The catalogue as PN-13 draws it (GAP-047), built when the report is.
    measures: Vec<gungnir_ui::panels::reports::MeasureLine>,
    /// The report's marking and its inputs, in words (GAP-062), built when the report is.
    marking: String,
    marking_inputs: String,
    last_export: Option<String>,
    /// The session exists but the journal holds nothing for it.
    nothing_recorded: bool,
    /// DN-19's product, assembled when the report is (GAP-025), or why it could not be.
    order_of_battle: Option<Result<gungnir_reporting::order_of_battle::OrderOfBattle, String>>,
    /// DN-19's pattern of life over the same sessions, folded when the report is
    /// (GAP-025), or why it could not be.
    pattern_of_life: Option<Result<gungnir_reporting::order_of_battle::PatternOfLife, String>>,
}

impl ReportState {
    /// Fold the journal into a report.
    pub fn generate(&mut self, state: &AppState) -> Result<(), gungnir_reporting::ReportError> {
        let Some(session) = state.session() else {
            return Ok(());
        };
        let generator = JournalReportGenerator {
            journal: &state.journal,
            // No ground truth in a live session, so no tracking metrics. `None` here is
            // the honest value; a zeroed summary would read as a perfect tracker.
            metrics: None,
        };
        match generator.generate(session) {
            Ok(report) => {
                self.counts = count_lines(&report);
                self.measures = measure_lines(&report);
                self.marking = marking_words(&report.releasability);
                self.marking_inputs = format!(
                    "{} internal, {} to parties, {} to all peers",
                    report.marking_inputs.internal,
                    report.marking_inputs.parties,
                    report.marking_inputs.all_peers
                );
                self.report = Some(report);
                self.nothing_recorded = false;
                Ok(())
            }
            // The journal creates a session's file on its first append, so a session
            // that has published nothing is absent from it. Nothing failed.
            Err(gungnir_reporting::ReportError::Store(
                gungnir_store::StoreError::UnknownSession(_),
            )) => {
                self.report = None;
                self.counts.clear();
                self.measures.clear();
                self.marking.clear();
                self.marking_inputs.clear();
                self.nothing_recorded = true;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Hold the order of battle assembled beside the report (GAP-025).
    pub fn set_order_of_battle(
        &mut self,
        product: Result<gungnir_reporting::order_of_battle::OrderOfBattle, String>,
    ) {
        self.order_of_battle = Some(product);
    }

    /// The last order of battle assembled, if any.
    #[must_use]
    pub fn order_of_battle(&self) -> Option<&gungnir_reporting::order_of_battle::OrderOfBattle> {
        self.order_of_battle.as_ref().and_then(|r| r.as_ref().ok())
    }

    /// Hold the pattern of life folded beside the report (GAP-025).
    pub fn set_pattern_of_life(
        &mut self,
        product: Result<gungnir_reporting::order_of_battle::PatternOfLife, String>,
    ) {
        self.pattern_of_life = Some(product);
    }

    /// The last pattern of life folded, if any.
    #[must_use]
    pub fn pattern_of_life(&self) -> Option<&gungnir_reporting::order_of_battle::PatternOfLife> {
        self.pattern_of_life.as_ref().and_then(|r| r.as_ref().ok())
    }

    /// Write the report out under the data directory.
    pub fn export(&mut self, state: &AppState) -> Result<(), gungnir_reporting::ReportError> {
        let Some(report) = self.report.as_ref() else {
            return Ok(());
        };
        let dir = std::path::Path::new(&state.config.data_dir).join(REPORT_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("session-{}.json", report.session.0));
        let generator = JournalReportGenerator {
            journal: &state.journal,
            metrics: None,
        };
        generator.export(report, &path)?;
        self.last_export = Some(path.display().to_string());
        Ok(())
    }
}

/// The counts, labelled, with a note on the two that are read as each other.
/// The catalogue rows as PN-13 takes them: the same figures, the same reasons.
fn measure_lines(report: &MissionReport) -> Vec<gungnir_ui::panels::reports::MeasureLine> {
    use gungnir_reporting::MeasureValue;
    use gungnir_ui::panels::reports::{MeasureLine, MeasureLineValue};
    report
        .measures
        .iter()
        .map(|m| MeasureLine {
            id: m.id.clone(),
            name: m.name.clone(),
            target: m.target.clone(),
            value: match &m.value {
                MeasureValue::Fraction {
                    value,
                    numerator,
                    denominator,
                } => MeasureLineValue::Fraction {
                    value: *value,
                    numerator: *numerator,
                    denominator: *denominator,
                },
                MeasureValue::Count(n) => MeasureLineValue::Count(*n),
                MeasureValue::NoInstances { of } => {
                    MeasureLineValue::NoInstances { of: of.clone() }
                }
                MeasureValue::NotComputable { reason } => MeasureLineValue::NotComputable {
                    reason: reason.clone(),
                },
            },
            note: m.note.clone(),
        })
        .collect()
}

/// A marking in words (DN-17 §7).
fn marking_words(marking: &gungnir_model::Releasability) -> String {
    use gungnir_model::Releasability;
    match marking {
        Releasability::Internal => "Internal".to_string(),
        Releasability::AllPeers => "releasable to all peers".to_string(),
        Releasability::Parties { parties } => format!(
            "releasable to {}",
            parties.iter().cloned().collect::<Vec<_>>().join(", ")
        ),
    }
}

fn count_lines(report: &MissionReport) -> Vec<CountLine<'static>> {
    let c = &report.counts;
    vec![
        CountLine {
            label: "Detections accepted",
            value: c.detections_accepted,
            note: None,
        },
        CountLine {
            label: "Detections quarantined",
            value: c.detections_quarantined,
            note: None,
        },
        CountLine {
            label: "Tracks initiated",
            value: c.tracks_initiated,
            note: None,
        },
        CountLine {
            label: "Tracks deleted",
            value: c.tracks_deleted,
            note: None,
        },
        CountLine {
            label: "Plans proposed",
            value: c.plans_proposed,
            note: None,
        },
        CountLine {
            label: "Decisions",
            value: c.decisions,
            note: Some("a person chose"),
        },
        CountLine {
            label: "Expired",
            value: c.decisions_expired,
            note: Some("not rejections: nobody decided"),
        },
        CountLine {
            label: "Escalated",
            value: c.decisions_escalated,
            note: Some("offered upward; not an outcome"),
        },
        CountLine {
            label: "Engagements opened",
            value: c.engagements_opened,
            note: Some("one per accepted solution (DN-06)"),
        },
        CountLine {
            label: "Effective (corroborated)",
            value: c.engagements_effective_corroborated,
            note: Some("an effector or a person reported it"),
        },
        CountLine {
            label: "Effective (track-inferred)",
            value: c.engagements_effective_track_inferred,
            note: Some("the track left the picture; never added to the corroborated count"),
        },
        CountLine {
            label: "Ineffective (corroborated)",
            value: c.engagements_ineffective_corroborated,
            note: None,
        },
        CountLine {
            label: "Ineffective (track-inferred)",
            value: c.engagements_ineffective_track_inferred,
            note: Some("the track outlasted the window; never added to the corroborated count"),
        },
        CountLine {
            label: "Indeterminate",
            value: c.engagements_indeterminate,
            note: Some("the window closed with nothing observed: not a success, not a failure"),
        },
        CountLine {
            label: "Aborted",
            value: c.engagements_aborted,
            note: None,
        },
        CountLine {
            label: "Events total",
            value: c.total,
            note: None,
        },
    ]
}

/// Build PN-13's view.
#[must_use]
pub fn reports_view<'a>(state: &'a AppState, reports: &'a ReportState) -> ReportsView<'a> {
    ReportsView {
        session: state.session().map(|s| s.0),
        counts: reports.report.as_ref().map(|_| reports.counts.as_slice()),
        first_event_s: reports
            .report
            .as_ref()
            .and_then(|r| r.first_event)
            .map(|t| t.0),
        last_event_s: reports
            .report
            .as_ref()
            .and_then(|r| r.last_event)
            .map(|t| t.0),
        metrics: reports
            .report
            .as_ref()
            .and_then(|r| r.metrics)
            .map(|m| MetricsLine {
                mota: m.mota,
                motp: m.motp,
                purity: m.purity,
                fragmentation: m.fragmentation,
            })
            .ok_or(TRUTH),
        measures: reports.report.as_ref().map(|_| reports.measures.as_slice()),
        export: ExportState::Available {
            // GAP-062: the report's own combined marking, computed when it was generated
            // and carried inside the exported file; before generation it is not known.
            marking: if reports.report.is_some() {
                &reports.marking
            } else {
                "not yet known"
            },
            path: REPORT_DIR,
            inputs: if reports.report.is_some() {
                &reports.marking_inputs
            } else {
                "no report generated"
            },
        },
        last_export: reports.last_export.as_deref(),
        nothing_recorded: reports.nothing_recorded,
        // Filled in by `workspace::render_reports`, which holds the review; on its own
        // this view says a review could be opened, and the caller corrects that.
        review: gungnir_ui::panels::reports::ReviewView {
            case: None,
            cannot_open: None,
        },
        order_of_battle: reports
            .order_of_battle
            .as_ref()
            .map(|product| match product {
                Ok(oob) => gungnir_ui::panels::reports::OrderOfBattleLine {
                    version: oob.version,
                    sessions: oob.sources.len(),
                    entries: oob.entries.len(),
                    across_sessions: oob
                        .entries
                        .iter()
                        .filter(|e| {
                            e.sources
                                .iter()
                                .map(|(s, _)| s.0)
                                .collect::<std::collections::BTreeSet<_>>()
                                .len()
                                > 1
                        })
                        .count(),
                    unattributed: oob.unattributed_tracks,
                    caveat: state.identity.unreadable.as_deref(),
                },
                Err(reason) => gungnir_ui::panels::reports::OrderOfBattleLine {
                    version: 0,
                    sessions: 0,
                    entries: 0,
                    across_sessions: 0,
                    unattributed: 0,
                    caveat: Some(reason),
                },
            }),
        pattern_of_life: reports
            .pattern_of_life
            .as_ref()
            .map(|product| match product {
                Ok(pattern) => gungnir_ui::panels::reports::PatternOfLifeLine {
                    sessions: pattern.sessions_covered,
                    sessions_with_activity: pattern.sessions_with_activity,
                    busiest: pattern.busiest_hour(),
                    routes: pattern.routes.len(),
                    caveat: state.identity.unreadable.as_deref(),
                },
                // Zeroed with the reason drawn beside it: a pattern that could not be
                // folded is not a pattern of no activity.
                Err(reason) => gungnir_ui::panels::reports::PatternOfLifeLine {
                    sessions: 0,
                    sessions_with_activity: 0,
                    busiest: None,
                    routes: 0,
                    caveat: Some(reason),
                },
            }),
    }
}

/// The desktop's configuration-editor state: a candidate loaded from disk.
#[derive(Default)]
pub struct ConfigEditorState {
    candidate: Option<gungnir_config::ConfigBaseline>,
    candidate_path: Option<String>,
    candidate_error: Option<String>,
    validated: Option<Result<(), String>>,
    in_force: Option<Result<(), String>>,
}

impl ConfigEditorState {
    /// Re-read the baseline file as a candidate.
    pub fn reload(&mut self, state: &AppState) {
        self.validated = None;
        match state.config_store.as_ref() {
            None => {
                self.candidate = None;
                self.candidate_error =
                    Some("no baseline file is configured for this desktop".to_owned());
            }
            Some(store) => match store.load() {
                Ok(baseline) => {
                    self.candidate = Some(baseline);
                    self.candidate_path = Some(store.path().display().to_string());
                    self.candidate_error = None;
                }
                Err(err) => {
                    self.candidate = None;
                    self.candidate_error = Some(err.to_string());
                }
            },
        }
    }

    /// Validate the candidate, and the baseline in force alongside it.
    ///
    /// Both, because they answer different questions: whether the file about to be
    /// applied is sound, and whether what this desktop is running is.
    pub fn validate(&mut self, state: &AppState) {
        self.in_force = Some(gungnir_config::validate(&state.config).map_err(|e| e.to_string()));
        self.validated = self
            .candidate
            .as_ref()
            .map(|c| gungnir_config::validate(c).map_err(|e| e.to_string()));
    }

    pub fn discard(&mut self) {
        self.candidate = None;
        self.candidate_path = None;
        self.candidate_error = None;
        self.validated = None;
    }

    /// The error from the last reload, if it failed.
    #[must_use]
    pub fn candidate_error(&self) -> Option<&str> {
        self.candidate_error.as_deref()
    }

    /// Persist the validated candidate and audit it.
    ///
    /// Refuses anything not validated: `ConfigStore::apply` validates again, but a panel
    /// that let an unvalidated baseline through would be relying on that second check to
    /// catch what it should not have offered.
    pub fn apply(&mut self, state: &mut AppState) -> Result<(), String> {
        if !matches!(self.validated, Some(Ok(()))) {
            return Err("the candidate has not been validated".to_owned());
        }
        let Some(candidate) = self.candidate.clone() else {
            return Err("no candidate is loaded".to_owned());
        };
        let version = candidate.version;
        // Read before the store is borrowed, and passed in rather than fetched inside:
        // promotion is refused outside the baseline's validity window (DN-08 §5), and
        // that judgement must use the clock this session runs on.
        let now = state.clock.now();
        let Some(store) = state.config_store.as_mut() else {
            return Err("no baseline file is configured for this desktop".to_owned());
        };
        store.apply(candidate, now).map_err(|e| e.to_string())?;
        state.audit.record(AuditEntry {
            // No operator session (GAP-057). A fabricated actor in an audit trail is
            // worse than an absent one.
            operator: None,
            action: actions::APPLY_CONFIG.to_owned(),
            mission_time: state.clock.now().0,
            detail: format!("baseline version {version} written; in force on restart"),
        });
        self.discard();
        Ok(())
    }
}

/// One PN-14 line per candidate algorithm baseline (DN-24 §9, GAP-086).
///
/// Borrows from the registry's own baselines rather than copying, which is what keeps the
/// panel a projection of what is in force rather than a second copy of it.
#[must_use]
pub fn profile_lines(candidates: &[gungnir_modelops::ModelBaseline]) -> Vec<ProfileLine<'_>> {
    candidates
        .iter()
        .map(|b| ProfileLine {
            profile: b.id.profile.as_str(),
            name: b.id.name.as_str(),
            filter: b.config.filter_selection.as_str(),
            gate_threshold: b.config.gate_threshold,
            promoted: b.state == gungnir_modelops::PromotionState::Promoted,
            validated_by: b.validated_by.as_deref(),
        })
        .collect()
}

/// Which of the four things PN-14 has to say about algorithm governance is true.
///
/// **The four are kept apart** because "nothing declared", "one configuration and no
/// profiles", "several profiles" and "declared and refused" are four different situations
/// and only one of them is a fault.
#[must_use]
pub fn governed_profiles<'a>(
    state: &'a AppState,
    lines: &'a [ProfileLine<'a>],
) -> GovernedProfiles<'a> {
    if let Some(reason) = state.governance.unavailable() {
        return GovernedProfiles::Refused { reason };
    }
    if lines.is_empty() {
        return GovernedProfiles::NothingDeclared;
    }
    if state.config.mission_profiles.is_empty() {
        return GovernedProfiles::SingleImplicit { lines };
    }
    GovernedProfiles::Declared {
        active: state.config.active_profile.as_deref(),
        lines,
    }
}

/// Build PN-14's view.
#[must_use]
pub fn config_editor_view<'a>(
    state: &'a AppState,
    editor: &'a ConfigEditorState,
    sections: &'a [ConfigSection<'a>],
    audit: &'a [AuditLine<'a>],
    role_name: &'a str,
    validity: Option<&'a str>,
    profiles: GovernedProfiles<'a>,
) -> ConfigEditorView<'a> {
    let apply = if !role_permits(state.role(), actions::APPLY_CONFIG) {
        ApplyState::NotPermitted { role: role_name }
    } else if state.config_store.is_none() {
        ApplyState::NoFile {
            env_var: crate::state::CONFIG_ENV_VAR,
        }
    } else {
        ApplyState::PersistOnly
    };

    ConfigEditorView {
        profiles,
        version: state.config.version,
        revision: state.config.revision,
        sections,
        in_force: to_validation(editor.in_force.as_ref()),
        validity,
        candidate: editor.candidate.as_ref().map(|c| Candidate {
            path: editor
                .candidate_path
                .as_deref()
                .unwrap_or("the configured baseline file"),
            version: c.version,
            revision: c.revision,
            validation: to_validation(editor.validated.as_ref()),
        }),
        apply,
        audit,
        editing: EDITING,
    }
}

fn to_validation(result: Option<&Result<(), String>>) -> Validation<'_> {
    match result {
        None => Validation::NotRun,
        Some(Ok(())) => Validation::Valid,
        Some(Err(reason)) => Validation::Invalid { reason },
    }
}

/// The baseline's sections, summarised for PN-14.
#[must_use]
pub fn config_sections(state: &AppState) -> Vec<ConfigSection<'static>> {
    let c = &state.config;
    vec![
        ConfigSection {
            name: "Sensors",
            count: Some(c.sensors.len()),
            summary: "ingest allow-list and coverage",
        },
        ConfigSection {
            name: "Resources",
            count: Some(c.resources.len()),
            summary: "effectors the allocator may task",
        },
        ConfigSection {
            name: "Defended assets",
            count: Some(c.assets.len()),
            summary: "what assessment scores against (DN-01)",
        },
        ConfigSection {
            name: "Endpoints",
            count: Some(c.endpoints.len()),
            summary: "parties this deployment may send to (D-08)",
        },
        ConfigSection {
            name: "Control status",
            count: Some(c.policy.control_status.by_layer.len()),
            summary: "per effector layer; an unconfigured layer is at Hold",
        },
        ConfigSection {
            name: "Authority rules",
            count: Some(c.policy.authority.rules.len()),
            summary: "who may decide what, and what is pre-delegated (D-15)",
        },
        ConfigSection {
            name: "Decision deadlines",
            count: Some(c.policy.decisions.expiry_s.len()),
            summary: "expiry per layer; silence preserves (DN-10)",
        },
        ConfigSection {
            name: "Display vocabulary",
            count: Some(c.vocabulary.override_count()),
            summary: "terms this deployment has renamed; the rest are the glossary's (D-12)",
        },
        ConfigSection {
            name: "Geofences",
            count: Some(c.geofences.len()),
            summary: "rules about where we may act; a no-go fence denies an intercept (GAP-088)",
        },
        ConfigSection {
            name: "Hazards and barriers",
            count: Some(c.hazards.len()),
            summary: "booms, nets, wrecks, shoals; descriptive, never a rule (DN-14)",
        },
        ConfigSection {
            name: "Journal",
            count: None,
            summary: "the data directory this session records to",
        },
    ]
}

/// The configuration audit trail, for PN-14.
#[must_use]
pub fn audit_lines(state: &AppState) -> Vec<AuditLine<'_>> {
    state
        .audit
        .entries()
        .iter()
        .map(|e| AuditLine {
            action: &e.action,
            #[allow(clippy::cast_possible_truncation)]
            mission_time_s: e.mission_time as i64,
            detail: &e.detail,
            operator: e.operator.map(|o| o.0),
        })
        .collect()
}

/// The baseline's validity window as a sentence, if it has one.
///
/// An open-ended window says so rather than printing an infinity: `valid_until: None`
/// means this baseline stays promotable, which is a decision the deployment took, not a
/// missing end date.
#[must_use]
pub fn validity_sentence(state: &AppState) -> Option<String> {
    state.config.validity.map(|w| match w.valid_until {
        Some(until) => format!(
            "from {:.0} s to {:.0} s mission time",
            w.valid_from.0, until.0
        ),
        None => format!("from {:.0} s mission time, with no end", w.valid_from.0),
    })
}

/// Mission time as the panels read it.
#[must_use]
pub fn now(state: &AppState) -> MissionTime {
    state.clock.now()
}

/// The sensors as PN-10 shows them (GAP-003, GAP-004).
#[must_use]
pub fn sensor_rows(state: &AppState) -> Vec<gungnir_ui::panels::sensor_management::SensorRow<'_>> {
    use gungnir_model::SensorMode;
    use gungnir_sensor_management::{SensorControl, SensorRegistry};
    use gungnir_ui::panels::sensor_management::SensorRow;
    state
        .sensors
        .sensors()
        .iter()
        .map(|s| SensorRow {
            id: s.id.0,
            modality: &s.modality,
            mode: s.mode,
            requested: state.sensors.mode_status(s.id).and_then(|m| m.requested),
            // The most recent task, not the most recent *open* one: after a command
            // fails, the row should still say it failed rather than reverting to "no
            // command issued" and losing the reason.
            task: state
                .sensors
                .tasks()
                .iter()
                .rev()
                .find(|t| t.sensor == s.id)
                .map(|t| task_progress(&t.state)),
            calibration_version: &s.calibration_version,
            max_range_m: s.max_range_m,
            // The same rule `SensorRegistry::coverage` applies, derived here so the
            // panel and the map cannot disagree about which sensors count. From the
            // confirmed mode: a requested Search contributes nothing, because nothing
            // is searching yet.
            contributing: matches!(s.mode, SensorMode::Search | SensorMode::Track),
            controllable: s.control_endpoint.is_some(),
        })
        .collect()
}

fn task_progress(
    state: &gungnir_sensor_management::tasking::TaskState,
) -> gungnir_ui::panels::sensor_management::TaskProgress<'_> {
    use gungnir_sensor_management::tasking::TaskState;
    use gungnir_ui::panels::sensor_management::TaskProgress;
    match state {
        TaskState::Issued => TaskProgress::Issued,
        TaskState::Sent => TaskProgress::Sent,
        TaskState::Acknowledged { .. } => TaskProgress::Acknowledged,
        TaskState::Failed { reason } => TaskProgress::Failed { reason },
        TaskState::Unacknowledged => TaskProgress::Unacknowledged,
    }
}

/// Command a sensor into a mode (GAP-004).
///
/// **Changes nothing about what the sensor is confirmed to be doing.** It records a task
/// and publishes that it was asked. The confirmed mode moves only on an acknowledgement,
/// which nothing produces until GAP-001 brings the adapters -- so today every command
/// issued against a configured endpoint ends up unacknowledged, and the panel says so.
///
/// A sensor with no configured endpoint returns `NotControllable` and records nothing.
pub fn command_sensor_mode(
    state: &mut AppState,
    sensor: u32,
    mode: gungnir_model::SensorMode,
) -> Result<(), gungnir_sensor_management::SensorManagementError> {
    use gungnir_sensor_management::SensorControl;
    let id = gungnir_model::SensorId(sensor);
    let now = state.clock.now();
    let task = state.sensors.issue(
        id,
        gungnir_sensor_management::tasking::SensorCommand::SetMode { mode },
        None,
        now,
    )?;
    publish_task(
        state,
        now,
        gungnir_model::events::SensorTaskEvent::Issued {
            task,
            sensor: id,
            at: now,
        },
    );
    // GAP-059: commanding a sensor is a gated act, on the audit trail.
    crate::audit::record(
        state,
        actions::TASK_SENSOR,
        format!("sensor {sensor} commanded to {mode:?}"),
    );
    Ok(())
}

/// Record what an operator knows a sensor is doing, commanding nothing (GAP-004).
///
/// This is what the GAP-003 control did, kept and relabelled rather than removed: while
/// no adapter exists it is the only one of the two that changes anything, and an
/// operator who has been told on the radio that a radar is searching needs somewhere to
/// put that. PN-10 labels it apart from a command so the record can say which happened.
///
/// The event carries both ends: "set to Search" without the previous mode does not say
/// whether coverage grew or shrank, and a coverage answer that changed is what an
/// after-action review asks about.
pub fn record_observed_mode(
    state: &mut AppState,
    sensor: u32,
    mode: gungnir_model::SensorMode,
) -> Result<(), gungnir_sensor_management::SensorManagementError> {
    use gungnir_sensor_management::SensorRegistry;
    let id = gungnir_model::SensorId(sensor);
    let from = state
        .sensors
        .sensors()
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.mode);
    state.sensors.record_observed_mode(id, mode)?;
    let now = state.clock.now();
    if let Some(from) = from {
        let event = gungnir_model::events::SensorEvent::ModeChanged {
            sensor: id,
            from,
            to: mode,
            at: now,
        };
        if let Err(err) = state
            .events
            .publish(now, gungnir_eventing::Event::Sensor(event))
        {
            tracing::error!(%err, "sensor mode change publish failed");
        }
    }
    Ok(())
}

/// Time out every command past its acknowledgement window, alerting for each.
///
/// Runs every frame rather than only when something is issued, because a window closes
/// on the clock and not on new input -- the same reason the approval sweep does.
///
/// It never retries. DN-11 §5 rule 3: retry is an operator action, and a system that
/// quietly re-sent would make the record of what was asked untrue.
pub fn sweep_sensor_tasks(state: &mut AppState) {
    use gungnir_sensor_management::SensorControl;
    let now = state.clock.now();
    let window = state.config.sensor_task_ack_window_s;
    let timed_out = state.sensors.sweep(now, window);
    if timed_out.is_empty() {
        return;
    }
    let unanswered: Vec<(gungnir_model::SensorTaskId, gungnir_model::SensorId)> = state
        .sensors
        .tasks()
        .iter()
        .filter(|t| timed_out.contains(&t.id))
        .map(|t| (t.id, t.sensor))
        .collect();
    for (task, sensor) in unanswered {
        state.alerts.push(format!(
            "Sensor {} did not acknowledge command {} within {:.0} s; it has not been \
             retried.",
            sensor.0, task.0, window
        ));
        publish_task(
            state,
            now,
            gungnir_model::events::SensorTaskEvent::Unacknowledged {
                task,
                sensor,
                at: now,
            },
        );
    }
}

pub fn publish_task(
    state: &mut AppState,
    now: MissionTime,
    event: gungnir_model::events::SensorTaskEvent,
) {
    if let Err(err) = state
        .events
        .publish(now, gungnir_eventing::Event::SensorTask(event))
    {
        tracing::error!(%err, "sensor task publish failed");
    }
}

/// What this deployment is defending, and what is threatening it (GAP-026, DN-01).
///
/// `AssetListAssessor` has existed since the design set landed and **nothing constructed
/// one**, so the asset list was in the baseline, validated, and scored against by nobody.
/// `anchor_list`'s own documentation calls itself "a convenience for the binaries", and
/// this is the binary.
///
/// The declared assets placed in the local frame, or `None` without an origin
/// (GAP-020: prediction needs them to say what a track approaches).
#[must_use]
pub fn asset_anchors(state: &AppState) -> Option<Vec<gungnir_assessment::AssetAnchor>> {
    let frame = local_frame(state)?;
    let list = gungnir_model::AssetListView {
        baseline_version: state.config.revision,
        assets: state
            .config
            .assets
            .iter()
            .map(gungnir_config::AssetConfig::to_asset)
            .collect(),
    };
    Some(gungnir_assessment::assets::anchor_list(&list, |geodetic| {
        frame.to_enu(geodetic)
    }))
}

/// Returns `None` when the deployment has declared no local frame origin: asset positions
/// are geodetic and tracks are in ENU, and without the origin there is no way to put them
/// in the same picture. Guessing one would rank every track against an asset placed
/// somewhere plausible and wrong -- the same reason coverage refuses to draw rings.
#[must_use]
pub fn asset_assessor(state: &AppState) -> Option<gungnir_assessment::AssetListAssessor> {
    let frame = local_frame(state)?;
    let list = gungnir_model::AssetListView {
        baseline_version: state.config.revision,
        assets: state
            .config
            .assets
            .iter()
            .map(gungnir_config::AssetConfig::to_asset)
            .collect(),
    };
    let anchors = gungnir_assessment::assets::anchor_list(&list, |geodetic| frame.to_enu(geodetic));
    Some(
        gungnir_assessment::AssetListAssessor::new(
            state.config.version,
            anchors,
            state.config.assessment.max_range_m,
        )
        // GAP-027: the declared platform class, weighed by the baseline's table.
        .with_class_weights(crate::cooperative::class_weights(state)),
    )
}

/// Score every track against the asset list, or say why nothing was scored.
///
/// **A zero score and an unscored track are different claims**, which is the distinction
/// `AssetListAssessor::is_unconfigured` exists to preserve and the reason this returns a
/// reason rather than an empty vector.
/// The ranking as PN-17 lists it: each scored track with the asset it threatens, by name
/// and priority (GAP-026, DN-01 §7). Tracks that threaten no listed asset are left out;
/// a score resting on no asset is not an exposure.
#[must_use]
pub fn exposure_lines<'a>(
    state: &'a AppState,
    ranking: &'a AssetRanking,
) -> Vec<gungnir_ui::panels::commander_summary::ExposureLine<'a>> {
    ranking
        .scores()
        .iter()
        .filter_map(|score| {
            let exposure = score.exposure.as_ref()?;
            let asset = state
                .config
                .assets
                .iter()
                .find(|a| a.id == exposure.asset.0)?;
            Some(gungnir_ui::panels::commander_summary::ExposureLine {
                track: score.track_id.0,
                asset: &asset.name,
                priority: &asset.priority,
                score: score.score,
                time_to_impact_s: score.time_to_impact_s,
            })
        })
        .collect()
}

/// The factors behind one track's score, for PN-04's evidence card (DN-01 §7: the
/// threatened asset and the priority's contribution). Owned strings; the card borrows.
#[must_use]
pub fn score_factors(
    state: &AppState,
    ranking: &AssetRanking,
    track: gungnir_model::TrackId,
) -> Vec<(String, f32)> {
    let Some(score) = ranking.scores().iter().find(|s| s.track_id == track) else {
        return Vec::new();
    };
    let Some(exposure) = score.exposure.as_ref() else {
        return vec![("threatens no listed asset".to_string(), score.score)];
    };
    let (name, priority) = state
        .config
        .assets
        .iter()
        .find(|a| a.id == exposure.asset.0)
        .map_or(("an asset no longer listed", "unknown"), |a| {
            (a.name.as_str(), a.priority.as_str())
        });
    let weight = gungnir_model::AssetPriority::parse(priority)
        .map_or(0.0, gungnir_model::AssetPriority::weight);
    #[allow(clippy::cast_possible_truncation)]
    let weight = weight as f32;
    let mut factors = vec![
        (
            format!("threatened asset: {name}, {:.0} m away", exposure.range_m),
            score.score,
        ),
        (format!("priority {priority}: weight"), weight),
    ];
    // GAP-027: the affiliation's lethality is a factor, and the card says which.
    if let Some(t) = state.tracking.tracks().iter().find(|t| t.id == track) {
        #[allow(clippy::cast_possible_truncation)]
        let lethality = t.classification.lethality_weight() as f32;
        factors.push((
            format!("classified {:?}: lethality", t.classification).to_lowercase(),
            lethality,
        ));
    }
    // GAP-027: the declared platform class, when a cooperative source declared one and
    // the baseline weighs it; a class the table does not list is said and weighs 1.
    if let Some(class) = state
        .cooperative
        .by_track
        .get(&track)
        .and_then(|l| l.platform_class.as_deref())
    {
        match state.config.assessment.lethality_by_class.get(class) {
            #[allow(clippy::cast_possible_truncation)]
            Some(w) => factors.push((format!("declared {class}: class lethality"), *w as f32)),
            None => factors.push((
                format!("declared {class}: not in assessment.lethality_by_class"),
                1.0,
            )),
        }
    }
    match score.time_to_impact_s {
        Some(t) => factors.push((format!("closing, {t:.0} s to impact"), 1.0)),
        None => factors.push(("not closing".to_string(), 0.0)),
    }
    factors
}

pub fn asset_exposure(state: &AppState) -> AssetRanking {
    use gungnir_assessment::ThreatAssessor;

    let Some(assessor) = asset_assessor(state) else {
        return AssetRanking::NotScored {
            reason: "this deployment has declared no local frame origin, so assets and \
                     tracks cannot be placed in the same picture"
                .into(),
        };
    };
    if assessor.is_unconfigured() {
        return AssetRanking::NotScored {
            reason: "this deployment has declared no defended assets, so there is \
                     nothing to rank a track against"
                .into(),
        };
    }
    let mut scores = assessor.assess(state.tracking.tracks());
    // Highest risk first, which is the order a saturation triage reads them in (MT-01).
    scores.sort_by(|a, b| b.score.total_cmp(&a.score));
    AssetRanking::Scored {
        scores,
        assets: state.config.assets.len(),
    }
}

/// Tracks ranked against the asset list, or why they were not.
#[derive(Debug, Clone, PartialEq)]
pub enum AssetRanking {
    Scored {
        scores: Vec<gungnir_assessment::RiskScore>,
        /// How many assets the ranking was against, so an empty score list can be told
        /// from an unconfigured one.
        assets: usize,
    },
    /// Nothing was scored, and why. **Not an empty ranking**: a system that ranked
    /// nothing and a system with nothing to rank are different states, and only the
    /// second is the deployment being incomplete.
    NotScored { reason: String },
}

impl AssetRanking {
    /// The scores, or an empty slice when nothing was scored.
    #[must_use]
    pub fn scores(&self) -> &[gungnir_assessment::RiskScore] {
        match self {
            AssetRanking::Scored { scores, .. } => scores,
            AssetRanking::NotScored { .. } => &[],
        }
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            AssetRanking::Scored { .. } => None,
            AssetRanking::NotScored { reason } => Some(reason),
        }
    }
}

/// The coverage layer for the viewport (GAP-007).
///
/// Placing a sensor's geodetic position in the ENU picture needs the local frame, which
/// a deployment may not have declared. There is no sound default for it -- guessing from
/// the first sensor would put every ring somewhere plausible and wrong -- so with no
/// origin this returns the reason rather than any circles.
///
/// The rings are **observed** since GAP-003: `SensorRegistry::coverage` reports only
/// sensors that are searching or tracking, and scores confidence from the mode. A
/// standby sensor has a configured range and covers nothing, and this is where that
/// distinction stops being a caveat and becomes the answer.
#[must_use]
pub fn coverage_circles(state: &AppState) -> Vec<CoverageCircle> {
    use gungnir_sensor_management::SensorRegistry;
    let Some(frame) = local_frame(state) else {
        return Vec::new();
    };
    state
        .sensors
        .coverage()
        .into_iter()
        .map(|region| CoverageCircle {
            sensor: region.sensor.0,
            center: frame.to_enu(region.center),
            radius_m: region.radius_m,
            confidence: region.confidence,
        })
        .collect()
}

/// The local ENU frame this deployment declared, if it declared one.
#[must_use]
pub fn local_frame(state: &AppState) -> Option<gungnir_model::LocalFrame> {
    state.config.origin.map(|[lat_rad, lon_rad, alt_m]| {
        gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
    })
}

/// Build the viewport's coverage layer for this frame.
#[must_use]
pub fn coverage_layer<'a>(state: &AppState, circles: &'a [CoverageCircle]) -> CoverageLayer<'a> {
    if state.config.origin.is_none() {
        return CoverageLayer::None(NoCoverage::NoOrigin { setting: "origin" });
    }
    if circles.is_empty() {
        return CoverageLayer::None(NoCoverage::NoSensorsActive);
    }
    CoverageLayer::Circles {
        circles,
        // Observed since GAP-003: the registry reports what is searching or tracking.
        nominal: None,
    }
}

/// The coverage report for the declared approaches (DN-12, GAP-006).
///
/// `None` when there is nothing to report on: no local frame to place the approaches
/// in, or no approaches declared. Both are said on screen rather than shown as full
/// coverage, because "nothing is missing" and "nothing was measured" are opposite
/// claims about a sector.
///
/// Only sensors that are searching or tracking contribute, which is DN-12 §5 rule 1 and
/// is what `coverage_from_registry` applies. A fresh desktop has every sensor at
/// Standby, so it reports the whole of every approach as uncovered -- correctly.
#[must_use]
pub fn coverage_report(state: &AppState) -> Option<gungnir_analytics::CoverageReport> {
    let frame = local_frame(state)?;
    if state.config.approaches.is_empty() {
        return None;
    }
    let volumes = gungnir_analytics::coverage_from_registry(
        &state.sensors,
        state.config.analytics.coverage_min_elevation_rad,
        |record| frame.to_enu(record.position),
    );

    let routes: Vec<Vec<[f64; 3]>> = state
        .config
        .approaches
        .iter()
        .map(|a| {
            a.points
                .iter()
                .map(|[lat_rad, lon_rad, alt_m]| {
                    frame.to_enu(gungnir_model::Geodetic {
                        lat_rad: *lat_rad,
                        lon_rad: *lon_rad,
                        alt_m: *alt_m,
                    })
                })
                .collect()
        })
        .collect();
    let approaches: Vec<&[[f64; 3]]> = routes.iter().map(Vec::as_slice).collect();

    // GAP-023: mask against the loaded terrain when there is one; flat otherwise, and
    // the parameters say which, because flat is optimistic by construction.
    let spacing = state.config.analytics.coverage_sample_spacing_m;
    let parameters = |masked: bool| gungnir_analytics::CoverageParameters {
        sample_spacing_m: spacing,
        terrain_masking_applied: masked,
    };
    Some(match state.data.terrains.first() {
        Some(terrain) if state.terrain.is_masking() => gungnir_analytics::combined_coverage(
            &volumes,
            &gungnir_analytics::TerrainMaskLineOfSight {
                terrain,
                sample_spacing_m: spacing,
            },
            &approaches,
            parameters(true),
        ),
        _ => gungnir_analytics::combined_coverage(
            &volumes,
            &gungnir_analytics::FlatTerrainLineOfSight,
            &approaches,
            parameters(false),
        ),
    })
}

/// A laydown's `CoverageVolume`s: the sensors it places that are searching or tracking,
/// each with its position and mode taken from the laydown but its range looked up from
/// the baseline's own sensor declaration.
///
/// A laydown places every sensor and resource it declares (DN-26 §4 rule 4), but a
/// laydown's own placement carries no range or modality of its own -- those are the
/// physical sensor's, unchanged by where a laydown puts it.
fn laydown_coverage_volumes(
    laydown: &gungnir_model::Laydown,
    sensor_ranges: &std::collections::HashMap<u32, f64>,
    min_elevation_rad: f64,
) -> Vec<(gungnir_model::SensorId, gungnir_analytics::CoverageVolume)> {
    laydown
        .sensors
        .iter()
        .filter(|s| {
            matches!(
                s.mode,
                gungnir_model::SensorMode::Search | gungnir_model::SensorMode::Track
            )
        })
        .filter_map(|s| {
            sensor_ranges.get(&s.sensor.0).map(|&max_range_m| {
                (
                    s.sensor,
                    gungnir_analytics::CoverageVolume {
                        sensor_enu: s.position_enu,
                        max_range_m,
                        min_elevation_rad,
                    },
                )
            })
        })
        .collect()
}

/// PN-16's rows: one per declared laydown, or the reason there are none (GAP-087,
/// `docs/design/DN-26-laydown-options.md` §5, §6).
pub enum PlanningRows {
    Rows(Vec<gungnir_ui::panels::planning::LaydownRow>),
    Empty { reason: &'static str },
}

/// Coverage for every declared laydown, under one line-of-sight model for all of them
/// (DN-26 §5's first rule), so a planner comparing options never reads a difference
/// between models as a difference between laydowns.
///
/// Mirrors [`coverage_report`]'s own frame, approach and terrain-masking choices exactly,
/// for the same reason PN-11 and this panel must agree: the current laydown's row here
/// and PN-11's live picture are the same computation over the same inputs.
#[must_use]
pub fn planning_rows(state: &AppState) -> PlanningRows {
    use gungnir_ui::panels::planning::{LaydownCoverage, LaydownRow};

    if state.config.laydowns.is_empty() {
        return PlanningRows::Empty {
            reason: "This deployment has declared no laydown alternatives.",
        };
    }

    let not_computed = |reason: &str| {
        state
            .config
            .laydowns
            .iter()
            .map(|l| LaydownRow {
                id: l.id.clone(),
                intent: l.intent.clone(),
                current: l.current,
                coverage: LaydownCoverage::NotComputed {
                    reason: reason.to_string(),
                },
            })
            .collect()
    };

    let Some(frame) = local_frame(state) else {
        return PlanningRows::Rows(not_computed(
            "this deployment has declared no local frame origin",
        ));
    };
    if state.config.approaches.is_empty() {
        return PlanningRows::Rows(not_computed(
            "no approach is declared to evaluate coverage along",
        ));
    }

    let routes: Vec<Vec<[f64; 3]>> = state
        .config
        .approaches
        .iter()
        .map(|a| {
            a.points
                .iter()
                .map(|[lat_rad, lon_rad, alt_m]| {
                    frame.to_enu(gungnir_model::Geodetic {
                        lat_rad: *lat_rad,
                        lon_rad: *lon_rad,
                        alt_m: *alt_m,
                    })
                })
                .collect()
        })
        .collect();
    let approaches: Vec<&[[f64; 3]]> = routes.iter().map(Vec::as_slice).collect();

    let sensor_ranges: std::collections::HashMap<u32, f64> = state
        .config
        .sensors
        .iter()
        .map(|s| (s.id, s.max_range_m))
        .collect();
    let min_elevation_rad = state.config.analytics.coverage_min_elevation_rad;
    let spacing = state.config.analytics.coverage_sample_spacing_m;

    let terrain_masking_applied = !state.data.terrains.is_empty() && state.terrain.is_masking();
    let parameters = gungnir_analytics::CoverageParameters {
        sample_spacing_m: spacing,
        terrain_masking_applied,
    };

    let report_for = |laydown: &gungnir_model::Laydown| -> gungnir_analytics::CoverageReport {
        let volumes = laydown_coverage_volumes(laydown, &sensor_ranges, min_elevation_rad);
        match state.data.terrains.first() {
            Some(terrain) if terrain_masking_applied => gungnir_analytics::combined_coverage(
                &volumes,
                &gungnir_analytics::TerrainMaskLineOfSight {
                    terrain,
                    sample_spacing_m: spacing,
                },
                &approaches,
                parameters,
            ),
            _ => gungnir_analytics::combined_coverage(
                &volumes,
                &gungnir_analytics::FlatTerrainLineOfSight,
                &approaches,
                parameters,
            ),
        }
    };

    let current_uncovered_m = state
        .config
        .laydowns
        .iter()
        .find(|l| l.current)
        .map(|l| report_for(l).gap_length_m(gungnir_analytics::GapSeverity::Uncovered));

    let rows = state
        .config
        .laydowns
        .iter()
        .map(|l| {
            let report = report_for(l);
            let uncovered_m = report.gap_length_m(gungnir_analytics::GapSeverity::Uncovered);
            let delta_uncovered_m = if l.current {
                None
            } else {
                current_uncovered_m.map(|current| uncovered_m - current)
            };
            LaydownRow {
                id: l.id.clone(),
                intent: l.intent.clone(),
                current: l.current,
                coverage: LaydownCoverage::Computed {
                    gap_segments: report.gaps.len(),
                    uncovered_m,
                    delta_uncovered_m,
                },
            }
        })
        .collect();

    PlanningRows::Rows(rows)
}

/// The selected laydown's sensor and resource positions, owned so a caller can borrow
/// slices from it into `gungnir_viewport3d::layers::LaydownPreview` (GAP-087's own
/// remaining item).
#[derive(Debug, Clone, PartialEq)]
pub struct LaydownPreviewData {
    pub intent: String,
    pub sensor_positions: Vec<[f64; 3]>,
    pub resource_positions: Vec<[f64; 3]>,
}

/// `None` when nothing is selected on PN-16, or when the selection no longer names a
/// laydown this baseline declares -- a reload could remove one mid-session, and a
/// stale preview of a placement that no longer exists is worse than none.
#[must_use]
pub fn laydown_preview(state: &AppState) -> Option<LaydownPreviewData> {
    let id = state.selected_laydown()?;
    let laydown = state.config.laydowns.iter().find(|l| &l.id == id)?;
    Some(LaydownPreviewData {
        intent: laydown.intent.clone(),
        sensor_positions: laydown.sensors.iter().map(|s| s.position_enu).collect(),
        resource_positions: laydown.resources.iter().map(|r| r.position_enu).collect(),
    })
}

/// The terrain model label [`planning_rows`] computed under, for the panel's caption.
#[must_use]
pub fn planning_terrain_model(state: &AppState) -> &'static str {
    if !state.data.terrains.is_empty() && state.terrain.is_masking() {
        "terrain-masked line of sight"
    } else {
        "flat-terrain line of sight"
    }
}

/// Why no sensor plan can be recommended, when none can (DN-13 §5, degradation).
///
/// Four reasons, kept apart because they have four different fixes, and **none of them is
/// "no change helps"** -- that answer is a real recommendation and comes back as an empty
/// `Ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSensorPlan {
    NoLocalFrame,
    NoApproaches,
    NoSensors,
    /// Every sensor is offline, so no mode change is legal.
    NothingCanChange,
}

impl NoSensorPlan {
    #[must_use]
    pub fn sentence(self) -> &'static str {
        match self {
            NoSensorPlan::NoLocalFrame => {
                "no local frame origin is declared, so coverage cannot be computed"
            }
            NoSensorPlan::NoApproaches => "no approaches are declared to measure coverage along",
            NoSensorPlan::NoSensors => "no sensors are configured",
            NoSensorPlan::NothingCanChange => "every sensor is offline; no mode change is legal",
        }
    }
}

/// Sensor re-tasking recommendations (GAP-037, DN-13).
///
/// Candidates are every legal mode change on every sensor that is not offline, judged by
/// the registry's own transition rules -- a change the registry would refuse is never
/// proposed (rule 1). Each is scored against the **same** coverage inputs PN-11 draws, so a
/// recommendation and the map cannot disagree about where the gaps are.
///
/// An empty `Ok` is a real answer: no change improves coverage (rule 3).
///
/// # Errors
///
/// A [`NoSensorPlan`] when coverage cannot be computed at all, rather than proposing
/// changes with an unstated rationale.
pub fn sensor_plans(
    state: &AppState,
) -> Result<Vec<gungnir_decision::sensor_plan::SensorPlan>, NoSensorPlan> {
    use gungnir_decision::sensor_plan::{SensorCandidate, SensorModeChange, SensorPlanner};
    use gungnir_model::SensorMode;
    use gungnir_sensor_management::SensorRegistry;

    let frame = local_frame(state).ok_or(NoSensorPlan::NoLocalFrame)?;
    if state.config.approaches.is_empty() {
        return Err(NoSensorPlan::NoApproaches);
    }
    let records = state.sensors.sensors();
    if records.is_empty() {
        return Err(NoSensorPlan::NoSensors);
    }

    let min_elevation = state.config.analytics.coverage_min_elevation_rad;
    let current = gungnir_analytics::coverage_from_registry(&state.sensors, min_elevation, |r| {
        frame.to_enu(r.position)
    });

    // Every legal change on every sensor that could make one. The registry's transition
    // rules are the constraint, not a preference (DN-13 rule 1).
    let mut candidates = Vec::new();
    for r in records.iter().filter(|r| r.mode != SensorMode::Offline) {
        for to in [SensorMode::Search, SensorMode::Track, SensorMode::Standby] {
            if to == r.mode || !r.mode.can_transition_to(to) {
                continue;
            }
            let radiating = matches!(to, SensorMode::Search | SensorMode::Track);
            candidates.push(SensorCandidate {
                change: SensorModeChange {
                    sensor: r.id,
                    from: format!("{:?}", r.mode),
                    to: format!("{to:?}"),
                },
                volume_after: radiating.then(|| gungnir_analytics::CoverageVolume {
                    sensor_enu: frame.to_enu(r.position),
                    max_range_m: r.max_range_m,
                    min_elevation_rad: min_elevation,
                }),
            });
        }
    }
    if candidates.is_empty() {
        return Err(NoSensorPlan::NothingCanChange);
    }

    let routes: Vec<Vec<[f64; 3]>> = state
        .config
        .approaches
        .iter()
        .map(|a| {
            a.points
                .iter()
                .map(|[lat_rad, lon_rad, alt_m]| {
                    frame.to_enu(gungnir_model::Geodetic {
                        lat_rad: *lat_rad,
                        lon_rad: *lon_rad,
                        alt_m: *alt_m,
                    })
                })
                .collect()
        })
        .collect();
    let approaches: Vec<&[[f64; 3]]> = routes.iter().map(Vec::as_slice).collect();

    let planner = SensorPlanner {
        current: &current,
        los: &gungnir_analytics::FlatTerrainLineOfSight,
        parameters: gungnir_analytics::CoverageParameters {
            sample_spacing_m: state.config.analytics.coverage_sample_spacing_m,
            terrain_masking_applied: false,
        },
        max_candidates: state.config.analytics.max_sensor_plan_candidates,
    };
    Ok(planner.recommend(&candidates, &approaches))
}

/// The names of the declared approaches, in order, for [`gap_polylines`].
///
/// Taken separately from the state so the caller can hold them while the viewport takes
/// `&mut` of its own field: a gap that borrowed the whole `AppState` would make drawing
/// it and updating the camera mutually exclusive.
#[must_use]
pub fn approach_names(state: &AppState) -> Vec<String> {
    state
        .config
        .approaches
        .iter()
        .map(|a| a.name.clone())
        .collect()
}

/// The gaps as the viewport draws them.
#[must_use]
pub fn gap_polylines<'a>(
    names: &'a [String],
    report: &'a gungnir_analytics::CoverageReport,
) -> Vec<gungnir_viewport3d::layers::GapPolyline<'a>> {
    report
        .gaps
        .iter()
        .map(|gap| gungnir_viewport3d::layers::GapPolyline {
            approach: names
                .get(gap.approach)
                .map_or("unnamed approach", String::as_str),
            samples: &gap.samples,
            uncovered: gap.severity == gungnir_analytics::GapSeverity::Uncovered,
        })
        .collect()
}

/// What the status strip says about coverage (DN-12 §7).
#[must_use]
pub fn coverage_status<'a>(
    state: &AppState,
    report: Option<&gungnir_analytics::CoverageReport>,
) -> gungnir_ui::panels::status_strip::CoverageStatus<'a> {
    use gungnir_ui::panels::status_strip::CoverageStatus;
    match report {
        Some(report) => CoverageStatus::Measured {
            uncovered_segments: report
                .gaps
                .iter()
                .filter(|g| g.severity == gungnir_analytics::GapSeverity::Uncovered)
                .count(),
            single_sensor_segments: report
                .gaps
                .iter()
                .filter(|g| g.severity == gungnir_analytics::GapSeverity::SingleSensor)
                .count(),
            terrain_masking: report.parameters.terrain_masking_applied,
        },
        None if state.config.approaches.is_empty() => CoverageStatus::NoApproaches,
        None => CoverageStatus::NotPlaceable { setting: "origin" },
    }
}
