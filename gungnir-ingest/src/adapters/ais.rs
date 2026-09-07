// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! An AIS receiver as a source (GAP-010, D-32; `docs/design/external-standards.md` §3).
//!
//! NMEA sentences arrive from a receiver over TCP (or from a recording), the
//! `gungnir_interop::ais` decoder reads them against ITU-R M.1371-6, and two things come
//! out. **Position reports become detections** on the receiver's sensor identity, placed
//! in the local frame like a radar plot, and go through the gateway like every other
//! observation: authenticated, validated, quarantined if they fail. **Every report also
//! goes to a cooperative sink** with the identity the vessel declared -- its MMSI, and the
//! name and call sign from the static messages the receiver has heard -- because a
//! detection carries no identity and the identity is the whole point of a cooperative
//! source. The host associates those reports with tracks and turns them into
//! identification evidence; this adapter knows no track.
//!
//! **What a cooperative report is not.** It is what the vessel says about itself, on a
//! channel anybody can transmit on. The adapter carries it verbatim and the host weighs
//! it (`gungnir-identification`'s evidence fusion, DN-15's cooperative detectors); nothing
//! here trusts it.
//!
//! **Time.** An AIS report carries only the UTC second its position was fixed in (0 to
//! 59), so `source_time` is the receipt time moved back to that second within the last
//! minute when the field is available, and the receipt time itself otherwise. In a
//! replayed session mission time is not wall time and the adjustment is still a
//! seconds-within-a-minute one, which is the best the message allows and is said here.

use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_interop::ais::{AisCodec, AisError, AisMessage, StaticDataReportPart};
use gungnir_model::{Geodetic, LocalFrame, MissionTime, Provenance, SensorId};

/// Per-axis variance stamped on an AIS position, metres squared
/// (docs/design/DN-27-bearing-only-detections.md §8).
///
/// The tracking baseline's own default measurement noise
/// (`gungnir_fusion_async::PipelineSettings::measurement_noise_var`), which is the
/// number the gate downstream already assumed for every detection when
/// `DetectionView` carried no error at all. **Restated rather than imported**: this
/// crate reaches the pipeline through `gungnir-tracking-service` and does not name it.
/// The migration therefore moves no behaviour; what changes is that the assumption is
/// written on the measurement instead of guessed from it. A receiver whose GNSS
/// accuracy is actually known should carry it here, which is a change to this adapter's
/// configuration and not to its mapping.
const BASELINE_POSITION_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// Where the sentences come from: a receiver's TCP port, a recording, or a test.
pub trait NmeaSource: Send {
    /// Every whole line received since the last call.
    fn take_lines(&mut self) -> Result<Vec<String>, IngestError>;
    fn describe(&self) -> String;
}

impl NmeaSource for Box<dyn NmeaSource> {
    fn take_lines(&mut self) -> Result<Vec<String>, IngestError> {
        (**self).take_lines()
    }

    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// A receiver's TCP port, read without blocking.
#[derive(Debug)]
pub struct TcpNmeaSource {
    stream: TcpStream,
    partial: Vec<u8>,
    description: String,
}

impl TcpNmeaSource {
    /// Connect to a receiver that serves NMEA over TCP, as most do.
    ///
    /// # Errors
    ///
    /// `IngestError::Io` when the connection cannot be made within `timeout`.
    pub fn connect(addr: SocketAddr, timeout: Duration) -> Result<Self, IngestError> {
        let io = |e: std::io::Error| IngestError::Io(format!("ais tcp {addr}: {e}"));
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(io)?;
        stream.set_nonblocking(true).map_err(io)?;
        Ok(Self {
            stream,
            partial: Vec::new(),
            description: format!("tcp:{addr}"),
        })
    }
}

impl NmeaSource for TcpNmeaSource {
    fn take_lines(&mut self) -> Result<Vec<String>, IngestError> {
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    return Err(IngestError::Io(format!(
                        "{}: the receiver closed the connection",
                        self.description
                    )))
                }
                Ok(n) => self.partial.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    return Err(IngestError::Io(format!("{}: {e}", self.description)));
                }
            }
        }
        let mut lines = Vec::new();
        while let Some(at) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=at).collect();
            lines.push(String::from_utf8_lossy(&line).trim_end().to_string());
        }
        Ok(lines)
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// A recording of sentences, released a batch per poll: a recording has no timing of
/// its own, so a host chooses the pace.
#[derive(Debug)]
pub struct RecordedNmeaSource {
    lines: VecDeque<String>,
    per_poll: usize,
    description: String,
}

