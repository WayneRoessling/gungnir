// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A UAS KLV metadata receiver as a source (GAP-099;
//! `docs/design/external-standards.md` §8 and §8.2).
//!
//! MISB ST 0601 metadata rides an elementary stream alongside a UAS's video, not
//! inside a self-delimited datagram the way an ASTERIX radar report or an AIS
//! sentence does, so this adapter buffers whatever bytes its [`KlvSource`] hands it
//! across polls and repeatedly asks `gungnir_interop::misb0601::decode_frame` for a
//! complete frame off the front. **Metadata only**: the video essence itself is out
//! of scope for this gap by design (the design survey separates "a video transport
//! carrying metadata" from "a detection message"), and nothing here parses, decodes,
//! or displays a frame of imagery.
//!
//! Two things come out of a decoded frame, mirroring the AIS and ADS-B adapters'
//! shape for a cooperative source: **the platform's own position becomes a
//! detection**, placed in the local frame like a radar plot, and goes through the
//! gateway like every other observation -- authenticated, validated, quarantined if
//! it fails. **Every frame also goes to a platform-report sink** carrying the
//! orientation and sensor-pointing fields no `Measurement` variant has room for,
//! which the host may draw or weigh as evidence; nothing here trusts a heading or a
//! platform designation beyond carrying it.
//!
//! **The checksum discard rule is enforced here, not in the codec.** MISB ST
//! 0601.8-08 says (quoted in `gungnir_interop::misb0601`'s module doc comment) that a
//! frame whose computed checksum disagrees with its stated one "shall be discarded".
//! `decode_frame` decodes such a frame's fields anyway -- KLV framing does not depend
//! on the checksum, and refusing to decode would make the codec unable to report
//! *why* a frame was bad -- and this adapter is where the "discarded" half of that
//! rule is applied: a checksum mismatch produces no detection and no platform report,
//! counted on [`MisbFeedStats::checksum_mismatches`] rather than silently accepted or
//! silently dropped.
//!
//! **What bounds the buffer, and what does not.** A header promising more than
//! `MAX_FRAME_BYTES` is treated as corrupt and resynchronized past rather than waited
//! for (that constant's own doc comment records the stall this closed), so one corrupt
//! length byte can no longer hold the feed or grow the buffer without limit. A header
//! within that bound whose frame never completes -- a source that stops mid-frame --
//! holds at most that many bytes until more arrive or the connection fails, which is
//! the right behaviour for a stream that has merely paused; and a stream carrying no
//! UAS Datalink LS key at all is trimmed to the fifteen bytes a key could be
//! straddling, as before.
//!
//! **Human-owned (the `gungnir-ingest` gateway); signed by the owner 2026-09-09**,
//! after the review before signing found and closed the corrupt-length stall above,
//! recorded the missing-altitude axis a placed fix silently carried, and made the
//! codec's fixed tag widths strict (`gungnir_interop::misb0601`).

use std::collections::VecDeque;
use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_interop::misb0601::{
    decode_frame, find_next_key, Misb0601Error, Misb0601Frame, UDS_KEY,
};
use gungnir_model::{
    EnuPoint, Geodetic, LocalFrame, MissionTime, Provenance, SensorId, UasPlatformReport,
};

/// Per-axis variance stamped on a UAS platform's own GPS/INS position, metres
/// squared (`docs/design/DN-27-bearing-only-detections.md` §8) -- the same
/// tracking-baseline default `gungnir_ingest::adapters::ais` uses, restated rather
/// than imported for the reason that module's own doc comment gives. None of the
/// seventeen tags this decoder reads states the platform's own position accuracy, so
/// there is nothing more specific to carry yet; a receiver whose GNSS/INS accuracy
/// is actually known should set this from that, which is a change to this adapter's
/// configuration and not to its mapping.
const BASELINE_POSITION_VARIANCE_M2: [f64; 3] = [400.0, 400.0, 900.0];

