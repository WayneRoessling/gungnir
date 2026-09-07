// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The live radar adapter: ASTERIX datagrams in, detections and service reports out
//! (GAP-001, the radar half).
//!
//! One datagram from a monoradar feed can carry several data blocks of more than one
//! category back to back, and a short frame arrives padded (`testdata/asterix/SOURCE.md`).
//! So the adapter splits every datagram with `gungnir_interop::asterix::data_blocks`
//! and routes each block by category: Category 048 target reports become
//! `DetectionView`s through the 048 codec and go to the gateway like any other
//! adapter's output; Category 034 service messages become `RadarServiceReport`s and
//! wait in this adapter's own queue, because the `ProtocolAdapter` boundary carries
//! detections only and a service message is not one
//! (`docs/design/external-standards.md` §1.7). The host drains that queue with
//! [`AsterixFeedAdapter::drain_service_reports`]; nothing else reads it yet.
//!
//! What this adapter refuses to hide: a datagram that does not frame, a block that
//! does not decode, a report from a radar the configuration does not name, and a block
//! of a category the build does not speak are each counted in [`AsterixFeedStats`] and
//! logged, never folded into the accepted count. A transport failure is an adapter
//! failure, which the gateway reports as unhealthy.
//!
//! The parser behind this adapter is gated on the `asterix_feed` fuzz target
//! (`docs/agentic-workflow.md`, "Parsers in `gungnir-ingest` adapters"); its corpus is
//! the public capture, and `tests/asterix_seeds.rs` checks the corpus still decodes.

use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_interop::asterix::{cat034, cat048, data_blocks, RadarSite};
use gungnir_interop::{AsterixCat034Codec, AsterixCat048Codec, InteropError, RadarServiceReport};
use gungnir_model::{Geodetic, LocalFrame, MissionTime, SensorId};
use std::collections::{BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};

/// Largest UDP payload; the receive buffer is this size so no datagram is truncated.
const MAX_DATAGRAM: usize = 65_535;
/// Datagrams one poll will take before yielding, so a flooding feed cannot hold the
/// gateway's tick for ever.
pub const MAX_DATAGRAMS_PER_POLL: usize = 4096;

/// A feed as a host describes it, so both binaries build adapters the same way
/// (GAP-001, handoff §4 step 2). Addresses are already validated by the baseline; a
/// bind that fails anyway (port in use, interface gone) is returned, not swallowed.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedSpec {
    pub name: String,
    pub bind_addr: SocketAddr,
    pub multicast: Option<(Ipv4Addr, Ipv4Addr)>,
    pub radars: Vec<RadarBinding>,
}

/// What a host keeps of a bound feed: the queue of service observations it drains and
/// the counters PN-09 shows (GAP-001). The gateway owns the adapter, so these are the
/// host's only view of it.
#[derive(Debug, Clone, Default)]
pub struct FeedSinks {
    pub observations: ServiceObservationSink,
    pub stats: FeedStatsSink,
}

/// The adapter's counters as of its last poll, for a host to read.
pub type FeedStatsSink = Arc<Mutex<AsterixFeedStats>>;

/// Bind one feed and build its adapter with the sinks the host keeps.
///
/// # Errors
///
/// `IngestError::Io` when the socket cannot be bound or the group joined.
pub fn bind_feed(
    spec: &FeedSpec,
    frame: &LocalFrame,
    sinks: &FeedSinks,
) -> Result<AsterixFeedAdapter<UdpDatagramSource>, IngestError> {
    let source = UdpDatagramSource::bind(spec.bind_addr, spec.multicast)?;
    Ok(
        AsterixFeedAdapter::new(spec.name.clone(), source, frame, &spec.radars)
            .with_observation_sink(sinks.observations.clone())
            .with_stats_sink(sinks.stats.clone()),
    )
}

/// One radar the feed is allowed to speak for: its ASTERIX identity, the sensor it is
/// in the registry, and where it stands. Comes from the applied configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadarBinding {
    pub sac: u8,
    pub sic: u8,
    pub sensor: SensorId,
    /// Antenna position; the adapter puts it in the local frame for the codecs.
    pub position: Geodetic,
}

