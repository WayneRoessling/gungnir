// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Delivering a `SensorTask` to a SAPIENT node: the sensor half of GAP-004, the
//! outbound counterpart of `gungnir-ingest`'s inbound SAPIENT spotter adapter
//! (`docs/design/DN-11-sensor-control-and-tasking.md`; `docs/design/external-standards.md`
//! §7).
//!
//! **Why this lives here and not in `gungnir-ingest`.** [`SensorControlAdapter`] is
//! defined in this crate, and nothing about building a `Task` message needs the
//! `DetectionView` machinery `gungnir-ingest` owns -- only `serde_json`, already a
//! workspace dependency. Building it here means no new `Cargo.toml` edge, which
//! `ARCHITECTURE.md` requires be argued when one is unavoidable and avoided when it is
//! not.
//!
//! # The specification this is written against
//!
//! The same pin `gungnir-ingest`'s spotter adapter uses: **SAPIENT Interface Control
//! Document v7, DSTL/PUB145591, 2023-02-01** (Open Government Licence v3.0), wire
//! schemas from the Apache-2.0 protobuf files at
//! `github.com/dstl/SAPIENT-Proto-Files/bsi_flex_335_v2_0/{task,task_ack,sapient_message,
//! location,range_bearing}.proto`. The risk that pin already carries applies here too:
//! BSI Flex 335 v2.0 is the current normative version and may not be redistributed, so
//! where it and the ICD differ the BSI text governs and this adapter is wrong until
//! somebody with it says otherwise.
//!
//! # What a `SensorCommand` becomes, and why
//!
//! `Task.command` is a `oneof` with nine members; three of `SensorCommand`'s four
//! variants map onto it and the fourth is refused rather than guessed:
//!
//! * [`SensorCommand::Cue`] becomes `look_at`, a `LocationOrRangeBearing`. That type's
//!   two arms are a `RangeBearingCone` (a region defined relative to the node) and a
//!   `LocationList` (a region defined as absolute points) -- both built for describing
//!   an *area*, and there is no dedicated single-point variant. A `Cue` names one
//!   absolute point, so it is sent as a `LocationList` holding exactly one `Location`;
//!   using `RangeBearingCone` instead would need the sensor's own position to convert an
//!   absolute `Geodetic` into a relative bearing, which this crate does not hold and
//!   [`SensorTask`] does not carry. `dwell_s`, when given, sets `task_end_time` that many
//!   seconds after `task_start_time`; a `Cue` with no dwell sets no end time, which per
//!   the ICD means the task runs until superseded.
//! * [`SensorCommand::Calibrate`] becomes `request`, the `oneof`'s free-text member ("the
//!   request being asked for"). This is the ICD's own escape hatch for a command it does
//!   not otherwise enumerate, and `procedure` is carried verbatim.
//! * [`SensorCommand::SetMode`] becomes `mode_change`, also free text: the ICD leaves
//!   ASM mode names to the deployment, so this adapter names its own, one per
//!   [`SensorMode`] variant, and any receiver has to agree on them out of band
//!   regardless of what strings are chosen. They are the lower-case spelling:
//!   `"standby"`, `"search"`, `"track"`, `"calibrating"`, `"offline"`.
//! * [`SensorCommand::Search`] **is refused.** A search area could be sent as a `Region`
//!   with `RegionType::AreaOfInterest`, but a `Region` also carries classification and
//!   behaviour filters that `AssetExtent` says nothing about, and a `Task` with an empty
//!   filter list is not obviously the same request as "search this area" -- it might
//!   mean "search it for anything" or "the filters are TBD". That is an interface
//!   agreement nobody has made, not a detail this adapter can fill in with a plausible
//!   default, so it returns [`SensorManagementError::Refused`] naming the reason rather
//!   than emitting a `Task` whose meaning is guessed.
//!
//! # What is not built
//!
//! **Delivery, not transport.** `issue` builds the JSON message and a well-formed
//! `SensorTask` becomes a well-formed `Task` on the wire; where the JSON text then goes
//! is an injected channel, not a socket. The spotter adapter reads the same protobuf-JSON
//! encoding rather than the binary wire format for the reason recorded there: decoding
//! binary protobuf needs a runtime this workspace has not admitted under §2.9. The
//! binary bearer is that adapter's open row and is this one's too.
//!
//! **`TaskAck` parsing is `gungnir-ingest`'s (2026-09-07), not this crate's.** A `TaskAck`
//! arrives on the same inbound stream `gungnir-ingest`'s SAPIENT adapter already reads,
//! and this crate has no edge to `gungnir-ingest` to read a stream with (siblings in
//! `ARCHITECTURE.md` §7.1, and adding one to make this convenient is exactly the edge
//! that rule exists to refuse). What this crate contributes instead is
//! [`decode_task_id`], the inverse of the ULID `task_id` [`SapientTaskAdapter::issue`]
//! mints: a caller that holds both crates -- `gungnir-app`, `gungnir-node` -- decodes a
//! parsed `TaskAck`'s wire `task_id` back to the [`gungnir_model::SensorTaskId`]
//! [`crate::SensorControl::acknowledge`]/[`crate::SensorControl::fail`] takes, with no
//! stored correlation table on either side of the round trip.

