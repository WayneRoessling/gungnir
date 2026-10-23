// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A desktop's picture as a running deployment has it, for tests that decide on a plan
//! and are about something other than a degraded picture (GAP-195, D-132).
//!
//! A desktop under test starts degraded, truthfully: `AppState::health` reports every
//! service unhealthy until the first `update::tick` has heard from them, and the ingest
//! gateway is unhealthy with no adapter bound. Since D-132 a decision on a degraded picture
//! needs each condition acknowledged, so a test that decides on such a desktop was really
//! testing a degraded decision without saying so. [`healthy`] gives it the picture it
//! assumes instead: a feed bound and the services having reported.
//!
//! Included by `#[path]` from each test file that needs it; Cargo builds no test target
//! from a file in a subdirectory of `tests/`.

use gungnir_app::state::AppState;
use gungnir_ingest::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_model::MissionTime;

/// A feed that is bound and has nothing to report: a quiet sector, not a missing sensor.
struct QuietFeed;

impl ProtocolAdapter for QuietFeed {
    fn name(&self) -> &'static str {
        "quiet-feed"
    }

    fn poll(&mut self, _: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        Ok(Vec::new())
    }
}

/// Bind a quiet feed for each configured sensor -- the gateway expects one per sensor, and
/// at least one -- and let the services report once, so the picture is healthy unless
/// something in the test made it otherwise: every feed connected, nothing in the sky. A
/// decision then asks no degraded acknowledgement, because nothing is degraded.
pub fn healthy(state: &mut AppState) {
    for _ in 0..state.config.sensors.len().max(1) {
        state.ingest.add_adapter(Box::new(QuietFeed));
    }
    gungnir_app::update::tick(state);
}