/// The largest frame this adapter will wait for, in bytes -- this adapter's own bound,
/// not MISB's. ST 0601 puts no limit on a local set's BER length, but a UAS Datalink LS
/// is a few hundred bytes to a few kilobytes in practice (the vendored worked example is
/// 228), so a header promising more than this is far more likely one corrupt length
/// byte than a frame worth waiting for. Waiting is not free: every byte that arrives
/// behind such a header is swallowed until the promised count is met, so before this
/// bound existed (2026-09-09, found in review before signing) a single corrupt length
/// stalled the feed for good -- twenty valid frames queued behind a header claiming
/// four gigabytes produced nothing across two hundred polls, with no error, no
/// resynchronization, and a buffer that only grew. A header past this bound is now
/// treated exactly like a malformed one: counted on
/// [`MisbFeedStats::frames_undecodable`] and resynchronized past.
const MAX_FRAME_BYTES: usize = 65_535;

/// Where the KLV bytes come from: a receiver's TCP port, a recording, or a test.
pub trait KlvSource: Send {
    /// Every byte received since the last call, in arrival order.
    fn take_bytes(&mut self) -> Result<Vec<u8>, IngestError>;
    fn describe(&self) -> String;
}

impl KlvSource for Box<dyn KlvSource> {
    fn take_bytes(&mut self) -> Result<Vec<u8>, IngestError> {
        (**self).take_bytes()
    }

    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// A receiver's TCP port (an IP re-streamer for a metadata elementary stream), read
/// without blocking.
#[derive(Debug)]
pub struct TcpKlvSource {
    stream: TcpStream,
    description: String,
}

impl TcpKlvSource {
    /// # Errors
    ///
    /// `IngestError::Io` when the connection cannot be made within `timeout`.
    pub fn connect(addr: SocketAddr, timeout: Duration) -> Result<Self, IngestError> {
        let io = |e: std::io::Error| IngestError::Io(format!("misb tcp {addr}: {e}"));
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(io)?;
        stream.set_nonblocking(true).map_err(io)?;
        Ok(Self {
            stream,
            description: format!("tcp:{addr}"),
        })
    }
}

impl KlvSource for TcpKlvSource {
    fn take_bytes(&mut self) -> Result<Vec<u8>, IngestError> {
        let mut buf = [0u8; 4096];
        let mut out = Vec::new();
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    return Err(IngestError::Io(format!(
                        "{}: the receiver closed the connection",
                        self.description
                    )))
                }
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(IngestError::Io(format!("{}: {e}", self.description))),
            }
        }
        Ok(out)
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// A recording of KLV bytes, released a chunk per poll: a recording has no timing of
/// its own, so a host (or a test) chooses the pace.
#[derive(Debug)]
pub struct RecordedKlvSource {
    remaining: VecDeque<u8>,
    per_poll: usize,
    description: String,
}

impl RecordedKlvSource {
    /// # Errors
    ///
    /// `IngestError::Io` when the file cannot be read.
    pub fn open(path: &std::path::Path) -> Result<Self, IngestError> {
        let bytes =
            std::fs::read(path).map_err(|e| IngestError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self::from_bytes(
            bytes,
            format!("recorded:{}", path.display()),
        ))
    }

    #[must_use]
    pub fn from_bytes(bytes: impl IntoIterator<Item = u8>, description: String) -> Self {
        Self {
            remaining: bytes.into_iter().collect(),
            per_poll: usize::MAX,
            description,
        }
    }

    /// Release at most `n` bytes per poll, so a test can exercise a frame split
    /// across polls the way a real stream splits one across TCP segments.
    #[must_use]
    pub fn with_bytes_per_poll(mut self, n: usize) -> Self {
        self.per_poll = n.max(1);
        self
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.remaining.len()
    }
}