use crate::tasking::{SensorCommand, SensorControlAdapter, SensorTask};
use crate::{SensorManagementError, SensorMode};
use gungnir_coord::Geodetic;
use std::collections::HashMap;
use std::sync::Mutex;

/// Where each sensor's SAPIENT node lives: its UUID under `destination_id`.
///
/// A UUID is identity, assigned once when a sensor is provisioned onto the SAPIENT
/// network -- the same reason `gungnir-node`'s peer identities are configuration and not
/// something an adapter invents. Held as a plain map rather than read from
/// `gungnir-config` for the same reason [`crate::tasking`] takes its settings as data:
/// this crate may not depend on `gungnir-config`.
#[derive(Debug, Clone, Default)]
pub struct SapientDestinations {
    by_sensor: HashMap<u32, String>,
}

impl SapientDestinations {
    #[must_use]
    pub fn from_sensors(destinations: impl IntoIterator<Item = (u32, String)>) -> Self {
        Self {
            by_sensor: destinations.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn get(&self, sensor: u32) -> Option<&str> {
        self.by_sensor.get(&sensor).map(String::as_str)
    }
}

/// Delivers a [`SensorTask`] as a SAPIENT `Task` message, JSON-encoded per the pinned
/// protobuf-JSON mapping, onto an injected sink.
///
/// The sink is `Fn(String) + Send + Sync` rather than a live connection: what carries
/// the bytes from here to a SAPIENT node is the open row this module's own
/// documentation names. A test, or a future binary wiring a real transport, supplies
/// the sink; this type never opens a socket.
pub struct SapientTaskAdapter<F: Fn(String) + Send + Sync> {
    /// This node's own SAPIENT identity (`SapientMessage.node_id`, a UUID).
    node_id: String,
    destinations: SapientDestinations,
    sink: F,
    /// Guards nothing but the fetch-and-increment below; the ULID's randomness is what
    /// makes two tasks issued in the same millisecond distinct, and the counter is
    /// belt-and-braces on top of it so a test can assert monotonic ordering.
    sequence: Mutex<u32>,
}

impl<F: Fn(String) + Send + Sync> SapientTaskAdapter<F> {
    pub fn new(node_id: impl Into<String>, destinations: SapientDestinations, sink: F) -> Self {
        Self {
            node_id: node_id.into(),
            destinations,
            sink,
            sequence: Mutex::new(0),
        }
    }
}

/// The mode name this adapter sends for `mode_change`. Documented in the module's own
/// header: the ICD leaves this to the deployment, and these names are ours.
fn mode_change_name(mode: SensorMode) -> &'static str {
    match mode {
        SensorMode::Standby => "standby",
        SensorMode::Search => "search",
        SensorMode::Track => "track",
        SensorMode::Calibrating => "calibrating",
        SensorMode::Offline => "offline",
    }
}

/// One `Location`, in the pinned JSON mapping: radians and metres, WGS-84 ellipsoid
/// height -- `gungnir_coord::Geodetic`'s own convention, so no unit conversion loses
/// precision converting to degrees and back.
fn location_json(g: Geodetic) -> serde_json::Value {
    serde_json::json!({
        "x": g.lon_rad,
        "y": g.lat_rad,
        "z": g.alt_m,
        "coordinateSystem": "LOCATION_COORDINATE_SYSTEM_LAT_LNG_RAD_M",
        "datum": "LOCATION_DATUM_WGS84_E",
    })
}

/// Build `Task.command`'s JSON, or the reason this command cannot be sent.
fn command_json(command: &SensorCommand) -> Result<serde_json::Value, String> {
    match command {
        SensorCommand::SetMode { mode } => Ok(serde_json::json!({
            "modeChange": mode_change_name(*mode),
        })),
        SensorCommand::Cue { target, .. } => Ok(serde_json::json!({
            "lookAt": {
                "locationList": {
                    "locations": [location_json(*target)],
                },
            },
        })),
        SensorCommand::Calibrate { procedure } => Ok(serde_json::json!({
            "request": procedure,
        })),
        SensorCommand::Search { .. } => Err(
            "SAPIENT tasking has no agreed representation for \"search this area\": a \
             Region carries classification and behaviour filters AssetExtent does not \
             state, and sending a Task with an empty filter list would be guessing what \
             the search means rather than saying what was asked for"
                .to_string(),
        ),
    }
}

impl<F: Fn(String) + Send + Sync> SensorControlAdapter for SapientTaskAdapter<F> {
    fn issue(&self, task: &SensorTask) -> Result<(), SensorManagementError> {
        let Some(destination_id) = self.destinations.get(task.sensor.0) else {
            return Err(SensorManagementError::Refused {
                sensor: task.sensor,
                reason: format!(
                    "no SAPIENT destination_id is configured for sensor {}",
                    task.sensor.0
                ),
            });
        };

        let command =
            command_json(&task.command).map_err(|reason| SensorManagementError::Refused {
                sensor: task.sensor,
                reason,
            })?;

        let seq = {
            let mut n = self
                .sequence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let v = *n;
            *n = n.wrapping_add(1);
            v
        };
        let task_id = ulid::task_id(task.issued, task.id.0, seq);
        let issued_at = ulid::rfc3339(task.issued.0);

        let mut region = serde_json::Map::new();
        region.insert("taskId".into(), task_id.into());
        region.insert("control".into(), "CONTROL_START".into());
        region.insert("taskStartTime".into(), issued_at.clone().into());
        if let SensorCommand::Cue {
            dwell_s: Some(dwell_s),
            ..
        } = &task.command
        {
            region.insert(
                "taskEndTime".into(),
                ulid::rfc3339(task.issued.0 + dwell_s).into(),
            );
        }
        region.insert("command".into(), command);

        let message = serde_json::json!({
            "timestamp": issued_at,
            "nodeId": self.node_id,
            "destinationId": destination_id,
            "task": region,
        });

        (self.sink)(message.to_string());
        Ok(())
    }
}

/// A ULID-shaped task identifier and an RFC 3339 timestamp, built without a `chrono` or
/// `ulid` dependency: `task.proto`'s `task_id` is annotated `is_ulid: true`, and
/// `sapient_message.proto`'s `timestamp` needs a UTC instant, and both are small enough
/// to write directly against the well-known algorithms rather than adding a crate for
/// them (`docs/agentic-coding-standards.md` §2.9).
mod ulid {
    use gungnir_model::MissionTime;

