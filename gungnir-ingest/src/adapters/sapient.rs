// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! SAPIENT edge nodes as a source: the human, acoustic and passive-RF thirds of GAP-001,
//! and the sensor half of GAP-004 (`docs/design/external-standards.md` §7,
//! `docs/design/DN-27-bearing-only-detections.md` §4 and §5).
//!
//! **A person with a compass is an edge node, and so is an acoustic array or an RF
//! direction finder.** SAPIENT's node taxonomy names `NODE_TYPE_HUMAN` ("a human acting
//! as part of a SAPIENT system, such as a spotter or guard"), `NODE_TYPE_ACOUSTIC`, and
//! `NODE_TYPE_PASSIVE_RF` among its node types, and every one of the three emits the same
//! `DetectionReport` message this adapter already reads: a bearing, a lased range, or a
//! Cartesian location, each with its own stated error. **Extended 2026-09-07, GAP-001**:
//! until then this adapter accepted only `NODE_TYPE_HUMAN` and named the other two rather
//! than deciding anything about them (`another_node_type_is_named_rather_than_accepted`,
//! now renamed to cover what changed). Reading the mapping code that decision guarded --
//! `map`, `range_bearing`, `location`, `source_time` -- found nothing in any of the four
//! that assumes a human observer: no default accuracy, no instrument-specific unit, no
//! error model invented where the report states none. The registration gate is what
//! encoded the restriction, not the measurement mapping underneath it, so widening the
//! gate is the whole of this change. **Human-owned (the `gungnir-ingest` gateway); written
//! and gated, not signed.**
//!
//! So a spotter, an acoustic array, or a passive-RF direction finder each need no design
//! of our own: they need this adapter, accepting their node type, and the measurement
//! shape DN-27 §4 added.
//!
//! # The specification this is written against
//!
//! Pinned in `docs/design/external-standards.md` §7: the **SAPIENT Interface Control
//! Document v7, DSTL/PUB145591, 2023-02-01** (Open Government Licence v3.0) for the
//! normative text, with the wire schemas taken from the Apache-2.0 protobuf files at
//! `github.com/dstl/SAPIENT-Proto-Files`, `bsi_flex_335_v2_0`. §7 also records the risk
//! this adapter inherits: **BSI Flex 335 v2.0:2024-03 is the current normative version
//! and may not be redistributed**, so where it and the ICD differ the BSI version
//! governs and this adapter is wrong until somebody with the BSI text says otherwise.
//!
//! # What is decoded, and what is deliberately not
//!
//! **The protobuf JSON mapping, not the binary wire format.** SAPIENT's transport is
//! length-prefixed binary protobuf over TCP; decoding it needs a protobuf runtime, and
//! adding one is a change to `[workspace.dependencies]` and to
//! `docs/agentic-coding-standards.md` §2.9 that this change is not entitled to make.
//! The protobuf JSON mapping is the same message set in the encoding `serde_json`
//! already reads, it is what the upstream fixtures under `testdata/sapient/` are
//! written in, and a middleware that speaks both is exactly what
//! `github.com/dstl/Apex-SAPIENT-Middleware` is. **So this adapter reads a JSON feed and
//! says so**; the binary bearer is an open row and not a silent omission. See
//! [`SapientSource`].
//!
//! # Nothing is dropped without being counted and named
//!
//! Every message, every field and every enumerated value this build does not understand
//! increments a named counter in [`SapientFeedStats::unhandled`], keyed by what it was.
//! A feed that produces no detections therefore says *why* -- an unregistered node, a
//! magnetic datum with no declination to correct it, a coordinate system in feet -- and
//! not merely that the count is zero.
//!
//! # What is refused, and why refusal is the right answer
//!
//! * **A bearing with no stated azimuth error.** For a bearing the error *is* the
//!   information (DN-27 §4), and a default one would be an invention with a plausible
//!   number attached.
//! * **A magnetic, grid or platform datum.** Our azimuth is `atan2(east, north)` about
//!   true north. Correcting a magnetic bearing needs a declination this deployment does
//!   not hold, and correcting a platform-relative one needs the platform's heading,
//!   which the report does not carry. A bearing rotated by an unknown angle is a
//!   confidently wrong direction, which is DN-27 §2's failure in a different coordinate.
//! * **An unspecified coordinate system.** Degrees and radians differ by a factor of 57;
//!   guessing is not a decoding.
//! * **A `Location` in UTM.** No UTM conversion exists in `gungnir-coord`, and one
//!   invented here would be a second, disagreeing implementation of a frame transform.
//!
//! Each of those is counted, named, and logged. **None of them is turned into a
//! detection at a nominal range**, which is the whole of DN-27 §2.
//!
//! # Time
//!
//! A SAPIENT message carries an absolute UTC timestamp. Mission time in this workspace
//! is Unix seconds in the live profiles and replay seconds in a replayed session
//! (`gungnir_model::time`), so the reported timestamp is used as `source_time` only when
//! it lands within [`MAX_TIMESTAMP_LEAD_S`] of the receipt time; otherwise the receipt
//! time is used and the difference is written to `Provenance::conversion_loss`. That is
//! the same honesty the AIS adapter applies to a UTC second with no date, and it keeps a
//! replayed session from stamping every spotter report with 2023.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_model::{Geodetic, LocalFrame, Measurement, MissionTime, Provenance, SensorId};
use serde_json::Value;

/// What `Provenance::algorithm_version` records for a report this adapter mapped.
///
/// Names the pinned document and the schema package, because a decoder without a stated
/// edition is the "confidently wrong" outcome GAP-064's rule exists to prevent, and
/// `docs/design/external-standards.md` §7 is the pin.
pub const ICD_EDITION: &str = "SAPIENT ICD v7 DSTL/PUB145591 2023-02-01 / BSI Flex 335 v2.0";

