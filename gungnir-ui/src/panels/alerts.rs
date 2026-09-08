// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Alert list -- backend fallbacks, journal failures, and the degraded-but-
//! recoverable conditions the services report. Newest first.

use crate::theme;
use egui::RichText;

/// One warning owed to an asset (DN-03 §7, GAP-042).
#[derive(Debug, Clone, PartialEq)]
pub struct WarningLine {
    pub asset: String,
    pub track: u64,
    pub channel: String,
    pub state: &'static str,
    pub detail: String,
    /// Seconds until it is due; negative once late.
    pub remaining_s: f64,
    /// Late or failed: sorts to the top and draws in the alert colour.
    pub loud: bool,
}

/// Warnings owed to assets, late and failed first. Draws nothing when there are none:
/// an empty warning list is the normal state, not a report.
pub fn render_warnings(ui: &mut egui::Ui, palette: &theme::Palette, warnings: &[WarningLine]) {
    if warnings.is_empty() {
        return;
    }
    ui.heading("Warnings owed");
    for w in warnings {
        let text = format!(
            "{} because of track {}: {} via {} ({:+.0} s){}",
            w.asset,
            w.track,
            w.state,
            w.channel,
            w.remaining_s,
            if w.detail.is_empty() {
                String::new()
            } else {
                format!(" -- {}", w.detail)
            }
        );
        ui.label(RichText::new(text).color(if w.loud {
            palette.alert_color
        } else {
            palette.warning_color
        }));
    }
    ui.separator();
}

pub fn render_alert_list(ui: &mut egui::Ui, palette: &theme::Palette, alerts: &[String]) {
    ui.heading("Alerts");
    if alerts.is_empty() {
        ui.label(RichText::new("No alerts").color(palette.muted_text_color()));
        return;
    }
    for alert in alerts.iter().rev() {
        ui.label(RichText::new(alert).color(palette.alert_color));
    }
}