    const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

    /// A 26-character Crockford base32 ULID: 48 bits of millisecond timestamp, then 80
    /// bits of payload. **Not cryptographically random** -- the payload here is the
    /// task's own 64-bit id folded with a per-adapter sequence number, which is enough
    /// to make two tasks issued in the same millisecond distinct without a random number
    /// generator this crate has no other use for. Encoding verified against the public
    /// ULID spec's own worked example: encoding timestamp `1469922850259` ms (the value
    /// `01ARZ3NDEKTSV4RRFFQ69G5FAV` decodes to) alone into the first 10 characters
    /// reproduces `01ARZ3NDEK` byte for byte (see the test below).
    pub(super) fn task_id(issued: MissionTime, task_id: u64, sequence: u32) -> String {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "mission time is always non-negative and far below 2^48 ms in any \
                      real or replayed session"
        )]
        let timestamp_ms = (issued.0 * 1000.0).max(0.0) as u64;
        let payload: u128 = (u128::from(task_id) << 32) | u128::from(sequence);
        encode(timestamp_ms, payload)
    }

    /// The inverse of [`task_id`]: the 64-bit task id folded into a ULID string's
    /// payload, or `None` for anything that is not a 26-character string over
    /// [`CROCKFORD`] -- including a foreign ULID this adapter never minted, since a
    /// task id we did not fold in is not this function's to recover.
    pub(super) fn decode_task_id(s: &str) -> Option<u64> {
        let value = decode(s)?;
        let payload_80 = value & ((1u128 << 80) - 1);
        Some((payload_80 >> 32) as u64)
    }

    /// The inverse of [`encode`]: 26 Crockford base32 characters, most significant
    /// first, back to the 128-bit value they came from. The top character of a validly
    /// encoded value only ever carries its low 3 bits (`26 * 5 = 130` bits encode a
    /// 128-bit value, so the first symbol's top 2 bits are always zero); reconstructing
    /// by repeated `(value << 5) | digit` relies on exactly that and needs no separate
    /// case for the first character.
    fn decode(s: &str) -> Option<u128> {
        if s.len() != 26 {
            return None;
        }
        let mut value: u128 = 0;
        for b in s.bytes() {
            let digit = CROCKFORD.iter().position(|&c| c == b)?;
            value = (value << 5) | u128::try_from(digit).ok()?;
        }
        Some(value)
    }

    fn encode(timestamp_ms: u64, payload_80: u128) -> String {
        let value: u128 = (u128::from(timestamp_ms) << 80) | (payload_80 & ((1u128 << 80) - 1));
        let mut chars = [0u8; 26];
        let mut v = value;
        for slot in chars.iter_mut().rev() {
            *slot = CROCKFORD[(v & 0x1F) as usize];
            v >>= 5;
        }
        // Every byte comes from `CROCKFORD`, which is ASCII by construction, so this
        // can never fail to decode; built through `char::from(u8)` instead of
        // `String::from_utf8(..).expect(..)` so that invariant does not need an
        // unreachable panic to hold (CLAUDE.md: no unwrap/expect outside tests and main).
        chars.iter().map(|&b| char::from(b)).collect()
    }

    /// `mission time` (Unix seconds) as an RFC 3339 UTC instant, whole seconds. The
    /// inverse of the parser `gungnir-ingest`'s spotter adapter reads timestamps with
    /// (`unix_seconds`, itself built on Howard Hinnant's `days_from_civil`); this is
    /// `civil_from_days`, its standard companion, duplicated here rather than shared
    /// because sharing it would need an edge between two crates for fifteen lines of
    /// public-domain integer arithmetic.
    pub(super) fn rfc3339(unix_s: f64) -> String {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "mission time in any real or replayed session is far below i64::MAX \
                      seconds"
        )]
        let total_seconds = unix_s.floor() as i64;
        let days = total_seconds.div_euclid(86_400);
        let sod = total_seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let hour = sod / 3600;
        let minute = (sod % 3600) / 60;
        let second = sod % 60;
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// Howard Hinnant's `civil_from_days`: exact in integer arithmetic, no lookup table.
    fn civil_from_days(days: i64) -> (i64, i64, i64) {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { y + 1 } else { y };
        (year, month, day)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_encoder_reproduces_the_published_ulid_specs_own_example() {
            // Verified independently before writing this: decoding the spec's example
            // ULID 01ARZ3NDEKTSV4RRFFQ69G5FAV gives timestamp_ms = 1469922850259, and
            // re-encoding that same value round-trips to the same string. This test
            // pins the timestamp half of that round trip, which is the half this
            // module's algorithm is responsible for; the payload half is this
            // module's own scheme and has no external spec to check against.
            assert_eq!(encode(1_469_922_850_259, 0)[..10], *"01ARZ3NDEK");
        }

        #[test]
        fn every_character_is_in_the_crockford_alphabet_and_the_length_is_26() {
            let id = task_id(MissionTime(1_700_000_000.123), 42, 7);
            assert_eq!(id.len(), 26, "{id}");
            assert!(
                id.bytes().all(|b| CROCKFORD.contains(&b)),
                "a ULID must not contain I, L, O, or U: {id}"
            );
        }

        #[test]
        fn two_tasks_in_the_same_millisecond_get_different_ids() {
            let a = task_id(MissionTime(1_700_000_000.0), 1, 0);
            let b = task_id(MissionTime(1_700_000_000.0), 2, 0);
            assert_ne!(a, b, "different task ids must not collide");
            let c = task_id(MissionTime(1_700_000_000.0), 1, 0);
            let d = task_id(MissionTime(1_700_000_000.0), 1, 1);
            assert_ne!(c, d, "the sequence number must also separate them");
        }

        #[test]
        fn rfc3339_matches_a_hand_checked_date() {
            // 2016-07-30T23:54:10Z, the whole-second truncation of the timestamp the
            // ULID test above verifies (1469922850259 ms -> 1469922850.259 s), checked
            // independently against a standard calendar converter before writing this.
            assert_eq!(rfc3339(1_469_922_850.259), "2016-07-30T23:54:10Z");
        }

        #[test]
        fn rfc3339_pads_every_field_to_a_fixed_width() {
            // 2001-01-02T03:04:05Z = 978404645 unix seconds, computed independently
            // (Python's datetime) rather than by hand, and every field here is one a
            // naive `{}` format would render without its leading zero, which would
            // produce a string `unix_seconds` (the reader this is the inverse of)
            // rejects outright on length.
            assert_eq!(rfc3339(978_404_645.0), "2001-01-02T03:04:05Z");
        }

        #[test]
        fn decode_recovers_the_task_id_folded_into_a_freshly_minted_ulid() {
            let id = task_id(MissionTime(1_700_000_000.123), 424_242, 7);
            assert_eq!(decode_task_id(&id), Some(424_242));
        }

        #[test]
        fn decode_ignores_the_sequence_number_the_task_id_was_folded_alongside() {
            let a = task_id(MissionTime(1_700_000_000.0), 9, 0);
            let b = task_id(MissionTime(1_700_000_000.0), 9, 5);
            assert_eq!(decode_task_id(&a), Some(9));
            assert_eq!(decode_task_id(&b), Some(9));
        }

        #[test]
        fn decode_refuses_a_string_of_the_wrong_length() {
            assert_eq!(decode_task_id("TOOSHORT"), None);
        }

        #[test]
        fn decode_refuses_a_character_outside_the_crockford_alphabet() {
            // 'I', 'L', 'O', 'U' are deliberately excluded from Crockford base32.
            let mut id = task_id(MissionTime(1_700_000_000.0), 1, 0);
            id.replace_range(5..6, "U");
            assert_eq!(decode_task_id(&id), None);
        }

        #[test]
        fn the_published_ulid_specs_own_example_round_trips() {
            // The spec's example ULID, decoded and re-encoded, reproduces itself byte
            // for byte -- the companion check to `the_encoder_reproduces_the_published_
            // ulid_specs_own_example` above, which only pins the timestamp half.
            let example = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
            let value = decode(example).expect("a valid 26-character Crockford string");
            let timestamp_ms = (value >> 80) as u64;
            let payload = value & ((1u128 << 80) - 1);
            assert_eq!(encode(timestamp_ms, payload), example);
        }
    }
}