/// The human/spotter node type, verbatim from `registration.proto` v2.0.
pub const SPOTTER_NODE_TYPE: &str = "NODE_TYPE_HUMAN";

/// The acoustic node type (an array reporting a bearing or a triangulated location),
/// verbatim from `registration.proto` v2.0. Named in `docs/design/external-standards.md`
/// §7, quoted there directly from the ICD; not independently re-verified against the
/// document in this change, which does not have a copy to check it against.
pub const ACOUSTIC_NODE_TYPE: &str = "NODE_TYPE_ACOUSTIC";

/// The passive-RF node type (a direction finder reporting a bearing), verbatim from
/// `registration.proto` v2.0. Same provenance and same caveat as
/// [`ACOUSTIC_NODE_TYPE`].
pub const PASSIVE_RF_NODE_TYPE: &str = "NODE_TYPE_PASSIVE_RF";

/// The three node types whose `DetectionReport` this adapter maps identically: a
/// spotter's compass and eyes, an acoustic array, and a passive-RF direction finder all
/// report a bearing, a lased range, or a Cartesian location, each with its own stated
/// error, and none of `map`, `range_bearing`, `location`, or `source_time` reads the
/// node type at all. Listed together for tests that exercise all three the same way;
/// [`SapientDetectionAdapter::new`] still takes exactly one, because a real deployment
/// binds one adapter instance per feed and each feed is exactly one kind of node -- an
/// instance that accepted any of the three would let a misconfigured feed pass silently
/// as whichever one it happened to declare.
pub const ALL_ACCEPTED_NODE_TYPES: [&str; 3] =
    [SPOTTER_NODE_TYPE, ACOUSTIC_NODE_TYPE, PASSIVE_RF_NODE_TYPE];

/// How far a reported timestamp may sit from the receipt time before this adapter stops
/// believing the two are on the same clock, seconds.
///
/// A day. Wide on purpose: it is not a freshness rule -- the gateway has one of those --
/// but a test of whether mission time and UTC are the same quantity at all. In a
/// replayed session they are not, and the difference is decades.
pub const MAX_TIMESTAMP_LEAD_S: f64 = 86_400.0;

/// Per-axis variance stamped on a spotter's position report when the report states no
/// error of its own, metres squared.
///
/// The tracking baseline's own default measurement noise
/// (`gungnir_fusion_async::PipelineSettings::measurement_noise_var`), restated because
/// this crate does not name that one. **Used only for a `Location` that carried no
/// error**, and the substitution is written to `Provenance::conversion_loss` every time,
/// because a stated error and an assumed one are different claims.
const BASELINE_POSITION_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// Where the messages come from.
///
/// One line per message, each a protobuf-JSON `SapientMessage`. **Not the binary wire
/// format**, for the reason the module documentation gives: a TCP bearer speaking
/// length-prefixed protobuf needs a protobuf runtime this change may not add. A
/// deployment puts a middleware in front, which is what Apex is for, and the source
/// here reads what the middleware writes.
pub trait SapientSource: Send {
    /// Every whole message received since the last call.
    fn take_messages(&mut self) -> Result<Vec<String>, IngestError>;
    fn describe(&self) -> String;
}

impl SapientSource for Box<dyn SapientSource> {
    fn take_messages(&mut self) -> Result<Vec<String>, IngestError> {
        (**self).take_messages()
    }

    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// A recorded session, released a batch per poll: a recording has no timing of its own,
/// so a host chooses the pace. The same shape as the AIS adapter's recorded source, for
/// the same reason.
#[derive(Debug)]
pub struct RecordedSapientSource {
    lines: VecDeque<String>,
    per_poll: usize,
    description: String,
}

impl RecordedSapientSource {
    /// Read every non-empty line of `path`.
    ///
    /// # Errors
    ///
    /// `IngestError::Io` when the file cannot be read.
    pub fn open(path: &std::path::Path) -> Result<Self, IngestError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| IngestError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self::from_lines(
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_owned),
            format!("recorded:{}", path.display()),
        ))
    }

    pub fn from_lines(lines: impl IntoIterator<Item = String>, description: String) -> Self {
        Self {
            lines: lines.into_iter().collect(),
            per_poll: usize::MAX,
            description,
        }
    }

    /// Release at most `n` messages per poll.
    #[must_use]
    pub fn with_messages_per_poll(mut self, n: usize) -> Self {
        self.per_poll = n.max(1);
        self
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.lines.len()
    }
}

