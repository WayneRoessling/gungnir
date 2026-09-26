// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The observation model: one scan of one sensor over one target, and one scan's false
//! alarms (docs/design/DN-32-re-observation-for-a-laydown.md §4).
//!
//! This is the block that sat inside `gungnir-scenario`'s `tracks.rs::run`, moved
//! statement for statement: the gates in the same order, the same draws from the same
//! [`PythonRandom`] in the same order, the same [`Num`] arithmetic. The generator now
//! calls it, and `gungnir-scenario/tests/reference_parity.rs` -- unchanged -- still
//! reproduces all ten committed sample sets byte for byte, which is what makes "the
//! re-observation model is the verified one" a fact rather than an intention.
//!
//! **Every [`Observation`] carries a [`SimulationMark`]** (DN-32 §6 mechanism 1). The
//! type's fields are private and this module is the only place one is built, so there is
//! no path in the workspace that produces an unmarked simulated observation.

use std::ops::{Add, Mul, Neg, Sub};
use std::sync::Arc;

use crate::model::SensorParams;
use crate::pynum::Num;
use crate::vec3::{add, degrees, norm, pymod, radar_horizon_m, scale, sub, unit, V3};
use crate::PythonRandom;

/// The recorded adapter's receipt window: receipt minus source stays under the gateway's
/// five seconds, and an electronic-attack skew counts against it (the reference
/// generator's `MAX_LATENCY_S` says why 4.9).
pub const MAX_LATENCY_S: f64 = 4.9;

/// Where a simulated observation came from. Carried by every [`Observation`], set by
/// the functions in this module and by nothing else.
///
/// `gungnir-app` turns it into `gungnir_model::RehearsalOrigin` on the detection's
/// provenance, which the live ingest gateway refuses (DN-32 §6 mechanism 2). This crate
/// sits beneath `gungnir-model` in the graph and may not name that type, so the mark is
/// its own and the conversion is total (DN-32 §12).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulationMark {
    /// The recording re-observed, by its test-track label (`TT-01`).
    pub scenario: String,
    /// What placed the sensors: a laydown's identifier, or the recording's own name for
    /// its own placement when the generator itself is the caller.
    pub placement: String,
    /// The seed every random stream of the run derives from.
    pub seed: u64,
}

/// What a scan is subject to beyond the sensor's own model: electronic attack and the
/// sea state (DN-32 §5.3). Resolved by the caller, which knows which windows are in
/// force; [`ScanContext::new`] folds it into the numbers the model reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanConditions {
    /// The clock skew applied to source times, seconds (lagging).
    pub skew_s: Num,
    pub dropout_multiplier: Num,
    pub fa_multiplier: Num,
    pub sea_state: i64,
}

impl ScanConditions {
    /// No electronic attack, at the given sea state.
    #[must_use]
    pub fn calm(sea_state: i64) -> Self {
        Self {
            skew_s: Num::Float(0.0),
            dropout_multiplier: Num::Float(1.0),
            fa_multiplier: Num::Float(1.0),
            sea_state,
        }
    }

    /// An `ea_skew` window is in force: the window's own skew, or the sensor's.
    #[must_use]
    pub fn with_skew(mut self, sensor: &SensorParams, stated: Option<Num>) -> Self {
        self.skew_s = stated.unwrap_or(sensor.ea.skew_s);
        self
    }

    /// An `ea_dropout` window is in force: the window's own multiplier, or the
    /// sensor's, and the sensor's false-alarm multiplier.
    #[must_use]
    pub fn with_dropout(mut self, sensor: &SensorParams, stated: Option<Num>) -> Self {
        self.dropout_multiplier = stated.unwrap_or(sensor.ea.dropout_multiplier);
        self.fa_multiplier = sensor.ea.fa_multiplier;
        self
    }
}

/// One scan: which sensor, when, and under what conditions.
#[derive(Debug, Clone)]
pub struct ScanContext {
    sensor: i64,
    scan_time_s: f64,
    skew: Num,
    dropout: Num,
    fa_multiplier: Num,
    mark: Arc<SimulationMark>,
}

impl ScanContext {
    #[must_use]
    pub fn new(
        sensor_id: i64,
        sensor: &SensorParams,
        scan_time_s: f64,
        conditions: ScanConditions,
        mark: Arc<SimulationMark>,
    ) -> Self {
        let sea = sensor
            .sea_state_dropout
            .get(&conditions.sea_state)
            .copied()
            .unwrap_or(Num::Float(0.0));
        let dropout =
            Num::Float(0.95).min2(sensor.dropout.mul(conditions.dropout_multiplier).add(sea));
        Self {
            sensor: sensor_id,
            scan_time_s,
            skew: conditions.skew_s,
            dropout,
            fa_multiplier: conditions.fa_multiplier,
            mark,
        }
    }

