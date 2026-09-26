// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **Geometry moves detections** -- the third of
//! `docs/design/DN-32-re-observation-for-a-laydown.md` §10's rows, a Draft row of
//! `docs/verification-capability-table.md` §2.
//!
//! Method: a property test. Move a sensor so a target leaves its range band, then back
//! so it enters again. Criterion: detections of that target stop, then start, **and
//! nothing else changes**.
//!
//! "Nothing else" is checked three ways, each of which a single interleaved random stream
//! would break: the sensor that did not move produces exactly the same observations,
//! measurement for measurement; the moved sensor still detects the other target at
//! exactly the same scans; and it raises exactly as many false alarms at the same scan
//! times, its clutter having moved with it. That is D-74's common random numbers at work:
//! every draw is keyed by sensor and target, never by where anything stands.

use gungnir_sensor_sim::pynum::Num;
use gungnir_sensor_sim::{
    reobserve, EntityRecord, Observation, PlacedSensor, Recording, SensorEvents, SensorParams,
    Signature, TruthRecord,
};
use proptest::prelude::*;

const TICK_S: f64 = 1.0;
const DURATION_S: f64 = 60.0;

fn radar() -> SensorParams {
    serde_json::from_value(serde_json::json!({
        "signature_key": "rcs",
        "range_m": {"large": 50000, "small": 5000},
        "pd_in_range": 0.8,
        "update_period_s": 2.0,
        "noise": {"range_m": 20, "cross_m": 40, "height_m": 60},
        "field_of_regard_deg": [0, 360],
        "altitude_m": [0, 10000],
        "horizon": false,
        "latency_s": {"mean": 0.2, "jitter": 0.1},
        "dropout": 0.05,
        "out_of_order": 0.05,
        "false_alarms_per_scan": 0.5,
        "ea": {"skew_s": 0, "dropout_multiplier": 1.0, "fa_multiplier": 1.0}
    }))
    .expect("the test radar parses")
}

fn entity(id: &str, rcs: &str) -> EntityRecord {
    EntityRecord {
        id: id.to_owned(),
        spawn_s: 0.0,
        occluded_window_s: None,
        signature: Signature {
            rcs: Some(rcs.to_owned()),
            ..Signature::default()
        },
        decoy: false,
        adsb_intermittent: None,
        ais_spoof_offset_m: None,
        surface: false,
        destroyed_at_s: None,
    }
}

/// Two stationary targets for a minute: `A`, small, and `B`, large.
fn recording(seed: u64, a: [f64; 3], b: [f64; 3]) -> Recording {
    let mut truth = Vec::new();
    let mut t = 0.0f64;
    while t <= DURATION_S + 1e-9 {
        for (id, pos) in [("A", a), ("B", b)] {
            truth.push(TruthRecord {
                t: (t * 1000.0).round() / 1000.0,
                entity: id.to_owned(),
                pos: pos.map(Num::Float),
                vel: [Num::Float(0.0); 3],
                alive: true,
            });
        }
        t += TICK_S;
    }
    Recording {
        scenario: "TT-99".to_owned(),
        seed,
        duration_s: DURATION_S,
        tick_s: TICK_S,
        truth,
        entities: vec![entity("A", "small"), entity("B", "large")],
        environment: Vec::new(),
    }
}

fn run(rec: &Recording, moved: [f64; 3], fixed: [f64; 3]) -> Vec<Observation> {
    reobserve(
        rec,
        &[
            PlacedSensor {
                id: 1,
                model: radar(),
                position: moved,
            },
            PlacedSensor {
                id: 2,
                model: radar(),
                position: fixed,
            },
        ],
        SensorEvents::NotApplied,
        "candidate",
    )
    .expect("a well-formed recording re-observes")
    .observations
}

/// The scans, in order, at which `sensor` detected `target`.
fn detections_of(obs: &[Observation], sensor: i64, target: Option<&str>) -> Vec<f64> {
    obs.iter()
        .filter(|o| o.sensor() == sensor && o.truth() == target)
        .map(Observation::scan_time_s)
        .collect()
}

