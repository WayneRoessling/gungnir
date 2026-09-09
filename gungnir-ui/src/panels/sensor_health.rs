// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

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

/// Which backend is registering the configured point-cloud pair, and why (GAP-024,
/// GAP-098). Four states, the same reason `PointCloudStatus`/`TerrainStatus` each use
/// more than a boolean: collapsing "no pair" and "GPU" and "the CPU fallback" into one
/// flag is exactly how an operator mistakes a quiet fallback for the GPU path working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointCloudRegistrationLine<'a> {
    /// No pair is configured, still loading, or failed to load (GAP-098's
    /// all-or-nothing rule): none of those is a pair to register against.
    NotConfigured,
    /// A pair is loaded and this tick has not yet resolved which backend runs it.
    /// Not expected to outlive a single tick in a running deployment
    /// (`gungnir_app::pointcloud::register` resolves the backend the same tick a pair
    /// finishes loading) -- named rather than folded into `NotConfigured` (a pair does
    /// exist) or into either backend (neither has actually run yet).
    Pending,
    /// A pair is loaded and registration is running on the GPU path.
    Gpu,
    /// A pair is loaded; the GPU path was unavailable
    /// (`RenderError::NoAdapter`/`GpuInit`), so this is the CPU reference, and why.
    CpuFallback { reason: &'a str },
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

/// One bound spotter/acoustic/passive-RF feed's counters (GAP-001, GAP-096): what a
/// SAPIENT node reported. Every message is in one of `bearings`, `ranged`, `positions`
/// or `refused`, the same accounting `FeedLine` gives a radar's datagrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BearingFeedLine<'a> {
    pub name: &'a str,
    pub messages: u64,
    /// Detections that became a `Measurement::Bearing`: direction and no range.
    pub bearings: u64,
    /// Detections that became a `Measurement::RangeAzimuthElevation`: a lased or
    /// triangulated range, which stays polar (DN-27 §4).
    pub ranged: u64,
    /// Detections that became a `Measurement::Position`.
    pub positions: u64,
    pub refused: u64,
}

/// The pipeline's own bearing counters (DN-27 §5 rule 3; GAP-096): what happened to
/// every bearing offered to `FusionPipeline::offer_bearing`, across every feed. Shown
/// once rather than once per feed, because the pipeline is one thing every feed's
/// bearings pass through, not a property of any single feed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BearingPipelineLine {
    pub offered: u64,
    pub updated: u64,
    pub retained: u64,
    pub expired: u64,
    pub refused: u64,
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
    /// The point-cloud registration backend (GAP-024, GAP-098): GPU, the CPU fallback
    /// and why, or that no pair is configured at all.
    pub point_cloud_registration: PointCloudRegistrationLine<'a>,
    /// The radar feeds the baseline bound (GAP-001); empty when none is configured.
    pub feeds: &'a [FeedLine<'a>],
    /// The AIS feeds the baseline bound (GAP-010); empty when none is configured.
    pub cooperative_feeds: &'a [CooperativeFeedLine<'a>],
    /// The peer links (GAP-009). Empty means none configured.
    pub peers: &'a [PeerLine<'a>],
    /// The spotter/acoustic/passive-RF feeds the baseline bound (GAP-001, GAP-096);
    /// empty when none is configured.
    pub bearing_feeds: &'a [BearingFeedLine<'a>],
    /// The pipeline's own bearing counters (GAP-096), read alongside `bearing_feeds`.
    pub bearing_pipeline: BearingPipelineLine,
}

pub fn render_sensor_health(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    view: &SensorHealthView<'_>,
) {
    use crate::panels::status_strip::EncryptionState;
    let SensorHealthView {
        health,
        encryption,
        sensors,
        clocks,
        detectors,
        terrain,
        point_cloud_registration,
        feeds,
        cooperative_feeds,
        peers: _,
        bearing_feeds,
        bearing_pipeline,
    } = *view;

    ui.heading("System health");
    render_indicator(ui, palette, "Tracking pipeline", health.tracking_healthy);
    render_indicator(ui, palette, "Intercept planner", health.intercept_healthy);
    render_indicator(ui, palette, "Ingest gateway", health.ingest_healthy);

    // Encryption is not a fourth `SystemHealth` flag, because it has three states and
    // that struct has booleans. **Reporting whether encryption is active is a boolean
    // and not a disclosure** (DN-22 §6), so it belongs on the health summary; which of
    // the two off states applies is what tells an administrator whether to act.
    ui.separator();
    match encryption {
        EncryptionState::Active => render_indicator(ui, palette, "Journal encryption", true),
        EncryptionState::NotConfigured => {
            render_indicator(ui, palette, "Journal encryption", false);
            ui.label(
                egui::RichText::new("not configured for this deployment")
                    .color(palette.muted_text_color())
                    .size(palette.small_font_size),
            );
        }
        EncryptionState::UnavailableWritingPlaintext { reason } => {
            render_indicator(ui, palette, "Journal encryption", false);
            ui.label(
                egui::RichText::new(format!(
                    "configured, and the keystore could not be reached: {reason}. The \
                     journal is being written in the clear."
                ))
                .color(palette.class_hostile_color)
                .size(palette.small_font_size),
            );
        }
    }

    // GAP-023: whether line of sight is masked against terrain, and if not, why not.
    render_indicator(ui, palette, "Terrain masking", terrain.masking);
    ui.label(
        egui::RichText::new(terrain.detail)
            .color(if terrain.masking {
                palette.muted_text_color()
            } else {
                palette.warning_color
            })
            .size(palette.small_font_size),
    );

    render_point_cloud_registration(ui, palette, point_cloud_registration);
    render_sensors(ui, palette, sensors);
    render_feeds(ui, palette, feeds);
    render_cooperative_feeds(ui, palette, cooperative_feeds);
    render_bearing_feeds(ui, palette, bearing_feeds, bearing_pipeline);
    render_peers(ui, palette, view.peers);
    render_clocks(ui, palette, clocks);
    render_detectors(ui, palette, detectors);
}