impl KlvSource for RecordedKlvSource {
    fn take_bytes(&mut self) -> Result<Vec<u8>, IngestError> {
        let n = self.per_poll.min(self.remaining.len());
        Ok(self.remaining.drain(..n).collect())
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// The queue a host drains for platform reports.
pub type PlatformReportSink = Arc<Mutex<VecDeque<UasPlatformReport>>>;

/// What the feed has done since the adapter was built.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MisbFeedStats {
    /// Complete frames the codec decoded, whether or not their checksum validated.
    pub frames_decoded: u64,
    /// Bytes that did not extend into a decodable frame: a key mismatch or a
    /// malformed local set. Each one triggers a resynchronization search.
    pub frames_undecodable: u64,
    /// Times the buffer was advanced past undecodable bytes to the next occurrence
    /// of the UAS Datalink LS key.
    pub resynchronized: u64,
    /// Decoded frames MISB ST 0601.8-08 says must be discarded: the computed
    /// checksum did not match the stated one. Produces no detection and no report.
    pub checksum_mismatches: u64,
    /// Frames whose platform position was placed in the local frame and emitted as
    /// a detection.
    pub positions_placed: u64,
    /// Accepted frames with no usable platform position (Sensor Latitude or
    /// Longitude absent).
    pub positions_not_placed: u64,
}

/// The counters as of the last poll, for a host to read.
pub type MisbStatsSink = Arc<Mutex<MisbFeedStats>>;

/// The adapter.
pub struct UasMetadataAdapter<S: KlvSource> {
    name: String,
    sensor: SensorId,
    frame: LocalFrame,
    source: S,
    buffer: Vec<u8>,
    stats: MisbFeedStats,
    reports: Option<PlatformReportSink>,
    stats_sink: Option<MisbStatsSink>,
}

impl<S: KlvSource> UasMetadataAdapter<S> {
    /// `sensor` is the receiving feed's identity in the sensor list; `frame` places
    /// what it hears.
    pub fn new(name: impl Into<String>, sensor: SensorId, frame: LocalFrame, source: S) -> Self {
        let name = name.into();
        Self {
            name: format!("misb0601:{name}:{}", source.describe()),
            sensor,
            frame,
            source,
            buffer: Vec::new(),
            stats: MisbFeedStats::default(),
            reports: None,
            stats_sink: None,
        }
    }

    /// Hand every decoded, checksum-valid frame's platform report to a host through
    /// `sink`.
    #[must_use]
    pub fn with_report_sink(mut self, sink: PlatformReportSink) -> Self {
        self.reports = Some(sink);
        self
    }

    /// Publish the counters at the end of every poll.
    #[must_use]
    pub fn with_stats_sink(mut self, sink: MisbStatsSink) -> Self {
        self.stats_sink = Some(sink);
        self
    }

    #[must_use]
    pub fn stats(&self) -> MisbFeedStats {
        self.stats
    }

    fn place(&self, lat_deg: f64, lon_deg: f64, alt_m: f64) -> [f64; 3] {
        self.frame.to_enu(Geodetic {
            lat_rad: lat_deg.to_radians(),
            lon_rad: lon_deg.to_radians(),
            alt_m,
        })
    }

    /// A lat/lon pair (with an optional elevation) into a placed [`EnuPoint`], or
    /// `None` when either the latitude or the longitude is absent -- ST 0601 gives a
    /// point no meaning from one alone.
    fn point(
        &self,
        lat_deg: Option<f64>,
        lon_deg: Option<f64>,
        elev_m: Option<f64>,
    ) -> Option<EnuPoint> {
        let (lat, lon) = (lat_deg?, lon_deg?);
        Some(EnuPoint {
            enu: self.place(lat, lon, elev_m.unwrap_or(0.0)),
            elevation_reported: elev_m.is_some(),
        })
    }