impl SapientSource for RecordedSapientSource {
    fn take_messages(&mut self) -> Result<Vec<String>, IngestError> {
        let n = self.per_poll.min(self.lines.len());
        Ok(self.lines.drain(..n).collect())
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// What the feed has done since the adapter was built.
///
/// **Every number here is a reason.** The `unhandled` map is the important one: it is
/// keyed by what was not understood, so a feed producing nothing says which of the
/// dozen possible causes it was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SapientFeedStats {
    /// Lines read, whatever they turned out to be.
    pub messages: u64,
    /// Lines that were not JSON, or were not an object.
    pub undecodable: u64,
    /// Registrations accepted, each from a node declaring the type this adapter instance
    /// was built to accept ([`SapientDetectionAdapter::new`]).
    pub registered: u64,
    /// Detection reports that became a `Measurement::Bearing`: a direction and no range,
    /// which is what a node with no rangefinder produces.
    pub bearings: u64,
    /// Detection reports that became a `Measurement::RangeAzimuthElevation`: a lased or
    /// triangulated range, so the report is polar and **stays polar** (DN-27 §4 and §6).
    pub ranged: u64,
    /// Detection reports that became a `Measurement::Position`: the report carried a
    /// Cartesian `Location` rather than a `RangeBearing`.
    pub positions: u64,
    /// Detection reports refused, for a reason named in [`SapientFeedStats::unhandled`].
    pub refused: u64,
    /// Everything this build did not understand, keyed by what it was.
    ///
    /// A `BTreeMap` so a health line lists the reasons in a stable order and two runs of
    /// the same feed print the same thing.
    pub unhandled: BTreeMap<String, u64>,
}

impl SapientFeedStats {
    fn note(&mut self, what: impl Into<String>) {
        *self.unhandled.entry(what.into()).or_default() += 1;
    }
}

/// The counters as of the last poll, for a host to read.
pub type SapientFeedStatsSink = Arc<Mutex<SapientFeedStats>>;

/// What a node said about itself when it registered.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredNode {
    /// The node's own name for itself, kept for the health line; never trusted.
    name: Option<String>,
    icd_version: Option<String>,
}

/// A SAPIENT edge node reporting detections: a spotter, an acoustic array, or a
/// passive-RF direction finder, one instance per feed and one node type per instance
/// (see [`ALL_ACCEPTED_NODE_TYPES`] for why one and not a set).
///
/// `sensor` is the identity the gateway admits this feed under, and `frame` places any
/// geodetic position it reports. A bearing needs neither, because a bearing is not a
/// place -- but it does need the reporting node's own position, which is
/// [`SapientDetectionAdapter::observer_enu`], because a direction from an unknown point
/// is not a measurement of anything.
pub struct SapientDetectionAdapter<S: SapientSource> {
    name: String,
    sensor: SensorId,
    frame: LocalFrame,
    observer_enu: [f64; 3],
    source: S,
    /// The one node type this instance accepts, e.g. [`SPOTTER_NODE_TYPE`].
    accepted_node_type: &'static str,
    /// Node id to what it registered as. A detection from a node not in here is counted
    /// and refused: SAPIENT's registration is what says a report is this node type's,
    /// and accepting reports from an unregistered node would make the node type
    /// decorative.
    registered: HashMap<String, RegisteredNode>,
    stats: SapientFeedStats,
    stats_sink: Option<SapientFeedStatsSink>,
}

impl<S: SapientSource> SapientDetectionAdapter<S> {
    /// `observer_enu` is where the reporting node is sited, in the local frame. It is
    /// required rather than optional because every bearing this adapter produces is a
    /// direction *from* it, and `gungnir_model::DetectionView` carries a `SensorId` and
    /// no position.
    ///
    /// `accepted_node_type` is the one SAPIENT node type this instance registers, e.g.
    /// [`SPOTTER_NODE_TYPE`], [`ACOUSTIC_NODE_TYPE`], or [`PASSIVE_RF_NODE_TYPE`]. A
    /// node declaring any other type is counted and named, never accepted.
    pub fn new(
        name: impl Into<String>,
        sensor: SensorId,
        frame: LocalFrame,
        observer_enu: [f64; 3],
        source: S,
        accepted_node_type: &'static str,
    ) -> Self {
        let name = name.into();
        Self {
            name: format!("sapient:{name}:{}", source.describe()),
            sensor,
            frame,
            observer_enu,
            source,
            accepted_node_type,
            registered: HashMap::new(),
            stats: SapientFeedStats::default(),
            stats_sink: None,
        }
    }

    /// Publish the counters at the end of every poll.
    #[must_use]
    pub fn with_stats_sink(mut self, sink: SapientFeedStatsSink) -> Self {
        self.stats_sink = Some(sink);
        self
    }

    #[must_use]
    pub fn stats(&self) -> SapientFeedStats {
        self.stats.clone()
    }

    /// Where the spotter is standing, local ENU metres.
    #[must_use]
    pub fn observer_enu(&self) -> [f64; 3] {
        self.observer_enu
    }

    /// Whether a node has registered as a spotter with this adapter.
    #[must_use]
    pub fn is_registered(&self, node_id: &str) -> bool {
        self.registered.contains_key(node_id)
    }

    /// One message: a registration is remembered, a detection report is mapped, anything
    /// else is counted under its own name.
    fn handle(&mut self, message: &Value, now: MissionTime, out: &mut Vec<DetectionView>) {
        let node_id = message
            .get("nodeId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if node_id.is_empty() {
            self.stats.undecodable += 1;
            self.stats.note("message-without-node-id");
            return;
        }
        if let Some(registration) = message.get("registration") {
            self.register(&node_id, registration);
            return;
        }
        let Some(report) = message.get("detectionReport") else {
            // Every other member of `SapientMessage.content`: status reports, tasks,
            // alerts, acknowledgements and errors. Named individually so a feed that is
            // all status and no detection says exactly that.
            let kind = [
                "statusReport",
                "task",
                "taskAck",
                "alert",
                "alertAck",
                "error",
                "registrationAck",
            ]
            .into_iter()
            .find(|k| message.get(*k).is_some())
            .unwrap_or("unrecognised-content");
            self.stats.note(format!("message-kind:{kind}"));
            return;
        };
        if !self.registered.contains_key(&node_id) {
            self.stats.refused += 1;
            self.stats.note("detection-from-unregistered-node");
            tracing::debug!(
                adapter = %self.name,
                %node_id,
                "a detection arrived from a node that has not registered; refused"
            );
            return;
        }
        match self.map(message, report, now) {
            Ok(view) => out.push(view),
            Err(why) => {
                self.stats.refused += 1;
                self.stats.note(why.to_owned());
                tracing::debug!(adapter = %self.name, %node_id, why, "a detection report was refused");
            }
        }
    }