/// The point-cloud registration backend (GAP-024): GPU, the CPU fallback and why, or
/// that no pair is configured at all. **Never silent about a fallback** -- an operator
/// who cannot tell the CPU reference from the GPU path apart would read a quietly
/// degraded registration as the primary path working, exactly what this workspace's
/// health flags exist to refuse (`gungnir_app::fusion::FusionBackend`'s own doc
/// comment states the same rule for the engine this line reports on).
fn render_point_cloud_registration(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    line: PointCloudRegistrationLine<'_>,
) {
    ui.separator();
    let (text, colour) = match line {
        PointCloudRegistrationLine::NotConfigured => (
            "Point-cloud registration: no source/target pair configured".to_owned(),
            palette.muted_text_color(),
        ),
        PointCloudRegistrationLine::Pending => (
            "Point-cloud registration: pair loaded, backend not yet resolved".to_owned(),
            palette.muted_text_color(),
        ),
        PointCloudRegistrationLine::Gpu => (
            "Point-cloud registration: GPU path".to_owned(),
            palette.healthy_color(),
        ),
        PointCloudRegistrationLine::CpuFallback { reason } => (
            format!("Point-cloud registration: CPU fallback ({reason})"),
            palette.warning_color,
        ),
    };
    ui.label(
        RichText::new(text)
            .color(colour)
            .size(palette.small_font_size),
    );
}