impl RecordedNmeaSource {
    /// Read every line of `path`. Lines that are not AIS sentences are kept: a real
    /// feed interleaves GPS sentences, and the decoder skips them.
    ///
    /// # Errors
    ///
    /// `IngestError::Io` when the file cannot be read.
    pub fn open(path: &std::path::Path) -> Result<Self, IngestError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| IngestError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self::from_lines(
            text.lines().map(str::to_owned),
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

    /// Release at most `n` lines per poll.
    #[must_use]
    pub fn with_lines_per_poll(mut self, n: usize) -> Self {
        self.per_poll = n.max(1);
        self
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.lines.len()
    }
}

impl NmeaSource for RecordedNmeaSource {
    fn take_lines(&mut self) -> Result<Vec<String>, IngestError> {
        let n = self.per_poll.min(self.lines.len());
        Ok(self.lines.drain(..n).collect())
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// What kind of station reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationKind {
    ClassA,
    ClassB,
    AidToNavigation,
}

/// What a vessel has said about itself, as the host receives it. Nothing here is
/// verified; the MMSI is what the transmitter put in the message.
#[derive(Debug, Clone, PartialEq)]
pub struct CooperativeReport {
    pub mmsi: u32,
    pub kind: StationKind,
    /// The position in the local frame, when the report carried one that was available.
    pub position_enu: Option<[f64; 3]>,
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
    /// From the static messages this receiver has heard for the MMSI, if any.
    pub name: Option<String>,
    pub call_sign: Option<String>,
    pub ship_type: Option<u8>,
}

/// The queue a host drains for cooperative reports.
pub type CooperativeSink = Arc<Mutex<VecDeque<CooperativeReport>>>;

/// What the feed has done since the adapter was built.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AisFeedStats {
    /// Lines read, AIS or not.
    pub lines: u64,
    /// AIS sentences that decoded to a message.
    pub decoded: u64,
    /// Position reports placed in the frame and emitted as detections.
    pub positions: u64,
    /// Static and voyage messages that updated the identity cache.
    pub static_reports: u64,
    /// Messages of a type this decoder does not read.
    pub unsupported: u64,
    /// Sentences that failed to parse or decode.
    pub undecodable: u64,
    /// Position reports whose position was the "not available" sentinel.
    pub not_placed: u64,
}

/// The counters as of the last poll, for a host to read.
pub type AisStatsSink = Arc<Mutex<AisFeedStats>>;

#[derive(Debug, Clone, Default)]
struct StaticInfo {
    name: Option<String>,
    call_sign: Option<String>,
    ship_type: Option<u8>,
}

/// The adapter.
pub struct AisReceiverAdapter<S: NmeaSource> {
    name: String,
    sensor: SensorId,
    frame: LocalFrame,
    source: S,
    codec: AisCodec,
    known: HashMap<u32, StaticInfo>,
    stats: AisFeedStats,
    reports: Option<CooperativeSink>,
    stats_sink: Option<AisStatsSink>,
}

impl<S: NmeaSource> AisReceiverAdapter<S> {
    /// `sensor` is the receiver's identity in the sensor list; `frame` places what it
    /// hears.
    pub fn new(name: impl Into<String>, sensor: SensorId, frame: LocalFrame, source: S) -> Self {
        let name = name.into();
        Self {
            name: format!("ais:{name}:{}", source.describe()),
            sensor,
            frame,
            source,
            codec: AisCodec::default(),
            known: HashMap::new(),
            stats: AisFeedStats::default(),
            reports: None,
            stats_sink: None,
        }
    }

    /// Hand every report to a host through `sink`.
    #[must_use]
    pub fn with_report_sink(mut self, sink: CooperativeSink) -> Self {
        self.reports = Some(sink);
        self
    }

    /// Publish the counters at the end of every poll.
    #[must_use]
    pub fn with_stats_sink(mut self, sink: AisStatsSink) -> Self {
        self.stats_sink = Some(sink);
        self
    }

    #[must_use]
    pub fn stats(&self) -> AisFeedStats {
        self.stats
    }