fn offset(origin: [f64; 3], range: f64, bearing: f64) -> [f64; 3] {
    [
        origin[0] + range * bearing.sin(),
        origin[1] + range * bearing.cos(),
        20.0,
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn moving_a_sensor_moves_only_the_detections_its_geometry_changes(
        seed in any::<u64>(),
        ax in -20_000.0f64..20_000.0,
        ay in -20_000.0f64..20_000.0,
        b_range in 0.0f64..8_000.0,
        b_bearing in 0.0f64..std::f64::consts::TAU,
        in_range in 100.0f64..4_000.0,
        out_range in 6_500.0f64..30_000.0,
        bearing in 0.0f64..std::f64::consts::TAU,
        fixed_range in 100.0f64..4_000.0,
        fixed_bearing in 0.0f64..std::f64::consts::TAU,
    ) {
        let a = [ax, ay, 300.0];
        let b = offset(a, b_range, b_bearing);
        let b = [b[0], b[1], 500.0];
        let rec = recording(seed, a, b);
        let inside = offset(a, in_range, bearing);
        let outside = offset(a, out_range, bearing);
        let fixed = offset(a, fixed_range, fixed_bearing);

        let before = run(&rec, inside, fixed);
        let away = run(&rec, outside, fixed);
        let back = run(&rec, inside, fixed);

        // A leaves the moved sensor's band, and comes back into it.
        prop_assert!(!detections_of(&before, 1, Some("A")).is_empty(), "A is seen from inside");
        prop_assert!(detections_of(&away, 1, Some("A")).is_empty(), "A is not seen from outside");
        prop_assert_eq!(
            detections_of(&back, 1, Some("A")),
            detections_of(&before, 1, Some("A")),
            "A is seen again, at the same scans"
        );
        // The same placement twice is the same run, measurement for measurement.
        prop_assert_eq!(&back, &before);

        // Nothing else changes. The sensor that stayed produces the same observations,
        // whole, whether or not the other one moved.
        let stayed = |obs: &[Observation]| -> Vec<Observation> {
            obs.iter().filter(|o| o.sensor() == 2).cloned().collect()
        };
        prop_assert_eq!(stayed(&away), stayed(&before));
        // The moved sensor still sees B -- inside its large band from every placement
        // here -- at exactly the same scans.
        prop_assert!(!detections_of(&before, 1, Some("B")).is_empty());
        prop_assert_eq!(detections_of(&away, 1, Some("B")), detections_of(&before, 1, Some("B")));
        // And raises the same false alarms at the same scans; only where they fall moved.
        prop_assert_eq!(detections_of(&away, 1, None), detections_of(&before, 1, None));
    }
}

/// The half of the property a proptest shrinks poorly: a target that *moves* through a
/// fixed sensor's band is detected only while it is inside, which is the same rule seen
/// from the other side.
#[test]
fn a_target_crossing_the_band_is_detected_only_inside_it() {
    let mut rec = recording(7, [0.0, 0.0, 300.0], [0.0, 0.0, 500.0]);
    // A flies east at 400 m/s from 12 km west: inside 5 km of the origin between
    // t = 17.5 s and t = 42.5 s.
    for r in &mut rec.truth {
        if r.entity == "A" {
            r.pos = [
                Num::Float(-12_000.0 + 400.0 * r.t),
                Num::Float(0.0),
                Num::Float(300.0),
            ];
            r.vel = [Num::Float(400.0), Num::Float(0.0), Num::Float(0.0)];
        }
    }
    let obs = run(&rec, [0.0, 0.0, 20.0], [90_000.0, 0.0, 20.0]);
    let seen = detections_of(&obs, 1, Some("A"));
    assert!(!seen.is_empty());
    for st in seen {
        // The scan saw the record of the first tick at or after it.
        let t = st.ceil();
        let x = -12_000.0 + 400.0 * t;
        assert!(
            x.hypot(280.0) <= 5_000.0,
            "A detected at scan {st} when the record it saw put it {x} m east"
        );
    }
}