    fn report(&mut self, report: UasPlatformReport) {
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
        conversion_loss: Option<&'static str>,
    ) -> DetectionView {
        DetectionView {
            sensor: self.sensor,
            source_time,
            receipt_time: now,
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(enu[0], enu[1], enu[2]),
                variance_m2: BASELINE_POSITION_VARIANCE_M2,
            },
            provenance: Provenance {
                source_sensor_ids: vec![self.sensor.0],
                calibration_baseline_version: None,
                algorithm_version: format!(
                    "misb0601 ({})",
                    gungnir_interop::misb0601::CROSS_CHECK_SOURCE
                ),
                conversion_loss: conversion_loss.map(str::to_owned),
                ..Provenance::default()
            },
        }
    }

    /// One decoded frame: a checksum mismatch is discarded and counted per MISB ST
    /// 0601.8-08; otherwise a placeable position becomes a detection and every
    /// frame becomes a platform report.
    fn handle(&mut self, klv: &Misb0601Frame, now: MissionTime, out: &mut Vec<DetectionView>) {
        if !klv.checksum_valid() {
            self.stats.checksum_mismatches += 1;
            tracing::warn!(
                adapter = %self.name,
                stated = ?klv.stated_checksum,
                computed = klv.computed_checksum,
                "a KLV frame's checksum did not validate; discarded per MISB ST 0601.8-08"
            );
            return;
        }
        let source_time = klv.source_time(now);
        let platform_position = self.point(
            klv.sensor_latitude_deg,
            klv.sensor_longitude_deg,
            klv.sensor_true_altitude_m,
        );
        let frame_center = self.point(
            klv.frame_center_latitude_deg,
            klv.frame_center_longitude_deg,
            klv.frame_center_elevation_m,
        );
        match platform_position {
            Some(point) => {
                self.stats.positions_placed += 1;
                // The fix is still placed, but an absent Sensor True Altitude puts it
                // at 0 m in the local frame, and that axis is then not a measurement:
                // named in the provenance, the same way `cat048::map` records "no
                // height in report" rather than passing the radar site's height off
                // as measured (2026-09-09, found in review before signing).
                let loss = (!point.elevation_reported).then_some(
                    "no Sensor True Altitude (Tag 15) in frame; up set to 0 m in the \
                     local frame",
                );
                out.push(self.detection(point.enu, source_time, now, loss));
            }
            None => self.stats.positions_not_placed += 1,
        }
        self.report(UasPlatformReport {
            sensor: self.sensor,
            platform_position,
            platform_heading_rad: klv.platform_heading_deg.map(f64::to_radians),
            platform_pitch_rad: klv.platform_pitch_deg.map(f64::to_radians),
            platform_roll_rad: klv.platform_roll_deg.map(f64::to_radians),
            sensor_relative_azimuth_rad: klv.sensor_relative_azimuth_deg.map(f64::to_radians),
            sensor_relative_elevation_rad: klv.sensor_relative_elevation_deg.map(f64::to_radians),
            sensor_relative_roll_rad: klv.sensor_relative_roll_deg.map(f64::to_radians),
            slant_range_m: klv.slant_range_m,
            frame_center,
            platform_designation: klv.platform_designation.clone(),
            platform_tail_number: klv.platform_tail_number.clone(),
            mission_id: klv.mission_id.clone(),
            image_source_sensor: klv.image_source_sensor.clone(),
            uas_lds_version: klv.uas_lds_version,
            source_time,
            receipt_time: now,
        });
    }
}