/// Where datagrams come from. Non-blocking by contract: a poll must return with
/// whatever is waiting, never wait for more.
pub trait DatagramSource: Send {
    /// The next datagram into `buf`, `Ok(None)` when nothing is waiting, `Err` when
    /// the transport itself failed.
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, IngestError>;
    /// For the adapter's name and log lines.
    fn describe(&self) -> String;
}

/// A bound, non-blocking UDP socket, optionally joined to a multicast group.
#[derive(Debug)]
pub struct UdpDatagramSource {
    socket: UdpSocket,
    description: String,
}

impl UdpDatagramSource {
    /// Bind to `addr`. With `multicast = Some((group, interface))` the socket joins
    /// `group` on `interface` as well, which is how most radar feeds are delivered.
    pub fn bind(
        addr: SocketAddr,
        multicast: Option<(Ipv4Addr, Ipv4Addr)>,
    ) -> Result<Self, IngestError> {
        let io = |e: std::io::Error| IngestError::Io(format!("udp {addr}: {e}"));
        let socket = UdpSocket::bind(addr).map_err(io)?;
        socket.set_nonblocking(true).map_err(io)?;
        let mut description = format!("udp:{addr}");
        if let Some((group, interface)) = multicast {
            socket.join_multicast_v4(&group, &interface).map_err(io)?;
            let _ = write!(description, " multicast {group} via {interface}");
        }
        Ok(Self {
            socket,
            description,
        })
    }
}