    /// The receipt time moved back to the report's UTC second, when it has one.
    fn source_time(now: MissionTime, time_stamp: u8) -> MissionTime {
        if time_stamp >= 60 {
            return now;
        }
        let second_in_minute = now.0.rem_euclid(60.0);
        let back = (second_in_minute - f64::from(time_stamp)).rem_euclid(60.0);
        MissionTime(now.0 - back)
    }

    fn place(&self, degrees: Option<(f64, f64)>) -> Option<[f64; 3]> {
        let (lon, lat) = degrees?;
        Some(self.frame.to_enu(Geodetic {
            lat_rad: lat.to_radians(),
            lon_rad: lon.to_radians(),
            alt_m: 0.0,
        }))
    }

    fn report(&mut self, report: CooperativeReport) {
        if let Some(sink) = &self.reports {
            if let Ok(mut q) = sink.lock() {
                q.push_back(report);
            }
        }
    }

    fn detection(
        &self,
        enu: [f64; 3],
        source_time: MissionTime,
        now: MissionTime,
    ) -> DetectionView {
        DetectionView {
            sensor: self.sensor,
            source_time,
            receipt_time: now,
            // DN-27 §8: an AIS position report is a position and stays one; what
            // changes is that it now states its error rather than leaving the gate
            // downstream to assume one.
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(enu[0], enu[1], enu[2]),
                variance_m2: BASELINE_POSITION_VARIANCE_M2,
            },
            provenance: Provenance {
                source_sensor_ids: vec![self.sensor.0],
                calibration_baseline_version: None,
                algorithm_version: format!("ais {}", AisCodec::EDITION),
                ..Provenance::default()
            },
        }
    }

    /// One decoded message: a position becomes a detection and a report; static data
    /// updates the cache and is reported without a position.
    fn handle(&mut self, message: &AisMessage, now: MissionTime, out: &mut Vec<DetectionView>) {
        self.stats.decoded += 1;
        let (mmsi, kind, position, time_stamp) = match message {
            AisMessage::Position(p) => (
                p.header.mmsi,
                StationKind::ClassA,
                Some(p.position.degrees()),
                p.time_stamp,
            ),
            AisMessage::ClassBPosition(p) => (
                p.header.mmsi,
                StationKind::ClassB,
                Some(p.position.degrees()),
                p.time_stamp,
            ),
            AisMessage::ClassBExtended(p) => {
                self.known.entry(p.header.mmsi).or_default().name = Some(p.name.clone());
                (
                    p.header.mmsi,
                    StationKind::ClassB,
                    Some(p.position.degrees()),
                    p.time_stamp,
                )
            }
            AisMessage::AidToNavigation(a) => {
                self.known.entry(a.header.mmsi).or_default().name = Some(a.full_name());
                (
                    a.header.mmsi,
                    StationKind::AidToNavigation,
                    Some(a.position.degrees()),
                    a.time_stamp,
                )
            }
            AisMessage::StaticVoyage(v) => {
                let info = self.known.entry(v.header.mmsi).or_default();
                info.name = Some(v.name.clone());
                info.call_sign = Some(v.call_sign.clone());
                info.ship_type = Some(v.ship_type);
                self.stats.static_reports += 1;
                (v.header.mmsi, StationKind::ClassA, None, 60)
            }
            AisMessage::StaticData(d) => {
                let info = self.known.entry(d.header.mmsi).or_default();
                match &d.part {
                    StaticDataReportPart::A { name } => info.name = Some(name.clone()),
                    StaticDataReportPart::B {
                        ship_type,
                        call_sign,
                        ..
                    } => {
                        info.call_sign = Some(call_sign.clone());
                        info.ship_type = Some(*ship_type);
                    }
                }
                self.stats.static_reports += 1;
                (d.header.mmsi, StationKind::ClassB, None, 60)
            }
            AisMessage::Unsupported { .. } => {
                self.stats.unsupported += 1;
                return;
            }
        };
        let source_time = Self::source_time(now, time_stamp);
        let position_enu = match position {
            Some(degrees) => {
                let placed = self.place(degrees);
                if placed.is_some() {
                    self.stats.positions += 1;
                } else {
                    self.stats.not_placed += 1;
                }
                placed
            }
            None => None,
        };
        if let Some(enu) = position_enu {
            out.push(self.detection(enu, source_time, now));
        }
        let info = self.known.get(&mmsi).cloned().unwrap_or_default();
        self.report(CooperativeReport {
            mmsi,
            kind,
            position_enu,
            source_time,
            receipt_time: now,
            name: info.name,
            call_sign: info.call_sign,
            ship_type: info.ship_type,
        });
    }
}

