// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Real sensor & external-system integration, per docs/gungnir-capabilities.md
//! §5.2. Without this, a detection is only an internal test-fixture concept --
//! malformed, malicious, or out-of-spec input must be caught here, before it ever
//! reaches `gungnir_tracking_service::TrackingService::submit_detection`.
//!
//! The gateway is a trust boundary for external data and is human-owned
//! (agentic-workflow.md): agents may draft, never merge unsupervised.

pub mod adapters;
pub mod gateway;

use gungnir_model::events::IngestEvent;
use gungnir_model::MissionTime;
use gungnir_tracking_service::TrackingService;

pub use gungnir_model::{DetectionView, SensorId};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IngestError {
    #[error("schema validation failed: {0}")]
    SchemaInvalid(String),
    #[error("source {0:?} is not authenticated")]
    Unauthenticated(SensorId),
    #[error("message from {sensor:?} quarantined: {reason}")]
    Quarantined { sensor: SensorId, reason: String },
    #[error("adapter I/O failed: {0}")]
    Io(String),
    /// A detection a laydown rehearsal re-observed from a recording reached a gateway not
    /// built for one (docs/design/DN-32-re-observation-for-a-laydown.md §6, mechanism 2).
    /// Nothing is wrong with the sensor it names: the detection is synthetic, and a live
    /// picture never takes one.
    #[error(
        "detection from {sensor:?} was {origin}; a live gateway refuses a rehearsal's \
         detection (DN-32 §6)"
    )]
    RehearsalRefused { sensor: SensorId, origin: String },
}

/// One adapter per external protocol (radar-specific, EO/IR, ADS-B/AIS, lidar,
/// external C2, recorded/simulated). Each adapter's only job is producing
/// well-formed `DetectionView`s or reporting why it couldn't.
pub trait ProtocolAdapter: Send {
    fn name(&self) -> &str;
    /// Non-blocking: everything available since the last poll.
    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError>;
}

/// Decides whether a sensor is allowed to feed this system at all.
pub trait SourceAuthenticator: Send + Sync {
    fn authenticate(&self, sensor: SensorId) -> Result<(), IngestError>;

    /// How strong an admission by this authenticator is (GAP-002). The gateway stamps
    /// it on every accepted detection's provenance. Defaults to the weakest claim, so
    /// an authenticator that does not say is never read as stronger than it is.
    fn strength(&self) -> gungnir_model::SourceAuthentication {
        gungnir_model::SourceAuthentication::Unauthenticated
    }
}

/// Accepts every sensor. Development and replay only; never configure in a
/// deployed profile.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAllAuthenticator;

impl SourceAuthenticator for AllowAllAuthenticator {
    fn authenticate(&self, _sensor: SensorId) -> Result<(), IngestError> {
        Ok(())
    }
}

/// Accepts only sensors named in the applied `gungnir-config` baseline.
#[derive(Debug, Default, Clone)]
pub struct AllowListAuthenticator {
    pub allowed: Vec<SensorId>,
}

impl SourceAuthenticator for AllowListAuthenticator {
    fn authenticate(&self, sensor: SensorId) -> Result<(), IngestError> {
        if self.allowed.contains(&sensor) {
            Ok(())
        } else {
            Err(IngestError::Unauthenticated(sensor))
        }
    }

    fn strength(&self) -> gungnir_model::SourceAuthentication {
        gungnir_model::SourceAuthentication::AllowList
    }
}

/// Admits the sensors a client certificate may speak for (GAP-002, D-02).
///
/// Used by the node's machine-submission adapter alone. The transport has already
/// checked that the certificate presented speaks for the detection's sensor
/// (`machine_identities`), so what this authenticator adds is the claim the record
/// carries: `MachineIdentity`, which no other authenticator may stamp. A sensor outside
/// the vouched list is refused here even if the transport let it through, so a bug on
/// one side cannot promote a detection on the other.
#[derive(Debug, Default, Clone)]
pub struct MachineIdentityAuthenticator {
    pub vouched: Vec<SensorId>,
}

impl SourceAuthenticator for MachineIdentityAuthenticator {
    fn authenticate(&self, sensor: SensorId) -> Result<(), IngestError> {
        if self.vouched.contains(&sensor) {
            Ok(())
        } else {
            Err(IngestError::Unauthenticated(sensor))
        }
    }

