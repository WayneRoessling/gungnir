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
//!
//! **The Category 205 arm and its [`DfBinding`] (GAP-100): human-owned (the
//! `gungnir-ingest` gateway; signatures: docs/signatures.md)**; its review confirmed
//! the codec's bearing scale and angular reference against the primary text's own item
//! definitions (`gungnir_interop::asterix::cat205`'s module documentation records the
//! one discrepancy inside that text) and added the angular range refusals those
//! definitions state.
//!
//! **The Category 129 arm, its [`UasBinding`] and `uas_detection` (GAP-101): human-owned
//! (the `gungnir-ingest` gateway)**; its review checked every item's scale, width and
//! sign against the primary text's own item pages and reversed the codec's reading of
//! the one item that text contradicts itself on (`gungnir_interop::asterix::cat129`'s
//! module documentation: I129/120, one octet).
//!
//! **A UAS's height above mean sea level, placed on the ellipsoid (GAP-196, D-123,
//! D-124): human-owned (the `gungnir-ingest` gateway; see docs/signatures.md).**
//! I129/090 is the only absolute height a Category 129 record carries, and it is above
//! mean sea level, not the WGS-84 ellipsoid every other height on the picture is
//! measured from (`gungnir_interop::asterix::cat129`'s module documentation). Where the
//! host lent a geoid model through [`FeedSinks::geoid`] (`crate::geoid`), the adapter
//! adds its separation at the report's position before the report is placed, so the
//! detection, the report and every consumer of either see one ellipsoidal height. Where
//! it did not -- no model on this binary, the grid missing or refused, the position off
//! the grid -- the height stays as sent and the report's `UasAltitudeReference` says it
//! is mean sea level and why; the detection carries that in its provenance, its vertical
//! variance is widened by the square of the largest separation EGM2008 has anywhere
//! (`gungnir_model::EGM2008_MAX_ABS_SEPARATION_M`), and [`AsterixFeedStats`] counts it.
//!
//! **A UAS's height above ground, placed on a DEM (GAP-199, D-130): human-owned (the
//! `gungnir-ingest` gateway).** A record with I129/100 and no I129/090 states its height
//! only above the ground under it. Where the host lent a ground elevation model through
//! [`FeedSinks::terrain`] (`crate::terrain`) and it covers the report's position, the
//! adapter places the UAS at the ground's ellipsoidal height plus I129/100, and the
//! detection adds the DEM's own vertical error to its up variance. Where it did not -- no
//! DEM on this binary (the node's case), none loaded, the position off it or in a hole --
//! or the record carries no I129/100 either, the height stays 0, the report says it is
//! unknown and why, the up variance is widened by the square of
//! `gungnir_model::UNKNOWN_UAS_HEIGHT_BOUND_M`, and [`AsterixFeedStats`] counts it.

use crate::geoid::GeoidHandle;
use crate::terrain::TerrainHandle;
use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_interop::asterix::{cat034, cat048, cat129, cat205, data_blocks, RadarSite};
use gungnir_interop::{
    AsterixCat034Codec, AsterixCat048Codec, AsterixCat129Codec, AsterixCat205Codec, DfSite,
    InteropError, RadarServiceReport, UasSite,
};
use gungnir_model::{
    Geodetic, LocalFrame, MissionTime, SensorId, UasAltitudeReference, UasIdentificationReport,
    EGM2008_MAX_ABS_SEPARATION_M, UNKNOWN_UAS_HEIGHT_BOUND_M,
};
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
/// Per-axis variance stamped on a Category 129 report's position when it also becomes a
/// `DetectionView` (GAP-101). The same restated-not-imported reasoning as
/// `gungnir_ingest::adapters::ais`'s and `::adsb`'s own copy of this constant: this is
/// the tracking baseline's own default measurement noise
/// (`gungnir_fusion_async::PipelineSettings::measurement_noise_var`), which is the only
/// stated accuracy this workspace has for a cooperatively-reported position. Category
/// 129's own I129/110 GNSS accuracy is carried on `UasIdentificationReport` instead of
/// folded in here, because none of AIS, ADS-B or MISB folds a per-message accuracy
/// indicator into this variance either, and doing it only for this category would be a
/// new, unreviewed pattern rather than a consistent one.
const BASELINE_POSITION_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// The per-axis variance a Category 129 detection carries: the baseline, with the up
/// axis widened by what the height does not know. A height still above mean sea level
/// adds the square of the largest geoid separation EGM2008 has anywhere (GAP-196, D-124);
/// a height placed on a DEM adds the square of the DEM's own vertical error, and a height
/// nobody could place adds the square of [`UNKNOWN_UAS_HEIGHT_BOUND_M`] (GAP-199, D-130).
/// A geoid-corrected height keeps the baseline.
#[must_use]
pub fn uas_position_variance(reference: &UasAltitudeReference) -> [f64; 3] {
    let mut variance = BASELINE_POSITION_VARIANCE_M2;
    variance[2] += match reference {
        UasAltitudeReference::GeoidCorrected { .. } => 0.0,
        UasAltitudeReference::MeanSeaLevelUncorrected { .. } => {
            EGM2008_MAX_ABS_SEPARATION_M * EGM2008_MAX_ABS_SEPARATION_M
        }
        UasAltitudeReference::TerrainPlaced { ground_sigma_m, .. } => {
            ground_sigma_m * ground_sigma_m
        }
        UasAltitudeReference::NoAbsoluteHeight { .. } => {
            UNKNOWN_UAS_HEIGHT_BOUND_M * UNKNOWN_UAS_HEIGHT_BOUND_M
        }
    };
    variance
}

/// A feed as a host describes it, so both binaries build adapters the same way
/// (GAP-001, handoff §4 step 2). Addresses are already validated by the baseline; a
/// bind that fails anyway (port in use, interface gone) is returned, not swallowed.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedSpec {
    pub name: String,
    pub bind_addr: SocketAddr,
    pub multicast: Option<(Ipv4Addr, Ipv4Addr)>,
    pub radars: Vec<RadarBinding>,
    /// Direction finders this feed's Category 205 blocks may be attributed to
    /// (GAP-100), bound by [`bind_feed`] the same way `radars` already is. Empty means
    /// no Category 205 report on this feed is attributed to anything, the same
    /// honest-empty state an empty `radars` gives Category 048 and 034.
    pub df_sites: Vec<DfBinding>,
    /// UAS Identification and Target Report gateways this feed's Category 129 blocks may
    /// be attributed to (GAP-101), bound by [`bind_feed`] the same way `df_sites`
    /// already is. Empty means no Category 129 report on this feed is attributed to
    /// anything, the same honest-empty state an empty `df_sites` gives Category 205.
    pub uas_sites: Vec<UasBinding>,
}

