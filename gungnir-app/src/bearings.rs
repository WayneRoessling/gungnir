// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What reaches PN-08 for a retained bearing (GAP-096; docs/design/
//! DN-27-bearing-only-detections.md §5 rule 3, §7): "a bearing offered to a pipeline
//! holding no track" is not a contact, so it is drawn as a ray on PN-02 and never as a
//! symbol -- but the operator still has to be *told*, and a ray on a map nobody is
//! looking at is not a notification. DN-27 §7 names the alert list as this case's own
//! placement, beside PN-09's health line (`crate::sapient::bearing_feed_lines`).
//!
//! **One alert per bearing, not one per frame.** `tracking.bearing_rays()` is a live
//! snapshot the pipeline replaces wholesale on every poll (`gungnir-tracking-service`'s
//! `LiveTrackingService::poll`), so a bearing still inside its retention lifetime is
//! reported again on every one of the frames it survives. `AppState.alerts` is an
//! accumulating record of events, not a live view (`crate::update::tick`'s other
//! `state.alerts.push` call sites), so pushing on every frame would flood it with the
//! same bearing dozens of times before it expired. This module's whole job is telling
//! "still here" from "just arrived", the same distinction `state.skew_alerted` draws for
//! a sensor's clock skew.

use crate::state::AppState;
use gungnir_model::BearingRayView;

/// The alert line for one newly retained bearing.
///
/// **Names only what the system actually knows.** DN-27 §7's own example --
/// "a gunshot, bearing 037" -- is a classification an acoustic array might supply
/// alongside its direction; nothing in this workspace's `Measurement::Bearing` or
/// `BearingRayView` carries one; (`gungnir-model`'s `Measurement::Bearing` has no
/// classification field, and `docs/design/DN-27-bearing-only-detections.md` §4 does not
/// add one). Reporting a classification here would be inventing evidence the sensor
/// never gave, which is the same shortcut §2 forbids for position. What is reported is
/// the sensor, the azimuth, and that nothing has explained it.
fn alert_line(ray: &BearingRayView, now: gungnir_model::MissionTime) -> String {
    let degrees = ray.azimuth_rad.to_degrees().rem_euclid(360.0);
    format!(
        "sensor {} reports a bearing at {degrees:03.0} degrees with no track behind it \
         (retained {:.0} s unless it refines one or expires)",
        ray.sensor.0,
        ray.valid_until.seconds_since(now)
    )
}

/// Alert text for every ray in `current` that is not already in `previous`.
///
/// Structural equality is the identity check: a retained bearing's fields never change
/// after `FusionPipeline::offer_bearing` first retains it (`until_s` is fixed at that
/// moment and only ever removed, never revised), so the same bearing produces the same
/// [`BearingRayView`] on every snapshot until it updates a track or expires, and a
/// genuinely new one differs in at least its azimuth, its sensor, or its `valid_until`.
#[must_use]
pub fn alert_lines_for_new_rays(
    previous: &[BearingRayView],
    current: &[BearingRayView],
    now: gungnir_model::MissionTime,
) -> Vec<String> {
    current
        .iter()
        .filter(|ray| !previous.contains(ray))
        .map(|ray| alert_line(ray, now))
        .collect()
}

/// The tick step (GAP-096): alert once per bearing that newly appears in
/// `tracking.bearing_rays()`, then remember the set so the next tick can tell the
/// difference again.
pub fn tick(state: &mut AppState) {
    let now = state.clock.now();
    let current = state.tracking.bearing_rays();
    if current.is_empty() && state.last_bearing_rays.is_empty() {
        // The overwhelmingly common case, and the allocation below is worth skipping
        // for it: no acoustic, passive-RF or spotter feed has reported anything
        // unmatched, on this tick or the last one.
        return;
    }
    for line in alert_lines_for_new_rays(&state.last_bearing_rays, current, now) {
        state.alerts.push(line);
    }
    state.last_bearing_rays = current.to_vec();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{MissionTime, SensorId};

    fn ray(sensor: u32, azimuth_rad: f64, until_s: f64) -> BearingRayView {
        BearingRayView {
            sensor: SensorId(sensor),
            origin_enu: [0.0, 0.0, 0.0],
            azimuth_rad,
            elevation_rad: None,
            azimuth_one_sigma_rad: 0.02,
            valid_until: MissionTime(until_s),
        }
    }

    /// The line names the sensor and the bearing, in degrees, and nothing it was not
    /// actually told -- no invented classification (see `alert_line`'s own doc comment).
    #[test]
    fn the_alert_names_the_sensor_and_the_bearing_in_degrees() {
        let line = alert_line(&ray(4, std::f64::consts::PI / 2.0, 45.0), MissionTime(0.0));
        assert!(line.contains("sensor 4"), "{line}");
        // atan2 convention: pi/2 rad is due east, 090 degrees.
        assert!(line.contains("090"), "{line}");
        assert!(
            !line.to_lowercase().contains("gunshot"),
            "the alert must not invent a classification the sensor never gave: {line}"
        );
    }

    /// One alert per bearing that is actually new; a bearing still inside its lifetime
    /// on a later tick must not be repeated, or the list would fill with duplicates of
    /// the same unmatched bearing every frame it survives (`bearing_retention_s`
    /// defaults to 60 s, tens of frames at any real tick rate).
    #[test]
    fn only_a_newly_retained_bearing_produces_an_alert() {
        let now = MissionTime(0.0);
        let first_tick = alert_lines_for_new_rays(&[], &[ray(4, 0.6, 60.0)], now);
        assert_eq!(first_tick.len(), 1, "{first_tick:?}");

        // The same bearing, still retained on the next tick: no second alert.
        let second_tick = alert_lines_for_new_rays(&[ray(4, 0.6, 60.0)], &[ray(4, 0.6, 60.0)], now);
        assert!(
            second_tick.is_empty(),
            "an already-alerted bearing was reported again: {second_tick:?}"
        );

        // A second, genuinely different bearing alongside the first: one new alert.
        let third_tick = alert_lines_for_new_rays(
            &[ray(4, 0.6, 60.0)],
            &[ray(4, 0.6, 60.0), ray(7, 1.9, 90.0)],
            now,
        );
        assert_eq!(third_tick.len(), 1, "{third_tick:?}");
        assert!(third_tick[0].contains("sensor 7"), "{third_tick:?}");
    }

    /// A bearing that has left the view (expired, or refined a track) raises no alert
    /// of its own -- DN-27 §7 places the *arrival* of an unexplained bearing on the
    /// alert list, not its departure, which is not new information for an operator to
    /// act on.
    #[test]
    fn a_bearing_leaving_the_view_raises_no_alert() {
        let lines = alert_lines_for_new_rays(&[ray(4, 0.6, 60.0)], &[], MissionTime(60.0));
        assert!(lines.is_empty(), "{lines:?}");
    }
}
