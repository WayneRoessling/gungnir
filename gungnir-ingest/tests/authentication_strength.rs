// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The provenance of an accepted detection says how strongly its source was
//! authenticated (GAP-002), stamped by the gateway from the authenticator that admitted
//! it: an allow-list admission and an admit-everything one are different claims.

use gungnir_ingest::{
    AllowAllAuthenticator, AllowListAuthenticator, IngestGateway, ProtocolAdapter,
};
use gungnir_model::events::IngestEvent;
use gungnir_model::{
    DetectionView, MissionTime, Provenance, SensorId, SourceAuthentication, TrackView,
};
use gungnir_tracking_service::{SubmitError, TrackingService};

struct OneDetection(bool);
impl ProtocolAdapter for OneDetection {
    fn name(&self) -> &'static str {
        "one"
    }
    fn poll(
        &mut self,
        now: MissionTime,
    ) -> Result<Vec<DetectionView>, gungnir_ingest::IngestError> {
        if self.0 {
            return Ok(Vec::new());
        }
        self.0 = true;
        Ok(vec![DetectionView {
            sensor: SensorId(1),
            source_time: now,
            receipt_time: now,
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::zeros(),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        }])
    }
}

struct Sink;
impl TrackingService for Sink {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &[]
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn accepted_with(auth: Box<dyn gungnir_ingest::SourceAuthenticator>) -> SourceAuthentication {
    let mut gateway = IngestGateway::new(auth);
    gateway.add_adapter(Box::new(OneDetection(false)));
    let events = gateway.tick(MissionTime(1.0), &mut Sink);
    match events.as_slice() {
        [IngestEvent::Accepted(d)] => d.provenance.authentication,
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_allow_list_admission_is_recorded_as_such_and_admit_all_as_nothing() {
    assert_eq!(
        accepted_with(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(1)]
        })),
        SourceAuthentication::AllowList
    );
    assert_eq!(
        accepted_with(Box::new(AllowAllAuthenticator)),
        SourceAuthentication::Unauthenticated
    );
}