    #[must_use]
    pub fn scan_time_s(&self) -> f64 {
        self.scan_time_s
    }
}

/// A target's signature classes: which range band each kind of sensor reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Signature {
    pub rcs: Option<String>,
    pub ir: Option<String>,
    pub acoustic: Option<String>,
    /// The emission class a cooperative or passive sensor reads (`datalink`, `ais`,
    /// `none`, ...), already resolved from the platform and the entity's flags.
    pub emission: Option<String>,
}

/// A target as the recording has it at the scan.
// The reference keeps these flags apart, and so does the model: each gates a different
// thing, and folding them into a state enum would describe no state the target is in.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub struct TargetState<'a> {
    pub id: &'a str,
    pub position: V3,
    pub velocity: V3,
    pub alive: bool,
    pub occluded: bool,
    pub signature: &'a Signature,
    /// A decoy shows a large radar cross-section whatever its platform's is.
    pub decoy: bool,
    /// The chance an emission sensor misses an intermittent transponder, per scan.
    pub adsb_intermittent: Option<Num>,
    /// A spoofed AIS position's offset, east and north, metres.
    pub ais_spoof_offset_m: Option<&'a [Num]>,
    /// A platform held at zero altitude reports zero height whatever the noise.
    pub surface: bool,
    /// When an uncued sensor last detected this target, for a cued sensor's gate.
    pub last_seen_s: Option<f64>,
}

/// One simulated detection. Built only here, and always marked.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    sensor: i64,
    scan_time_s: f64,
    source_time: Num,
    receipt_time: Num,
    measurement: V3,
    truth: Option<String>,
    mark: Arc<SimulationMark>,
}

impl Observation {
    #[must_use]
    pub fn sensor(&self) -> i64 {
        self.sensor
    }

    /// When the sensor scanned, unskewed and unrounded.
    #[must_use]
    pub fn scan_time_s(&self) -> f64 {
        self.scan_time_s
    }

    /// Mission seconds the sensor reports, to the millisecond: the scan time less any
    /// electronic-attack skew.
    #[must_use]
    pub fn source_time(&self) -> Num {
        self.source_time
    }

    /// Mission seconds the system receives it, to the millisecond.
    #[must_use]
    pub fn receipt_time(&self) -> Num {
        self.receipt_time
    }

    /// ENU metres, noise, bias and any spoof offset applied, unrounded.
    #[must_use]
    pub fn measurement(&self) -> V3 {
        self.measurement
    }

    /// The target that caused it, or `None` for a false alarm. Association ground
    /// truth: never part of what a tracker is given.
    #[must_use]
    pub fn truth(&self) -> Option<&str> {
        self.truth.as_deref()
    }

    #[must_use]
    pub fn mark(&self) -> &SimulationMark {
        &self.mark
    }
}

