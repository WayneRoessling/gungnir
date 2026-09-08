// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What a UAS platform's own KLV metadata stream says about itself (GAP-099;
//! `docs/design/external-standards.md` §8 and §8.2).
//!
//! MISB ST 0601's UAS Datalink Local Set is telemetry a platform (and the sensor ball
//! it carries) reports about itself, riding alongside -- not inside -- the video it
//! also carries. It is a cooperative source in exactly AIS's and ADS-B's sense
//! (`gungnir_ingest::adapters::ais::CooperativeReport`,
//! `gungnir_ingest::adapters::adsb::CooperativeReport`): nobody at the ingest boundary
//! verifies a platform designation or an orientation angle, the codec carries what the
//! stream said, and `gungnir-identification`'s evidence fusion is what weighs it.
//!
//! **Why this type lives here and not beside its adapter, unlike AIS's and ADS-B's
//! `CooperativeReport`.** [`UasPlatformReport`] is a position plus an orientation plus
//! a sensor-pointing angle, closer in shape to what DN-27 gives `gungnir-model` for a
//! bearing (`crate::Measurement::Bearing`) than to a single cooperative-identity
//! claim, so a future consumer drawing sensor footprint or platform orientation reads
//! it from the one crate every layer already depends on, rather than reaching into an
//! ingest adapter.

use crate::{MissionTime, SensorId};

/// A platform position or ground point, in the local ENU frame, metres.
///
/// ST 0601 requires latitude and longitude together for a point to mean anything; an
/// elevation the stream did not carry is recorded as absent rather than guessed at
/// zero, because a UAS orbiting at altitude with no stated sensor elevation is not the
/// same claim as one sitting on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnuPoint {
    pub enu: [f64; 3],
    /// `false` when the source carried no elevation and `enu[2]` is `0.0` by
    /// construction rather than by report.
    pub elevation_reported: bool,
}

/// What a UAS's KLV metadata stream (MISB ST 0601's UAS Datalink Local Set) says
/// about the platform and its sensor, decoded by `gungnir_interop::misb0601` and
/// carried by `gungnir_ingest::adapters::misb::UasMetadataAdapter`.
///
/// Every field is `Option` because ST 0601 defines well over eighty tags and a
/// producer sends whichever subset its own platform and sensor support; a local set
/// with three tags and one with thirty are both valid frames. Nothing here is a
/// detection of another entity -- see [`crate::DetectionView`] for the platform's own
/// position placed as one, which the adapter emits alongside this when a fix is
/// available, exactly as AIS and ADS-B do for their own cooperative reports.
#[derive(Debug, Clone, PartialEq)]
pub struct UasPlatformReport {
    /// The identity of the feed that decoded this metadata stream, in the sensor
    /// registry -- the ground station or relay, not a claim broadcast by the
    /// platform. ST 0601 carries no equivalent of AIS's MMSI; `platform_tail_number`
    /// and `platform_designation` are the closest the standard comes to an identity,
    /// and neither is guaranteed unique or present.
    pub sensor: SensorId,
    /// The platform's own position: MISB's Sensor Latitude/Longitude/True Altitude
    /// tags (13/14/15), which for an airborne platform is the aircraft's own GPS/INS
    /// fix -- not a claim about where its camera is looking. That is `frame_center`.
    pub platform_position: Option<EnuPoint>,
    /// Platform heading, radians, `[0, 2*pi)`. ST 0601 tag 5.
    pub platform_heading_rad: Option<f64>,
    /// Platform pitch, radians. ST 0601 tag 6.
    pub platform_pitch_rad: Option<f64>,
    /// Platform roll, radians. ST 0601 tag 7.
    pub platform_roll_rad: Option<f64>,
    /// The sensor's (gimbal's) pointing relative to the platform: azimuth, radians,
    /// `[0, 2*pi)`. ST 0601 tag 18.
    pub sensor_relative_azimuth_rad: Option<f64>,
    /// Sensor relative elevation, radians. ST 0601 tag 19.
    pub sensor_relative_elevation_rad: Option<f64>,
    /// Sensor relative roll, radians. ST 0601 tag 20.
    pub sensor_relative_roll_rad: Option<f64>,
    /// Line-of-sight distance from the sensor to what it is looking at, metres. ST
    /// 0601 tag 21.
    pub slant_range_m: Option<f64>,
    /// The ground point the sensor is looking at, the video frame's own centre:
    /// Frame Center Latitude/Longitude/Elevation, tags 23/24/25. Distinct from
    /// `platform_position` whenever the sensor's relative azimuth or elevation is
    /// non-zero -- the platform and the point it is looking at are different places.
    pub frame_center: Option<EnuPoint>,
    /// Free-text platform type or model, e.g. "Predator". ST 0601 tag 10.
    pub platform_designation: Option<String>,
    /// Free-text tail number. ST 0601 tag 4.
    pub platform_tail_number: Option<String>,
    /// Free-text mission identifier. ST 0601 tag 3.
    pub mission_id: Option<String>,
    /// Free-text sensor/payload name, e.g. "EO Nose". ST 0601 tag 11.
    pub image_source_sensor: Option<String>,
    /// The UAS Datalink LS version the producer says it wrote, when tag 65 was
    /// present.
    pub uas_lds_version: Option<u8>,
    /// ST 0601's own precision time stamp (tag 2), converted from microseconds since
    /// the Unix epoch to `MissionTime`'s seconds, when the frame carried one; the
    /// receipt time otherwise. Unlike AIS's message time (a bare seconds-within-a-
    /// minute field), tag 2 is a full absolute timestamp, so no within-the-minute
    /// reconstruction is needed here.
    pub source_time: MissionTime,
    pub receipt_time: MissionTime,
}