    fn strength(&self) -> gungnir_model::SourceAuthentication {
        gungnir_model::SourceAuthentication::MachineIdentity
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IngestStats {
    pub accepted: u64,
    pub quarantined: u64,
    pub adapter_failures: u64,
    /// Valid detections the tracking pipeline refused (GAP-066).
    ///
    /// Counted apart from `quarantined`, which blames the sensor, and apart from
    /// `accepted`, which claims the detection is being tracked. **The gateway used to
    /// count these as accepted** because the submit call returned nothing.
    pub not_accepted: u64,
    /// Detections a laydown rehearsal re-observed that reached a gateway not built for
    /// one, and were refused (DN-32 §6, mechanism 2). **A named subset of
    /// `quarantined`**, as every refusal is counted there: this counter says which of
    /// them was a rehearsal's leaking rather than a sensor's fault, and any value above
    /// zero on a live desktop or node is a defect to chase, since containment's other
    /// four mechanisms exist so that it stays zero.
    pub rehearsal_refused: u64,
}

/// Runs every configured adapter's output through authentication, validation, and
/// quarantine before handing normalized detections to the Tracking Service -- the
/// pipeline described in docs/gungnir-capabilities.md §5.2.
/// An adapter and, when it brings its own, the authenticator that admits its detections.
struct Bound {
    adapter: Box<dyn ProtocolAdapter>,
    /// `None` means the gateway's own authenticator. An adapter whose sources are
    /// verified another way -- a client certificate the transport checked (GAP-002) --
    /// brings the authenticator that can say so, and the gateway still does the
    /// stamping: the adapter never writes the strength itself.
    authenticator: Option<Box<dyn SourceAuthenticator>>,
}

pub struct IngestGateway {
    adapters: Vec<Bound>,
    authenticator: Box<dyn SourceAuthenticator>,
    stats: IngestStats,
    last_receipt: Option<MissionTime>,
    last_tick_had_failure: bool,
    expected_adapters: usize,
    /// Whether a detection carrying `Provenance::rehearsal` may pass (DN-32 §6,
    /// mechanism 2). Set only by [`IngestGateway::for_rehearsal`]; never by `new`, and
    /// there is no setter, so a gateway built as a live one stays a live one.
    admits_rehearsal: bool,
}

impl IngestGateway {
    /// A live gateway: it refuses, and counts, every detection a laydown rehearsal
    /// re-observed (`IngestStats::rehearsal_refused`).
    pub fn new(authenticator: Box<dyn SourceAuthenticator>) -> Self {
        Self {
            adapters: Vec::new(),
            authenticator,
            stats: IngestStats::default(),
            last_receipt: None,
            last_tick_had_failure: false,
            expected_adapters: 0,
            admits_rehearsal: false,
        }
    }

    /// **A rehearsal's own gateway**, the one construction that admits a detection
    /// carrying `Provenance::rehearsal` (docs/design/DN-32-re-observation-for-a-laydown.md
    /// §6, mechanism 2). `gungnir-app`'s laydown rehearsal builds it for the throwaway
    /// desktop it runs on (mechanism 3); nothing that feeds a live picture may.
    /// Authentication and validation apply exactly as they do on a live gateway.
    pub fn for_rehearsal(authenticator: Box<dyn SourceAuthenticator>) -> Self {
        Self {
            admits_rehearsal: true,
            ..Self::new(authenticator)
        }
    }

    /// Whether this gateway was built by [`IngestGateway::for_rehearsal`].
    pub fn admits_rehearsal(&self) -> bool {
        self.admits_rehearsal
    }

    pub fn add_adapter(&mut self, adapter: Box<dyn ProtocolAdapter>) {
        self.adapters.push(Bound {
            adapter,
            authenticator: None,
        });
    }

    /// Register an adapter whose detections a different authenticator admits (GAP-002):
    /// the machine-identity adapter, whose sources the transport verified by client
    /// certificate. The gateway still validates and stamps; only who vouches differs.
    pub fn add_adapter_with_authenticator(
        &mut self,
        adapter: Box<dyn ProtocolAdapter>,
        authenticator: Box<dyn SourceAuthenticator>,
    ) {
        self.adapters.push(Bound {
            adapter,
            authenticator: Some(authenticator),
        });
    }

    /// How many adapters the configuration says should exist; health is false while
    /// fewer are registered.
    pub fn set_expected_adapters(&mut self, n: usize) {
        self.expected_adapters = n;
    }

    pub fn stats(&self) -> IngestStats {
        self.stats
    }

    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }

    /// Seconds since the last accepted detection, if any.
    pub fn seconds_since_last_receipt(&self, now: MissionTime) -> Option<f64> {
        self.last_receipt.map(|t| now.seconds_since(t))
    }

    /// True when every expected adapter is registered and the last tick had no
    /// adapter failure. Quarantined messages do not make the gateway unhealthy;
    /// they are the gateway doing its job.
    pub fn is_healthy(&self) -> bool {
        !self.adapters.is_empty()
            && self.adapters.len() >= self.expected_adapters
            && !self.last_tick_had_failure
    }

    /// Non-blocking: pulls from each adapter, authenticates, validates, forwards.
    /// Malformed input is quarantined and logged, never allowed to reach the
    /// tracking core. Returns the ingest events for the caller to publish.
    pub fn tick(&mut self, now: MissionTime, sink: &mut dyn TrackingService) -> Vec<IngestEvent> {
        let mut events = Vec::new();
        self.last_tick_had_failure = false;
        let default_authenticator = &self.authenticator;
        for bound in &mut self.adapters {
            let adapter = &mut bound.adapter;
            let authenticator: &dyn SourceAuthenticator = bound
                .authenticator
                .as_deref()
                .unwrap_or(default_authenticator.as_ref());
            let detections = match adapter.poll(now) {
                Ok(d) => d,
                Err(err) => {
                    self.stats.adapter_failures += 1;
                    self.last_tick_had_failure = true;
                    tracing::error!(adapter = adapter.name(), %err, "adapter poll failed");
                    continue;
                }
            };
            for mut detection in detections {
                // DN-32 §6, mechanism 2: a detection a laydown rehearsal re-observed from a
                // recording never reaches a live picture. Checked first, so no
                // authenticator, validator or sink on a live gateway ever handles one,
                // and counted as a quarantine like every other refusal, under a name of
                // its own.
                if !self.admits_rehearsal {
                    if let Some(origin) = &detection.provenance.rehearsal {
                        let err = IngestError::RehearsalRefused {
                            sensor: detection.sensor,
                            origin: origin.to_string(),
                        };
                        self.stats.quarantined += 1;
                        self.stats.rehearsal_refused += 1;
                        tracing::error!(
                            adapter = adapter.name(),
                            sensor = detection.sensor.0,
                            %err,
                            "a rehearsal's detection reached a live gateway"
                        );
                        events.push(IngestEvent::Quarantined {
                            sensor: detection.sensor,
                            reason: err.to_string(),
                        });
                        continue;
                    }
                }
                let checked = authenticator
                    .authenticate(detection.sensor)
                    .and_then(|()| gateway::validate_detection(&detection, now));
                // GAP-002: the record says how strongly the
                // source was authenticated, from the authenticator that admitted it, never
                // from the adapter.
                if checked.is_ok() {
                    detection.provenance.authentication = authenticator.strength();
                }
                match checked {
                    Ok(()) => match sink.submit_detection(detection.clone()) {
                        Ok(()) => {
                            self.stats.accepted += 1;
                            self.last_receipt = Some(now);
                            events.push(IngestEvent::Accepted(detection));
                        }
                        // Counted and published only after the sink took it. A detection
                        // the pipeline refused is not an accepted one, and the receipt
                        // clock is not touched: the watchdog measures how long it has been
                        // since a detection *reached the tracker*, which is the question it
                        // is actually asked.
                        Err(err) => {
                            self.stats.not_accepted += 1;
                            tracing::warn!(
                                sensor = detection.sensor.0,
                                %err,
                                "the tracking pipeline refused a valid detection"
                            );
                            events.push(IngestEvent::NotAccepted {
                                sensor: detection.sensor,
                                reason: err.to_string(),
                            });
                        }
                    },
                    Err(err) => {
                        self.stats.quarantined += 1;
                        tracing::warn!(adapter = adapter.name(), sensor = detection.sensor.0, %err, "detection quarantined");
                        events.push(IngestEvent::Quarantined {
                            sensor: detection.sensor,
                            reason: err.to_string(),
                        });
                    }
                }
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::adapters::simulated::SimulatedAdapter;
    use super::*;
    use gungnir_model::{Provenance, TrackView};

    /// Records what reaches the tracking service.
    #[derive(Default)]
    struct SinkSpy {
        received: Vec<DetectionView>,
    }

    impl TrackingService for SinkSpy {
        fn submit_detection(
            &mut self,
            detection: DetectionView,
        ) -> Result<(), gungnir_tracking_service::SubmitError> {
            self.received.push(detection);
            Ok(())
        }
        fn poll(&mut self, _now: MissionTime) {}
        fn tracks(&self) -> &[TrackView] {
            &[]
        }
        fn is_healthy(&self) -> bool {
            true
        }
    }

    fn detection(sensor: u32, x: f64) -> DetectionView {
        DetectionView {
            sensor: SensorId(sensor),
            source_time: MissionTime(9.0),
            receipt_time: MissionTime(10.0),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(x, 0.0, 0.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        }
    }

    #[test]
    fn valid_detections_pass_and_invalid_ones_are_quarantined() {
        let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(1)],
        }));
        let mut adapter = SimulatedAdapter::new("sim");
        adapter.push(detection(1, 5.0));
        adapter.push(detection(1, f64::NAN));
        adapter.push(detection(2, 5.0));
        gateway.add_adapter(Box::new(adapter));
        gateway.set_expected_adapters(1);
        let mut sink = SinkSpy::default();
        let events = gateway.tick(MissionTime(10.0), &mut sink);
        assert_eq!(sink.received.len(), 1);
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], IngestEvent::Accepted(_)));
        assert!(matches!(events[1], IngestEvent::Quarantined { .. }));
        assert!(matches!(
            events[2],
            IngestEvent::Quarantined {
                sensor: SensorId(2),
                ..
            }
        ));
        assert_eq!(
            gateway.stats(),
            IngestStats {
                accepted: 1,
                quarantined: 2,
                adapter_failures: 0,
                not_accepted: 0,
                rehearsal_refused: 0,
            }
        );
        assert!(gateway.is_healthy());
        assert_eq!(
            gateway.seconds_since_last_receipt(MissionTime(12.0)),
            Some(2.0)
        );
    }

    #[test]
    fn gateway_without_adapters_is_unhealthy() {
        let gateway = IngestGateway::new(Box::new(AllowAllAuthenticator));
        assert!(!gateway.is_healthy());
    }
}