impl<S: NmeaSource> ProtocolAdapter for AisReceiverAdapter<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        let lines = self.source.take_lines()?;
        let mut out = Vec::new();
        for line in lines {
            self.stats.lines += 1;
            match self.codec.decode_line(&line) {
                Ok(Some(message)) => self.handle(&message, now, &mut out),
                Ok(None) | Err(AisError::NotAisSentence) => {}
                Err(err) => {
                    self.stats.undecodable += 1;
                    tracing::debug!(adapter = %self.name, %err, "an AIS sentence did not decode");
                }
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

    fn frame() -> LocalFrame {
        // The Dutch coast, near the vessels in the gpsd capture.
        LocalFrame::new(Geodetic {
            lat_rad: 52.8_f64.to_radians(),
            lon_rad: 5.7_f64.to_radians(),
            alt_m: 0.0,
        })
    }

    #[test]
    fn a_position_report_becomes_a_detection_and_a_report_with_the_cached_name() {
        let sink = CooperativeSink::default();
        let lines = vec![
            // Message 5 for MMSI 371255000 (SEA ENTERPRISE), then a type 1 for another
            // vessel, from the gpsd capture (testdata/ais/SOURCE.md).
            "!AIVDM,2,1,9,B,55R3Vn82=ILTQ3KKS>1<D60Dq@E918U<F222221J1`?164vc03S1CCAD,0*2A"
                .to_string(),
            "!AIVDM,2,2,9,B,`88888888888880,2*76".to_string(),
            "$GPRMC,213950.00,A,5250.53669,N,00542.34920,E,0.020,,070420,,,A*7D".to_string(),
            "!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5C".to_string(),
        ];
        let source = RecordedNmeaSource::from_lines(lines, "test".into());
        let mut adapter = AisReceiverAdapter::new("kal", SensorId(10), frame(), source)
            .with_report_sink(sink.clone());
        let detections = adapter.poll(MissionTime(1_000.0)).expect("polls");
        // The static message places nothing; the position report is one detection.
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(10));
        assert!(detections[0]
            .provenance
            .algorithm_version
            .contains("M.1371-6"));
        let stats = adapter.stats();
        assert_eq!(
            (
                stats.lines,
                stats.decoded,
                stats.positions,
                stats.static_reports
            ),
            (4, 2, 1, 1)
        );
        let reports: Vec<CooperativeReport> = sink.lock().expect("sink").drain(..).collect();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].mmsi, 371_255_000);
        assert_eq!(reports[0].name.as_deref(), Some("SEA ENTERPRISE"));
        assert_eq!(reports[0].call_sign.as_deref(), Some("HP6683"));
        assert!(reports[0].position_enu.is_none());
        assert_eq!(reports[1].mmsi, 477_553_000);
        assert!(reports[1].position_enu.is_some());
        assert!(reports[1].name.is_none(), "no static message for it yet");
    }

    #[test]
    fn the_source_second_moves_the_receipt_back_within_the_minute() {
        // Receipt at 10 s past the minute, report fixed at second 55: 15 s earlier.
        assert_eq!(
            AisReceiverAdapter::<RecordedNmeaSource>::source_time(MissionTime(3_610.0), 55),
            MissionTime(3_595.0)
        );
        assert_eq!(
            AisReceiverAdapter::<RecordedNmeaSource>::source_time(MissionTime(3_610.0), 60),
            MissionTime(3_610.0)
        );
    }

    #[test]
    fn a_bad_sentence_is_counted_and_the_feed_carries_on() {
        let lines = vec![
            "!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5D".to_string(),
            "!AIVDM,1,1,,B,177KQJ5000G?tO`K>RA1wUbN0TKH,0*5C".to_string(),
        ];
        let source = RecordedNmeaSource::from_lines(lines, "test".into());
        let mut adapter = AisReceiverAdapter::new("kal", SensorId(10), frame(), source);
        let detections = adapter.poll(MissionTime(0.0)).expect("polls");
        assert_eq!(detections.len(), 1);
        assert_eq!(adapter.stats().undecodable, 1);
    }
}
