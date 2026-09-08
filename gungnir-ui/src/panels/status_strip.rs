// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-01, the status strip (GAP-072).
//!
//! `docs/ux/information-architecture.md` §2 specifies eight elements, left to right, on
//! every layout: backend, session, mission time and clock source, health, weapons
//! control status per layer, delegation in force, alert counts, and role. §2 closes
//! with "the strip is the one place principles 4 and 6 are enforced for every role",
//! which is why it is in every workspace by construction rather than by each role
//! listing it ([`gungnir_workflow::WorkspaceLayout::ALWAYS`]).
//!
//! # It has to be able to say "I do not know"
//!
//! The strip is the surface an operator reads to answer *is this thing working, and
//! what is it allowed to do right now*. That makes it the panel where a confident
//! wrong answer is most expensive, and where architecture principle AP-02 bites
//! hardest. Two of the eight elements cannot currently be answered fully, and the types
//! here make that sayable rather than guessable:
//!
//! * [`AlertSummary`] distinguishes counts *by lifecycle state* from a flat list whose
//!   states are not tracked. The desktop still carries `Vec<String>` alerts, so today
//!   it reports a total and says the states are unknown, instead of printing three
//!   zeros that would read as "nothing new".
//! * [`ControlStatusLine::configured`] separates a layer someone set to `Hold` from a
//!   layer nobody configured. Both are at `Hold` -- that is
//!   `ControlStatusSettings::for_layer`'s deliberate safe default -- but "the commander
//!   set this" and "nobody has set this" are different operational facts.
//!
//! # No duplicate state
//!
//! `rust-ui-architecture-coding-standards.md` §2 forbids a panel holding its own copy
//! of business data. [`StatusStripView`] is a borrowed projection assembled by the
//! caller each frame: it owns nothing but small `Copy` values and holds `&str` and
//! slices into state that lives in `AppState`. Nothing here can drift from the state it
//! describes, because there is nothing here to drift.

use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::{EffectorLayer, MissionTime, SystemHealth, Vocabulary, WeaponsControlStatus};

/// Which services layer is answering, and whether it is reachable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackendStatus<'a> {
    /// The disconnected profile: services in this process.
    Embedded,
    /// A service node, reachable or not. When detached, `queued` and `dropped` are
    /// `gungnir_remote::RemoteTrackingService`'s outbox counters, which is the only
    /// place an operator can see that submissions are being held or lost.
    Node {
        endpoint: &'a str,
        connected: bool,
        queued: usize,
        dropped: u64,
        /// How recently the node was heard from (D-23).
        freshness: LinkFreshness,
    },
}

/// How recently a node was heard from, judged against its heartbeat (D-23).
///
/// **Three states, not an age.** An age alone makes the operator do the arithmetic against
/// a beat interval they do not know. The binary knows the interval and the timeout, so it
/// says which of these is true; the strip draws the age as well, because "how stale" is
/// the question the number answers once the colour has answered "is it stale".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinkFreshness {
    /// No snapshot has ever arrived. Distinct from a link that has gone quiet: nothing
    /// has been heard, so there is no age to show and no picture to be stale.
    NeverHeard,
    /// Heard inside the beat interval, give or take one beat.
    Heard { age_s: f32 },
    /// Silent for longer than a beat should ever be. The picture on screen is ageing and
    /// the operator is told by how much, before the link task has declared it gone.
    Overdue { age_s: f32 },
}

/// Where mission time comes from. A replayed session showing a wall clock, or the
/// reverse, would make every timestamp on screen mean something other than it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSource {
    Wall,
    Replay,
}

impl ClockSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ClockSource::Wall => "wall",
            ClockSource::Replay => "replay",
        }
    }
}

/// The session line: which recorded session this is, and what it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStatus {
    pub id: u64,
    /// `Live`, `Replaying` or `Closed`, from `gungnir_mission::MissionState`.
    pub state: &'static str,
}

/// The operator line (PN-01, DN-23 §7, GAP-057): who is signed in and for how much
/// longer, or why nobody is. Distinct from `role`, which decides the layout and has never
/// been an authorisation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OperatorLine<'a> {
    SignedIn {
        operator: u64,
        role: &'a str,
        /// Seconds until the session lapses; `None` for a session that does not expire.
        expires_in_s: Option<f64>,
    },
    NobodySignedIn,
    /// The session lapsed on the clock and nothing is attributed until a sign-in.
    Expired {
        operator: u64,
    },
    /// The account store cannot be reached, so nobody *can* sign in.
    StoreUnavailable,
}