/// The free-text `request` a handoff is sent as (GAP-207, the owner's choice of
/// 2026-10-27).
///
/// BSI Flex 335 v2.0's `Task.Command` has no engage member: its `oneof` is `request`,
/// three thresholds, `mode_change`, `look_at`, `move_to`, `patrol` and `follow`, and the
/// effector node types (`NODE_TYPE_KINETIC`, `_JAMMER`, `_CYBER`, `_LDEW`, `_RFDEW`)
/// register without a command of their own. So the vocabulary is ours, documented here as
/// the mode names above are, and an effector has to agree on it out of band. `request` was
/// chosen over `look_at` because a node that does not know the word rejects the task,
/// where a pointable sensor handed `look_at` would point and accept.
pub const ENGAGE_REQUEST: &str = "engage";

/// A handoff as a SAPIENT `Task`, ready for the wire (GAP-207).
#[derive(Debug, Clone, PartialEq)]
pub struct EngageTask {
    /// The ULID `task_id` an effector's `TaskAck` will name.
    pub task_id: String,
    /// The whole `SapientMessage`, in the pinned protobuf-JSON mapping.
    pub message: serde_json::Value,
}

/// Build the `Task` that hands `handoff` to the SAPIENT effector at `destination_id`
/// (GAP-207; DN-07 §5).
///
/// * `command` is `{"request": "engage"}` ([`ENGAGE_REQUEST`]).
/// * One `REGION_TYPE_AREA_OF_INTEREST` per engaged target, each a one-point
///   `LocationList` at the solved intercept point -- the same single-point encoding a
///   [`SensorCommand::Cue`] uses, for the reason given there -- named for the track.
/// * `task_name` is `engage`, and `task_description` names the decision, the plan and who
///   decided, so the effector's own log can be matched to this deployment's record.
///
/// Only the solutions tasking one of `resources` -- the resources whose
/// `handoff_endpoint` is this effector -- are sent: a plan that tasks two effectors is two
/// handoffs in DN-07's shape, and this one has no business telling a jammer about a
/// missile's target.
///
/// **Deterministic.** The task id is minted from the decision and the issue time, so a
/// handoff posted again by store-and-forward carries the same `task_id`, and an effector
/// that already has it can say so rather than act twice.
///
/// # Errors
///
/// Why the handoff cannot be sent, in words for the record. A refusal here is final for
/// this handoff -- no retry would change it -- and DN-07 §5 case 3 then records it
/// undelivered and tells the operator to reach the effector another way:
///
/// * a fires task, which SAPIENT has no shape for;
/// * no solution tasking one of `resources`;
/// * a solution with no intercept point, because the task has to say where. A resource
///   with no `intercept_speed_mps` has no geometry solved (DN-04 §9), and that is what to
///   set.
pub fn engage_task(
    node_id: &str,
    destination_id: &str,
    handoff: &gungnir_model::handoff::Handoff,
    resources: &[gungnir_model::ResourceId],
) -> Result<EngageTask, String> {
    if handoff.kind.fires().is_some() {
        return Err(
            "a fires task has no SAPIENT representation: BSI Flex 335 v2.0 tasks a node, \
             and a fire mission is a request to a fires system"
                .into(),
        );
    }
    let solutions: Vec<_> = handoff
        .kind
        .solutions()
        .iter()
        .filter(|s| resources.contains(&s.resource))
        .collect();
    if solutions.is_empty() {
        return Err("the plan tasks no resource this effector serves".into());
    }
    let mut regions = Vec::new();
    for (index, s) in solutions.iter().enumerate() {
        let Some(point) = s.intercept_point else {
            return Err(format!(
                "no intercept point is solved for track {} and a SAPIENT task has to say \
                 where; give resource {} an intercept_speed_mps so its geometry is solved",
                s.track.0, s.resource.0
            ));
        };
        if ![point.lat_rad, point.lon_rad, point.alt_m]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(format!(
                "the intercept point for track {} is not finite",
                s.track.0
            ));
        }
        let sequence = u32::try_from(index + 1).unwrap_or(u32::MAX);
        regions.push(serde_json::json!({
            "type": "REGION_TYPE_AREA_OF_INTEREST",
            "regionId": ulid::task_id(handoff.issued, s.track.0, sequence),
            "regionName": format!("track {}", s.track.0),
            "regionArea": {
                "locationList": { "locations": [location_json(point)] },
            },
        }));
    }
    // The decision's low 64 bits: a UUID v7's random half, so two decisions issued in the
    // same millisecond still mint different ids.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the low 64 bits are taken on purpose; the mapping back is by table"
    )]
    let folded = handoff.decision.0 as u64;
    let task_id = ulid::task_id(handoff.issued, folded, 0);
    let issued_at = ulid::rfc3339(handoff.issued.0);
    let decided = &handoff.decided_by;
    let message = serde_json::json!({
        "timestamp": issued_at,
        "nodeId": node_id,
        "destinationId": destination_id,
        "task": {
            "taskId": task_id,
            "taskName": ENGAGE_REQUEST,
            "taskDescription": format!(
                "decision {} on plan {}, by {} ({})",
                handoff.decision, handoff.plan, decided.operator, decided.role
            ),
            "control": "CONTROL_START",
            "taskStartTime": issued_at,
            "region": regions,
            "command": { "request": ENGAGE_REQUEST },
        },
    });
    Ok(EngageTask { task_id, message })
}

