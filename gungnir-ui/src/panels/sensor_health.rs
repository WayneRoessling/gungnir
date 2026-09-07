//! System health indicator: tracking pipeline, intercept planner, ingest gateway.
//! Reads `gungnir_model::SystemHealth` as reported by `gungnir-observability`.

use crate::theme;
use egui::RichText;
use gungnir_model::{MissionTime, SystemHealth};

/// Why a sensor is or is not on the air (DN-21 §5, GAP-054).
///
/// **Four states, because a silent sensor means four different things and only two of them
/// need somebody.** Collapsing them into a health boolean is how a scheduled outage becomes
/// an unnoticed hole -- and how a real failure gets shrugged off as "that's the maintenance
/// window".
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensorPresence<'a> {
    /// On the air.
    Radiating,
    /// Off the air inside a planned window. Expected, and not a fault.
    InMaintenance { until: MissionTime, reason: &'a str },
    /// Off the air with no window open. A fault.
    Failed,
    /// A window closed and the sensor never came back.
    ///
    /// Louder than `Failed`: somebody planned this outage, it has not ended, and the plan
    /// said it would have.
    Overrun {
        window_closed: MissionTime,
        reason: &'a str,
    },
}

/// One anomaly detector's state on the health panel (DN-15 §6, GAP-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectorLine<'a> {
    pub name: &'a str,
    /// `None` when off; `Some(None)` running; `Some(Some(reason))` configured and unable.
    pub state: Option<Option<&'a str>>,
}

/// Clock synchronization across the sources that have been heard from (GAP-008).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSyncLine {
    pub sources_observed: u32,
    pub sources_out_of_sync: u32,
    pub max_skew_s: f32,
}

/// One sensor's line on the health panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorHealthLine<'a> {
    pub id: u32,
    pub modality: &'a str,
    pub presence: SensorPresence<'a>,
}

/// The terrain line (GAP-023): masking, or flat and why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainLine<'a> {
    pub masking: bool,
    pub detail: &'a str,
}

/// One bound radar feed's counters (GAP-001). Every datagram is in one of the columns:
/// a feed that is bound and hears nothing shows zeros, one that hears an unreadable
/// stream shows them under `not_decoded`, and neither passes for a working feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedLine<'a> {
    pub name: &'a str,
    pub datagrams: u64,
    pub detections: u64,
    pub service_reports: u64,
    /// Malformed datagrams and blocks, and categories this build does not decode.
    pub not_decoded: u64,
    /// Records from a radar no binding names; never attributed, never guessed.
    pub unknown_radar: u64,
}

/// One bound AIS feed's counters (GAP-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooperativeFeedLine<'a> {
    pub name: &'a str,
    pub sentences: u64,
    pub positions: u64,
    pub static_reports: u64,
    pub not_decoded: u64,
}

/// One peer link (GAP-009): a partner's node, linked or not, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerLine<'a> {
    pub name: &'a str,
    pub endpoint: &'a str,
    pub connected: bool,
    /// The link's last error when not connected; "no answer yet" before the first.
    pub reason: &'a str,
}

/// Everything PN-09 draws, borrowed from the binary's state.
#[derive(Debug, Clone, Copy)]
pub struct SensorHealthView<'a> {
    pub health: &'a SystemHealth,
    pub encryption: crate::panels::status_strip::EncryptionState<'a>,
    pub sensors: &'a [SensorHealthLine<'a>],
    pub clocks: ClockSyncLine,
    pub detectors: &'a [DetectorLine<'a>],
    pub terrain: TerrainLine<'a>,
    /// The radar feeds the baseline bound (GAP-001); empty when none is configured.
    pub feeds: &'a [FeedLine<'a>],
    /// The AIS feeds the baseline bound (GAP-010); empty when none is configured.
    pub cooperative_feeds: &'a [CooperativeFeedLine<'a>],
    /// The peer links (GAP-009). Empty means none configured.
    pub peers: &'a [PeerLine<'a>],
}