/// Weapons control status for one effector layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlStatusLine {
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
    /// Whether a baseline actually set this layer. `false` means the status shown is
    /// the safe default rather than somebody's decision.
    pub configured: bool,
}

/// A pre-delegated authority in force (decision D-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegationLine<'a> {
    pub action: &'a str,
    pub role: &'a str,
    pub layer: Option<EffectorLayer>,
    pub class: Option<&'a str>,
}

/// Alert counts, or the honest absence of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSummary {
    /// Counts by lifecycle state, from `gungnir_workflow::AlertLifecycle`.
    ByState {
        new: usize,
        acknowledged: usize,
        escalated: usize,
    },
    /// The alert list carries no lifecycle state yet, so only a total is known.
    /// Reporting three zeros here would read as "nothing needs attention".
    Unclassified { total: usize },
}

/// Whether the journal is being encrypted, as PN-01 and PN-09 word it (DN-22 §7).
///
/// A view of `gungnir_security::EncryptionStatus`, which this crate may not depend on.
/// Three states rather than a boolean, because "encrypting", "configured but the keystore
/// is unavailable" and "not configured" are three different facts and only the middle one
/// is a fault. **A system that claimed encryption it was not performing would be worse
/// than one that admits it is not** -- DN-22 §5 applies AP-02 to a security feature, and
/// this is where an operator sees the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionState<'a> {
    Active,
    /// Configured, and the keystore could not be reached, so the journal is in the clear.
    UnavailableWritingPlaintext {
        reason: &'a str,
    },
    NotConfigured,
}

impl EncryptionState<'_> {
    /// What the strip says, or `None` when nothing needs saying.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        match self {
            EncryptionState::Active => None,
            EncryptionState::UnavailableWritingPlaintext { reason } => {
                Some(format!("journal NOT encrypted: {reason}"))
            }
            EncryptionState::NotConfigured => {
                Some("journal not encrypted: none configured".to_owned())
            }
        }
    }

    /// True when the deployment asked for encryption and is not getting it. Distinct
    /// from never having asked, which is not a fault.
    #[must_use]
    pub fn is_fault(&self) -> bool {
        matches!(self, EncryptionState::UnavailableWritingPlaintext { .. })
    }
}

/// Where the baseline in force stands against its own validity window (DN-08 §5).
///
/// Four states rather than a boolean, because **"valid" and "no window was configured"
/// are not the same claim** and neither is "not yet". A deployment that declared a window
/// and has run past it is in a different situation from one that never declared one, and
/// an operator whose plans are suddenly being superseded needs to be able to tell which.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BaselineValidity {
    /// No window configured, so the baseline is always valid -- the defaults table's row
    /// in DN-08 §5, and the ordinary case.
    NoWindowConfigured,
    /// Inside its window; `until` is absent when the window has no end.
    InForce { until: Option<MissionTime> },
    /// Declared, and its start has not arrived. Plans made now are superseded.
    NotYet { from: MissionTime },
    /// Declared, and past its end. Plans made now are superseded.
    Expired { since: MissionTime },
}

impl BaselineValidity {
    /// True when a plan produced under this baseline is superseded rather than applied.
    #[must_use]
    pub fn supersedes_plans(&self) -> bool {
        matches!(
            self,
            BaselineValidity::NotYet { .. } | BaselineValidity::Expired { .. }
        )
    }
}

/// Everything the strip draws, borrowed from `AppState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusStripView<'a> {
    pub backend: BackendStatus<'a>,
    pub session: Option<SessionStatus>,
    pub mission_time: MissionTime,
    pub clock_source: ClockSource,
    pub health: SystemHealth,
    pub control_status: &'a [ControlStatusLine],
    pub delegations: &'a [DelegationLine<'a>],
    pub alerts: AlertSummary,
    pub role: &'a str,
    /// The words this deployment uses (D-12).
    pub vocabulary: &'a Vocabulary,
    /// Coverage of the declared approaches (DN-12 §7, GAP-006).
    pub coverage: CoverageStatus<'a>,
    /// Whether the journal is encrypted (DN-22 §7, GAP-084).
    pub encryption: EncryptionState<'a>,
    /// Where the baseline stands against its validity window (DN-08 §7, GAP-052).
    pub validity: BaselineValidity,
    /// The mission profile this deployment is operating in, **and only when more than one
    /// is declared** (DN-24 §9, GAP-086).
    ///
    /// `None` for a deployment with one profile or none: it gains nothing from being told
    /// which, and an element that is always the same trains an operator past it.
    pub profile: Option<&'a str>,
    /// The seeded session in progress, named, for the whole session (GAP-089). A record
    /// made under it is a rehearsal and the strip never lets that be forgotten.
    pub rehearsal: Option<&'a str>,
    /// Who is signed in (GAP-057, DN-23 §7).
    pub operator: OperatorLine<'a>,
}