/// The task a handoff to `endpoint` is sent as, when the baseline puts a SAPIENT effector
/// behind that endpoint (GAP-207); `None` when it does not, and the endpoint is sent the
/// handoff's own JSON as before.
///
/// Both binaries ask this from their handoff transport, so a desktop and a node build the
/// same task for the same baseline: the effector's row, this deployment's
/// `sapient_node_id`, and the resources whose `handoff_endpoint` is this one.
///
/// # Errors
///
/// As [`engage_task`], and a missing `sapient_node_id`, which validation already refuses.
#[must_use]
pub fn engage_payload(
    config: &gungnir_config::ConfigBaseline,
    endpoint: &str,
    handoff: &gungnir_model::handoff::Handoff,
) -> Option<Result<EngageTask, String>> {
    let row = config
        .sapient_effectors
        .iter()
        .find(|r| r.endpoint == endpoint)?;
    let Some(node_id) = config.sapient_node_id.as_deref() else {
        return Some(Err(
            "no sapient_node_id is configured for a task to come from".into(),
        ));
    };
    let resources: Vec<gungnir_model::ResourceId> = config
        .resources
        .iter()
        .filter(|r| r.handoff_endpoint.as_deref() == Some(endpoint))
        .map(|r| gungnir_model::ResourceId(r.id))
        .collect();
    Some(engage_task(
        node_id,
        &row.destination_id,
        handoff,
        &resources,
    ))
}