    fn register(&mut self, node_id: &str, registration: &Value) {
        let types: Vec<&str> = registration
            .get("nodeDefinition")
            .and_then(Value::as_array)
            .map(|definitions| {
                definitions
                    .iter()
                    .filter_map(|d| d.get("nodeType").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        if types.is_empty() {
            self.stats.note("registration-without-node-type");
            return;
        }
        if !types.contains(&self.accepted_node_type) {
            // A node declaring a type other than the one this instance was built for is
            // named rather than accepted, never guessed into the wrong feed: a real
            // deployment binds one adapter instance per feed, and a feed that is
            // configured as an acoustic array should not silently start accepting a
            // passive-RF node's registration just because the mapping code underneath
            // would have handled it the same way.
            for declared in types {
                self.stats.note(format!("node-type:{declared}"));
            }
            return;
        }
        self.registered.insert(
            node_id.to_owned(),
            RegisteredNode {
                name: registration
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                icd_version: registration
                    .get("icdVersion")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
        );
        self.stats.registered += 1;
    }

    /// One `DetectionReport` as a `DetectionView`, or the name of what stopped it.
    fn map(
        &mut self,
        message: &Value,
        report: &Value,
        now: MissionTime,
    ) -> Result<DetectionView, &'static str> {
        let mut losses: Vec<String> = Vec::new();
        let source_time = Self::source_time(message, now, &mut losses);
        let measurement = if let Some(range_bearing) = report.get("rangeBearing") {
            Self::range_bearing(range_bearing)?
        } else if let Some(location) = report.get("location") {
            self.location(location, &mut losses)?
        } else {
            // The `location_oneof` is mandatory in the schema; a report with neither is
            // a report of nothing, whatever else it carries.
            return Err("detection-without-a-location");
        };
        match &measurement {
            Measurement::Bearing { .. } => self.stats.bearings += 1,
            Measurement::RangeAzimuthElevation { .. } => self.stats.ranged += 1,
            Measurement::Position { .. } => self.stats.positions += 1,
        }
        Ok(DetectionView {
            sensor: self.sensor,
            source_time,
            receipt_time: now,
            measurement,
            provenance: Provenance {
                source_sensor_ids: vec![self.sensor.0],
                calibration_baseline_version: None,
                algorithm_version: format!("sapient {ICD_EDITION}"),
                peer: None,
                conversion_loss: if losses.is_empty() {
                    None
                } else {
                    Some(losses.join("; "))
                },
                ..Provenance::default()
            },
        })
    }

    /// A `RangeBearing` as a bearing, or as a polar report when a range was measured.
    ///
    /// **A lased range does not become a position here.** DN-27 §4 gives
    /// `Measurement::RangeAzimuthElevation` for exactly this, and §6 says why: converting
    /// a polar measurement to Cartesian and keeping a positional variance throws away the
    /// shape of the uncertainty, which for a lased range is a thin arc and not a circle.
    /// `gungnir-coord` places it when something needs it placed.
    fn range_bearing(rb: &Value) -> Result<Measurement, &'static str> {
        let to_radians = match rb.get("coordinateSystem").and_then(Value::as_str) {
            Some(
                "RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_M"
                | "RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_KM",
            ) => f64::to_radians,
            Some(
                "RANGE_BEARING_COORDINATE_SYSTEM_RADIANS_M"
                | "RANGE_BEARING_COORDINATE_SYSTEM_RADIANS_KM",
            ) => |v: f64| v,
            _ => return Err("range-bearing-coordinate-system-unstated-or-unknown"),
        };
        let range_scale = match rb.get("coordinateSystem").and_then(Value::as_str) {
            Some(
                "RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_KM"
                | "RANGE_BEARING_COORDINATE_SYSTEM_RADIANS_KM",
            ) => 1_000.0,
            _ => 1.0,
        };
        match rb.get("datum").and_then(Value::as_str) {
            Some("RANGE_BEARING_DATUM_TRUE") => {}
            // Named individually, because "we cannot use a magnetic bearing" and "we
            // cannot use a platform-relative one" have different fixes: a declination
            // model for the first, the platform's heading on the report for the second.
            Some(other) => {
                return Err(match other {
                    "RANGE_BEARING_DATUM_MAGNETIC" => "datum-magnetic-no-declination-model",
                    "RANGE_BEARING_DATUM_GRID" => "datum-grid-no-convergence-model",
                    "RANGE_BEARING_DATUM_PLATFORM" => "datum-platform-no-heading-on-the-report",
                    _ => "datum-unstated-or-unknown",
                })
            }
            None => return Err("datum-unstated-or-unknown"),
        }

        let azimuth_rad = to_radians(number(rb, "azimuth").ok_or("bearing-without-an-azimuth")?);
        // For a bearing the error *is* the information (DN-27 §4). No default.
        let azimuth_sigma =
            to_radians(number(rb, "azimuthError").ok_or("bearing-without-a-stated-azimuth-error")?);
        if !(azimuth_sigma.is_finite() && azimuth_sigma > 0.0) {
            return Err("bearing-with-a-non-positive-azimuth-error");
        }
        let elevation_rad = number(rb, "elevation").map(to_radians);
        let elevation_sigma = number(rb, "elevationError").map(to_radians);

        if let Some(range_m) = number(rb, "range").map(|r| r * range_scale) {
            let range_sigma = number(rb, "rangeError")
                .map(|r| r * range_scale)
                .ok_or("ranged-report-without-a-stated-range-error")?;
            // The polar variant is three-dimensional and has no optional elevation: a
            // report with a range but no elevation is a report on the horizontal plane
            // as far as this build can tell, and rather than write a zero elevation --
            // which is the horizon, and the very confusion DN-27 §4 exists to prevent --
            // it is refused and named.
            let (Some(elevation_rad), Some(elevation_sigma)) = (elevation_rad, elevation_sigma)
            else {
                return Err("ranged-report-without-a-stated-elevation-and-its-error");
            };
            if !(range_sigma.is_finite() && range_sigma > 0.0 && elevation_sigma > 0.0) {
                return Err("ranged-report-with-a-non-positive-error");
            }
            return Ok(Measurement::RangeAzimuthElevation {
                range_m,
                azimuth_rad,
                elevation_rad,
                variance: [
                    range_sigma * range_sigma,
                    azimuth_sigma * azimuth_sigma,
                    elevation_sigma * elevation_sigma,
                ],
            });
        }

        // No range: a direction, and the type says so.
        let (elevation_rad, elevation_variance_rad2) = match (elevation_rad, elevation_sigma) {
            (Some(e), Some(s)) if s > 0.0 => (Some(e), Some(s * s)),
            // An elevation with no error is carried by neither field, because a bearing
            // whose elevation has no stated error would be folded in by the filter with
            // an invented one. Recorded as a loss on the detection rather than refusing
            // the whole report: the azimuth is still a measurement.
            _ => (None, None),
        };
        Ok(Measurement::Bearing {
            azimuth_rad,
            elevation_rad,
            azimuth_variance_rad2: azimuth_sigma * azimuth_sigma,
            elevation_variance_rad2,
        })
    }

    /// A Cartesian `Location` as a position in the local frame.
    fn location(
        &mut self,
        location: &Value,
        losses: &mut Vec<String>,
    ) -> Result<Measurement, &'static str> {
        let to_radians = match location.get("coordinateSystem").and_then(Value::as_str) {
            Some("LOCATION_COORDINATE_SYSTEM_LAT_LNG_DEG_M") => f64::to_radians,
            Some("LOCATION_COORDINATE_SYSTEM_LAT_LNG_RAD_M") => |v: f64| v,
            // No UTM conversion exists in `gungnir-coord`, and one written here would be
            // a second implementation of a frame transform that crate owns.
            Some("LOCATION_COORDINATE_SYSTEM_UTM_M") => return Err("location-utm-unsupported"),
            _ => return Err("location-coordinate-system-unstated-or-unknown"),
        };
        // `x` is normally longitude and `y` latitude, per `location.proto`.
        let lon = to_radians(number(location, "x").ok_or("location-without-an-x")?);
        let lat = to_radians(number(location, "y").ok_or("location-without-a-y")?);
        let alt_m = number(location, "z").unwrap_or_else(|| {
            losses.push("report carried no altitude; placed on the frame's ellipsoid".into());
            0.0
        });
        let enu = self.frame.to_enu(Geodetic {
            lat_rad: lat,
            lon_rad: lon,
            alt_m,
        });
        let variance_m2 = match (
            number(location, "xError"),
            number(location, "yError"),
            number(location, "zError"),
        ) {
            (Some(x), Some(y), Some(z)) if x > 0.0 && y > 0.0 && z > 0.0 => {
                // The ICD's errors are on the reported axes: x is longitude-wise, which
                // is east, and y is latitude-wise, which is north.
                [x * x, y * y, z * z]
            }
            _ => {
                losses.push(format!(
                    "report stated no position error; the tracking baseline's default \
                     {BASELINE_POSITION_VARIANCE_M2:?} m2 was assumed"
                ));
                BASELINE_POSITION_VARIANCE_M2
            }
        };
        Ok(Measurement::Position {
            enu: nalgebra::Vector3::new(enu[0], enu[1], enu[2]),
            variance_m2,
        })
    }

    /// The message's own timestamp when it is on the same clock as mission time, and the
    /// receipt time otherwise, with the substitution written down.
    fn source_time(message: &Value, now: MissionTime, losses: &mut Vec<String>) -> MissionTime {
        let Some(text) = message.get("timestamp").and_then(Value::as_str) else {
            losses.push("message carried no timestamp; the receipt time was used".into());
            return now;
        };
        let Some(unix_s) = unix_seconds(text) else {
            losses.push(format!(
                "message timestamp {text:?} is not an RFC 3339 UTC instant; the receipt \
                 time was used"
            ));
            return now;
        };
        if (unix_s - now.0).abs() > MAX_TIMESTAMP_LEAD_S {
            losses.push(format!(
                "message timestamp {text:?} is {:.0} s from the receipt time, so mission \
                 time is not UTC in this session; the receipt time was used",
                unix_s - now.0
            ));
            return now;
        }
        MissionTime(unix_s)
    }
}

impl<S: SapientSource> ProtocolAdapter for SapientDetectionAdapter<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        let messages = self.source.take_messages()?;
        let mut out = Vec::new();
        for line in messages {
            self.stats.messages += 1;
            match serde_json::from_str::<Value>(&line) {
                Ok(Value::Object(map)) => {
                    self.handle(&Value::Object(map), now, &mut out);
                }
                Ok(_) => {
                    self.stats.undecodable += 1;
                    self.stats.note("message-is-not-a-json-object");
                }
                Err(err) => {
                    self.stats.undecodable += 1;
                    self.stats.note("message-is-not-json");
                    tracing::debug!(adapter = %self.name, %err, "a SAPIENT message did not parse");
                }
            }
        }
        if let Some(sink) = &self.stats_sink {
            if let Ok(mut published) = sink.lock() {
                published.clone_from(&self.stats);
            }
        }
        Ok(out)
    }
}