/// The operator line. Nobody signed in is drawn in the warning colour because every
/// decision taken now goes on the record unattributed (DN-23 §5); an expired session
/// says so in the same colour rather than vanishing into "nobody".
fn draw_operator(ui: &mut Ui, palette: &theme::Palette, line: OperatorLine<'_>) {
    match line {
        OperatorLine::SignedIn {
            operator,
            role,
            expires_in_s,
        } => match expires_in_s {
            Some(s) if s <= 0.0 => {
                ui.label(
                    RichText::new(format!("Operator {operator} ({role}) session lapsing"))
                        .color(palette.warning_color),
                );
            }
            Some(s) => {
                ui.label(format!(
                    "Operator {operator} ({role}), {} left",
                    format_remaining(s)
                ));
            }
            None => {
                ui.label(format!("Operator {operator} ({role})"));
            }
        },
        OperatorLine::NobodySignedIn => {
            ui.label(
                RichText::new("Nobody signed in: decisions are unattributed")
                    .color(palette.warning_color),
            );
        }
        OperatorLine::Expired { operator } => {
            ui.label(
                RichText::new(format!(
                    "Operator {operator}'s session expired; sign in again"
                ))
                .color(palette.warning_color),
            );
        }
        OperatorLine::StoreUnavailable => {
            ui.label(
                RichText::new("No account store: nobody can sign in").color(palette.alert_color),
            );
        }
    }
}