/// The AIS feeds (GAP-010): what the receiver heard and what became a placed report.
fn render_peers(ui: &mut egui::Ui, palette: &theme::Palette, peers: &[PeerLine<'_>]) {
    if peers.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Peer links").strong());
    for p in peers {
        let (text, colour) = if p.connected {
            (
                format!("{}: linked to {}", p.name, p.endpoint),
                palette.healthy_color(),
            )
        } else {
            (
                format!("{}: not linked to {} ({})", p.name, p.endpoint, p.reason),
                palette.warning_color,
            )
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(palette.small_font_size),
        );
    }
}

fn render_cooperative_feeds(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    feeds: &[CooperativeFeedLine<'_>],
) {
    if feeds.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("AIS feeds").strong());
    for f in feeds {
        let (text, colour) = if f.sentences == 0 {
            (
                format!("{}: bound, nothing received yet", f.name),
                palette.warning_color,
            )
        } else {
            let text = format!(
                "{}: {} sentences, {} positions placed, {} static reports, {} not decoded",
                f.name, f.sentences, f.positions, f.static_reports, f.not_decoded
            );
            let colour = if f.not_decoded > 0 {
                palette.warning_color
            } else {
                palette.healthy_color()
            };
            (text, colour)
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(palette.small_font_size),
        );
    }
}

/// The spotter/acoustic/passive-RF feeds (GAP-001, GAP-096): what each SAPIENT node
/// reported, and beside them, what the pipeline did with the bearings among them.
///
/// **This is the line that did not exist.** Before GAP-096, `sapient_stats` was written
/// in `gungnir-app`'s state every frame and read by nothing (docs/design/
/// DN-27-bearing-only-detections.md §7's status table), so a bound spotter feed and an
/// unbound one looked identical here: neither had a line.
fn render_bearing_feeds(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    feeds: &[BearingFeedLine<'_>],
    pipeline: BearingPipelineLine,
) {
    if feeds.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Spotter / acoustic / passive-RF feeds").strong());
    for f in feeds {
        let (text, colour) = if f.messages == 0 {
            (
                format!("{}: bound, nothing received yet", f.name),
                palette.warning_color,
            )
        } else {
            let text = format!(
                "{}: {} messages, {} bearings, {} ranged, {} positions, {} refused",
                f.name, f.messages, f.bearings, f.ranged, f.positions, f.refused
            );
            let colour = if f.refused > 0 {
                palette.warning_color
            } else {
                palette.healthy_color()
            };
            (text, colour)
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(palette.small_font_size),
        );
    }
    // The pipeline's own counters (GAP-096): pipeline-wide rather than any one feed's,
    // so drawn once beside the feeds rather than repeated on each of their lines.
    ui.label(
        RichText::new(format!(
            "Bearing pipeline: {} offered, {} updated a track, {} retained, {} expired, \
             {} refused",
            pipeline.offered,
            pipeline.updated,
            pipeline.retained,
            pipeline.expired,
            pipeline.refused
        ))
        .color(palette.muted_text_color())
        .size(palette.small_font_size),
    );
}

/// The radar feeds (GAP-001). A bound feed with nothing received is said in the
/// warning colour: it is the state a wrong multicast group or a quiet radar produces,
/// and it looks like health until somebody reads the number.
fn render_feeds(ui: &mut egui::Ui, palette: &theme::Palette, feeds: &[FeedLine<'_>]) {
    if feeds.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Radar feeds").strong());
    for f in feeds {
        let (text, colour) = if f.datagrams == 0 {
            (
                format!("{}: bound, nothing received yet", f.name),
                palette.warning_color,
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
                palette.warning_color
            } else {
                palette.healthy_color()
            };
            (text, colour)
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(palette.small_font_size),
        );
    }
}

/// Which anomaly detectors are running. **An unconfigured detector is listed as off**
/// rather than left out, so its absence is visible (DN-15 §6); one that is configured and
/// cannot evaluate in this build says why, rather than passing for running.
fn render_detectors(ui: &mut egui::Ui, palette: &theme::Palette, detectors: &[DetectorLine<'_>]) {
    ui.separator();
    ui.label(RichText::new("Anomaly detectors").strong());
    for d in detectors {
        let (text, colour) = match d.state {
            None => (format!("{}: off", d.name), palette.muted_text_color()),
            Some(None) => (format!("{}: running", d.name), palette.healthy_color()),
            Some(Some(reason)) => (
                format!("{}: configured, cannot run: {reason}", d.name),
                palette.class_hostile_color,
            ),
        };
        ui.label(
            RichText::new(text)
                .color(colour)
                .size(palette.small_font_size),
        );
    }
}

/// Clock skew across sources, judged against the late-data policy (GAP-008, MOP-09).
///
/// **Says how many sources the figure covers**, because "no skew" across nothing is the
/// statement this element used to make.
fn render_clocks(ui: &mut egui::Ui, palette: &theme::Palette, clocks: ClockSyncLine) {
    ui.separator();
    if clocks.sources_observed == 0 {
        ui.label(
            RichText::new("Clock sync: no source heard from yet")
                .color(palette.muted_text_color())
                .size(palette.small_font_size),
        );
        return;
    }
    render_indicator(ui, palette, "Clock sync", clocks.sources_out_of_sync == 0);
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
                palette.muted_text_color()
            } else {
                palette.class_hostile_color
            })
            .size(palette.small_font_size),
    );
}

/// The per-sensor rows.
///
/// Drawn after the three service flags because a sensor is a narrower question than
/// "is the pipeline running", and an operator scanning this panel reads the wide ones
/// first.
fn render_sensors(ui: &mut egui::Ui, palette: &theme::Palette, sensors: &[SensorHealthLine<'_>]) {
    ui.separator();
    if sensors.is_empty() {
        // Not a blank: no sensors configured is a deployment state, and it is different
        // from every sensor being silent.
        ui.label(
            RichText::new("No sensors are configured for this deployment.")
                .color(palette.muted_text_color())
                .size(palette.small_font_size),
        );
        return;
    }
    ui.label(RichText::new("Sensors").strong());
    for line in sensors {
        let (healthy, note, colour) = match line.presence {
            SensorPresence::Radiating => (true, None, palette.muted_text_color()),
            SensorPresence::InMaintenance { until, reason } => (
                true,
                Some(format!(
                    "planned maintenance until {}: {reason}",
                    crate::panels::status_strip::format_clock(until)
                )),
                palette.muted_text_color(),
            ),
            SensorPresence::Failed => (
                false,
                Some("off the air, with no maintenance window open".to_owned()),
                palette.class_hostile_color,
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
                palette.class_hostile_color,
            ),
        };
        render_indicator(
            ui,
            palette,
            &format!("Sensor {} ({})", line.id, line.modality),
            healthy,
        );
        if let Some(note) = note {
            ui.label(
                RichText::new(note)
                    .color(colour)
                    .size(palette.small_font_size),
            );
        }
    }
}

fn render_indicator(ui: &mut egui::Ui, palette: &theme::Palette, label: &str, healthy: bool) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(
                palette.status_dot_radius * 2.0,
                palette.status_dot_radius * 2.0,
            ),
            egui::Sense::hover(),
        );
        let color = if healthy {
            palette.healthy_color()
        } else {
            palette.degraded_color()
        };
        ui.painter()
            .circle_filled(rect.center(), palette.status_dot_radius, color);
        ui.label(label);
        ui.label(RichText::new(if healthy { "OK" } else { "DEGRADED" }).color(color));
    });
}