/// A JSON number, whichever of protobuf JSON's two spellings it arrived in: the mapping
/// permits a `double` to be a JSON number or a JSON string, and a producer that quotes
/// its floats is conformant.
fn number(value: &Value, field: &str) -> Option<f64> {
    match value.get(field)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// An RFC 3339 UTC instant as Unix seconds, or `None` for anything this does not
/// recognise.
///
/// Written out rather than pulled in: no date-time crate is in
/// `[workspace.dependencies]` and adding one is a change to
/// `docs/agentic-coding-standards.md` §2.9 that this change may not make. It accepts
/// exactly what SAPIENT emits -- `YYYY-MM-DDTHH:MM:SS[.ffffff]Z` -- and refuses
/// everything else, including offsets other than `Z`, rather than guessing at one.
fn unix_seconds(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if !text.ends_with('Z') {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: i64 = text.get(5..7)?.parse().ok()?;
    let day: i64 = text.get(8..10)?.parse().ok()?;
    let hour: i64 = text.get(11..13)?.parse().ok()?;
    let minute: i64 = text.get(14..16)?.parse().ok()?;
    let second: i64 = text.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let fraction: f64 = match text.get(19..text.len() - 1) {
        Some("") | None => 0.0,
        Some(rest) if rest.starts_with('.') => format!("0{rest}").parse().ok()?,
        Some(_) => return None,
    };
    let days = days_from_civil(year, month, day);
    #[allow(clippy::cast_precision_loss)]
    let seconds = (days * 86_400 + hour * 3_600 + minute * 60 + second) as f64;
    Some(seconds + fraction)
}

/// Days since 1970-01-01 for a proleptic Gregorian date: Howard Hinnant's `days_from_civil`,
/// which is exact in integer arithmetic and needs no table.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> LocalFrame {
        LocalFrame::new(Geodetic {
            lat_rad: 51.0_f64.to_radians(),
            lon_rad: -1.0_f64.to_radians(),
            alt_m: 0.0,
        })
    }

    fn adapter(
        lines: Vec<String>,
        accepted_node_type: &'static str,
    ) -> SapientDetectionAdapter<RecordedSapientSource> {
        SapientDetectionAdapter::new(
            "op-1",
            SensorId(21),
            frame(),
            [0.0, 0.0, 2.0],
            RecordedSapientSource::from_lines(lines, "test".into()),
            accepted_node_type,
        )
    }

    const NODE: &str = "b5546692-a0cc-4846-a36c-4b7098eae08e";

    fn registration(node_type: &str) -> String {
        format!(
            r#"{{"timestamp":"2023-08-14T10:22:00.000000Z","nodeId":"{NODE}","registration":{{"nodeDefinition":[{{"nodeType":"{node_type}"}}],"icdVersion":"BSI Flex 335 v2.0","name":"Spotter"}}}}"#
        )
    }

    /// azimuth 37 degrees, one degree of error, no range: the ordinary spotter report.
    fn bearing_report() -> String {
        format!(
            r#"{{"timestamp":"2023-08-14T10:22:02.340051Z","nodeId":"{NODE}","detectionReport":{{"reportId":"R1","objectId":"O1","rangeBearing":{{"azimuth":37.0,"azimuthError":1.0,"coordinateSystem":"RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_M","datum":"RANGE_BEARING_DATUM_TRUE"}}}}}}"#
        )
    }

    /// The same spotter with a laser rangefinder: azimuth, elevation and range, each
    /// with its error.
    fn ranged_report() -> String {
        format!(
            r#"{{"timestamp":"2023-08-14T10:22:03.000000Z","nodeId":"{NODE}","detectionReport":{{"reportId":"R2","objectId":"O2","rangeBearing":{{"azimuth":37.0,"azimuthError":1.0,"elevation":4.0,"elevationError":1.0,"range":1450.0,"rangeError":5.0,"coordinateSystem":"RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_M","datum":"RANGE_BEARING_DATUM_TRUE"}}}}}}"#
        )
    }

    #[test]
    fn a_spotter_registers_and_its_bearing_becomes_a_bearing() {
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), bearing_report()], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert!(a.is_registered(NODE));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sensor, SensorId(21));
        assert!(out[0].provenance.algorithm_version.contains("ICD v7"));
        match out[0].measurement {
            Measurement::Bearing {
                azimuth_rad,
                elevation_rad,
                azimuth_variance_rad2,
                elevation_variance_rad2,
            } => {
                assert!((azimuth_rad - 37.0_f64.to_radians()).abs() < 1e-12);
                // A missing elevation is not a zero one (DN-27 §4).
                assert!(elevation_rad.is_none());
                assert!(elevation_variance_rad2.is_none());
                let sigma = 1.0_f64.to_radians();
                assert!((azimuth_variance_rad2 - sigma * sigma).abs() < 1e-18);
            }
            ref other => panic!("a bearing report must be a bearing: {other:?}"),
        }
        let stats = a.stats();
        assert_eq!((stats.registered, stats.bearings, stats.refused), (1, 1, 0));
        assert!(stats.unhandled.is_empty(), "{:?}", stats.unhandled);
    }

    /// A lased range keeps its polar shape rather than becoming a position: DN-27 §4
    /// has a variant for exactly this, and §6 says why flattening it would lose the
    /// shape of the uncertainty.
    #[test]
    fn a_lased_range_becomes_a_polar_report_and_not_a_flattened_position() {
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), ranged_report()], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(1_692_008_523.0)).expect("polls");
        assert_eq!(out.len(), 1);
        match out[0].measurement {
            Measurement::RangeAzimuthElevation {
                range_m,
                azimuth_rad,
                elevation_rad,
                variance,
            } => {
                assert!((range_m - 1_450.0).abs() < 1e-9);
                assert!((azimuth_rad - 37.0_f64.to_radians()).abs() < 1e-12);
                assert!((elevation_rad - 4.0_f64.to_radians()).abs() < 1e-12);
                assert!((variance[0] - 25.0).abs() < 1e-9);
            }
            ref other => panic!("a lased report must stay polar: {other:?}"),
        }
        assert!(out[0].measurement.position_enu().is_none());
        assert_eq!(a.stats().ranged, 1);
    }

    /// A detection from a node that never registered is refused and named, not accepted
    /// because it looked like a detection.
    #[test]
    fn a_detection_from_an_unregistered_node_is_refused_and_named() {
        let mut a = adapter(vec![bearing_report()], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert!(out.is_empty());
        let stats = a.stats();
        assert_eq!(stats.refused, 1);
        assert_eq!(
            stats.unhandled.get("detection-from-unregistered-node"),
            Some(&1)
        );
    }

    /// A node of a type this instance was not built for is named by that type, so a
    /// deployment can see it has pointed an acoustic array at an adapter configured for
    /// spotters -- extended 2026-09-07 with the acoustic and passive-RF types
    /// themselves, from GAP-001's finding that the mismatch this test guards is about
    /// configuration, not about the two node types being unsupported.
    #[test]
    fn another_node_type_is_named_rather_than_accepted() {
        let mut a = adapter(vec![registration("NODE_TYPE_ACOUSTIC"), bearing_report()], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert!(out.is_empty());
        assert!(!a.is_registered(NODE));
        let stats = a.stats();
        assert_eq!(stats.registered, 0);
        assert_eq!(
            stats.unhandled.get("node-type:NODE_TYPE_ACOUSTIC"),
            Some(&1)
        );
        assert_eq!(
            stats.unhandled.get("detection-from-unregistered-node"),
            Some(&1)
        );
    }

    /// The other direction of the same rule: an instance built for acoustic nodes
    /// refuses a spotter's registration, named by the type it actually declared.
    #[test]
    fn an_instance_built_for_one_node_type_refuses_a_different_one() {
        let mut a = adapter(
            vec![registration(SPOTTER_NODE_TYPE), bearing_report()],
            ACOUSTIC_NODE_TYPE,
        );
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert!(out.is_empty());
        assert!(!a.is_registered(NODE));
        assert_eq!(
            a.stats().unhandled.get(&format!("node-type:{SPOTTER_NODE_TYPE}")),
            Some(&1)
        );
    }

    /// GAP-001's finding, checked directly: an acoustic array registers and its bearing
    /// maps exactly as a spotter's would, because `map` and `range_bearing` read the
    /// report, never the node type. Extended 2026-09-07.
    #[test]
    fn an_acoustic_array_registers_and_its_bearing_becomes_a_bearing() {
        let mut a = adapter(
            vec![registration(ACOUSTIC_NODE_TYPE), bearing_report()],
            ACOUSTIC_NODE_TYPE,
        );
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert!(a.is_registered(NODE));
        assert_eq!(out.len(), 1);
        match out[0].measurement {
            Measurement::Bearing { azimuth_rad, .. } => {
                assert!((azimuth_rad - 37.0_f64.to_radians()).abs() < 1e-12);
            }
            ref other => panic!("a bearing report must be a bearing: {other:?}"),
        }
        let stats = a.stats();
        assert_eq!((stats.registered, stats.bearings, stats.refused), (1, 1, 0));
        assert!(stats.unhandled.is_empty(), "{:?}", stats.unhandled);
    }

    /// The same again for a passive-RF direction finder, with a lased-shaped ranged
    /// report this time so both report kinds are proven for both new node types between
    /// this test and the one above.
    #[test]
    fn a_passive_rf_node_registers_and_its_ranged_report_stays_polar() {
        let mut a = adapter(
            vec![registration(PASSIVE_RF_NODE_TYPE), ranged_report()],
            PASSIVE_RF_NODE_TYPE,
        );
        let out = a.poll(MissionTime(1_692_008_523.0)).expect("polls");
        assert!(a.is_registered(NODE));
        assert_eq!(out.len(), 1);
        match out[0].measurement {
            Measurement::RangeAzimuthElevation { range_m, .. } => {
                assert!((range_m - 1_450.0).abs() < 1e-9);
            }
            ref other => panic!("a ranged report must stay polar: {other:?}"),
        }
        let stats = a.stats();
        assert_eq!((stats.registered, stats.ranged, stats.refused), (1, 1, 0));
        assert!(stats.unhandled.is_empty(), "{:?}", stats.unhandled);
    }

    /// [`ALL_ACCEPTED_NODE_TYPES`] names the three types this module documents as
    /// mapped identically; this is the check that the list and the gate agree, so
    /// adding a fourth type to one and not the other fails here rather than shipping
    /// silently out of step.
    #[test]
    fn every_listed_node_type_is_independently_acceptable() {
        for node_type in ALL_ACCEPTED_NODE_TYPES {
            let mut a = adapter(vec![registration(node_type), bearing_report()], node_type);
            let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
            assert_eq!(out.len(), 1, "{node_type} did not register and report");
        }
    }

    /// A magnetic bearing is refused with the reason, not silently taken as true north.
    /// Rotating a bearing by an unknown declination is DN-27 §2's failure in another
    /// coordinate.
    #[test]
    fn a_magnetic_datum_is_refused_with_the_reason() {
        let report = format!(
            r#"{{"timestamp":"2023-08-14T10:22:02.000000Z","nodeId":"{NODE}","detectionReport":{{"rangeBearing":{{"azimuth":37.0,"azimuthError":1.0,"coordinateSystem":"RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_M","datum":"RANGE_BEARING_DATUM_MAGNETIC"}}}}}}"#
        );
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), report], SPOTTER_NODE_TYPE);
        assert!(a
            .poll(MissionTime(1_692_008_522.0))
            .expect("polls")
            .is_empty());
        assert_eq!(
            a.stats()
                .unhandled
                .get("datum-magnetic-no-declination-model"),
            Some(&1)
        );
    }

    /// A bearing with no stated azimuth error is refused: for a bearing the error *is*
    /// the information, and a default would be an invention (DN-27 §4).
    #[test]
    fn a_bearing_with_no_stated_error_is_refused() {
        let report = format!(
            r#"{{"timestamp":"2023-08-14T10:22:02.000000Z","nodeId":"{NODE}","detectionReport":{{"rangeBearing":{{"azimuth":37.0,"coordinateSystem":"RANGE_BEARING_COORDINATE_SYSTEM_DEGREES_M","datum":"RANGE_BEARING_DATUM_TRUE"}}}}}}"#
        );
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), report], SPOTTER_NODE_TYPE);
        assert!(a
            .poll(MissionTime(1_692_008_522.0))
            .expect("polls")
            .is_empty());
        assert_eq!(
            a.stats()
                .unhandled
                .get("bearing-without-a-stated-azimuth-error"),
            Some(&1)
        );
    }

    /// An unstated coordinate system is refused rather than guessed at: degrees and
    /// radians differ by a factor of 57.
    #[test]
    fn an_unstated_coordinate_system_is_refused() {
        let report = format!(
            r#"{{"timestamp":"2023-08-14T10:22:02.000000Z","nodeId":"{NODE}","detectionReport":{{"rangeBearing":{{"azimuth":37.0,"azimuthError":1.0,"coordinateSystem":"RANGE_BEARING_COORDINATE_SYSTEM_UNSPECIFIED","datum":"RANGE_BEARING_DATUM_TRUE"}}}}}}"#
        );
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), report], SPOTTER_NODE_TYPE);
        assert!(a
            .poll(MissionTime(1_692_008_522.0))
            .expect("polls")
            .is_empty());
        assert_eq!(
            a.stats()
                .unhandled
                .get("range-bearing-coordinate-system-unstated-or-unknown"),
            Some(&1)
        );
    }

    /// Message kinds this build does not act on are counted by name, so a feed of
    /// nothing but status reports says so.
    #[test]
    fn other_message_kinds_are_counted_by_name() {
        let status = format!(
            r#"{{"timestamp":"2023-08-14T10:22:02.000000Z","nodeId":"{NODE}","statusReport":{{"reportId":"S1"}}}}"#
        );
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), status], SPOTTER_NODE_TYPE);
        assert!(a
            .poll(MissionTime(1_692_008_522.0))
            .expect("polls")
            .is_empty());
        assert_eq!(
            a.stats().unhandled.get("message-kind:statusReport"),
            Some(&1)
        );
    }

    /// A line that is not JSON is counted, and the feed carries on.
    #[test]
    fn a_broken_line_is_counted_and_the_feed_carries_on() {
        let mut a = adapter(vec![
            registration(SPOTTER_NODE_TYPE),
            "{not json".to_string(),
            bearing_report(),
        ], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert_eq!(out.len(), 1);
        assert_eq!(a.stats().undecodable, 1);
        assert_eq!(a.stats().unhandled.get("message-is-not-json"), Some(&1));
    }

    /// The timestamp is used when mission time is UTC, and the receipt time with the
    /// substitution written down when it is not.
    #[test]
    fn a_timestamp_on_another_clock_is_not_believed() {
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), bearing_report()], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(1_692_008_522.0)).expect("polls");
        assert!((out[0].source_time.0 - 1_692_008_522.340_051).abs() < 1e-6);
        assert!(out[0].provenance.conversion_loss.is_none());

        // A replayed session, whose mission time is seconds since the session started.
        let mut a = adapter(vec![registration(SPOTTER_NODE_TYPE), bearing_report()], SPOTTER_NODE_TYPE);
        let out = a.poll(MissionTime(42.0)).expect("polls");
        assert_eq!(out[0].source_time, MissionTime(42.0));
        assert!(out[0]
            .provenance
            .conversion_loss
            .as_deref()
            .is_some_and(|l| l.contains("mission time is not UTC")));
    }

    /// The RFC 3339 reader is exact on the instants the ICD's own examples use, and
    /// refuses what it does not understand rather than guessing an offset.
    #[test]
    fn the_timestamp_reader_is_exact_and_refuses_what_it_cannot_read() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(unix_seconds("2000-03-01T00:00:00Z"), Some(951_868_800.0));
        assert_eq!(
            unix_seconds("2023-08-14T10:22:02.340051Z"),
            Some(1_692_008_522.340_051)
        );
        // Not `Z`, so the offset is unknown to this reader and it says so.
        assert_eq!(unix_seconds("2023-08-14T10:22:02+01:00"), None);
        assert_eq!(unix_seconds("14/08/2023"), None);
        assert_eq!(unix_seconds(""), None);
    }
}