/// `Nh Nm` or `Nm Ns`, for a session's remaining time.
fn format_remaining(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--".into();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let s = seconds as u64;
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

/// `HH:MM:SS` from mission seconds.
///
/// Mission time is seconds from the session start in replay and Unix seconds live, so
/// this shows the time of day in both cases by taking the remainder of a day. A
/// negative or non-finite value is shown as `--:--:--` rather than wrapping into a
/// plausible-looking time.
#[must_use]
pub fn format_clock(t: MissionTime) -> String {
    if !t.0.is_finite() || t.0 < 0.0 {
        return "--:--:--".to_owned();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total = t.0 as u64 % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// One health dot with its hover text.
fn health_dot(ui: &mut Ui, palette: &theme::Palette, name: &str, ok: bool, reason_when_bad: &str) {
    let (mark, colour) = if ok {
        ("●", palette.healthy_color())
    } else {
        ("●", palette.alert_color)
    };
    let response = ui.label(RichText::new(mark).color(colour).strong());
    let tooltip = if ok {
        format!("{name}: reporting healthy")
    } else {
        format!("{name}: {reason_when_bad}")
    };
    response.on_hover_text(tooltip);
}

/// The backend element: which services layer answers, and whether it is reachable.
/// The encryption state, and **only when there is something to say**.
///
/// A strip that announced "encrypted" on every frame would train an operator to stop
/// reading it; the case that matters is the one where it is off.
fn draw_encryption(ui: &mut Ui, palette: &theme::Palette, state: EncryptionState<'_>) {
    match state.warning() {
        None => {
            ui.label(RichText::new("journal encrypted").color(palette.healthy_color()));
        }
        Some(warning) => {
            ui.label(RichText::new(warning).color(if state.is_fault() {
                palette.alert_color
            } else {
                palette.warning_color
            }));
        }
    }
}

/// The baseline validity state, and **only when there is something to say**.
///
/// Same argument as the encryption element: a strip that said "baseline valid" on every
/// frame would be read past. The case that matters is the one where plans made now will
/// not be applied, and that case is drawn loudly because nothing else on the strip
/// explains why the approval queue has stopped filling.
fn draw_validity(ui: &mut Ui, palette: &theme::Palette, validity: BaselineValidity) {
    match validity {
        // Nothing drawn: no window was configured, which is the ordinary case and not a
        // state anybody needs told about.
        BaselineValidity::NoWindowConfigured => {}
        BaselineValidity::InForce { until: None } => {
            ui.label(RichText::new("baseline in force").color(palette.healthy_color()));
        }
        BaselineValidity::InForce { until: Some(t) } => {
            ui.label(
                RichText::new(format!("baseline valid until {}", format_clock(t)))
                    .color(palette.healthy_color()),
            );
        }
        BaselineValidity::NotYet { from } => {
            ui.label(
                RichText::new(format!(
                    "baseline not in force until {}: plans are superseded",
                    format_clock(from)
                ))
                .color(palette.alert_color),
            );
        }
        BaselineValidity::Expired { since } => {
            ui.label(
                RichText::new(format!(
                    "baseline expired {}: plans are superseded",
                    format_clock(since)
                ))
                .color(palette.alert_color),
            );
        }
    }
}

/// "heard N s ago", coloured by whether N is what a live link looks like (D-23).
///
/// Drawn beside a connected node and never beside a detached one: once the link is
/// declared gone the detached line already says so, and a second number would be
/// competing with it.
fn draw_freshness(ui: &mut Ui, palette: &theme::Palette, freshness: LinkFreshness) {
    match freshness {
        LinkFreshness::NeverHeard => {
            ui.label(RichText::new("nothing heard yet").color(palette.warning_color));
        }
        LinkFreshness::Heard { age_s } => {
            ui.label(
                RichText::new(format!("heard {age_s:.1} s ago")).color(palette.healthy_color()),
            );
        }
        LinkFreshness::Overdue { age_s } => {
            ui.label(
                RichText::new(format!("nothing heard for {age_s:.1} s"))
                    .color(palette.alert_color)
                    .strong(),
            );
        }
    }
}

fn draw_backend(ui: &mut Ui, palette: &theme::Palette, backend: BackendStatus<'_>) {
    match backend {
        BackendStatus::Embedded => {
            ui.label(RichText::new("Embedded").color(palette.healthy_color()));
        }
        BackendStatus::Node {
            endpoint,
            connected,
            queued,
            dropped,
            freshness,
        } => {
            if connected {
                ui.label(RichText::new(format!("Node {endpoint}")).color(palette.healthy_color()));
                draw_freshness(ui, palette, freshness);
            } else {
                // The counts are the point: an operator needs to know submissions are
                // being held, and how many have already been lost.
                ui.label(
                    RichText::new(format!(
                        "Detached from {endpoint}, {queued} queued, {dropped} dropped"
                    ))
                    .color(palette.alert_color)
                    .strong(),
                );
            }
        }
    }
}

/// The health element: one dot per service, each with the reason it is down.
fn draw_health(ui: &mut Ui, palette: &theme::Palette, health: SystemHealth) {
    ui.label("Health");
    health_dot(
        ui,
        palette,
        "Tracking",
        health.tracking_healthy,
        "the tracking pipeline is not running",
    );
    health_dot(
        ui,
        palette,
        "Intercept",
        health.intercept_healthy,
        "the intercept service reported unhealthy",
    );
    health_dot(
        ui,
        palette,
        "Ingest",
        health.ingest_healthy,
        "no adapter has delivered within the expected interval",
    );
}

/// The weapons control status element: one chip per effector layer.
fn draw_control_status(
    ui: &mut Ui,
    palette: &theme::Palette,
    lines: &[ControlStatusLine],
    vocabulary: &Vocabulary,
) {
    if lines.is_empty() {
        ui.label(RichText::new("Control status unknown").color(palette.muted_text_color()));
        return;
    }
    for line in lines {
        let colour = match line.status {
            WeaponsControlStatus::Free => palette.alert_color,
            WeaponsControlStatus::Tight => palette.warning_color,
            WeaponsControlStatus::Hold => palette.healthy_color(),
        };
        let chip = ui.label(
            RichText::new(format!(
                "{} {}",
                vocabulary.layer(line.layer),
                vocabulary.control_status(line.status)
            ))
            .color(colour)
            .strong(),
        );
        if line.configured {
            chip.on_hover_text("Set by the configuration baseline in force.");
        } else {
            chip.on_hover_text(
                "No baseline sets this layer. Hold is the safe default, not a decision.",
            );
        }
    }
}

/// The delegation element: the pre-delegated authorities in force (D-15).
fn draw_delegations(
    ui: &mut Ui,
    palette: &theme::Palette,
    delegations: &[DelegationLine<'_>],
    vocabulary: &Vocabulary,
) {
    if delegations.is_empty() {
        ui.label("No delegation");
        return;
    }
    for d in delegations {
        // Built in one format so the string is allocated once, not grown twice.
        let text = match (d.layer, d.class) {
            (Some(layer), Some(class)) => {
                format!(
                    "{} delegated to {} ({}) [{class}]",
                    d.action,
                    d.role,
                    vocabulary.layer(layer)
                )
            }
            (Some(layer), None) => {
                format!(
                    "{} delegated to {} ({})",
                    d.action,
                    d.role,
                    vocabulary.layer(layer)
                )
            }
            (None, Some(class)) => {
                format!("{} delegated to {} [{class}]", d.action, d.role)
            }
            (None, None) => format!("{} delegated to {}", d.action, d.role),
        };
        ui.label(RichText::new(text).color(palette.warning_color));
    }
}

/// The alert element, which can say that it does not know the states.
fn draw_alerts(ui: &mut Ui, palette: &theme::Palette, alerts: AlertSummary) {
    match alerts {
        AlertSummary::ByState {
            new,
            acknowledged,
            escalated,
        } => {
            ui.label(format!(
                "Alerts {new} new, {acknowledged} ack, {escalated} escalated"
            ));
        }
        AlertSummary::Unclassified { total } => {
            ui.label(RichText::new(format!("Alerts {total}")).color(palette.muted_text_color()))
                .on_hover_text(
                    "The desktop carries a flat alert list; lifecycle states are not \
                     tracked yet, so new, acknowledged and escalated counts are not \
                     available.",
                );
        }
    }
}

/// What the strip says about coverage (DN-12 §7).
///
/// The three cases are separate because they are separate operational facts, and the
/// one that must never be shown as the others is the middle one: a deployment with no
/// declared approaches has nothing to measure coverage *along*, which is not the same
/// as measuring it and finding none missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus<'a> {
    /// Approaches are declared and this many segments are covered by nobody. Zero here
    /// is a real result.
    Measured {
        uncovered_segments: usize,
        single_sensor_segments: usize,
        /// False when the answer was computed on flat terrain, which is optimistic by
        /// construction (DN-12 §5 rule 3).
        terrain_masking: bool,
    },
    /// Nothing to measure along.
    NoApproaches,
    /// Approaches are declared but cannot be placed in the picture: no local frame.
    NotPlaceable { setting: &'a str },
}

fn draw_coverage(ui: &mut Ui, palette: &theme::Palette, coverage: CoverageStatus<'_>) {
    match coverage {
        CoverageStatus::Measured {
            uncovered_segments,
            single_sensor_segments,
            terrain_masking,
        } => {
            let colour = if uncovered_segments > 0 {
                palette.alert_color
            } else {
                palette.healthy_color()
            };
            ui.label(
                RichText::new(format!(
                    "Coverage {uncovered_segments} uncovered, {single_sensor_segments} \
                     single-sensor"
                ))
                .color(colour),
            );
            if !terrain_masking {
                ui.label(RichText::new("(flat terrain)").color(palette.warning_color))
                    .on_hover_text(
                        "No terrain model was available, so coverage was computed on flat \
                     ground. That is optimistic: a valley the sensor cannot see into is \
                     reported as covered.",
                    );
            }
        }
        CoverageStatus::NoApproaches => {
            ui.label(RichText::new("Coverage not measured").color(palette.muted_text_color()))
                .on_hover_text(
                    "No approach axes are declared, so there is nothing to measure \
                     coverage along. This is not the same as full coverage.",
                );
        }
        CoverageStatus::NotPlaceable { setting } => {
            ui.label(RichText::new("Coverage unplaceable").color(palette.muted_text_color()))
                .on_hover_text(format!(
                    "Approaches are declared but this deployment has no local frame \
                     origin ({setting}), so they cannot be placed in the picture."
                ));
        }
    }
}

/// Render the strip. Reads only; writes nothing (`docs/ux/ux-to-code-map.md` §1).
///
/// The elements are drawn left to right in the order
/// `docs/ux/information-architecture.md` §2 lists them, each by its own function so
/// that the order is visible here and the detail is not.
pub fn render_status_strip(ui: &mut Ui, palette: &theme::Palette, view: &StatusStripView<'_>) {
    ui.horizontal_wrapped(|ui| {
        draw_backend(ui, palette, view.backend);
        ui.separator();
        draw_encryption(ui, palette, view.encryption);
        ui.separator();
        // Drawn immediately after encryption and before the session, because both answer
        // "is what I am doing being recorded and honoured?" and neither is visible
        // anywhere else on the strip.
        if !matches!(view.validity, BaselineValidity::NoWindowConfigured) {
            draw_validity(ui, palette, view.validity);
            ui.separator();
        }
        // Only when the deployment has a choice to have made (DN-24 §9).
        if let Some(profile) = view.profile {
            ui.label(format!("Profile {profile}"));
            ui.separator();
        }
        // GAP-089: a seeded session says so for as long as it runs, in the alert colour,
        // so no record made under it is mistaken for an operation.
        if let Some(rehearsal) = view.rehearsal {
            ui.label(
                RichText::new(rehearsal.to_uppercase())
                    .color(palette.alert_color)
                    .strong(),
            );
            ui.separator();
        }

        match view.session {
            Some(s) => {
                ui.label(format!("Session {} {}", s.id, s.state));
            }
            None => {
                ui.label(RichText::new("No session").color(palette.warning_color));
            }
        }
        ui.separator();

        draw_operator(ui, palette, view.operator);
        ui.separator();

        ui.label(format!(
            "{} ({})",
            format_clock(view.mission_time),
            view.clock_source.label()
        ));
        ui.separator();

        draw_health(ui, palette, view.health);
        ui.separator();

        draw_control_status(ui, palette, view.control_status, view.vocabulary);
        ui.separator();

        draw_delegations(ui, palette, view.delegations, view.vocabulary);
        ui.separator();

        draw_coverage(ui, palette, view.coverage);
        ui.separator();
        draw_alerts(ui, palette, view.alerts);
        ui.separator();

        ui.label(RichText::new(view.role).strong());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_formats_time_of_day() {
        assert_eq!(format_clock(MissionTime(0.0)), "00:00:00");
        assert_eq!(format_clock(MissionTime(3661.0)), "01:01:01");
        // Wraps at a day, so a Unix timestamp shows a time of day rather than a
        // meaningless five-digit hour.
        assert_eq!(format_clock(MissionTime(86_400.0)), "00:00:00");
        assert_eq!(format_clock(MissionTime(86_399.0)), "23:59:59");
    }

    /// A time the strip cannot render must not become a plausible-looking one.
    #[test]
    fn a_bad_clock_shows_as_unknown_not_as_midnight() {
        assert_eq!(format_clock(MissionTime(f64::NAN)), "--:--:--");
        assert_eq!(format_clock(MissionTime(f64::INFINITY)), "--:--:--");
        assert_eq!(format_clock(MissionTime(-1.0)), "--:--:--");
    }

    /// The distinction the strip exists to preserve: a flat alert list must not be
    /// reported as three zeros, which would read as "nothing needs attention".
    #[test]
    fn unclassified_alerts_are_not_reported_as_zero_of_each_state() {
        let summary = AlertSummary::Unclassified { total: 4 };
        assert_ne!(
            summary,
            AlertSummary::ByState {
                new: 0,
                acknowledged: 0,
                escalated: 0
            }
        );
        match summary {
            AlertSummary::Unclassified { total } => assert_eq!(total, 4),
            AlertSummary::ByState { .. } => panic!("a flat list is not state-classified"),
        }
    }

    /// An unconfigured layer and a layer someone set to Hold are both at Hold, and the
    /// strip keeps them distinguishable.
    #[test]
    fn an_unset_layer_is_distinguishable_from_a_layer_set_to_hold() {
        let unset = ControlStatusLine {
            layer: EffectorLayer::Area,
            status: WeaponsControlStatus::Hold,
            configured: false,
        };
        let held = ControlStatusLine {
            configured: true,
            ..unset
        };
        assert_eq!(unset.status, held.status);
        assert_ne!(unset, held);
    }

    #[test]
    fn a_detached_node_carries_its_outbox_counts() {
        let backend = BackendStatus::Node {
            endpoint: "node.example:7410",
            connected: false,
            queued: 1234,
            dropped: 7,
            freshness: LinkFreshness::NeverHeard,
        };
        match backend {
            BackendStatus::Node {
                connected,
                queued,
                dropped,
                ..
            } => {
                assert!(!connected);
                assert_eq!(queued, 1234);
                assert_eq!(dropped, 7);
            }
            BackendStatus::Embedded => panic!("expected a node"),
        }
    }
}