impl<S: KlvSource> ProtocolAdapter for UasMetadataAdapter<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        let bytes = self.source.take_bytes()?;
        self.buffer.extend_from_slice(&bytes);
        let mut out = Vec::new();
        loop {
            match decode_frame(&self.buffer) {
                Ok((frame, consumed)) => {
                    self.stats.frames_decoded += 1;
                    self.handle(&frame, now, &mut out);
                    self.buffer.drain(..consumed);
                    continue;
                }
                // A header promising more than any real frame carries is one corrupt
                // length byte, not a frame in transit: waiting for it would swallow
                // every frame behind it (`MAX_FRAME_BYTES`). Fall through to the
                // resynchronization below exactly as a malformed frame does.
                Err(Misb0601Error::Truncated { needed, .. }) if needed > MAX_FRAME_BYTES => {
                    tracing::warn!(
                        adapter = %self.name,
                        needed,
                        bound = MAX_FRAME_BYTES,
                        "a KLV header promises an implausibly large frame; treated as corrupt"
                    );
                }
                // Not corrupt: the frame the header promises has not fully arrived.
                // Wait for the next poll rather than treating this as a fault.
                Err(Misb0601Error::Truncated { .. }) => break,
                Err(
                    Misb0601Error::KeyMismatch
                    | Misb0601Error::BerLengthMalformed
                    | Misb0601Error::LocalSetTruncated { .. },
                ) => {
                    tracing::debug!(adapter = %self.name, "a KLV frame did not decode; resynchronizing");
                }
            }
            // Whatever is at the front is not a frame this adapter will decode: count
            // it and move to the next key.
            self.stats.frames_undecodable += 1;
            if let Some(offset) = find_next_key(&self.buffer, 1) {
                self.stats.resynchronized += 1;
                self.buffer.drain(..offset);
            } else {
                // No key anywhere in what is buffered. Keep the last
                // `UDS_KEY.len() - 1` bytes in case a key is straddling the end of
                // this read, and wait for more.
                let keep = self.buffer.len().min(UDS_KEY.len() - 1);
                let drop_to = self.buffer.len() - keep;
                self.buffer.drain(..drop_to);
                break;
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
    use gungnir_interop::misb0601::packet_checksum;

    fn frame() -> LocalFrame {
        // Coordinates near the fixture's own Sensor Latitude/Longitude
        // (testdata/misb/SOURCE.md), so the placed detection has a modest ENU
        // magnitude rather than spanning half the globe.
        LocalFrame::new(Geodetic {
            lat_rad: 60.0_f64.to_radians(),
            lon_rad: 128.0_f64.to_radians(),
            alt_m: 0.0,
        })
    }

    /// A well-formed KLV frame from local-set items, with a correct trailing
    /// checksum computed by the real `packet_checksum` -- hand-built test
    /// scaffolding in the sense `docs/design/external-standards.md` §1.8 already
    /// uses for ASTERIX, not a fixture.
    fn build_frame(items: &[(u8, &[u8])]) -> Vec<u8> {
        let mut value = Vec::new();
        for (tag, bytes) in items {
            value.push(*tag);
            value.push(u8::try_from(bytes.len()).expect("test values stay short-form"));
            value.extend_from_slice(bytes);
        }
        value.push(1);
        value.push(2);
        let checksum_at = UDS_KEY.len() + 1 + value.len();
        value.push(0);
        value.push(0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&UDS_KEY);
        bytes.push(u8::try_from(value.len()).expect("test frames stay short-form"));
        bytes.extend_from_slice(&value);
        let cs = packet_checksum(&bytes).to_be_bytes();
        bytes[checksum_at] = cs[0];
        bytes[checksum_at + 1] = cs[1];
        bytes
    }

    #[test]
    fn a_frame_becomes_a_detection_and_a_platform_report() {
        let sink = PlatformReportSink::default();
        let bytes = build_frame(&[
            (13, &60_200_000_i32.to_be_bytes()), // near the local frame's own origin
            (14, &128_100_000_i32.to_be_bytes()),
            (5, &30_000_u16.to_be_bytes()),
            (10, b"Predator"),
        ]);
        let source = RecordedKlvSource::from_bytes(bytes, "test".into());
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(20), frame(), source)
            .with_report_sink(sink.clone());
        let detections = adapter.poll(MissionTime(1_000.0)).expect("polls");
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].sensor, SensorId(20));
        assert!(detections[0]
            .provenance
            .algorithm_version
            .contains("misb0601"));
        let stats = adapter.stats();
        assert_eq!(
            (
                stats.frames_decoded,
                stats.positions_placed,
                stats.checksum_mismatches
            ),
            (1, 1, 0)
        );
        let reports: Vec<UasPlatformReport> = sink.lock().expect("sink").drain(..).collect();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].platform_designation.as_deref(), Some("Predator"));
        assert!(reports[0].platform_position.is_some());
        assert!(reports[0].platform_heading_rad.is_some());
    }

    #[test]
    fn a_bad_checksum_is_discarded_and_counted_not_silently_accepted() {
        let sink = PlatformReportSink::default();
        // The real vendored fixture: its own stated checksum does not validate
        // (`testdata/misb/SOURCE.md`), which is exactly the case this test proves
        // the adapter enforces MISB ST 0601.8-08's "shall be discarded" rule on.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../testdata/misb/DynamicConstantMISMMSPacketData.bin");
        let source = RecordedKlvSource::open(&path).expect("fixture reads");
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(21), frame(), source)
            .with_report_sink(sink.clone());
        let detections = adapter.poll(MissionTime(1_000.0)).expect("polls");
        assert!(
            detections.is_empty(),
            "a bad checksum must not become a detection"
        );
        assert!(
            sink.lock().expect("sink").is_empty(),
            "nor a platform report"
        );
        let stats = adapter.stats();
        assert_eq!((stats.frames_decoded, stats.checksum_mismatches), (1, 1));
    }

    #[test]
    fn a_frame_split_across_polls_still_decodes_once_it_is_whole() {
        let bytes = build_frame(&[(65, &[6])]);
        let total = bytes.len();
        let source = RecordedKlvSource::from_bytes(bytes, "test".into()).with_bytes_per_poll(7);
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(22), frame(), source);
        let mut polls = 0;
        let mut total_detections = 0;
        loop {
            let out = adapter.poll(MissionTime(f64::from(polls))).expect("polls");
            total_detections += out.len();
            polls += 1;
            if adapter.stats().frames_decoded > 0 || polls > i32::try_from(total).unwrap_or(1000) {
                break;
            }
        }
        assert_eq!(adapter.stats().frames_decoded, 1);
        // Tag 65 alone carries no position, so no detection -- only confirming the
        // frame reassembled and decoded at all.
        assert_eq!(total_detections, 0);
        assert!(polls > 1, "the fixture must actually have been split");
    }

    #[test]
    fn garbage_before_a_real_frame_is_skipped_and_counted() {
        let mut bytes = vec![0xFFu8; 20];
        bytes.extend_from_slice(&build_frame(&[(65, &[6])]));
        let source = RecordedKlvSource::from_bytes(bytes, "test".into());
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(23), frame(), source);
        adapter.poll(MissionTime(0.0)).expect("polls");
        let stats = adapter.stats();
        assert_eq!(stats.frames_decoded, 1);
        assert_eq!(stats.frames_undecodable, 1);
        assert_eq!(stats.resynchronized, 1);
    }

    /// The stall `MAX_FRAME_BYTES` closes (2026-09-09, found in review before signing):
    /// one header claiming four gigabytes, then twenty valid frames. Before the bound,
    /// every one of those frames was swallowed behind the header -- nothing decoded,
    /// nothing counted, nothing resynchronized, across two hundred polls -- because a
    /// `Truncated` frame was always one to wait for. Now the header is treated as the
    /// corrupt length it is, and the frames behind it decode.
    #[test]
    fn one_corrupt_length_no_longer_stalls_the_frames_behind_it() {
        let good = build_frame(&[
            (13, &60_200_000_i32.to_be_bytes()),
            (14, &128_100_000_i32.to_be_bytes()),
            (15, &30_000_u16.to_be_bytes()),
        ]);
        let mut stream = Vec::new();
        stream.extend_from_slice(&UDS_KEY);
        stream.extend_from_slice(&[0x84, 0xFF, 0xFF, 0xFF, 0xFF]); // BER long form: 4 GiB
        for _ in 0..20 {
            stream.extend_from_slice(&good);
        }
        let source = RecordedKlvSource::from_bytes(stream, "test".into()).with_bytes_per_poll(64);
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(24), frame(), source);
        let mut detections = 0;
        for poll in 0..200 {
            detections += adapter
                .poll(MissionTime(f64::from(poll)))
                .expect("polls")
                .len();
        }
        let stats = adapter.stats();
        assert_eq!(
            detections, 20,
            "every frame behind the corrupt header must decode"
        );
        assert_eq!(
            (
                stats.frames_decoded,
                stats.frames_undecodable,
                stats.resynchronized
            ),
            (20, 1, 1),
            "the corrupt header is counted once and resynchronized past once"
        );
    }

    /// A length that is wrong but plausible -- ten bytes more than the frame really
    /// has -- cannot be told from a real header, so the next frame's first bytes are
    /// read as the tail of this one. What saves the stream is that the mangled local
    /// set does not parse (a key byte read as a tag whose length runs past the end),
    /// which resynchronizes to the next key: the mangled frame is lost, the one behind
    /// it is not.
    #[test]
    fn a_plausible_but_wrong_length_loses_one_frame_and_recovers_at_the_next_key() {
        let good = build_frame(&[
            (13, &60_200_000_i32.to_be_bytes()),
            (14, &128_100_000_i32.to_be_bytes()),
            (15, &30_000_u16.to_be_bytes()),
        ]);
        let mut mangled = good.clone();
        mangled[UDS_KEY.len()] += 10; // the short-form length byte
        let mut stream = mangled;
        for _ in 0..4 {
            stream.extend_from_slice(&good);
        }
        let source = RecordedKlvSource::from_bytes(stream, "test".into());
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(25), frame(), source);
        let detections = adapter.poll(MissionTime(0.0)).expect("polls").len();
        let stats = adapter.stats();
        assert_eq!(
            detections, 4,
            "the four frames behind the mangled one decode"
        );
        assert_eq!(
            (
                stats.frames_decoded,
                stats.frames_undecodable,
                stats.resynchronized
            ),
            (4, 1, 1)
        );
    }

    /// A fix with no Sensor True Altitude is still placed -- at 0 m in the local
    /// frame -- and says so in its provenance rather than carrying a 30 m vertical
    /// sigma for an axis nobody measured, the same record `cat048::map` keeps for a
    /// plot with no height (2026-09-09, found in review before signing).
    #[test]
    fn a_frame_without_an_altitude_is_placed_and_the_missing_axis_is_recorded() {
        let sink = PlatformReportSink::default();
        let bytes = build_frame(&[
            (13, &60_200_000_i32.to_be_bytes()),
            (14, &128_100_000_i32.to_be_bytes()),
        ]);
        let source = RecordedKlvSource::from_bytes(bytes, "test".into());
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(26), frame(), source)
            .with_report_sink(sink.clone());
        let detections = adapter.poll(MissionTime(1_000.0)).expect("polls");
        assert_eq!(detections.len(), 1);
        let loss = detections[0]
            .provenance
            .conversion_loss
            .as_deref()
            .expect("the missing altitude is recorded");
        assert!(loss.contains("Tag 15"), "{loss}");
        assert_eq!(adapter.stats().positions_placed, 1);
        let reports: Vec<UasPlatformReport> = sink.lock().expect("sink").drain(..).collect();
        assert!(
            !reports[0]
                .platform_position
                .expect("placed")
                .elevation_reported
        );

        // And a frame that does carry the altitude records no loss at all.
        let bytes = build_frame(&[
            (13, &60_200_000_i32.to_be_bytes()),
            (14, &128_100_000_i32.to_be_bytes()),
            (15, &30_000_u16.to_be_bytes()),
        ]);
        let source = RecordedKlvSource::from_bytes(bytes, "test".into());
        let mut adapter = UasMetadataAdapter::new("uas1", SensorId(27), frame(), source);
        let detections = adapter.poll(MissionTime(1_000.0)).expect("polls");
        assert!(detections[0].provenance.conversion_loss.is_none());
    }
}