/// What a host keeps of a bound feed: the queue of service observations it drains and
/// the counters PN-09 shows (GAP-001). The gateway owns the adapter, so these are the
/// host's only view of it.
#[derive(Debug, Clone, Default)]
pub struct FeedSinks {
    pub observations: ServiceObservationSink,
    pub stats: FeedStatsSink,
    /// The geoid model this host lends the feed, or why it lends none (GAP-196). The
    /// default is unavailable, so a host that sets nothing gets every Category 129
    /// height flagged as mean sea level and counted, never passed off as ellipsoidal.
    pub geoid: GeoidHandle,
    /// The ground elevation model this host lends the feed, or why it lends none
    /// (GAP-199). The default is unavailable, so a host that sets nothing gets every
    /// Category 129 height above ground carried as unknown and counted, never placed on
    /// an invented ground.
    pub terrain: TerrainHandle,
}

/// The adapter's counters as of its last poll, for a host to read.
pub type FeedStatsSink = Arc<Mutex<AsterixFeedStats>>;

/// Bind one feed and build its adapter with the sinks the host keeps. `spec.df_sites`
/// (GAP-100) is applied through [`AsterixFeedAdapter::with_df_sites`] and
/// `spec.uas_sites` (GAP-101) through [`AsterixFeedAdapter::with_uas_sites`], the same
/// way `spec.radars` is applied through [`AsterixFeedAdapter::new`]; an empty list is a
/// no-op in both cases, so a feed with no direction finder and no UAS gateway
/// configured builds exactly the adapter it always did.
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
            .with_df_sites(&spec.df_sites, frame)
            .with_uas_sites(&spec.uas_sites)
            .with_geoid(sinks.geoid.clone())
            .with_terrain(sinks.terrain.clone())
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

/// One direction finder a feed is allowed to speak for: its ASTERIX identity, the
/// sensor it is in the registry, where it stands, and the angular accuracy its own
/// Interface Control Document states (GAP-100; `gungnir_interop::asterix::cat205`'s
/// module documentation explains why the wire format itself carries none). Part of
/// [`FeedSpec`] (`df_sites`) and applied by [`bind_feed`] through
/// [`AsterixFeedAdapter::with_df_sites`], the same host-configuration wiring GAP-001's
/// Category 034 half also went without at first and was given later the same week; a
/// `ConfigBaseline` section (`RadarFeedConfig::df_sites`, in `gungnir-config`) names
/// direction finders the way `RadarFeedConfig::radars` already names radars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DfBinding {
    pub sac: u8,
    pub sic: u8,
    pub sensor: SensorId,
    /// Antenna position; the adapter puts it in the local frame for the codec, the
    /// same as [`RadarBinding::position`]. `cat205::DfSite` does not use it in today's
    /// mapping (`Measurement::Bearing` has no position field), but it is required here
    /// for the same reason a radar's is: a deployment states where its sensor stands
    /// rather than this adapter guessing.
    pub position: Geodetic,
    /// The direction finder's stated one-sigma bearing accuracy, radians, from its own
    /// Interface Control Document. **Never invented**: see `cat205::DfSite`'s own
    /// documentation.
    pub azimuth_sigma_rad: f64,
}

/// One UAS Identification and Target Report gateway a feed is allowed to speak for: its
/// ASTERIX identity and the sensor it is in the registry (GAP-101). Unlike
/// [`RadarBinding`] and [`DfBinding`], no position: `cat129::UasSite` carries none, for
/// the reason its own documentation gives (this category reports the UAS's own absolute
/// position, not a range or bearing that would need a receiver origin to resolve). Part
/// of [`FeedSpec`] (`uas_sites`) and applied by [`bind_feed`] through
/// [`AsterixFeedAdapter::with_uas_sites`], the same host-configuration wiring GAP-100's
/// Category 205 half also went without at first and was given later the same day; a
/// `ConfigBaseline` section (`RadarFeedConfig::uas_sites`, in `gungnir-config`) names
/// UAS gateways the way `RadarFeedConfig::radars` already names radars.
///
/// I129/010's own recommendation is `sac = 0, sic = 0` for an airborne-to-ground
/// broadcast (`cat129`'s module documentation), so the common case is one binding at
/// `(0, 0)` naming this deployment's one receiving gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UasBinding {
    pub sac: u8,
    pub sic: u8,
    pub sensor: SensorId,
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
    /// GAP-100: Category 205 (Radio Direction Finder Reports) blocks seen.
    pub blocks_cat205: u64,
    /// GAP-101: Category 129 (UAS Identification and Target Reports) blocks seen.
    pub blocks_cat129: u64,
    pub detections: u64,
    pub service_reports: u64,
    /// GAP-101: Category 129 records mapped to a `UasIdentificationReport`. Every
    /// mapped one also becomes a `DetectionView` counted under `detections`, so this is
    /// a subset of it, kept apart because nothing else on this adapter says how many of
    /// the detections came from a cooperative identity report rather than a radar plot
    /// or a bearing.
    pub uas_reports: u64,
    /// GAP-196: Category 129 heights above mean sea level placed on the WGS-84
    /// ellipsoid by adding the lent geoid model's separation (D-123).
    pub uas_heights_geoid_corrected: u64,
    /// GAP-196: Category 129 heights above mean sea level that **no** geoid model
    /// corrected, carried as mean sea level and flagged (D-124). Each is a report
    /// whose height is off the ellipsoid by the local separation; PN-09 shows the count.
    pub uas_heights_msl_uncorrected: u64,
    /// GAP-199: Category 129 heights above ground level placed on the WGS-84 ellipsoid
    /// by adding them to the lent DEM's ground height (D-130).
    pub uas_heights_terrain_placed: u64,
    /// GAP-199: Category 129 records with no I129/090 whose height **nobody** placed --
    /// no I129/100, no DEM lent, or the position off it -- carried as unknown at 0 and
    /// flagged (D-130). PN-09 shows the count.
    pub uas_heights_unknown: u64,
    /// Valid Category 048 or 205 records that are not observations (048's `TYP = 0`;
    /// 205's position message types and detection-end reports -- see
    /// `cat205::AsterixCat205Codec::map`).
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
    /// The deployment's local frame (GAP-101): unlike a radar's or a direction finder's
    /// fixed antenna, which is placed in ENU once at construction (`Self::new`,
    /// `Self::with_df_sites`), a Category 129 report carries the *target's* own position
    /// on every message, so the frame origin is needed again at every poll rather than
    /// only once.
    frame: LocalFrame,
    cat048: AsterixCat048Codec,
    cat034: AsterixCat034Codec,
    /// GAP-100. Empty (the `Default` codec) until [`Self::with_df_sites`] is called;
    /// an empty codec decodes Category 205 blocks losslessly and attributes none, the
    /// same honest-empty state `radars` gives `cat048`/`cat034` when nothing is bound.
    cat205: AsterixCat205Codec,
    /// GAP-101. Empty until [`Self::with_uas_sites`] is called, the same honest-empty
    /// convention as `cat205` above.
    cat129: AsterixCat129Codec,
    /// GAP-196: the geoid model the host lends, read on every Category 129 report so a
    /// model lent after binding is used from the next report on.
    geoid: GeoidHandle,
    /// GAP-199: the ground elevation model the host lends, read the same way.
    terrain: TerrainHandle,
    buf: Vec<u8>,
    service_reports: VecDeque<RadarServiceReport>,
    /// UAS identification reports mapped since the last drain (GAP-101), the same
    /// queue-on-the-adapter shape `service_reports` already has.
    uas_reports: VecDeque<UasIdentificationReport>,
    stats: AsterixFeedStats,
    /// SAC/SIC pairs already reported unknown, so a stray radar logs once, not per scan.
    unknown_seen: BTreeSet<(u8, u8)>,
    /// Where service observations go when a host wants them (GAP-064): the gateway
    /// owns the adapter, so the host keeps the other end of this and drains it.
    observation_sink: Option<ServiceObservationSink>,
    /// Where UAS identification reports go when a host wants them (GAP-101), the same
    /// shape as `observation_sink`.
    uas_report_sink: Option<UasIdentificationSink>,
    /// Where the counters go at the end of every poll (GAP-001), for PN-09.
    stats_sink: Option<FeedStatsSink>,
}