impl DatagramSource for UdpDatagramSource {
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, IngestError> {
        match self.socket.recv_from(buf) {
            Ok((n, _peer)) => Ok(Some(n)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(IngestError::Io(format!("{}: {e}", self.description))),
        }
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// Datagrams queued in memory: recorded captures in tests, and the fuzz target.
#[derive(Debug, Default)]
pub struct ReplayDatagramSource {
    queue: VecDeque<Vec<u8>>,
}

impl ReplayDatagramSource {
    pub fn new(datagrams: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            queue: datagrams.into_iter().collect(),
        }
    }

    pub fn push(&mut self, datagram: Vec<u8>) {
        self.queue.push_back(datagram);
    }

    pub fn remaining(&self) -> usize {
        self.queue.len()
    }
}

impl DatagramSource for ReplayDatagramSource {
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, IngestError> {
        let Some(d) = self.queue.pop_front() else {
            return Ok(None);
        };
        if d.len() > buf.len() {
            return Err(IngestError::Io(format!(
                "replayed datagram of {} octets exceeds the {} octet buffer",
                d.len(),
                buf.len()
            )));
        }
        buf[..d.len()].copy_from_slice(&d);
        Ok(Some(d.len()))
    }

    fn describe(&self) -> String {
        "replay".into()
    }
}

/// What the feed has done since the adapter was built. Every input that did not
/// become a detection or a service report is in here under the reason it did not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AsterixFeedStats {
    pub datagrams: u64,
    pub blocks_cat048: u64,
    pub blocks_cat034: u64,
    pub detections: u64,
    pub service_reports: u64,
    /// Valid Category 048 records that are not observations (`TYP = 0`, no position).
    pub not_detections: u64,
    /// Datagrams whose block framing failed; every block in them is lost.
    pub malformed_datagrams: u64,
    /// Blocks that framed but did not decode or map for a reason other than the radar.
    pub malformed_blocks: u64,
    /// Records from a SAC/SIC no binding names. Not attributed, not guessed.
    pub unknown_radar: u64,
    /// Blocks of a category this build does not decode.
    pub unsupported_category_blocks: u64,
}

/// The adapter. Generic over the source so the same code runs a socket and a capture.
pub struct AsterixFeedAdapter<S: DatagramSource> {
    name: String,
    source: S,
    cat048: AsterixCat048Codec,
    cat034: AsterixCat034Codec,
    buf: Vec<u8>,
    service_reports: VecDeque<RadarServiceReport>,
    stats: AsterixFeedStats,
    /// SAC/SIC pairs already reported unknown, so a stray radar logs once, not per scan.
    unknown_seen: BTreeSet<(u8, u8)>,
    /// Where service observations go when a host wants them (GAP-064): the gateway
    /// owns the adapter, so the host keeps the other end of this and drains it.
    observation_sink: Option<ServiceObservationSink>,
    /// Where the counters go at the end of every poll (GAP-001), for PN-09.
    stats_sink: Option<FeedStatsSink>,
}

/// What a host learns from a radar's service messages, in this crate's words so a host
/// need not depend on `gungnir-interop` to read it (the same inversion as the
/// detections).
#[derive(Debug, Clone, PartialEq)]
pub struct RadarServiceObservation {
    pub sensor: SensorId,
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
    pub kind: ServiceObservationKind,
    /// I034/041, when the message carried it.
    pub rotation_period_s: Option<f64>,
    /// The system status, when the message carried it.
    pub released_for_operational_use: Option<bool>,
    pub overloaded: Option<bool>,
    pub time_source_invalid: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceObservationKind {
    /// The antenna crossed north: one per revolution.
    NorthMarker,
    /// The antenna entered a sector.
    SectorCrossing,
    /// Something else edition 1.29 defines, or does not.
    Other,
}

/// The queue a host drains.
pub type ServiceObservationSink = Arc<Mutex<VecDeque<RadarServiceObservation>>>;

impl From<&RadarServiceReport> for RadarServiceObservation {
    fn from(r: &RadarServiceReport) -> Self {
        Self {
            sensor: r.sensor,
            source_time: r.source_time,
            receipt_time: r.receipt_time,
            kind: match r.event {
                gungnir_interop::ServiceEvent::NorthMarker => ServiceObservationKind::NorthMarker,
                gungnir_interop::ServiceEvent::SectorCrossing(_) => {
                    ServiceObservationKind::SectorCrossing
                }
                _ => ServiceObservationKind::Other,
            },
            rotation_period_s: r.rotation_period_s,
            released_for_operational_use: r.status.as_ref().map(|s| s.released_for_operational_use),
            overloaded: r.status.as_ref().map(|s| s.overloaded),
            time_source_invalid: r.status.as_ref().map(|s| s.time_source_invalid),
        }
    }
}

impl<S: DatagramSource> AsterixFeedAdapter<S> {
    /// `frame` is the deployment's local frame (a deployment without one has no
    /// adapter, the same rule as everything else that needs ENU); `radars` are the
    /// bindings from the configuration. An empty binding list is allowed and means
    /// every report is counted as unknown, which is the honest state of a feed nobody
    /// has configured.
    pub fn new(
        name: impl Into<String>,
        source: S,
        frame: &LocalFrame,
        radars: &[RadarBinding],
    ) -> Self {
        let sites: Vec<RadarSite> = radars
            .iter()
            .map(|r| RadarSite {
                sac: r.sac,
                sic: r.sic,
                sensor: r.sensor,
                origin_enu_m: frame.to_enu(r.position),
            })
            .collect();
        Self {
            name: format!("asterix:{}:{}", name.into(), source.describe()),
            source,
            cat048: AsterixCat048Codec::new(sites.clone()),
            cat034: AsterixCat034Codec::new(sites),
            buf: vec![0; MAX_DATAGRAM],
            service_reports: VecDeque::new(),
            stats: AsterixFeedStats::default(),
            unknown_seen: BTreeSet::new(),
            observation_sink: None,
            stats_sink: None,
        }
    }

    /// Hand service observations to a host through `sink` at the end of every poll
    /// (GAP-064). Without one they queue on the adapter for `drain_service_reports`.
    #[must_use]
    pub fn with_observation_sink(mut self, sink: ServiceObservationSink) -> Self {
        self.observation_sink = Some(sink);
        self
    }

    pub fn stats(&self) -> AsterixFeedStats {
        self.stats
    }

    /// Publish the counters to `sink` at the end of every poll (GAP-001).
    #[must_use]
    pub fn with_stats_sink(mut self, sink: FeedStatsSink) -> Self {
        self.stats_sink = Some(sink);
        self
    }