/// One scan of one sensor over one target, as the recording has the target. `None` is
/// "not detected this scan", for any of the reasons the model has.
///
/// The gates run in the reference generator's order and only the last three draw, so a
/// target that fails an early gate consumes nothing from `rng`: signature band, range,
/// horizon, altitude window, field of regard, moving-only, cueing, then an intermittent
/// transponder (one draw), the detection (one draw), and for a detection the noise
/// (three), the latency jitter (one) and the out-of-order draw (one, and one more when
/// it fires).
#[must_use]
pub fn observe(
    sensor: &SensorParams,
    sensor_position: V3,
    scan: &ScanContext,
    target: &TargetState<'_>,
    rng: &mut PythonRandom,
) -> Option<Observation> {
    let st = scan.scan_time_s;
    if !target.alive || target.occluded {
        return None;
    }
    let key = sensor.signature_key.as_str();
    let class: Option<&str> = match key {
        "rcs" => {
            if target.decoy {
                Some("large")
            } else {
                target.signature.rcs.as_deref()
            }
        }
        "ir" => target.signature.ir.as_deref(),
        "acoustic" => target.signature.acoustic.as_deref(),
        _ => target.signature.emission.as_deref(),
    };
    let rmax = class
        .and_then(|c| sensor.range_m.get(c))
        .copied()
        .unwrap_or(Num::Int(0));
    if rmax.le(Num::Int(0)) {
        return None;
    }
    let rel = sub(&target.position, &sensor_position);
    let r = norm(&rel);
    if r > rmax.f() {
        return None;
    }
    if sensor.horizon && r > radar_horizon_m(sensor_position[2], target.position[2]) {
        return None;
    }
    let [lo, hi] = sensor.altitude_m;
    let z = target.position[2];
    if !(lo.sub(Num::Int(1)).le(z) && z.le(hi.add(Num::Int(1)))) {
        return None;
    }
    let [a, b] = sensor.field_of_regard_deg;
    let bearing = pymod(degrees(rel[0].f().atan2(rel[1].f())) + 360.0, 360.0);
    let inside = (a.f() <= bearing && bearing <= b.f())
        || (a.gt(b) && (bearing >= a.f() || bearing <= b.f()));
    // The reference compares exactly, and so does the port.
    #[allow(clippy::float_cmp)]
    let whole = a.f() == 0.0 && b.f() == 360.0;
    if !inside && !whole {
        return None;
    }
    if sensor.moving_only && norm(&target.velocity) < 1.0 {
        return None;
    }
    if sensor.cued && st - target.last_seen_s.unwrap_or(-1e9) > 10.0 {
        return None;
    }
    if let Some(chance) = target.adsb_intermittent {
        if chance.f() != 0.0 && key == "emission" && rng.random() < chance.f() {
            return None;
        }
    }
    let pd = sensor.pd_in_range.f() * (1.0 - scan.dropout.f());
    if rng.random() >= pd {
        return None;
    }
    let los = unit(&rel);
    let cross = unit(&[los[1].neg(), los[0], Num::Float(0.0)]);
    let nz = sensor.noise;
    let mut meas = add(
        &target.position,
        &scale(&los, rng.gauss(0.0, nz.range_m.f())),
    );
    meas = add(&meas, &scale(&cross, rng.gauss(0.0, nz.cross_m.f())));
    meas[2] = meas[2].add(Num::Float(rng.gauss(0.0, nz.height_m.f())));
    if let Some(bias) = sensor.bias_m {
        meas = add(&meas, &[bias.east, bias.north, Num::Float(0.0)]);
    }
    if let Some(off) = target.ais_spoof_offset_m {
        if !off.is_empty() && key == "emission" {
            let east = off.first().copied().unwrap_or(Num::Int(0));
            let north = off.get(1).copied().unwrap_or(Num::Int(0));
            meas = add(&meas, &[east, north, Num::Float(0.0)]);
        }
    }
    if target.surface {
        meas[2] = Num::Float(0.0);
    }
    let skew = scan.skew;
    let source = Num::Float(st).sub(skew);
    let mut latency = sensor.latency_s.mean.f() + rng.gauss(0.0, sensor.latency_s.jitter.f()).abs();
    if rng.random() < sensor.out_of_order.f() {
        latency += rng.uniform(0.5, 1.5);
    }
    let latency = Num::Float(latency).min2(Num::Float(MAX_LATENCY_S).sub(skew));
    Some(Observation {
        sensor: scan.sensor,
        scan_time_s: st,
        source_time: source.round(3),
        receipt_time: Num::Float(st).add(latency).round(3),
        measurement: meas,
        truth: Some(target.id.to_owned()),
        mark: Arc::clone(&scan.mark),
    })
}

/// One scan's false alarms: a Poisson count at the sensor's rate, each placed uniformly
/// in bearing and in range out to the sensor's longest band, about the sensor.
#[must_use]
pub fn false_alarms(
    sensor: &SensorParams,
    sensor_position: V3,
    scan: &ScanContext,
    rng: &mut PythonRandom,
) -> Vec<Observation> {
    let st = scan.scan_time_s;
    let skew = scan.skew;
    let lam = sensor.false_alarms_per_scan.mul(scan.fa_multiplier);
    let mut k = 0usize;
    if lam.gt(Num::Int(0)) {
        let l = (-lam.f()).exp();
        let mut pk = 1.0;
        loop {
            pk *= rng.random();
            if pk <= l {
                break;
            }
            k += 1;
        }
    }
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        let rmax = sensor
            .range_m
            .values()
            .copied()
            .fold(None, |acc: Option<Num>, v| match acc {
                None => Some(v),
                Some(m) => Some(m.max2(v)),
            })
            .unwrap_or(Num::Int(0));
        let ang = rng.uniform(0.0, 2.0 * std::f64::consts::PI);
        let r = rng.uniform(0.05, 1.0) * rmax.f();
        let [lo, hi] = sensor.altitude_m;
        let z = if hi.gt(Num::Int(0)) {
            rng.uniform(lo.f(), hi.min2(Num::Float(5000.0)).f())
        } else {
            0.0
        };
        let meas = [
            sensor_position[0].add(Num::Float(r * ang.cos())),
            sensor_position[1].add(Num::Float(r * ang.sin())),
            Num::Float(z),
        ];
        let latency = Num::Float(
            sensor.latency_s.mean.f() + rng.gauss(0.0, sensor.latency_s.jitter.f()).abs(),
        )
        .min2(Num::Float(MAX_LATENCY_S).sub(skew));
        out.push(Observation {
            sensor: scan.sensor,
            scan_time_s: st,
            source_time: Num::Float(st).sub(skew).round(3),
            receipt_time: Num::Float(st).add(latency).round(3),
            measurement: meas,
            truth: None,
            mark: Arc::clone(&scan.mark),
        });
    }
    out
}