/// Recover the local task id [`SapientTaskAdapter::issue`] folded into a SAPIENT-shaped
/// ULID `taskId`, from a `TaskAck` a `gungnir-ingest` reader parsed off the wire.
/// `None` for a string that is not a validly-shaped 26-character Crockford ULID --
/// including one this adapter never issued, since a task id we did not mint is not this
/// crate's to translate, and `SensorControl::acknowledge`/`fail` would refuse an
/// unrecognised `SensorTaskId` anyway.
#[must_use]
pub fn decode_task_id(wire_task_id: &str) -> Option<crate::tasking::SensorTaskId> {
    ulid::decode_task_id(wire_task_id).map(crate::tasking::SensorTaskId)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasking::{SensorTask, SensorTaskId, TaskState};
    use crate::SensorId;
    use gungnir_model::{MissionTime, RequirementId};
    use std::sync::{Arc, Mutex as StdMutex};

    fn destinations() -> SapientDestinations {
        SapientDestinations::from_sensors([(4_u32, "b0f1c3e2-6f2a-4a4e-9b0e-1f2a3b4c5d6e".into())])
    }

    fn task(sensor: u32, command: SensorCommand) -> SensorTask {
        SensorTask {
            id: SensorTaskId(1),
            sensor: SensorId(sensor),
            command,
            requirement: None::<RequirementId>,
            issued: MissionTime(1_469_922_850.0),
            state: TaskState::Issued,
        }
    }

    fn adapter_capturing(
        sink: Arc<StdMutex<Vec<String>>>,
    ) -> SapientTaskAdapter<impl Fn(String) + Send + Sync> {
        SapientTaskAdapter::new(
            "a1b2c3d4-0000-0000-0000-000000000000",
            destinations(),
            move |s| {
                sink.lock().unwrap().push(s);
            },
        )
    }

    #[test]
    fn a_mode_change_reaches_the_sink_as_a_well_formed_sapient_task() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        adapter
            .issue(&task(
                4,
                SensorCommand::SetMode {
                    mode: SensorMode::Search,
                },
            ))
            .expect("a known sensor with a plain mode change is deliverable");

        let sent = sink.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&sent[0]).expect("valid JSON");
        assert_eq!(v["nodeId"], "a1b2c3d4-0000-0000-0000-000000000000");
        assert_eq!(v["destinationId"], "b0f1c3e2-6f2a-4a4e-9b0e-1f2a3b4c5d6e");
        assert_eq!(v["task"]["control"], "CONTROL_START");
        assert_eq!(v["task"]["command"]["modeChange"], "search");
        assert_eq!(v["timestamp"], "2016-07-30T23:54:10Z");
        assert_eq!(v["task"]["taskId"].as_str().unwrap().len(), 26);
    }

    #[test]
    fn a_cue_becomes_a_single_point_location_list() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        let target = Geodetic {
            lat_rad: 0.5,
            lon_rad: -1.0,
            alt_m: 120.0,
        };
        adapter
            .issue(&task(
                4,
                SensorCommand::Cue {
                    target,
                    dwell_s: Some(30.0),
                },
            ))
            .expect("a cue with a known destination is deliverable");

        let sent = sink.lock().unwrap();
        let v: serde_json::Value = serde_json::from_str(&sent[0]).expect("valid JSON");
        let locations = &v["task"]["command"]["lookAt"]["locationList"]["locations"];
        assert_eq!(
            locations.as_array().unwrap().len(),
            1,
            "one point, not a region"
        );
        let loc = &locations[0];
        assert!((loc["x"].as_f64().unwrap() - (-1.0)).abs() < 1e-12);
        assert!((loc["y"].as_f64().unwrap() - 0.5).abs() < 1e-12);
        assert!((loc["z"].as_f64().unwrap() - 120.0).abs() < 1e-12);
        assert_eq!(
            loc["coordinateSystem"],
            "LOCATION_COORDINATE_SYSTEM_LAT_LNG_RAD_M"
        );
        assert_eq!(loc["datum"], "LOCATION_DATUM_WGS84_E");
    }

    #[test]
    fn a_dwell_sets_a_task_end_time_after_the_task_start_time() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        let target = Geodetic {
            lat_rad: 0.0,
            lon_rad: 0.0,
            alt_m: 0.0,
        };
        adapter
            .issue(&task(
                4,
                SensorCommand::Cue {
                    target,
                    dwell_s: Some(30.0),
                },
            ))
            .expect("deliverable");
        let sent = sink.lock().unwrap();
        let v: serde_json::Value = serde_json::from_str(&sent[0]).expect("valid JSON");
        assert_eq!(v["task"]["taskStartTime"], "2016-07-30T23:54:10Z");
        assert_eq!(
            v["task"]["taskEndTime"], "2016-07-30T23:54:40Z",
            "thirty seconds after the start"
        );
    }

    #[test]
    fn a_cue_with_no_dwell_sets_no_end_time() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        let target = Geodetic {
            lat_rad: 0.0,
            lon_rad: 0.0,
            alt_m: 0.0,
        };
        adapter
            .issue(&task(
                4,
                SensorCommand::Cue {
                    target,
                    dwell_s: None,
                },
            ))
            .expect("deliverable");
        let sent = sink.lock().unwrap();
        let v: serde_json::Value = serde_json::from_str(&sent[0]).expect("valid JSON");
        assert!(
            v["task"].get("taskEndTime").is_none(),
            "no dwell means no end time, which the ICD reads as running until superseded: {v}"
        );
    }

    #[test]
    fn a_calibration_becomes_the_free_text_request() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        adapter
            .issue(&task(
                4,
                SensorCommand::Calibrate {
                    procedure: "boresight".into(),
                },
            ))
            .expect("deliverable");
        let sent = sink.lock().unwrap();
        let v: serde_json::Value = serde_json::from_str(&sent[0]).expect("valid JSON");
        assert_eq!(v["task"]["command"]["request"], "boresight");
    }

    #[test]
    fn a_search_command_is_refused_by_name_rather_than_guessed() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        let err = adapter
            .issue(&task(
                4,
                SensorCommand::Search {
                    area: gungnir_model::AssetExtent::Circle {
                        center: Geodetic {
                            lat_rad: 0.0,
                            lon_rad: 0.0,
                            alt_m: 0.0,
                        },
                        radius_m: 100.0,
                    },
                },
            ))
            .expect_err("SAPIENT tasking has no agreed area-search representation");
        match err {
            SensorManagementError::Refused { sensor, reason } => {
                assert_eq!(sensor, SensorId(4));
                assert!(
                    reason.contains("Region"),
                    "the reason must name what is missing: {reason}"
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(
            sink.lock().unwrap().is_empty(),
            "a refused task must not reach the sink"
        );
    }

    #[test]
    fn a_sensor_with_no_configured_destination_is_refused_by_name() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        let err = adapter
            .issue(&task(
                9,
                SensorCommand::SetMode {
                    mode: SensorMode::Track,
                },
            ))
            .expect_err("sensor 9 has no destination configured");
        match err {
            SensorManagementError::Refused { sensor, reason } => {
                assert_eq!(sensor, SensorId(9));
                assert!(reason.contains('9'), "{reason}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(sink.lock().unwrap().is_empty());
    }

    /// A handoff of plan 3, decided by `op-1`, engaging `tracks` with resource 5, each
    /// at the given intercept point.
    fn handoff(points: &[(u64, Option<Geodetic>)]) -> gungnir_model::handoff::Handoff {
        use gungnir_model::handoff::{DecisionAttribution, Handoff};
        use gungnir_model::{DecisionId, InterceptSolutionView, PlanId, PlanKind, ResourceId};
        Handoff::from_decision(
            DecisionId(0x0193_5c1d_7e00_7abc_9def_0123_4567_89ab),
            PlanId(3),
            PlanKind::Intercept {
                solutions: points
                    .iter()
                    .map(|(track, point)| InterceptSolutionView {
                        resource: ResourceId(5),
                        track: gungnir_model::TrackId(*track),
                        intercept_point: *point,
                        time_to_intercept_s: Some(12.0),
                    })
                    .collect(),
            },
            DecisionAttribution {
                operator: "op-1".into(),
                role: "Supervisor".into(),
                at: MissionTime(1_469_922_850.0),
                authority_rule: None,
            },
            Vec::new(),
            gungnir_model::Releasability::default(),
            MissionTime(1_469_922_850.0),
        )
    }

    const POINT: Geodetic = Geodetic {
        lat_rad: 0.9,
        lon_rad: 0.2,
        alt_m: 150.0,
    };

    /// GAP-207: a handoff becomes a request to engage, one area of interest per target at
    /// its intercept point, naming the decision, and the same task id every time it is
    /// built, so a retry is the same task.
    #[test]
    fn a_handoff_becomes_an_engage_request_at_each_intercept_point() {
        let h = handoff(&[(10, Some(POINT)), (11, Some(POINT))]);
        let resources = [gungnir_model::ResourceId(5)];
        let task = engage_task("our-node", "effector-node", &h, &resources).expect("built");
        let v = &task.message;
        assert_eq!(v["nodeId"], "our-node");
        assert_eq!(v["destinationId"], "effector-node");
        assert_eq!(v["task"]["command"]["request"], ENGAGE_REQUEST);
        assert_eq!(v["task"]["control"], "CONTROL_START");
        assert_eq!(v["task"]["taskId"], task.task_id.as_str());
        assert_eq!(task.task_id.len(), 26);
        let regions = v["task"]["region"].as_array().expect("regions");
        assert_eq!(regions.len(), 2, "one per target");
        assert_eq!(regions[0]["type"], "REGION_TYPE_AREA_OF_INTEREST");
        assert_eq!(regions[0]["regionName"], "track 10");
        assert_ne!(regions[0]["regionId"], regions[1]["regionId"]);
        let loc = &regions[0]["regionArea"]["locationList"]["locations"][0];
        assert!((loc["y"].as_f64().expect("lat") - 0.9).abs() < 1e-12);
        assert!((loc["x"].as_f64().expect("lon") - 0.2).abs() < 1e-12);
        assert!(v["task"]["taskDescription"]
            .as_str()
            .is_some_and(|d| d.contains(&h.decision.to_string()) && d.contains("op-1")));
        let again = engage_task("our-node", "effector-node", &h, &resources).expect("built");
        assert_eq!(again, task, "a retry is the same task");
    }

    /// GAP-207: what cannot be said is refused by name, never sent with a guess in it.
    #[test]
    fn a_handoff_with_no_point_or_no_resource_here_is_refused() {
        let resources = [gungnir_model::ResourceId(5)];
        let err = engage_task("n", "e", &handoff(&[(10, None)]), &resources).expect_err("no point");
        assert!(err.contains("intercept_speed_mps"), "{err}");
        let err = engage_task(
            "n",
            "e",
            &handoff(&[(10, Some(POINT))]),
            &[gungnir_model::ResourceId(9)],
        )
        .expect_err("not this effector's");
        assert!(err.contains("no resource this effector serves"), "{err}");
    }

    #[test]
    fn two_tasks_issued_in_the_same_call_get_distinct_task_ids() {
        let sink = Arc::new(StdMutex::new(Vec::new()));
        let adapter = adapter_capturing(sink.clone());
        adapter
            .issue(&task(
                4,
                SensorCommand::SetMode {
                    mode: SensorMode::Standby,
                },
            ))
            .expect("first");
        adapter
            .issue(&task(
                4,
                SensorCommand::SetMode {
                    mode: SensorMode::Track,
                },
            ))
            .expect("second");
        let sent = sink.lock().unwrap();
        let a: serde_json::Value = serde_json::from_str(&sent[0]).unwrap();
        let b: serde_json::Value = serde_json::from_str(&sent[1]).unwrap();
        assert_ne!(a["task"]["taskId"], b["task"]["taskId"]);
    }
}