    /// The service messages received since the last drain, in arrival order.
    pub fn drain_service_reports(&mut self) -> Vec<RadarServiceReport> {
        self.service_reports.drain(..).collect()
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    /// Decode one datagram received at `receipt_time`. Public so the fuzz target and
    /// the seed test reach the parser without a source.
    pub fn handle_datagram(
        &mut self,
        datagram: &[u8],
        receipt_time: MissionTime,
    ) -> Vec<DetectionView> {
        self.stats.datagrams += 1;
        let blocks = match data_blocks("asterix.feed", datagram) {
            Ok(b) => b,
            Err(err) => {
                self.stats.malformed_datagrams += 1;
                tracing::warn!(adapter = %self.name, %err, "datagram does not frame; dropped whole");
                return Vec::new();
            }
        };
        let mut detections = Vec::new();
        for block in blocks {
            match block.category {
                48 => {
                    self.stats.blocks_cat048 += 1;
                    match cat048::decode_block(&block) {
                        Ok(records) => {
                            for record in &records {
                                match self.cat048.map(record, receipt_time) {
                                    Ok(cat048::Mapped::Detection(d)) => {
                                        self.stats.detections += 1;
                                        detections.push(d);
                                    }
                                    Ok(cat048::Mapped::NotADetection(_)) => {
                                        self.stats.not_detections += 1;
                                    }
                                    Err(err) => self.count_map_error(&err),
                                }
                            }
                        }
                        Err(err) => self.count_decode_error(&err),
                    }
                }
                34 => {
                    self.stats.blocks_cat034 += 1;
                    match cat034::decode_block(&block) {
                        Ok(records) => {
                            for record in &records {
                                match self.cat034.map(record, receipt_time) {
                                    Ok(report) => {
                                        self.stats.service_reports += 1;
                                        self.service_reports.push_back(report);
                                    }
                                    Err(err) => self.count_map_error(&err),
                                }
                            }
                        }
                        Err(err) => self.count_decode_error(&err),
                    }
                }
                other => {
                    self.stats.unsupported_category_blocks += 1;
                    tracing::debug!(adapter = %self.name, category = other, "block of a category this build does not decode");
                }
            }
        }
        detections
    }

    fn count_decode_error(&mut self, err: &InteropError) {
        self.stats.malformed_blocks += 1;
        tracing::warn!(adapter = %self.name, %err, "block does not decode; dropped");
    }

    fn count_map_error(&mut self, err: &InteropError) {
        match err {
            InteropError::UnknownRadar { sac, sic, .. } => {
                self.stats.unknown_radar += 1;
                if self.unknown_seen.insert((*sac, *sic)) {
                    tracing::warn!(adapter = %self.name, sac, sic, "reports from a radar no binding names; not attributed");
                }
            }
            other => {
                self.stats.malformed_blocks += 1;
                tracing::warn!(adapter = %self.name, err = %other, "record does not map; dropped");
            }
        }
    }
}

impl<S: DatagramSource> ProtocolAdapter for AsterixFeedAdapter<S> {
    fn name(&self) -> &str {
        &self.name
    }

    /// Everything waiting on the source, up to [`MAX_DATAGRAMS_PER_POLL`], received at
    /// `now`. The receipt time is the poll time: the transport does not stamp
    /// datagrams, and the gateway's clock is the one the rest of the system uses.
    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        let mut out = Vec::new();
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            let mut buf = std::mem::take(&mut self.buf);
            let received = self.source.recv(&mut buf);
            let n = match received {
                Ok(Some(n)) => n,
                Ok(None) => {
                    self.buf = buf;
                    break;
                }
                Err(e) => {
                    self.buf = buf;
                    return Err(e);
                }
            };
            let detections = self.handle_datagram(&buf[..n], now);
            self.buf = buf;
            out.extend(detections);
        }
        if let Some(sink) = &self.observation_sink {
            if let Ok(mut queue) = sink.lock() {
                queue.extend(
                    self.service_reports
                        .drain(..)
                        .map(|r| RadarServiceObservation::from(&r)),
                );
            }
        }
        if let Some(sink) = &self.stats_sink {
            if let Ok(mut published) = sink.lock() {
                *published = self.stats;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_interop::ServiceEvent;

    fn frame() -> LocalFrame {
        LocalFrame::new(Geodetic {
            lat_rad: 0.9,
            lon_rad: 0.2,
            alt_m: 0.0,
        })
    }

    fn binding() -> RadarBinding {
        RadarBinding {
            sac: 1,
            sic: 2,
            sensor: SensorId(7),
            position: Geodetic {
                lat_rad: 0.9,
                lon_rad: 0.2,
                alt_m: 50.0,
            },
        }
    }

    /// A 048 plot (SSR, 1 NM at 90°, FL 10) and an 034 north marker in one datagram.
    fn mixed_datagram() -> Vec<u8> {
        let mut d = vec![
            0x30, 0x00, 0x10, 0xF4, 1, 2, 0x54, 0x60, 0x00, 0x40, 0x01, 0x00, 0x40, 0x00, 0x00,
            0x28,
        ];
        d.extend_from_slice(&[0x22, 0x00, 0x0A, 0xE0, 1, 2, 0x01, 0x54, 0x60, 0x00]);
        d
    }

    #[test]
    fn routes_each_block_by_category() {
        let source = ReplayDatagramSource::new(vec![mixed_datagram()]);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[binding()]);
        let detections = adapter.poll(MissionTime(43_205.0)).expect("polls");
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(7));
        // The antenna is 50 m above the frame origin, and the codec anchored it there.
        let enu = detections[0]
            .measurement
            .position_enu()
            .expect("a radar plot is a position");
        assert!((enu[2] - 304.8).abs() < 1e-9);
        let reports = adapter.drain_service_reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].event, ServiceEvent::NorthMarker);
        assert!(adapter.drain_service_reports().is_empty());
        let s = adapter.stats();
        assert_eq!((s.datagrams, s.blocks_cat048, s.blocks_cat034), (1, 1, 1));
        assert_eq!((s.detections, s.service_reports), (1, 1));
        assert_eq!(
            s.malformed_datagrams + s.malformed_blocks + s.unknown_radar,
            0
        );
    }

