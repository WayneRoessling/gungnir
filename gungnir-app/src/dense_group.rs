// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What reaches PN-08 when the tracker cannot resolve a scene (GAP-015).
//!
//! The pipeline's dense-group mode engages for a scan with more detections than exact
//! association attempts, and reports a count and a shape rather than tracks. PN-02 draws
//! it; this tells the operator, because a group on a map nobody is looking at is not a
//! notification, and because the tracks beside a group are **not the whole picture** --
//! that is the one thing about a raid an operator most needs to know and could not tell
//! from the symbols alone.
//!
//! **Once when a group appears and once when it goes**, not once per frame: the estimate
//! is replaced on every poll for as long as the mode runs, and `AppState.alerts` is a
//! record of events, the same distinction `crate::bearings` draws for a retained bearing.

use crate::state::AppState;
use gungnir_model::DenseGroupView;

/// The alert line for a group that has just appeared.
#[must_use]
pub fn engaged_line(group: &DenseGroupView) -> String {
    use std::fmt::Write as _;
    let mut line = format!(
        "the tracker cannot resolve a dense group: about {:.1} targets in a scan of {} \
         detections, drawn as a group and not as tracks ({})",
        group.expected_targets, group.scan_size, group.filter
    );
    if let Some(n) = group.most_probable_count {
        let _ = write!(line, "; most likely {n}");
    }
    line
}

/// The alert line for a group that has gone: the scene is resolvable again.
pub const RELEASED_LINE: &str =
    "the dense group is resolved: the tracks shown are the whole picture again";

/// The tick step: alert when the tracker's dense group appears and when it goes.
pub fn tick(state: &mut AppState) {
    match (state.dense_group_reported, state.tracking.dense_group()) {
        (None, Some(group)) => {
            let line = engaged_line(group);
            state.dense_group_reported = Some(group.epoch);
            state.alerts.push(line);
        }
        (Some(_), None) => {
            state.alerts.push(RELEASED_LINE.to_owned());
            state.dense_group_reported = None;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::MissionTime;

    fn group(most_probable: Option<u64>) -> DenseGroupView {
        DenseGroupView {
            epoch: MissionTime(12.0),
            expected_targets: 19.6,
            most_probable_count: most_probable,
            count_distribution: Vec::new(),
            components: Vec::new(),
            filter: "gm-phd".into(),
            scan_size: 21,
            is_stale: false,
        }
    }

    /// The line says the tracks are not the picture, gives the count as a mean, and gives
    /// a most likely count only where the filter has one.
    #[test]
    fn the_alert_says_not_tracks_and_a_mean_count() {
        let phd = engaged_line(&group(None));
        assert!(
            phd.contains("not as tracks") && phd.contains("19.6"),
            "{phd}"
        );
        assert!(
            !phd.contains("most likely"),
            "the PHD has no mode to report: {phd}"
        );
        let cphd = engaged_line(&group(Some(20)));
        assert!(cphd.contains("most likely 20"), "{cphd}");
    }
}