pub fn render_sensor_health(ui: &mut egui::Ui, view: &SensorHealthView<'_>) {
    use crate::panels::status_strip::EncryptionState;
    let SensorHealthView {
        health,
        encryption,
        sensors,
        clocks,
        detectors,
        terrain,
        feeds,
        cooperative_feeds,
        peers: _,
    } = *view;

    ui.heading("System health");
    render_indicator(ui, "Tracking pipeline", health.tracking_healthy);
    render_indicator(ui, "Intercept planner", health.intercept_healthy);
    render_indicator(ui, "Ingest gateway", health.ingest_healthy);

    // Encryption is not a fourth `SystemHealth` flag, because it has three states and
    // that struct has booleans. **Reporting whether encryption is active is a boolean
    // and not a disclosure** (DN-22 §6), so it belongs on the health summary; which of
    // the two off states applies is what tells an administrator whether to act.
    ui.separator();
    match encryption {
        EncryptionState::Active => render_indicator(ui, "Journal encryption", true),
        EncryptionState::NotConfigured => {
            render_indicator(ui, "Journal encryption", false);
            ui.label(
                egui::RichText::new("not configured for this deployment")
                    .color(theme::MUTED_TEXT_COLOR)
                    .size(theme::SMALL_FONT_SIZE),
            );
        }
        EncryptionState::UnavailableWritingPlaintext { reason } => {
            render_indicator(ui, "Journal encryption", false);
            ui.label(
                egui::RichText::new(format!(
                    "configured, and the keystore could not be reached: {reason}. The \
                     journal is being written in the clear."
                ))
                .color(theme::CLASS_HOSTILE_COLOR)
                .size(theme::SMALL_FONT_SIZE),
            );
        }
    }

    // GAP-023: whether line of sight is masked against terrain, and if not, why not.
    render_indicator(ui, "Terrain masking", terrain.masking);
    ui.label(
        egui::RichText::new(terrain.detail)
            .color(if terrain.masking {
                theme::MUTED_TEXT_COLOR
            } else {
                theme::WARNING_COLOR
            })
            .size(theme::SMALL_FONT_SIZE),
    );

    render_sensors(ui, sensors);
    render_feeds(ui, feeds);
    render_cooperative_feeds(ui, cooperative_feeds);
    render_peers(ui, view.peers);
    render_clocks(ui, clocks);
    render_detectors(ui, detectors);
}