    #[test]
    fn unknown_radar_is_counted_not_attributed() {
        let source = ReplayDatagramSource::new(vec![mixed_datagram()]);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[]);
        let detections = adapter.poll(MissionTime(0.0)).expect("polls");
        assert!(detections.is_empty());
        assert!(adapter.drain_service_reports().is_empty());
        assert_eq!(adapter.stats().unknown_radar, 2);
    }

    #[test]
    fn malformed_and_unsupported_input_is_counted_and_the_poll_goes_on() {
        let mut d = mixed_datagram();
        d[2] = 0xFF; // first block claims more octets than the datagram has
        let source = ReplayDatagramSource::new(vec![
            d,
            vec![0x3E, 0x00, 0x04, 0x00], // category 62, framed, not decoded here
            mixed_datagram(),
        ]);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[binding()]);
        let detections = adapter.poll(MissionTime(43_205.0)).expect("polls");
        assert_eq!(
            detections.len(),
            1,
            "the good datagram after the bad ones still counts"
        );
        let s = adapter.stats();
        assert_eq!(s.datagrams, 3);
        assert_eq!(s.malformed_datagrams, 1);
        assert_eq!(s.unsupported_category_blocks, 1);
    }

    #[test]
    fn transport_failure_is_an_adapter_failure() {
        struct Broken;
        impl DatagramSource for Broken {
            fn recv(&mut self, _buf: &mut [u8]) -> Result<Option<usize>, IngestError> {
                Err(IngestError::Io("cable".into()))
            }
            fn describe(&self) -> String {
                "broken".into()
            }
        }
        let mut adapter = AsterixFeedAdapter::new("test", Broken, &frame(), &[binding()]);
        assert!(matches!(
            adapter.poll(MissionTime(0.0)),
            Err(IngestError::Io(_))
        ));
    }

    #[test]
    fn a_poll_is_bounded() {
        let many = std::iter::repeat_n(mixed_datagram(), MAX_DATAGRAMS_PER_POLL + 5);
        let source = ReplayDatagramSource::new(many);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[binding()]);
        let first = adapter.poll(MissionTime(43_205.0)).expect("polls");
        assert_eq!(first.len(), MAX_DATAGRAMS_PER_POLL);
        assert_eq!(adapter.source().remaining(), 5);
        let second = adapter.poll(MissionTime(43_206.0)).expect("polls");
        assert_eq!(second.len(), 5);
    }

    #[test]
    fn udp_source_binds_and_reports_nothing_waiting() {
        let Ok(source) = UdpDatagramSource::bind("127.0.0.1:0".parse().expect("addr"), None) else {
            return; // no loopback in this environment; nothing to test
        };
        let mut adapter = AsterixFeedAdapter::new("loop", source, &frame(), &[binding()]);
        assert!(adapter.poll(MissionTime(0.0)).expect("polls").is_empty());
        assert!(adapter.name().starts_with("asterix:loop:udp:127.0.0.1:"));
    }
}