/// The queue a host drains for UAS identification reports (GAP-101), the same shape
/// `ServiceObservationSink` already has for service messages.
pub type UasIdentificationSink = Arc<Mutex<VecDeque<UasIdentificationReport>>>;

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
            frame: *frame,
            cat048: AsterixCat048Codec::new(sites.clone()),
            cat034: AsterixCat034Codec::new(sites),
            cat205: AsterixCat205Codec::default(),
            cat129: AsterixCat129Codec::default(),
            geoid: GeoidHandle::default(),
            terrain: TerrainHandle::default(),
            buf: vec![0; MAX_DATAGRAM],
            service_reports: VecDeque::new(),
            uas_reports: VecDeque::new(),
            stats: AsterixFeedStats::default(),
            unknown_seen: BTreeSet::new(),
            observation_sink: None,
            uas_report_sink: None,
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

    /// Configure the direction finders this feed's Category 205 blocks may be
    /// attributed to (GAP-100). `frame` is the same local frame `new` took; it is
    /// re-supplied rather than stored because nothing else on this adapter keeps one.
    /// A binding list with no entries (the default before this is ever called) means
    /// every Category 205 report is counted `unknown_radar` -- the same honest-empty
    /// state an unconfigured `radars` list gives Category 048 and 034.
    #[must_use]
    pub fn with_df_sites(mut self, df_sites: &[DfBinding], frame: &LocalFrame) -> Self {
        let sites: Vec<DfSite> = df_sites
            .iter()
            .map(|b| DfSite {
                sac: b.sac,
                sic: b.sic,
                sensor: b.sensor,
                origin_enu_m: frame.to_enu(b.position),
                azimuth_sigma_rad: b.azimuth_sigma_rad,
            })
            .collect();
        self.cat205 = AsterixCat205Codec::new(sites);
        self
    }

    /// Configure the UAS Identification and Target Report gateways this feed's
    /// Category 129 blocks may be attributed to (GAP-101). Unlike
    /// [`Self::with_df_sites`], this takes no `frame`: [`UasBinding`] carries no
    /// position for [`cat129::UasSite`] to place in ENU (module documentation on
    /// [`UasBinding`]), and the frame this adapter already stored (`Self::new`) is what
    /// converts each *report's own* position at [`Self::handle_datagram`] time instead.
    /// A binding list with no entries (the default before this is ever called) means
    /// every Category 129 report is counted `unknown_radar` -- the same honest-empty
    /// state an unconfigured `radars` list gives Category 048 and 034.
    #[must_use]
    pub fn with_uas_sites(mut self, uas_sites: &[UasBinding]) -> Self {
        let sites: Vec<UasSite> = uas_sites
            .iter()
            .map(|b| UasSite {
                sac: b.sac,
                sic: b.sic,
                sensor: b.sensor,
            })
            .collect();
        self.cat129 = AsterixCat129Codec::new(sites);
        self
    }

    /// The geoid model a Category 129 report's height above mean sea level is corrected
    /// with (GAP-196, D-123): a handle the host keeps and may set after binding. Without
    /// this the adapter holds an unavailable one, and every such height is carried as
    /// mean sea level, flagged and counted (D-124).
    #[must_use]
    pub fn with_geoid(mut self, geoid: GeoidHandle) -> Self {
        self.geoid = geoid;
        self
    }

    /// The ground elevation model a Category 129 report's height above ground is placed
    /// with (GAP-199, D-130): a handle the host keeps and may set after binding. Without
    /// this the adapter holds an unavailable one, and every such height is carried as
    /// unknown, flagged and counted.
    #[must_use]
    pub fn with_terrain(mut self, terrain: TerrainHandle) -> Self {
        self.terrain = terrain;
        self
    }

    /// Hand UAS identification reports to a host through `sink` at the end of every
    /// poll (GAP-101). Without one they queue on the adapter for
    /// [`Self::drain_uas_reports`], the same shape [`Self::with_observation_sink`] has
    /// for service messages.
    #[must_use]
    pub fn with_uas_report_sink(mut self, sink: UasIdentificationSink) -> Self {
        self.uas_report_sink = Some(sink);
        self
    }

    /// The service messages received since the last drain, in arrival order.
    pub fn drain_service_reports(&mut self) -> Vec<RadarServiceReport> {
        self.service_reports.drain(..).collect()
    }

    /// The UAS identification reports received since the last drain, in arrival order.
    pub fn drain_uas_reports(&mut self) -> Vec<UasIdentificationReport> {
        self.uas_reports.drain(..).collect()
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
                205 => {
                    self.stats.blocks_cat205 += 1;
                    match cat205::decode_block(&block) {
                        Ok(record) => match self.cat205.map(&record, receipt_time) {
                            Ok(cat205::Mapped::Detection(d)) => {
                                self.stats.detections += 1;
                                detections.push(d);
                            }
                            Ok(cat205::Mapped::NotADetection(_)) => {
                                self.stats.not_detections += 1;
                            }
                            Err(err) => self.count_map_error(&err),
                        },
                        Err(err) => self.count_decode_error(&err),
                    }
                }
                129 => {
                    self.stats.blocks_cat129 += 1;
                    match cat129::decode_block(&block) {
                        Ok(record) => match self.cat129.map(&record, receipt_time) {
                            Ok(mut report) => {
                                self.place_uas_height(&mut report);
                                self.stats.uas_reports += 1;
                                self.stats.detections += 1;
                                detections.push(self.uas_detection(&report));
                                self.uas_reports.push_back(report);
                            }
                            Err(err) => self.count_map_error(&err),
                        },
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

    /// Put a report's height above mean sea level on the WGS-84 ellipsoid, or say why
    /// it stays where it is (GAP-196; module documentation). Runs before the report is
    /// placed or queued, so the detection and the report agree.
    ///
    /// A height the codec left as mean sea level is corrected with the geoid; a record
    /// with no I129/090 is placed on the ground with I129/100, or carried as unknown
    /// ([`Self::place_above_ground`]). One already placed is not placed twice.
    fn place_uas_height(&mut self, report: &mut UasIdentificationReport) {
        if matches!(
            report.altitude_reference,
            UasAltitudeReference::NoAbsoluteHeight { .. }
        ) {
            self.place_above_ground(report);
            return;
        }
        let (Some(amsl), UasAltitudeReference::MeanSeaLevelUncorrected { .. }) =
            (report.altitude_amsl_m, &report.altitude_reference)
        else {
            return;
        };
        let lat_deg = report.position.lat_rad.to_degrees();
        let lon_deg = report.position.lon_rad.to_degrees();
        let corrected = self.geoid.current().and_then(|model| {
            model
                .separation_m(lat_deg, lon_deg)
                .map(|n| (model.model().to_owned(), n))
                .map_err(|e| format!("{} gives no separation here: {e}", model.model()))
        });
        match corrected {
            Ok((model, separation_m)) if separation_m.is_finite() => {
                report.position.alt_m = amsl + separation_m;
                report.altitude_reference = UasAltitudeReference::GeoidCorrected {
                    model,
                    separation_m,
                };
                self.stats.uas_heights_geoid_corrected += 1;
            }
            Ok((model, separation_m)) => {
                self.keep_msl(
                    report,
                    amsl,
                    format!("{model} returned a separation of {separation_m}"),
                );
            }
            Err(reason) => self.keep_msl(report, amsl, reason),
        }
    }

    /// The height stays I129/090 as sent, flagged with `reason` and counted (D-124).
    fn keep_msl(&mut self, report: &mut UasIdentificationReport, amsl: f64, reason: String) {
        report.position.alt_m = amsl;
        report.altitude_reference = UasAltitudeReference::MeanSeaLevelUncorrected { reason };
        self.stats.uas_heights_msl_uncorrected += 1;
    }

    /// Place a record with no I129/090 on the ground under it with I129/100, or say why
    /// its height is unknown (GAP-199, D-130; module documentation).
    ///
    /// A ground height and its error must both be finite, and the error positive: a model
    /// that answers otherwise is reported by name and the height is unknown, never placed
    /// on a ground nobody measured.
    fn place_above_ground(&mut self, report: &mut UasIdentificationReport) {
        let Some(agl) = report.altitude_agl_m.filter(|h| h.is_finite()) else {
            let reason = match report.altitude_agl_m {
                Some(h) => format!("I129/100 decoded as {h}"),
                None => gungnir_interop::asterix::cat129::NO_HEIGHT_IN_RECORD.to_owned(),
            };
            self.keep_unknown(report, reason);
            return;
        };
        let lat_deg = report.position.lat_rad.to_degrees();
        let lon_deg = report.position.lon_rad.to_degrees();
        let placed = self.terrain.current().and_then(|model| {
            model
                .ground(lat_deg, lon_deg)
                .map(|ground| (model.name().to_owned(), ground))
                .map_err(|e| format!("{} gives no ground height here: {e}", model.name()))
        });
        match placed {
            Ok((terrain, ground))
                if ground.ellipsoidal_m.is_finite()
                    && ground.sigma_m.is_finite()
                    && ground.sigma_m > 0.0 =>
            {
                report.position.alt_m = ground.ellipsoidal_m + agl;
                report.altitude_reference = UasAltitudeReference::TerrainPlaced {
                    terrain,
                    ground_m: ground.ellipsoidal_m,
                    ground_sigma_m: ground.sigma_m,
                };
                self.stats.uas_heights_terrain_placed += 1;
            }
            Ok((terrain, ground)) => self.keep_unknown(
                report,
                format!(
                    "{terrain} returned a ground height of {} m with an error of {} m",
                    ground.ellipsoidal_m, ground.sigma_m
                ),
            ),
            Err(reason) => self.keep_unknown(report, reason),
        }
    }

    /// The height stays 0, flagged as unknown with `reason` and counted (D-130).
    fn keep_unknown(&mut self, report: &mut UasIdentificationReport, reason: String) {
        report.position.alt_m = 0.0;
        report.altitude_reference = UasAltitudeReference::NoAbsoluteHeight { reason };
        self.stats.uas_heights_unknown += 1;
    }

    /// A UAS identification report's own claimed position (`Geodetic`, module
    /// documentation on `gungnir_model::UasIdentificationReport` explains why it stays
    /// geodetic through `gungnir-interop`) placed in this deployment's ENU frame and
    /// stamped with the same baseline variance AIS, ADS-B and MISB ST 0601 already use
    /// for a cooperatively-reported position (GAP-101).
    ///
    /// **GAP-196 (D-124).** A height still above mean sea level is off the ellipsoid
    /// by a separation nobody knows here, anywhere up to
    /// [`EGM2008_MAX_ABS_SEPARATION_M`]; the up variance is widened by that bound's
    /// square so the tracker weighs the bias instead of trusting the height as if it
    /// were ellipsoidal. **GAP-199 (D-130):** a height placed on a DEM adds the DEM's
    /// error, and one nobody placed says it is unknown ([`uas_position_variance`]).
    /// The report's every loss, the height's included, goes on the
    /// detection's provenance, so the picture carries the flag and not only the report.
    fn uas_detection(&self, report: &UasIdentificationReport) -> DetectionView {
        let enu = self.frame.to_enu(report.position);
        DetectionView {
            sensor: report.sensor,
            source_time: report.source_time,
            receipt_time: report.receipt_time,
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(enu[0], enu[1], enu[2]),
                variance_m2: uas_position_variance(&report.altitude_reference),
            },
            provenance: gungnir_model::Provenance {
                source_sensor_ids: vec![report.sensor.0],
                calibration_baseline_version: None,
                algorithm_version: format!("{}/ed{}", cat129::CODEC_NAME, cat129::EDITION),
                conversion_loss: Some(report.losses()),
                ..gungnir_model::Provenance::default()
            },
        }
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
        if let Some(sink) = &self.uas_report_sink {
            if let Ok(mut queue) = sink.lock() {
                queue.extend(self.uas_reports.drain(..));
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

    /// One Category 205 Sensor Data Report: FSPEC flags FRN 1 (I205/010), 3 (I205/000)
    /// and 9 (I205/070) -- `0xA1, 0x40`. SAC 50 SIC 6, message type 5, local bearing
    /// 90.00 deg (9000 x 0.01 = `0x2328`).
    fn cat205_datagram() -> Vec<u8> {
        vec![0xCD, 0x00, 0x0A, 0xA1, 0x40, 50, 6, 0x05, 0x23, 0x28]
    }

    fn df_binding() -> DfBinding {
        DfBinding {
            sac: 50,
            sic: 6,
            sensor: SensorId(21),
            position: Geodetic {
                lat_rad: 0.9,
                lon_rad: 0.2,
                alt_m: 30.0,
            },
            azimuth_sigma_rad: 1.5_f64.to_radians(),
        }
    }

    #[test]
    fn routes_category_205_blocks_when_df_sites_are_configured() {
        let source = ReplayDatagramSource::new(vec![cat205_datagram()]);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[])
            .with_df_sites(&[df_binding()], &frame());
        let detections = adapter.poll(MissionTime(43_205.0)).expect("polls");
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(21));
        match detections[0].measurement {
            gungnir_model::Measurement::Bearing { azimuth_rad, .. } => {
                assert!((azimuth_rad - 90.0_f64.to_radians()).abs() < 1e-6);
            }
            ref other => panic!("expected a bearing, got {other:?}"),
        }
        assert!(detections[0].measurement.is_finite());
        let s = adapter.stats();
        assert_eq!(s.blocks_cat205, 1);
        assert_eq!(s.detections, 1);
        assert_eq!(s.unknown_radar, 0);
    }

    #[test]
    fn category_205_reports_from_an_unconfigured_site_are_counted_unknown() {
        let source = ReplayDatagramSource::new(vec![cat205_datagram()]);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[]);
        let detections = adapter.poll(MissionTime(0.0)).expect("polls");
        assert!(detections.is_empty());
        assert_eq!(adapter.stats().blocks_cat205, 1);
        assert_eq!(adapter.stats().unknown_radar, 1);
    }

    /// GAP-100 host wiring: [`bind_feed`] -- the one function both `gungnir-app` and
    /// `gungnir-node` call to build a live ASTERIX feed adapter -- forwards
    /// `FeedSpec::df_sites` into [`AsterixFeedAdapter::with_df_sites`], exactly as it
    /// already forwards `radars` into [`AsterixFeedAdapter::new`]. Binds a real
    /// loopback socket on an OS-assigned port through the real `bind_feed` (proving the
    /// construction path a deployment's start-up takes actually runs), then drives the
    /// resulting adapter with `handle_datagram` directly rather than sending a UDP
    /// packet to a port this test cannot otherwise discover -- `handle_datagram` never
    /// touches the socket (it is "public so the fuzz target and the seed test reach the
    /// parser without a source", per its own doc comment), so this is a real exercise
    /// of the codec and site lookup `with_df_sites` configured, not a shortcut around
    /// them.
    #[test]
    fn bind_feed_wires_a_configured_direction_finder_into_the_live_adapter() {
        let spec = FeedSpec {
            name: "test".into(),
            bind_addr: "127.0.0.1:0".parse().expect("loopback, any port"),
            multicast: None,
            radars: Vec::new(),
            df_sites: vec![df_binding()],
            uas_sites: Vec::new(),
        };
        let mut adapter = bind_feed(&spec, &frame(), &FeedSinks::default())
            .expect("a loopback socket always binds");
        let detections = adapter.handle_datagram(&cat205_datagram(), MissionTime(43_205.0));
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(21));
        match detections[0].measurement {
            gungnir_model::Measurement::Bearing {
                azimuth_rad,
                azimuth_variance_rad2,
                ..
            } => {
                assert!((azimuth_rad - 90.0_f64.to_radians()).abs() < 1e-6);
                let sigma = 1.5_f64.to_radians();
                assert!((azimuth_variance_rad2 - sigma * sigma).abs() < 1e-12);
            }
            ref other => panic!("expected a bearing, got {other:?}"),
        }
        assert_eq!(adapter.stats().blocks_cat205, 1);
        assert_eq!(adapter.stats().unknown_radar, 0);
    }

    /// A minimal Category 129 record carrying only the four mandatory items: FSPEC
    /// flags FRN 1 (I129/010), 6 (I129/050), 7 (I129/070) and 8 (I129/080) --
    /// `0x87, 0x80`. SAC 10 SIC 20, country "DE", time 12:00:00, position 0N 0E.
    fn cat129_datagram() -> Vec<u8> {
        vec![
            0x81, 0x00, 0x14, 0x87, 0x80, 10, 20, b'D', b'E', 0x54, 0x60, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00,
        ]
    }

    fn uas_binding() -> UasBinding {
        UasBinding {
            sac: 10,
            sic: 20,
            sensor: SensorId(61),
        }
    }

    #[test]
    fn routes_category_129_blocks_when_uas_sites_are_configured() {
        let source = ReplayDatagramSource::new(vec![cat129_datagram()]);
        let mut adapter =
            AsterixFeedAdapter::new("test", source, &frame(), &[]).with_uas_sites(&[uas_binding()]);
        let detections = adapter.poll(MissionTime(43_205.0)).expect("polls");
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(61));
        assert!(matches!(
            detections[0].measurement,
            gungnir_model::Measurement::Position { .. }
        ));
        let reports = adapter.drain_uas_reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].sensor, SensorId(61));
        assert_eq!(reports[0].registration_country, "DE");
        assert!(adapter.drain_uas_reports().is_empty());
        let s = adapter.stats();
        assert_eq!(s.blocks_cat129, 1);
        assert_eq!(s.uas_reports, 1);
        assert_eq!(s.detections, 1);
        assert_eq!(s.unknown_radar, 0);
    }

    /// GAP-101 host wiring: [`bind_feed`] -- the one function both `gungnir-app` and
    /// `gungnir-node` call to build a live ASTERIX feed adapter -- forwards
    /// `FeedSpec::uas_sites` into [`AsterixFeedAdapter::with_uas_sites`], exactly as it
    /// already forwards `df_sites` into [`AsterixFeedAdapter::with_df_sites`]. The same
    /// construction this test's Category 205 twin
    /// (`bind_feed_wires_a_configured_direction_finder_into_the_live_adapter`) makes,
    /// and for the same reasons: a real loopback socket on an OS-assigned port through
    /// the real `bind_feed`, then `handle_datagram` directly rather than a UDP packet to
    /// a port this test cannot otherwise discover -- `handle_datagram` never touches the
    /// socket, so the codec and the gateway lookup `with_uas_sites` configured are both
    /// really exercised.
    #[test]
    fn bind_feed_wires_a_configured_uas_gateway_into_the_live_adapter() {
        let spec = FeedSpec {
            name: "test".into(),
            bind_addr: "127.0.0.1:0".parse().expect("loopback, any port"),
            multicast: None,
            radars: Vec::new(),
            df_sites: Vec::new(),
            uas_sites: vec![uas_binding()],
        };
        let mut adapter = bind_feed(&spec, &frame(), &FeedSinks::default())
            .expect("a loopback socket always binds");
        let detections = adapter.handle_datagram(&cat129_datagram(), MissionTime(43_205.0));
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(61));
        assert!(matches!(
            detections[0].measurement,
            gungnir_model::Measurement::Position { .. }
        ));
        let reports = adapter.drain_uas_reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].sensor, SensorId(61));
        assert_eq!(reports[0].registration_country, "DE");
        assert_eq!(adapter.stats().blocks_cat129, 1);
        assert_eq!(adapter.stats().uas_reports, 1);
        assert_eq!(adapter.stats().unknown_radar, 0);
    }

    #[test]
    fn category_129_reports_from_an_unconfigured_site_are_counted_unknown() {
        let source = ReplayDatagramSource::new(vec![cat129_datagram()]);
        let mut adapter = AsterixFeedAdapter::new("test", source, &frame(), &[]);
        let detections = adapter.poll(MissionTime(0.0)).expect("polls");
        assert!(detections.is_empty());
        assert!(adapter.drain_uas_reports().is_empty());
        assert_eq!(adapter.stats().blocks_cat129, 1);
        assert_eq!(adapter.stats().unknown_radar, 1);
    }

    /// GAP-196: a Category 129 record over the frame origin (0.9 rad N, 0.2 rad E, the
    /// nearest I129/080 counts, `0x1255_AB96` and `0x0413_09B0`) carrying I129/090 =
    /// 500.0 m above mean sea level (`0x00_1388`). FSPEC flags FRN 1, 6, 7 and FRN 8, 9:
    /// `0x87, 0xC0`. LEN 23.
    fn cat129_msl_datagram() -> Vec<u8> {
        vec![
            0x81, 0x00, 0x17, 0x87, 0xC0, 10, 20, b'D', b'E', 0x54, 0x60, 0x00, 0x12, 0x55, 0xAB,
            0x96, 0x04, 0x13, 0x09, 0xB0, 0x00, 0x13, 0x88,
        ]
    }

    /// A geoid model with one separation everywhere, or one that has none; stands in
    /// for the desktop's EGM2008 grid, which this crate cannot reach (the grid itself is
    /// tested through PROJ in `gungnir-data/tests/geoid.rs` and end to end in
    /// `gungnir-app/tests/uas_identification.rs`).
    struct FixedSeparation(Result<f64, &'static str>);
    impl crate::geoid::GeoidSeparation for FixedSeparation {
        fn model(&self) -> &'static str {
            "EGM2008"
        }
        fn separation_m(&self, _: f64, _: f64) -> Result<f64, String> {
            self.0.map_err(str::to_owned)
        }
    }

    fn uas_feed(sinks: &FeedSinks) -> AsterixFeedAdapter<UdpDatagramSource> {
        let spec = FeedSpec {
            name: "uas".into(),
            bind_addr: "127.0.0.1:0".parse().expect("loopback, any port"),
            multicast: None,
            radars: Vec::new(),
            df_sites: Vec::new(),
            uas_sites: vec![uas_binding()],
        };
        bind_feed(&spec, &frame(), sinks).expect("a loopback socket always binds")
    }

    fn up_and_variance(d: &DetectionView) -> (f64, [f64; 3]) {
        match &d.measurement {
            gungnir_model::Measurement::Position { enu, variance_m2 } => (enu[2], *variance_m2),
            other => panic!("a position, not {other:?}"),
        }
    }

    /// **The no-grid path (D-124).** A host that lends no geoid model gets the height as
    /// sent, flagged as mean sea level with the reason, counted, carried on the
    /// detection's provenance, and weighed with the up variance widened by the square of
    /// EGM2008's largest separation -- never passed off as an ellipsoidal height.
    #[test]
    fn a_height_above_msl_with_no_geoid_is_flagged_counted_and_widened() {
        let mut adapter = uas_feed(&FeedSinks::default());
        let detections = adapter.handle_datagram(&cat129_msl_datagram(), MissionTime(43_205.0));
        let report = adapter.drain_uas_reports().pop().expect("one report");
        assert_eq!(
            report.altitude_reference,
            UasAltitudeReference::MeanSeaLevelUncorrected {
                reason: crate::geoid::NO_GEOID_ATTACHED.to_owned()
            }
        );
        assert!((report.position.alt_m - 500.0).abs() < 1e-9, "as sent");
        let s = adapter.stats();
        assert_eq!(
            (s.uas_heights_msl_uncorrected, s.uas_heights_geoid_corrected),
            (1, 0)
        );
        let (up, variance) = up_and_variance(&detections[0]);
        // Over the frame origin, so up is the height as sent less the origin's 0 m.
        assert!((up - 500.0).abs() < 1e-3, "{up}");
        let widened = 900.0 + EGM2008_MAX_ABS_SEPARATION_M * EGM2008_MAX_ABS_SEPARATION_M;
        assert!((variance[2] - widened).abs() < 1e-9, "{variance:?}");
        assert!((variance[0] - 400.0).abs() < 1e-12 && (variance[1] - 400.0).abs() < 1e-12);
        let loss = detections[0]
            .provenance
            .conversion_loss
            .as_deref()
            .expect("the flag reaches the picture");
        assert!(loss.contains("NOT corrected"), "{loss}");
        assert!(loss.contains(crate::geoid::NO_GEOID_ATTACHED), "{loss}");
    }

    /// **The correction (D-123)**, with the model lent through the host's handle *after*
    /// the feed was bound, as the desktop's start-up grid check does: the report and the
    /// detection both carry I129/090 + N as a WGS-84 ellipsoidal height, the variance is
    /// the baseline's, and the count moves.
    #[test]
    // The baseline is returned untouched, so equality is the property.
    #[allow(clippy::float_cmp)]
    fn a_model_lent_after_binding_puts_the_height_on_the_ellipsoid() {
        let sinks = FeedSinks::default();
        let mut adapter = uas_feed(&sinks);
        sinks
            .geoid
            .set_available(std::sync::Arc::new(FixedSeparation(Ok(34.92))));
        let detections = adapter.handle_datagram(&cat129_msl_datagram(), MissionTime(43_205.0));
        let report = adapter.drain_uas_reports().pop().expect("one report");
        assert_eq!(
            report.altitude_reference,
            UasAltitudeReference::GeoidCorrected {
                model: "EGM2008".into(),
                separation_m: 34.92
            }
        );
        assert!((report.position.alt_m - 534.92).abs() < 1e-9);
        assert!((report.altitude_amsl_m.expect("kept as sent") - 500.0).abs() < 1e-9);
        let (up, variance) = up_and_variance(&detections[0]);
        assert!((up - 534.92).abs() < 1e-3, "{up}");
        assert_eq!(variance, BASELINE_POSITION_VARIANCE_M2);
        let s = adapter.stats();
        assert_eq!(
            (s.uas_heights_msl_uncorrected, s.uas_heights_geoid_corrected),
            (0, 1)
        );
        let loss = detections[0]
            .provenance
            .conversion_loss
            .as_deref()
            .expect("stated");
        assert!(loss.contains("EGM2008 geoid separation, 34.92 m"), "{loss}");
    }

    /// A model with no separation at the report's position -- off a clipped grid, or a
    /// grid PROJ can no longer read -- is a flagged mean-sea-level height with that
    /// reason, never a zero separation.
    #[test]
    fn a_model_with_no_separation_here_leaves_the_height_flagged_with_its_reason() {
        let sinks = FeedSinks::default();
        sinks
            .geoid
            .set_available(std::sync::Arc::new(FixedSeparation(Err("off the grid"))));
        let mut adapter = uas_feed(&sinks);
        adapter.handle_datagram(&cat129_msl_datagram(), MissionTime(43_205.0));
        let report = adapter.drain_uas_reports().pop().expect("one report");
        let UasAltitudeReference::MeanSeaLevelUncorrected { reason } = &report.altitude_reference
        else {
            panic!("{:?}", report.altitude_reference);
        };
        assert!(
            reason.contains("EGM2008 gives no separation here: off the grid"),
            "{reason}"
        );
        assert!((report.position.alt_m - 500.0).abs() < 1e-9);
        assert_eq!(adapter.stats().uas_heights_msl_uncorrected, 1);
    }

    /// A record with neither I129/090 nor I129/100 has no height to correct or place,
    /// even with a geoid and a DEM lent: it is unknown, counted as such, and the detection
    /// says so in its loss and its up variance (GAP-199, D-130).
    #[test]
    fn a_record_with_no_height_at_all_is_unknown_whatever_is_lent() {
        let sinks = FeedSinks::default();
        sinks
            .geoid
            .set_available(std::sync::Arc::new(FixedSeparation(Ok(34.92))));
        sinks
            .terrain
            .set_available(std::sync::Arc::new(FixedGround(Ok((120.0, 4.0)))));
        let mut adapter = uas_feed(&sinks);
        let detections = adapter.handle_datagram(&cat129_datagram(), MissionTime(43_205.0));
        let report = adapter.drain_uas_reports().pop().expect("one report");
        assert_eq!(
            report.altitude_reference,
            UasAltitudeReference::NoAbsoluteHeight {
                reason: cat129::NO_HEIGHT_IN_RECORD.to_owned()
            }
        );
        let s = adapter.stats();
        assert_eq!(
            (
                s.uas_heights_msl_uncorrected,
                s.uas_heights_geoid_corrected,
                s.uas_heights_terrain_placed,
                s.uas_heights_unknown
            ),
            (0, 0, 0, 1)
        );
        let (_, variance) = up_and_variance(&detections[0]);
        let unknown = 900.0 + UNKNOWN_UAS_HEIGHT_BOUND_M * UNKNOWN_UAS_HEIGHT_BOUND_M;
        assert!((variance[2] - unknown).abs() < 1e-6, "{variance:?}");
        assert!(detections[0]
            .provenance
            .conversion_loss
            .as_deref()
            .is_some_and(|l| l.contains("set to 0")));
    }

    /// GAP-199: the same record as [`cat129_msl_datagram`] with I129/100 = 120.0 m above
    /// ground level (`0x00_04B0`) in place of I129/090. FSPEC flags FRN 1, 6, 7 and FRN 8,
    /// 10: `0x87, 0xA0`. LEN 23.
    fn cat129_agl_datagram() -> Vec<u8> {
        vec![
            0x81, 0x00, 0x17, 0x87, 0xA0, 10, 20, b'D', b'E', 0x54, 0x60, 0x00, 0x12, 0x55, 0xAB,
            0x96, 0x04, 0x13, 0x09, 0xB0, 0x00, 0x04, 0xB0,
        ]
    }

    /// A ground model with one ground height and error everywhere, or none; stands in for
    /// the desktop's placed DEM, which this crate cannot reach (the lookup itself is
    /// tested in `gungnir-app`).
    struct FixedGround(Result<(f64, f64), &'static str>);
    impl crate::terrain::GroundElevation for FixedGround {
        fn name(&self) -> &'static str {
            "dem/site.tif"
        }
        fn ground(&self, _: f64, _: f64) -> Result<crate::terrain::GroundHeight, String> {
            self.0
                .map(|(ellipsoidal_m, sigma_m)| crate::terrain::GroundHeight {
                    ellipsoidal_m,
                    sigma_m,
                })
                .map_err(str::to_owned)
        }
    }

    /// **The no-DEM path (D-130), the node's.** A host that lends no ground model gets a
    /// height above ground carried as unknown at 0, with the reason, counted, on the
    /// detection's provenance, and weighed with an up variance that says the height is
    /// unknown -- not 0 m measured to 30 m.
    #[test]
    fn a_height_above_ground_with_no_dem_is_unknown_counted_and_widened() {
        let mut adapter = uas_feed(&FeedSinks::default());
        let detections = adapter.handle_datagram(&cat129_agl_datagram(), MissionTime(43_205.0));
        let report = adapter.drain_uas_reports().pop().expect("one report");
        assert_eq!(
            report.altitude_reference,
            UasAltitudeReference::NoAbsoluteHeight {
                reason: crate::terrain::NO_TERRAIN_ATTACHED.to_owned()
            }
        );
        assert!((report.altitude_agl_m.expect("kept as sent") - 120.0).abs() < 1e-9);
        assert!(report.position.alt_m.abs() < 1e-12, "held at 0");
        assert_eq!(adapter.stats().uas_heights_unknown, 1);
        let (up, variance) = up_and_variance(&detections[0]);
        assert!(up.abs() < 1e-3, "{up}");
        let unknown = 900.0 + UNKNOWN_UAS_HEIGHT_BOUND_M * UNKNOWN_UAS_HEIGHT_BOUND_M;
        assert!((variance[2] - unknown).abs() < 1e-6, "{variance:?}");
        assert!((variance[0] - 400.0).abs() < 1e-12 && (variance[1] - 400.0).abs() < 1e-12);
        let loss = detections[0]
            .provenance
            .conversion_loss
            .as_deref()
            .expect("the flag reaches the picture");
        assert!(loss.contains(crate::terrain::NO_TERRAIN_ATTACHED), "{loss}");
    }

    /// **The placement (D-130)**, with the DEM lent through the host's handle *after* the
    /// feed was bound, as the desktop's terrain load does: the report and the detection
    /// carry the ground's ellipsoidal height plus I129/100, the up variance adds the DEM's
    /// error, and the count moves.
    #[test]
    fn a_dem_lent_after_binding_places_the_height_above_its_ground() {
        let sinks = FeedSinks::default();
        let mut adapter = uas_feed(&sinks);
        sinks
            .terrain
            .set_available(std::sync::Arc::new(FixedGround(Ok((152.4, 4.0)))));
        let detections = adapter.handle_datagram(&cat129_agl_datagram(), MissionTime(43_205.0));
        let report = adapter.drain_uas_reports().pop().expect("one report");
        assert_eq!(
            report.altitude_reference,
            UasAltitudeReference::TerrainPlaced {
                terrain: "dem/site.tif".into(),
                ground_m: 152.4,
                ground_sigma_m: 4.0
            }
        );
        assert!((report.position.alt_m - 272.4).abs() < 1e-9);
        let (up, variance) = up_and_variance(&detections[0]);
        assert!((up - 272.4).abs() < 1e-3, "{up}");
        assert!((variance[2] - (900.0 + 16.0)).abs() < 1e-9, "{variance:?}");
        let s = adapter.stats();
        assert_eq!(
            (s.uas_heights_terrain_placed, s.uas_heights_unknown),
            (1, 0)
        );
        let loss = detections[0]
            .provenance
            .conversion_loss
            .as_deref()
            .expect("stated");
        assert!(loss.contains("152.4 m, from dem/site.tif"), "{loss}");
    }

    /// A DEM with no ground at the report's position -- off its edge, or in a hole -- or
    /// one that answers with an error that is no error, leaves the height unknown with
    /// that reason, never placed on a ground of 0.
    #[test]
    fn a_dem_with_no_ground_here_leaves_the_height_unknown_with_its_reason() {
        for (answer, expected) in [
            (
                Err("off the DEM"),
                "dem/site.tif gives no ground height here: off the DEM",
            ),
            (Ok((152.4, 0.0)), "with an error of 0 m"),
            (Ok((f64::NAN, 4.0)), "a ground height of NaN m"),
        ] {
            let sinks = FeedSinks::default();
            sinks
                .terrain
                .set_available(std::sync::Arc::new(FixedGround(answer)));
            let mut adapter = uas_feed(&sinks);
            adapter.handle_datagram(&cat129_agl_datagram(), MissionTime(43_205.0));
            let report = adapter.drain_uas_reports().pop().expect("one report");
            let UasAltitudeReference::NoAbsoluteHeight { reason } = &report.altitude_reference
            else {
                panic!("{:?}", report.altitude_reference);
            };
            assert!(reason.contains(expected), "{reason}");
            assert!(report.position.alt_m.abs() < 1e-12);
            assert_eq!(adapter.stats().uas_heights_unknown, 1);
        }
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