/// The AIS feeds (GAP-010): what the receiver heard and what became a placed report.
fn render_peers(ui: &mut egui::Ui, peers: &[PeerLine<'_>]) {
    if peers.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Peer links").strong());
    for p in peers {
        let (text, colour) = if p.connected {
            (
                format!("{}: linked to {}", p.name, p.endpoint),
                theme::HEALTHY_COLOR,
            )
        } else {
            (
                format!("{}: not linked to {} ({})", p.name, p.endpoint, p.reason),
                theme::WARNING_COLOR,
            )
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(theme::SMALL_FONT_SIZE),
        );
    }
}

fn render_cooperative_feeds(ui: &mut egui::Ui, feeds: &[CooperativeFeedLine<'_>]) {
    if feeds.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("AIS feeds").strong());
    for f in feeds {
        let (text, colour) = if f.sentences == 0 {
            (
                format!("{}: bound, nothing received yet", f.name),
                theme::WARNING_COLOR,
            )
        } else {
            let text = format!(
                "{}: {} sentences, {} positions placed, {} static reports, {} not decoded",
                f.name, f.sentences, f.positions, f.static_reports, f.not_decoded
            );
            let colour = if f.not_decoded > 0 {
                theme::WARNING_COLOR
            } else {
                theme::HEALTHY_COLOR
            };
            (text, colour)
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(theme::SMALL_FONT_SIZE),
        );
    }
}

/// The radar feeds (GAP-001). A bound feed with nothing received is said in the
/// warning colour: it is the state a wrong multicast group or a quiet radar produces,
/// and it looks like health until somebody reads the number.
fn render_feeds(ui: &mut egui::Ui, feeds: &[FeedLine<'_>]) {
    if feeds.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Radar feeds").strong());
    for f in feeds {
        let (text, colour) = if f.datagrams == 0 {
            (
                format!("{}: bound, nothing received yet", f.name),
                theme::WARNING_COLOR,
            )
        } else {
            let mut text = format!(
                "{}: {} datagrams, {} detections, {} service reports",
                f.name, f.datagrams, f.detections, f.service_reports
            );
            let colour = if f.not_decoded > 0 || f.unknown_radar > 0 {
                use std::fmt::Write as _;
                // Writing into a `String` cannot fail.
                let _ = write!(
                    text,
                    "; {} not decoded, {} from unknown radars",
                    f.not_decoded, f.unknown_radar
                );
                theme::WARNING_COLOR
            } else {
                theme::HEALTHY_COLOR
            };
            (text, colour)
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(theme::SMALL_FONT_SIZE),
        );
    }
}

/// Which anomaly detectors are running. **An unconfigured detector is listed as off**
/// rather than left out, so its absence is visible (DN-15 §6); one that is configured and
/// cannot evaluate in this build says why, rather than passing for running.
fn render_detectors(ui: &mut egui::Ui, detectors: &[DetectorLine<'_>]) {
    ui.separator();
    ui.label(RichText::new("Anomaly detectors").strong());
    for d in detectors {
        let (text, colour) = match d.state {
            None => (format!("{}: off", d.name), theme::MUTED_TEXT_COLOR),
            Some(None) => (format!("{}: running", d.name), theme::HEALTHY_COLOR),
            Some(Some(reason)) => (
                format!("{}: configured, cannot run: {reason}", d.name),
                theme::CLASS_HOSTILE_COLOR,
            ),
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(theme::SMALL_FONT_SIZE),
        );
    }
}

/// Clock skew across sources, judged against the late-data policy (GAP-008, MOP-09).
///
/// **Says how many sources the figure covers**, because "no skew" across nothing is the
/// statement this element used to make.
fn render_clocks(ui: &mut egui::Ui, clocks: ClockSyncLine) {
    ui.separator();
    if clocks.sources_observed == 0 {
        ui.label(
            RichText::new("Clock sync: no source heard from yet")
                .color(theme::MUTED_TEXT_COLOR)
                .size(theme::SMALL_FONT_SIZE),
        );
        return;
    }
    render_indicator(ui, "Clock sync", clocks.sources_out_of_sync == 0);
    let note = if clocks.sources_out_of_sync == 0 {
        format!(
            "{} source(s), largest skew {:.2} s",
            clocks.sources_observed, clocks.max_skew_s
        )
    } else {
        format!(
            "{} of {} source(s) out of sync with the late-data policy; largest skew {:.2} s",
            clocks.sources_out_of_sync, clocks.sources_observed, clocks.max_skew_s
        )
    };
    ui.label(
        RichText::new(note)
            .color(if clocks.sources_out_of_sync == 0 {
                theme::MUTED_TEXT_COLOR
            } else {
                theme::CLASS_HOSTILE_COLOR
            })
            .size(theme::SMALL_FONT_SIZE),
    );
}

/// The per-sensor rows.
///
/// Drawn after the three service flags because a sensor is a narrower question than
/// "is the pipeline running", and an operator scanning this panel reads the wide ones
/// first.
fn render_sensors(ui: &mut egui::Ui, sensors: &[SensorHealthLine<'_>]) {
    ui.separator();
    if sensors.is_empty() {
        // Not a blank: no sensors configured is a deployment state, and it is different
        // from every sensor being silent.
        ui.label(
            RichText::new("No sensors are configured for this deployment.")
                .color(theme::MUTED_TEXT_COLOR)
                .size(theme::SMALL_FONT_SIZE),
        );
        return;
    }
    ui.label(RichText::new("Sensors").strong());
    for line in sensors {
        let (healthy, note, colour) = match line.presence {
            SensorPresence::Radiating => (true, None, theme::MUTED_TEXT_COLOR),
            SensorPresence::InMaintenance { until, reason } => (
                true,
                Some(format!(
                    "planned maintenance until {}: {reason}",
                    crate::panels::status_strip::format_clock(until)
                )),
                theme::MUTED_TEXT_COLOR,
            ),
            SensorPresence::Failed => (
                false,
                Some("off the air, with no maintenance window open".to_owned()),
                theme::CLASS_HOSTILE_COLOR,
            ),
            SensorPresence::Overrun {
                window_closed,
                reason,
            } => (
                false,
                Some(format!(
                    "did not return from planned maintenance ({reason}); the window closed \
                     at {}",
                    crate::panels::status_strip::format_clock(window_closed)
                )),
                theme::CLASS_HOSTILE_COLOR,
            ),
        };
        render_indicator(
            ui,
            &format!("Sensor {} ({})", line.id, line.modality),
            healthy,
        );
        if let Some(note) = note {
            ui.label(
                RichText::new(note)
                    .color(colour)
                    .size(theme::SMALL_FONT_SIZE),
            );
        }
    }
}

fn render_indicator(ui: &mut egui::Ui, label: &str, healthy: bool) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                theme::STATUS_DOT_RADIUS * 2.0,
                theme::STATUS_DOT_RADIUS * 2.0,
            ),
            egui::Sense::hover(),
        );
        let color = if healthy {
            theme::HEALTHY_COLOR
        } else {
            theme::DEGRADED_COLOR
        };
        ui.painter()
            .circle_filled(rect.center(), theme::STATUS_DOT_RADIUS, color);
        ui.label(label);
        ui.label(RichText::new(if healthy { "OK" } else { "DEGRADED" }).color(color));
    });
}
