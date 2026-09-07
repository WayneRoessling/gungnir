// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The plan-07 test-track composition (GAP-016, GAP-046): `gen_tracks.py` ported.
//!
//! `docs/test-tracks/tools/gen_tracks.py` is the reference generator and the
//! specification: the same inputs and seed give byte-identical output, and this module
//! reproduces that output **byte for byte** for the four data files of a set
//! (`truth.jsonl`, `detections.jsonl`, `detections-truth.jsonl`, `events.jsonl`), which
//! `tests/reference_parity.rs` checks against every committed sample under
//! `testdata/tracks/samples/`. The two descriptors (`sensors.json`, `metadata.json`) are
//! reproduced in content and compared as values, because Python's `json.dump` writes a
//! mapping in its insertion order and this crate's typed sensor model does not keep one.
//!
//! Parity is the point, so the port keeps the reference's shape: the same draws from the
//! same [`PythonRandom`] in the same order, [`Num`] where a Python value may be an
//! integer, `repr(float)` for every float written, and the movement models transcribed
//! statement by statement rather than tidied. Where the reference is odd (a phase's
//! defaulted radius still draws a random number; an integer climb rate is written without
//! a decimal point) the port is odd in the same way, and says so where it matters.

// "CPython" is a proper noun and not an identifier; the lint would have it in backticks.
#![allow(clippy::doc_markdown)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::ops::{Add, Div, Mul, Neg, Sub};

use crate::library::{
    ClassProfile, EntitySpec, EventSpec, Key, PhaseSpec, Place, Platform, Range, Scalar,
    ScenarioSpec, SensorType, TrackLibrary,
};
use crate::pynum::{json_num, json_str, Num};
use crate::PythonRandom;

const GENERATOR: &str = "tt-gen 0.1.0";
const G: f64 = 9.806_65;
/// The recorded adapter's receipt window (see the reference for why 4.9).
const MAX_LATENCY_S: f64 = 4.9;

type V3 = [Num; 3];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TracksError {
    #[error("no scenario {0} in the library")]
    NoScenario(String),
    #[error("{scenario} has no sample block, and a sample set was asked for")]
    NoSample { scenario: String },
    #[error("{scenario} inherits from {base}, which is not in the library")]
    NoBase { scenario: String, base: String },
    #[error("{what} {name} is not in the library")]
    Unresolved { what: &'static str, name: String },
    #[error("sensor {sensor} override {key} is not one the generator applies")]
    UnknownOverride { sensor: i64, key: String },
    #[error("phase {phase} of {class} uses an unknown movement model {model}")]
    UnknownModel {
        class: String,
        phase: String,
        model: String,
    },
}

// ---------------------------------------------------------------------------------
// Vector helpers with the reference's numeric behaviour.

fn f3(a: [f64; 3]) -> V3 {
    [Num::Float(a[0]), Num::Float(a[1]), Num::Float(a[2])]
}

fn norm(v: &V3) -> f64 {
    let sum = v.iter().fold(Num::Int(0), |acc, x| acc.add(x.mul(*x)));
    sum.f().sqrt()
}

fn sub(a: &V3, b: &V3) -> V3 {
    [a[0].sub(b[0]), a[1].sub(b[1]), a[2].sub(b[2])]
}

fn add(a: &V3, b: &V3) -> V3 {
    [a[0].add(b[0]), a[1].add(b[1]), a[2].add(b[2])]
}

fn scale(a: &V3, k: f64) -> V3 {
    let k = Num::Float(k);
    [a[0].mul(k), a[1].mul(k), a[2].mul(k)]
}

fn unit(v: &V3) -> V3 {
    let n = norm(v);
    if n > 1e-9 {
        [v[0].div(n.into()), v[1].div(n.into()), v[2].div(n.into())]
    } else {
        f3([0.0, 0.0, 0.0])
    }
}

/// An entity index as the reference's integer.
fn index_num(i: usize) -> i64 {
    i64::try_from(i).unwrap_or(i64::MAX)
}

/// `math.degrees`: CPython divides by `pi / 180`, and the last bit differs from a
/// multiplication by `180 / pi`.
fn degrees(x: f64) -> f64 {
    x / (std::f64::consts::PI / 180.0)
}

/// Python's float `%` for a positive divisor.
fn pymod(x: f64, y: f64) -> f64 {
    let m = x % y;
    if m != 0.0 && ((y < 0.0) != (m < 0.0)) {
        m + y
    } else if m == 0.0 {
        0.0f64.copysign(y)
    } else {
        m
    }
}

fn draw(rng: &mut PythonRandom, r: Range) -> f64 {
    match r {
        Range::Span([a, b]) => rng.uniform(a.f(), b.f()),
        Range::Fixed(v) => v.f(),
    }
}

fn draw_or(rng: &mut PythonRandom, r: Option<Range>, default: [f64; 2]) -> f64 {
    match r {
        Some(r) => draw(rng, r),
        None => rng.uniform(default[0], default[1]),
    }
}

fn nums_eq(a: [Num; 2], b: [i64; 2]) -> bool {
    a[0].f() == Num::Int(b[0]).f() && a[1].f() == Num::Int(b[1]).f()
}

// ---------------------------------------------------------------------------------
// Entities.

#[derive(Debug, Clone)]
struct PhaseParams {
    duration: f64,
    speed: f64,
    alt: f64,
    radius: f64,
    weave: Num,
    stop_prob: Num,
    fire_stop: f64,
    glide_ratio: f64,
    apogee: f64,
}

#[derive(Debug, Clone, Default)]
struct Flags {
    at: Option<V3>,
    target: Option<V3>,
    adsb: Option<bool>,
    iff: Option<bool>,
    ais: Option<bool>,
    adsb_intermittent: Option<Num>,
    ais_spoof_offset_m: Option<Vec<Num>>,
    target_group: Option<String>,
    fired_at: Option<f64>,
}

// The reference keeps these five flags apart, and so does the port.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
struct Entity<'a> {
    id: String,
    spec: &'a EntitySpec,
    cls: &'a ClassProfile,
    platform: &'a Platform,
    side: String,
    route: Vec<V3>,
    spawn: f64,
    phases: Vec<&'a PhaseSpec>,
    phase_i: usize,
    phase_t: f64,
    pos: V3,
    vel: V3,
    heading: f64,
    alive: bool,
    spawned: bool,
    wp: usize,
    decoy: bool,
    no_terminal: bool,
    occluded_window: Option<[Num; 2]>,
    emitting: bool,
    flags: Flags,
    orbit_centre: Option<V3>,
    orbit_dir: i32,
    stopped_until: f64,
    fired: bool,
    /// The entity this one dashes at, by index.
    target: Option<usize>,
    speed: f64,
    p: PhaseParams,
}

impl<'a> Entity<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: String,
        spec: &'a EntitySpec,
        cls: &'a ClassProfile,
        platform: &'a Platform,
        route: Vec<V3>,
        spawn: f64,
        phases: Vec<&'a PhaseSpec>,
        rng: &mut PythonRandom,
        flags: Flags,
    ) -> Self {
        let pos =
            route
                .first()
                .copied()
                .or(flags.at)
                .unwrap_or([Num::Int(0), Num::Int(0), Num::Int(0)]);
        let orbit_dir = if rng.random() < 0.5 { 1 } else { -1 };
        let mut e = Self {
            id,
            spec,
            cls,
            platform,
            side: spec.side.clone(),
            route,
            spawn,
            phases,
            phase_i: 0,
            phase_t: 0.0,
            pos,
            vel: f3([0.0, 0.0, 0.0]),
            heading: 0.0,
            alive: true,
            spawned: false,
            wp: 1,
            decoy: spec.decoy,
            no_terminal: spec.no_terminal,
            occluded_window: spec.cover_gap_s,
            emitting: spec.emitting.unwrap_or(true),
            flags,
            orbit_centre: None,
            orbit_dir,
            stopped_until: -1.0,
            fired: false,
            target: None,
            speed: 0.0,
            p: PhaseParams {
                duration: 0.0,
                speed: 0.0,
                alt: 0.0,
                radius: 0.0,
                weave: Num::Float(0.0),
                stop_prob: Num::Float(0.0),
                fire_stop: 0.0,
                glide_ratio: 0.0,
                apogee: 0.0,
            },
        };
        e.enter_phase(rng);
        e
    }

    fn phase(&self) -> Option<&'a PhaseSpec> {
        self.phases.get(self.phase_i).copied()
    }

    fn enter_phase(&mut self, rng: &mut PythonRandom) {
        let Some(ph) = self.phase() else {
            self.alive = false;
            return;
        };
        self.phase_t = 0.0;
        // The reference draws every parameter, defaulted ones included.
        self.p = PhaseParams {
            duration: draw(rng, ph.duration_s),
            speed: draw(rng, ph.speed_mps),
            alt: draw(rng, ph.altitude_m),
            radius: draw_or(rng, ph.radius_m, [1000.0, 1000.0]),
            weave: ph.weave.unwrap_or(Num::Float(0.0)),
            stop_prob: ph.stop_prob.unwrap_or(Num::Float(0.0)),
            fire_stop: draw_or(rng, ph.fire_stop_s, [120.0, 120.0]),
            glide_ratio: draw_or(rng, ph.glide_ratio, [7.0, 7.0]),
            apogee: draw_or(rng, ph.apogee_m, [40000.0, 40000.0]),
        };
        self.speed = self.p.speed;
        if matches!(
            ph.model.as_str(),
            "orbit" | "hover" | "anchored" | "stationary" | "shoot-and-move"
        ) {
            self.orbit_centre = Some(self.pos);
        }
        if ph.model == "orbit" {
            if let Some(at) = self.flags.at {
                self.orbit_centre = Some(at);
                self.pos = add(&at, &f3([self.p.radius, 0.0, 0.0]));
            }
        }
    }

    fn next_phase(&mut self, rng: &mut PythonRandom) {
        self.phase_i += 1;
        self.enter_phase(rng);
    }

    fn is_occluded(&self, t: f64) -> bool {
        match self.occluded_window {
            Some([a, b]) => {
                let dt = t - self.spawn;
                a.f() <= dt && dt <= b.f()
            }
            None => false,
        }
    }

    fn envelope(&self, key: &str) -> [Num; 2] {
        self.cls
            .envelope
            .get(key)
            .copied()
            .unwrap_or([Num::Int(0), Num::Int(0)])
    }
}

/// Clamp a climb rate the way the reference does, keeping an integer bound an integer.
fn climb_rate(env_climb: [Num; 2], alt_target: f64, pos_z: Num, dt: f64) -> Num {
    let wanted = Num::Float(alt_target)
        .sub(pos_z)
        .div(Num::Float(dt.max(1.0)));
    env_climb[0].abs().neg().max2(env_climb[1].min2(wanted))
}

/// One entity's step. Free function because a dash may end another entity.
#[allow(clippy::too_many_lines)]
fn step(
    entities: &mut [Entity<'_>],
    i: usize,
    t: f64,
    dt: f64,
    rng: &mut PythonRandom,
) -> Result<(), TracksError> {
    let Some(ph) = entities[i].phase() else {
        return Ok(());
    };
    if !entities[i].alive {
        return Ok(());
    }
    let model = ph.model.as_str();
    let env_speed = entities[i].envelope("speed_mps");
    let env_climb = entities[i].envelope("climb_mps");
    let env_alt = entities[i].envelope("altitude_m");
    let turn_g = entities[i].envelope("turn_g");
    let max_turn = turn_g[0].max2(turn_g[1]).f() * G / entities[i].speed.max(1.0);
    let alt_is_ground = nums_eq(env_alt, [0, 0]);
    match model {
        "waypoint-cruise" | "nap-of-earth" | "road-move" | "sea-transit" | "evade" => {
            let e = &mut entities[i];
            if e.wp >= e.route.len() {
                if e.no_terminal {
                    e.pos = add(&e.pos, &scale(&e.vel, dt));
                    e.phase_t += dt;
                    if e.phase_t > e.p.duration {
                        e.alive = false;
                    }
                    return Ok(());
                }
                e.next_phase(rng);
                if e.phase().is_none() {
                    e.alive = false;
                }
                return Ok(());
            }
            if model == "road-move"
                && t >= e.stopped_until
                && rng.random() < e.p.stop_prob.f() * dt / 60.0
            {
                e.stopped_until = t + rng.uniform(30.0, 180.0);
            }
            if t < e.stopped_until {
                e.vel = f3([0.0, 0.0, 0.0]);
                e.phase_t += dt;
                return Ok(());
            }
            let mut target = e.route[e.wp];
            let mut to = sub(&target, &e.pos);
            to[2] = Num::Float(0.0);
            let dist = norm(&to);
            if dist < (e.speed * dt * 1.5).max(30.0) {
                e.wp += 1;
                if e.wp >= e.route.len() {
                    return Ok(());
                }
                target = e.route[e.wp];
                to = sub(&target, &e.pos);
                to[2] = Num::Float(0.0);
            }
            let mut desired = to[1].f().atan2(to[0].f());
            if model == "evade" {
                desired = e.heading + max_turn * dt * f64::from(e.orbit_dir) * 3.0;
            }
            let mut delta = pymod(
                desired - e.heading + std::f64::consts::PI,
                2.0 * std::f64::consts::PI,
            ) - std::f64::consts::PI;
            delta = (-max_turn * dt).max(delta.min(max_turn * dt));
            e.heading += delta;
            if e.p.weave.f() != 0.0 {
                e.heading += rng.gauss(0.0, e.p.weave.f()) * dt;
            }
            let jitter = if matches!(model, "road-move" | "sea-transit") {
                rng.gauss(0.0, 0.03)
            } else {
                0.0
            };
            let spd = Num::Float(e.speed * (1.0 + jitter));
            let spd = spd.max2(env_speed[0]).min2(env_speed[1]);
            e.vel = [
                spd.mul(Num::Float(e.heading.cos())),
                spd.mul(Num::Float(e.heading.sin())),
                Num::Float(0.0),
            ];
            let alt_target = e.p.alt
                + if model == "nap-of-earth" {
                    rng.gauss(0.0, 8.0)
                } else {
                    0.0
                };
            let mut climb = climb_rate(env_climb, alt_target, e.pos[2], dt);
            if alt_is_ground {
                climb = Num::Float(0.0);
            }
            e.vel[2] = climb;
            e.pos = add(&e.pos, &scale(&e.vel, dt));
            e.pos[2] = Num::Float(0.0).max2(e.pos[2]);
            e.phase_t += dt;
            // Terminal trigger for classes that end on the target.
            let last_is_dash = e.phases.last().is_some_and(|p| p.model == "dash-terminal");
            if e.phase_i + 1 < e.phases.len() && last_is_dash && !e.no_terminal {
                if let Some(fin) = e.route.last().copied() {
                    let d = norm(&sub(
                        &[fin[0], fin[1], Num::Float(0.0)],
                        &[e.pos[0], e.pos[1], Num::Float(0.0)],
                    ));
                    if d < 3000.0 {
                        e.phase_i = e.phases.len() - 1;
                        e.enter_phase(rng);
                    }
                }
            }
        }
        "orbit" => {
            let e = &mut entities[i];
            let c = e.orbit_centre.unwrap_or(e.pos);
            let r = e.p.radius.max(50.0);
            let omega = f64::from(e.orbit_dir) * e.speed / r;
            let ang = e.pos[1].sub(c[1]).f().atan2(e.pos[0].sub(c[0]).f()) + omega * dt;
            let mut new = [
                c[0].add(Num::Float(r * ang.cos())),
                c[1].add(Num::Float(r * ang.sin())),
                e.pos[2],
            ];
            let alt_target = if alt_is_ground { 0.0 } else { e.p.alt };
            new[2] =
                e.pos[2].add(climb_rate(env_climb, alt_target, e.pos[2], dt).mul(Num::Float(dt)));
            e.vel = scale(&sub(&new, &e.pos), 1.0 / dt);
            e.heading = e.vel[1].f().atan2(e.vel[0].f());
            e.pos = new;
            e.phase_t += dt;
            if e.phase_t > e.p.duration {
                e.next_phase(rng);
            }
        }
        "hover" | "anchored" | "stationary" => {
            let e = &mut entities[i];
            let drift = if model == "stationary" { 0.0 } else { 0.3 };
            e.vel = [
                Num::Float(rng.gauss(0.0, drift)),
                Num::Float(rng.gauss(0.0, drift)),
                Num::Float(0.0),
            ];
            e.pos = add(&e.pos, &scale(&e.vel, dt));
            if !alt_is_ground && model == "hover" {
                let k = (dt / 10.0).min(1.0);
                e.pos[2] = e.pos[2].add(Num::Float(e.p.alt).sub(e.pos[2]).mul(Num::Float(k)));
            }
            e.phase_t += dt;
            if e.phase_t > e.p.duration {
                e.next_phase(rng);
            }
        }
        "shoot-and-move" => {
            let e = &mut entities[i];
            e.vel = f3([0.0, 0.0, 0.0]);
            e.phase_t += dt;
            if !e.fired && e.phase_t >= e.p.fire_stop * 0.5 {
                e.fired = true;
                e.flags.fired_at = Some(t);
            }
            if e.phase_t > e.p.fire_stop {
                let ang = rng.uniform(0.0, 2.0 * std::f64::consts::PI);
                let dist = rng.uniform(800.0, 2000.0);
                e.route = vec![
                    e.pos,
                    [
                        e.pos[0].add(Num::Float(dist * ang.cos())),
                        e.pos[1].add(Num::Float(dist * ang.sin())),
                        Num::Float(0.0),
                    ],
                ];
                e.wp = 1;
                e.next_phase(rng);
            }
        }
        "dash-terminal" => {
            let target = entities[i].target.filter(|&j| entities[j].alive);
            let tgt = if let Some(j) = target {
                entities[j].pos
            } else {
                let e = &entities[i];
                e.flags
                    .target
                    .or_else(|| e.route.last().copied())
                    .unwrap_or(e.pos)
            };
            let e = &entities[i];
            let to = sub(&tgt, &e.pos);
            let dist = norm(&to);
            let dash = e.speed.max(e.p.speed);
            if dist <= dash * dt {
                let e = &mut entities[i];
                e.pos = tgt;
                e.alive = false;
                if let Some(j) = target {
                    entities[j].alive = false;
                }
                return Ok(());
            }
            let e = &mut entities[i];
            e.vel = scale(&unit(&to), dash);
            e.heading = e.vel[1].f().atan2(e.vel[0].f());
            e.pos = add(&e.pos, &scale(&e.vel, dt));
            e.phase_t += dt;
        }
        "glide" => {
            let e = &mut entities[i];
            let fin = e.route.last().copied().unwrap_or(e.pos);
            let to = sub(&fin, &e.pos);
            let horiz = norm(&[to[0], to[1], Num::Float(0.0)]);
            if horiz <= e.speed * dt || e.pos[2].f() <= 0.0 {
                e.alive = false;
                return Ok(());
            }
            e.vel = scale(&unit(&[to[0], to[1], Num::Float(0.0)]), e.speed);
            e.vel[2] = Num::Float(-e.speed / e.p.glide_ratio);
            e.pos = add(&e.pos, &scale(&e.vel, dt));
            e.phase_t += dt;
        }
        "ballistic" => {
            let e = &mut entities[i];
            let fin = e.route.last().copied().unwrap_or(e.pos);
            let start = e.route.first().copied().unwrap_or(e.pos);
            let frac = (e.phase_t / e.p.duration).min(1.0);
            let x = [
                start[0].add(fin[0].sub(start[0]).mul(Num::Float(frac))),
                start[1].add(fin[1].sub(start[1]).mul(Num::Float(frac))),
                Num::Float(4.0 * e.p.apogee * frac * (1.0 - frac)),
            ];
            e.vel = scale(&sub(&x, &e.pos), 1.0 / dt);
            e.pos = x;
            e.phase_t += dt;
            if frac >= 1.0 {
                e.alive = false;
            }
        }
        other => {
            return Err(TracksError::UnknownModel {
                class: entities[i].cls.id.clone(),
                phase: ph.name.clone(),
                model: other.to_string(),
            })
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Building a scenario.

/// A sensor instance as the run sees it.
#[derive(Debug, Clone)]
struct SensorInst<'a> {
    id: i64,
    kind: &'a SensorType,
    name: String,
    pos: V3,
    calibration: Option<String>,
    phase: f64,
    /// `bias_m: {east, north}` from an override.
    bias: Option<(Num, Num)>,
}

struct Built<'a> {
    scenario: &'a ScenarioSpec,
    duration: Num,
    seed: u64,
    tick: f64,
    rng: PythonRandom,
    sensors: Vec<SensorInst<'a>>,
    entities: Vec<Entity<'a>>,
    events: Vec<&'a EventSpec>,
    variant: Option<String>,
    full: bool,
}

fn resolve_point(lib: &TrackLibrary, p: &Place) -> Result<V3, TracksError> {
    match p {
        Place::Named(name) => {
            lib.scenarios
                .points
                .get(name)
                .copied()
                .ok_or_else(|| TracksError::Unresolved {
                    what: "point",
                    name: name.clone(),
                })
        }
        // The reference converts a literal to floats and leaves a named point as it was.
        Place::Enu(v) => Ok(f3([v[0].f(), v[1].f(), v[2].f()])),
    }
}

fn resolve_named(lib: &TrackLibrary, name: &str) -> Result<V3, TracksError> {
    resolve_point(lib, &Place::Named(name.to_string()))
}

fn jitter_route(rng: &mut PythonRandom, route: &[V3], lateral: f64) -> Vec<V3> {
    let last = route.len().saturating_sub(1);
    route
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if i == 0 || i == last {
                *w
            } else {
                [
                    w[0].add(Num::Float(rng.uniform(-lateral, lateral))),
                    w[1].add(Num::Float(rng.uniform(-lateral, lateral))),
                    w[2],
                ]
            }
        })
        .collect()
}

fn bias_from(
    overrides: &crate::library::OrderedMap,
    sensor: i64,
) -> Result<Option<(Num, Num)>, TracksError> {
    let mut bias = None;
    for (key, value) in overrides.iter() {
        match key {
            Key::Text(k) if k == "bias_m" => {
                let east = value
                    .get("east")
                    .and_then(Scalar::as_num)
                    .unwrap_or(Num::Float(0.0));
                let north = value
                    .get("north")
                    .and_then(Scalar::as_num)
                    .unwrap_or(Num::Float(0.0));
                bias = Some((east, north));
            }
            other => {
                return Err(TracksError::UnknownOverride {
                    sensor,
                    key: other.as_text(),
                })
            }
        }
    }
    Ok(bias)
}

#[allow(clippy::too_many_lines)]
fn build_scenario<'a>(
    lib: &'a TrackLibrary,
    scen: &'a ScenarioSpec,
    full: bool,
    seed_override: Option<u64>,
    variant: Option<&str>,
) -> Result<Built<'a>, TracksError> {
    let routes: BTreeMap<&str, Vec<V3>> = lib
        .scenarios
        .routes
        .iter()
        .map(|(k, v)| {
            v.iter()
                .map(|w| resolve_point(lib, w))
                .collect::<Result<Vec<_>, _>>()
                .map(|r| (k.as_str(), r))
        })
        .collect::<Result<_, _>>()?;
    let ents_spec: &[EntitySpec] = match &scen.entities {
        crate::library::Entities::Lines(lines) => lines,
        crate::library::Entities::Inherit(_) => {
            let base = scen.base.clone().unwrap_or_default();
            lib.scenarios
                .scenarios
                .iter()
                .find(|s| s.id == base)
                .map(|s| s.entities.lines())
                .ok_or_else(|| TracksError::NoBase {
                    scenario: scen.id.clone(),
                    base: base.clone(),
                })?
        }
    };
    let sample = scen.sample.as_ref();
    let need_sample = || {
        sample.ok_or_else(|| TracksError::NoSample {
            scenario: scen.id.clone(),
        })
    };
    let duration = if full {
        scen.duration_s
    } else {
        need_sample()?.duration_s
    };
    let scale_n = if full {
        Num::Float(1.0)
    } else {
        need_sample()?.entity_scale
    };
    let seed = match seed_override {
        Some(s) => s,
        None if full => need_sample()?.seed + 1000,
        None => need_sample()?.seed,
    };
    let sensor_sets: &[String] = if full {
        &scen.sensors
    } else {
        need_sample()?.sensors.as_deref().unwrap_or(&scen.sensors)
    };
    let events: Vec<&EventSpec> = if full {
        scen.events.iter().collect()
    } else {
        need_sample()?
            .events
            .as_ref()
            .map_or_else(|| scen.events.iter().collect(), |e| e.iter().collect())
    };
    let tick = if full {
        lib.scenarios.truth_tick_s.f()
    } else {
        2.0
    };
    let mut rng = PythonRandom::new(u128::from(seed));

    // Sensors.
    let mut sensors = Vec::new();
    for sname in sensor_sets {
        let set = lib
            .scenarios
            .sensor_sets
            .get(sname)
            .ok_or_else(|| TracksError::Unresolved {
                what: "sensor set",
                name: sname.clone(),
            })?;
        for s in set {
            let kind = lib
                .sensors
                .types
                .iter()
                .find(|t| t.id == s.kind)
                .ok_or_else(|| TracksError::Unresolved {
                    what: "sensor type",
                    name: s.kind.clone(),
                })?;
            let pos = resolve_point(lib, &s.pos)?;
            let phase = rng.uniform(0.0, kind.update_period_s.f());
            let bias = bias_from(&s.overrides, s.id)?;
            sensors.push(SensorInst {
                id: s.id,
                kind,
                name: s.name.clone(),
                pos,
                calibration: s.calibration.clone(),
                phase,
                bias,
            });
        }
    }
    let var: Option<String> = match variant {
        Some(v) => Some(v.to_string()),
        None if full => None,
        None => sample.and_then(|s| s.variant.clone()),
    };
    if let Some(var) = &var {
        if let Some(v) = scen.variants.iter().find(|x| &x.id == var) {
            for mv in &v.moves {
                let pos = resolve_point(lib, &mv.pos)?;
                for inst in &mut sensors {
                    if inst.id == mv.sensor {
                        inst.pos = pos;
                    }
                }
            }
        }
    }

    // Entities.
    let mut entities = Vec::new();
    let spawn_cap = duration.mul(Num::Float(0.7));
    let scen_tag = scen.id.replace('-', "");
    for spec in ents_spec {
        let cls = lib
            .classes
            .classes
            .iter()
            .find(|c| c.id == spec.class)
            .ok_or_else(|| TracksError::Unresolved {
                what: "class",
                name: spec.class.clone(),
            })?;
        let plat = lib
            .platforms
            .iter()
            .find(|p| p.id == spec.platform)
            .ok_or_else(|| TracksError::Unresolved {
                what: "platform",
                name: spec.platform.clone(),
            })?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (spec.count.mul(scale_n).f().round_ties_even() as i64).max(1) as usize;
        let phases: Vec<&PhaseSpec> = match &spec.phases {
            Some(names) => cls
                .phases
                .iter()
                .filter(|ph| names.contains(&ph.name))
                .collect(),
            None => cls.phases.iter().collect(),
        };
        let lateral = match cls.domain.as_str() {
            "air" => 2000.0,
            "sea" => 300.0,
            _ => 20.0,
        };
        for i in 0..count {
            let eid = format!(
                "{scen_tag}-{}-{:03}",
                spec.group.as_deref().unwrap_or(""),
                i + 1
            );
            let [spawn_lo, spawn_hi] = spec.spawn_s;
            let spawn = if spawn_hi.gt(Num::Int(0)) {
                rng.uniform(spawn_lo.min2(spawn_cap).f(), spawn_hi.min2(spawn_cap).f())
            } else {
                0.0
            };
            let mut route: Vec<V3> = Vec::new();
            if let Some(name) = spec.route.as_deref().filter(|r| *r != "none") {
                let mut r = routes
                    .get(name)
                    .cloned()
                    .ok_or_else(|| TracksError::Unresolved {
                        what: "route",
                        name: name.to_string(),
                    })?;
                if spec.reverse {
                    r.reverse();
                }
                r = jitter_route(&mut rng, &r, lateral);
                if let Some(spacing) = spec.spacing_m {
                    let off = Num::Int(index_num(i)).f() * spacing.f();
                    r = r
                        .iter()
                        .map(|w| {
                            [
                                w[0].sub(Num::Float(off * 0.7)),
                                w[1].sub(Num::Float(off * 0.7)),
                                w[2],
                            ]
                        })
                        .collect();
                }
                route = r;
            }
            let mut flags = Flags::default();
            if let Some(at) = &spec.at {
                let mut at = resolve_point(lib, at)?;
                if let Some(spacing) = spec.spacing_m {
                    at = [at[0].add(Num::Int(index_num(i)).mul(spacing)), at[1], at[2]];
                }
                flags.at = Some(at);
            }
            if let Some(target) = &spec.target {
                flags.target = Some(resolve_named(lib, target)?);
            }
            if let Some(launch) = &spec.launch {
                route = vec![resolve_named(lib, launch)?];
            }
            flags.adsb = spec.adsb;
            flags.iff = spec.iff;
            flags.ais = spec.ais;
            flags.adsb_intermittent = spec.adsb_intermittent;
            flags
                .ais_spoof_offset_m
                .clone_from(&spec.ais_spoof_offset_m);
            flags.target_group.clone_from(&spec.target_group);
            entities.push(Entity::new(
                eid,
                spec,
                cls,
                plat,
                route,
                spawn,
                phases.clone(),
                &mut rng,
                flags,
            ));
        }
    }
    Ok(Built {
        scenario: scen,
        duration,
        seed,
        tick,
        rng,
        sensors,
        entities,
        events,
        variant: var,
        full,
    })
}

// ---------------------------------------------------------------------------------
// The observation model.

fn emission_key(plat: &Platform, e: &Entity<'_>, lib: &TrackLibrary) -> String {
    let em = plat.emissions.as_deref().unwrap_or("");
    if e.flags.adsb == Some(true) {
        return "adsb".into();
    }
    if e.flags.iff == Some(true) {
        return "iff".into();
    }
    if e.flags.ais == Some(false) {
        return "none".into();
    }
    if em.contains("AIS") {
        return "ais".into();
    }
    if em.contains("blue-force tracking") && e.side == "blue" {
        return "bft".into();
    }
    if !e.emitting || em.starts_with("none") {
        return "none".into();
    }
    for (key, names) in &lib.sensors.emission_map {
        if names.iter().any(|n| n == em) {
            return key.clone();
        }
    }
    if em.contains("datalink") || em.contains("control") {
        return "datalink".into();
    }
    if em.contains("radar") {
        return "radar".into();
    }
    "none".into()
}

fn radar_horizon_m(h1: Num, h2: Num) -> f64 {
    4120.0 * (h1.max2(Num::Float(0.0)).f().sqrt() + h2.max2(Num::Float(0.0)).f().sqrt())
}

/// One detection as the reference writes it.
#[derive(Debug, Clone)]
struct DetectionLine {
    sensor: i64,
    source_time: Num,
    receipt_time: Num,
    measurement: V3,
    calibration: Option<String>,
    truth: Option<String>,
}

/// What one scenario run produced, as text ready to write.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedSet {
    pub scenario: String,
    pub variant: String,
    pub seed: u64,
    pub truth: String,
    pub detections: String,
    pub detections_truth: String,
    pub events: String,
    pub sensors_json: serde_json::Value,
    pub metadata_json: serde_json::Value,
    pub counts: Counts,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Counts {
    pub entities: usize,
    pub truth_records: usize,
    pub detections: usize,
    pub false_alarms: usize,
    pub sensors: usize,
}

struct ActiveEa<'a> {
    event: &'a EventSpec,
    until: Num,
}

fn write_truth(out: &mut String, t: f64, e: &Entity<'_>) {
    let phase = e.phase().map_or("ended", |p| p.name.as_str());
    let _ = writeln!(
        out,
        "{{\"t\":{},\"entity\":{},\"class\":{},\"platform\":{},\"side\":{},\"phase\":{},\"pos\":[{},{},{}],\"vel\":[{},{},{}],\"alive\":{},\"occluded\":{}}}",
        json_num(Num::Float(t).round(3)),
        json_str(&e.id),
        json_str(&e.cls.id),
        json_str(&e.platform.id),
        json_str(&e.side),
        json_str(phase),
        json_num(e.pos[0].round(1)),
        json_num(e.pos[1].round(1)),
        json_num(e.pos[2].round(1)),
        json_num(e.vel[0].round(2)),
        json_num(e.vel[1].round(2)),
        json_num(e.vel[2].round(2)),
        e.alive,
        e.is_occluded(t)
    );
}

fn write_detection(out: &mut String, d: &DetectionLine) {
    let calibration = d
        .calibration
        .as_deref()
        .map_or_else(|| "null".to_string(), json_str);
    let _ = writeln!(
        out,
        // `measurement` is the tagged `gungnir_model::Measurement`
        // (docs/design/DN-27-bearing-only-detections.md §4). A generated set is a
        // position feed and stays one; the variance is the tracking baseline's own
        // default, stated because a position with no error is one the gate downstream
        // has to guess for. `gungnir_model::SCHEMA_VERSION` went 2 to 3 in the same
        // change, and docs/test-tracks/data-format.md §9 stamps the sets with it.
        "{{\"sensor\":{},\"source_time\":{},\"receipt_time\":{},\"measurement\":{{\"Position\":{{\"enu\":[{},{},{}],\"variance_m2\":[400.0,400.0,900.0]}}}},\"provenance\":{{\"source_sensor_ids\":[{}],\"calibration_baseline_version\":{},\"algorithm_version\":{}}}}}",
        d.sensor,
        json_num(d.source_time),
        json_num(d.receipt_time),
        json_num(d.measurement[0].round(2)),
        json_num(d.measurement[1].round(2)),
        json_num(d.measurement[2].round(2)),
        d.sensor,
        calibration,
        json_str(GENERATOR)
    );
}

fn scalar_json(s: &Scalar) -> String {
    match s {
        Scalar::Null => "null".into(),
        Scalar::Bool(b) => b.to_string(),
        Scalar::Integer(i) => i.to_string(),
        Scalar::Number(x) => json_num(Num::Float(*x)),
        Scalar::Text(t) => json_str(t),
        Scalar::List(l) => format!(
            "[{}]",
            l.iter().map(scalar_json).collect::<Vec<_>>().join(",")
        ),
        Scalar::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}:{}", json_str(&k.as_text()), scalar_json(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn scalar_value(s: &Scalar) -> serde_json::Value {
    match s {
        Scalar::Null => serde_json::Value::Null,
        Scalar::Bool(b) => serde_json::Value::Bool(*b),
        Scalar::Integer(i) => serde_json::Value::from(*i),
        Scalar::Number(x) => serde_json::Value::from(*x),
        Scalar::Text(t) => serde_json::Value::String(t.clone()),
        Scalar::List(l) => serde_json::Value::Array(l.iter().map(scalar_value).collect()),
        Scalar::Map(m) => serde_json::Value::Object(
            m.iter()
                .map(|(k, v)| (k.as_text(), scalar_value(v)))
                .collect(),
        ),
    }
}

fn num_value(n: Num) -> serde_json::Value {
    match n {
        Num::Int(i) => serde_json::Value::from(i),
        Num::Float(x) => serde_json::Value::from(x),
    }
}

/// An event line: the YAML mapping in its own order, or a generated `fires` event.
enum EventLine<'a> {
    Spec(&'a EventSpec),
    Fires { t: f64, entity: String },
}

impl EventLine<'_> {
    fn t(&self) -> f64 {
        match self {
            EventLine::Spec(e) => e.t().map_or(0.0, Num::f),
            EventLine::Fires { t, .. } => *t,
        }
    }

    fn json(&self) -> String {
        match self {
            EventLine::Spec(e) => scalar_json(&Scalar::Map(e.0.clone())),
            EventLine::Fires { t, entity } => format!(
                "{{\"t\":{},\"kind\":\"fires\",\"entity\":{}}}",
                json_num(Num::Float(*t)),
                json_str(entity)
            ),
        }
    }
}

fn sensors_value(sensors: &[SensorInst<'_>]) -> serde_json::Value {
    let list: Vec<serde_json::Value> = sensors
        .iter()
        .map(|s| {
            let k = s.kind;
            let mut params = serde_json::Map::new();
            params.insert("signature_key".into(), k.signature_key.clone().into());
            params.insert(
                "range_m".into(),
                serde_json::Value::Object(
                    k.range_m
                        .iter()
                        .map(|(c, v)| (c.clone(), num_value(*v)))
                        .collect(),
                ),
            );
            params.insert("pd_in_range".into(), num_value(k.pd_in_range));
            params.insert("update_period_s".into(), num_value(k.update_period_s));
            params.insert(
                "noise".into(),
                serde_json::json!({
                    "range_m": num_value(k.noise.range_m),
                    "cross_m": num_value(k.noise.cross_m),
                    "height_m": num_value(k.noise.height_m),
                }),
            );
            params.insert(
                "field_of_regard_deg".into(),
                serde_json::Value::Array(k.field_of_regard_deg.iter().map(|n| num_value(*n)).collect()),
            );
            params.insert(
                "altitude_m".into(),
                serde_json::Value::Array(k.altitude_m.iter().map(|n| num_value(*n)).collect()),
            );
            params.insert("horizon".into(), k.horizon.into());
            params.insert(
                "latency_s".into(),
                serde_json::json!({"mean": num_value(k.latency_s.mean), "jitter": num_value(k.latency_s.jitter)}),
            );
            params.insert("dropout".into(), num_value(k.dropout));
            params.insert("out_of_order".into(), num_value(k.out_of_order));
            params.insert("false_alarms_per_scan".into(), num_value(k.false_alarms_per_scan));
            params.insert(
                "ea".into(),
                serde_json::json!({
                    "skew_s": num_value(k.ea.skew_s),
                    "dropout_multiplier": num_value(k.ea.dropout_multiplier),
                    "fa_multiplier": num_value(k.ea.fa_multiplier),
                }),
            );
            if k.cued {
                params.insert("cued".into(), true.into());
            }
            if k.moving_only {
                params.insert("moving_only".into(), true.into());
            }
            if !k.sea_state_dropout.is_empty() {
                params.insert(
                    "sea_state_dropout".into(),
                    serde_json::Value::Object(
                        k.sea_state_dropout
                            .iter()
                            .map(|(state, v)| (state.to_string(), num_value(*v)))
                            .collect(),
                    ),
                );
            }
            for (key, v) in &k.rest {
                params.insert(key.clone(), scalar_value(v));
            }
            if let Some((east, north)) = s.bias {
                params.insert(
                    "bias_m".into(),
                    serde_json::json!({"east": num_value(east), "north": num_value(north)}),
                );
            }
            serde_json::json!({
                "id": s.id,
                "type": k.id,
                "name": s.name,
                "pos": s.pos.iter().map(|n| num_value(*n)).collect::<Vec<_>>(),
                "params": params,
                "calibration": s.calibration,
            })
        })
        .collect();
    serde_json::json!({ "sensors": list })
}

/// Run one built scenario to its set.
#[allow(clippy::too_many_lines)]
fn run(lib: &TrackLibrary, mut built: Built<'_>) -> Result<GeneratedSet, TracksError> {
    let scen = built.scenario;
    let dt = built.tick;
    let duration = built.duration;
    let events = built.events.clone();
    let mut truth = String::new();
    let mut truth_records = 0usize;
    let mut detections: Vec<DetectionLine> = Vec::new();
    let mut event_lines: Vec<EventLine<'_>> = events.iter().map(|e| EventLine::Spec(e)).collect();
    let mut last_seen: HashMap<String, f64> = HashMap::new();
    let mut active_ea: HashMap<i64, HashMap<&str, ActiveEa<'_>>> = HashMap::new();
    let mut lost: BTreeSet<i64> = BTreeSet::new();
    let mut sea_state: i64 = 0;

    // Interceptor targets.
    let n = built.entities.len();
    for i in 0..n {
        let Some(tg) = built.entities[i].flags.target_group.clone() else {
            continue;
        };
        let cands: Vec<usize> = (0..n)
            .filter(|&j| built.entities[j].spec.group.as_deref() == Some(tg.as_str()))
            .collect();
        if cands.is_empty() {
            continue;
        }
        let others = (0..n)
            .filter(|&j| {
                j != i && built.entities[j].flags.target_group.as_deref() == Some(tg.as_str())
            })
            .count();
        built.entities[i].target = Some(cands[others % cands.len()]);
    }
    let mut next_scan: HashMap<i64, f64> = built.sensors.iter().map(|s| (s.id, s.phase)).collect();
    let mut t = 0.0f64;
    let mut fa_total = 0usize;
    let rng = &mut built.rng;
    while t <= duration.f() + 1e-9 {
        // Events.
        for ev in &events {
            let et = ev.t().map_or(0.0, Num::f);
            if (et - t).abs() < dt / 2.0 {
                match ev.kind() {
                    "sensor_lost" => {
                        if let Some(id) = ev.get("sensor").and_then(Scalar::as_i64) {
                            lost.insert(id);
                        }
                    }
                    "sensor_restored" => {
                        if let Some(id) = ev.get("sensor").and_then(Scalar::as_i64) {
                            lost.remove(&id);
                        }
                    }
                    kind @ ("ea_skew" | "ea_dropout") => {
                        let until = ev.get("until").and_then(Scalar::as_num).unwrap_or(duration);
                        for sid in ev.get("sensors").and_then(Scalar::as_list).unwrap_or(&[]) {
                            if let Some(sid) = sid.as_i64() {
                                active_ea
                                    .entry(sid)
                                    .or_default()
                                    .insert(kind, ActiveEa { event: ev, until });
                            }
                        }
                    }
                    "sea_state" => {
                        if let Some(v) = ev.get("value").and_then(Scalar::as_i64) {
                            sea_state = v;
                        }
                    }
                    _ => {}
                }
            }
        }
        // Entities.
        for i in 0..n {
            if !built.entities[i].spawned && t >= built.entities[i].spawn {
                built.entities[i].spawned = true;
            }
            if !built.entities[i].spawned || !built.entities[i].alive {
                continue;
            }
            step(&mut built.entities, i, t, dt, rng)?;
            let e = &built.entities[i];
            if e.fired && e.flags.fired_at == Some(t) {
                event_lines.push(EventLine::Fires {
                    t,
                    entity: e.id.clone(),
                });
            }
            write_truth(&mut truth, t, e);
            truth_records += 1;
        }
        // Sensors.
        for s in &built.sensors {
            if lost.contains(&s.id) {
                continue;
            }
            let p = s.kind;
            while let Some(st) = next_scan.get(&s.id).copied() {
                if st > t {
                    break;
                }
                next_scan.insert(s.id, st + p.update_period_s.f());
                let mut skew = Num::Float(0.0);
                let mut drop_mult = Num::Float(1.0);
                let mut fa_mult = Num::Float(1.0);
                if let Some(ea) = active_ea.get(&s.id) {
                    if let Some(a) = ea.get("ea_skew") {
                        if st <= a.until.f() {
                            skew = a
                                .event
                                .get("skew_s")
                                .and_then(Scalar::as_num)
                                .unwrap_or(p.ea.skew_s);
                        }
                    }
                    if let Some(a) = ea.get("ea_dropout") {
                        if st <= a.until.f() {
                            drop_mult = a
                                .event
                                .get("multiplier")
                                .and_then(Scalar::as_num)
                                .unwrap_or(p.ea.dropout_multiplier);
                            fa_mult = p.ea.fa_multiplier;
                        }
                    }
                }
                let sea = p
                    .sea_state_dropout
                    .get(&sea_state)
                    .copied()
                    .unwrap_or(Num::Float(0.0));
                let dropout = Num::Float(0.95).min2(p.dropout.mul(drop_mult).add(sea));
                let bands = &p.range_m;
                for i in 0..n {
                    let e = &built.entities[i];
                    if !e.spawned || !e.alive || e.is_occluded(st) {
                        continue;
                    }
                    let key = p.signature_key.as_str();
                    let cl: String = match key {
                        "rcs" => {
                            if e.decoy {
                                "large".into()
                            } else {
                                e.platform.rcs_class.clone().unwrap_or_default()
                            }
                        }
                        "ir" => e.platform.ir_class.clone().unwrap_or_default(),
                        "acoustic" => e.platform.acoustic_class.clone().unwrap_or_default(),
                        _ => emission_key(e.platform, e, lib),
                    };
                    let rmax = bands.get(&cl).copied().unwrap_or(Num::Int(0));
                    if rmax.le(Num::Int(0)) {
                        continue;
                    }
                    let rel = sub(&e.pos, &s.pos);
                    let r = norm(&rel);
                    if r > rmax.f() {
                        continue;
                    }
                    if p.horizon && r > radar_horizon_m(s.pos[2], e.pos[2]) {
                        continue;
                    }
                    let [lo, hi] = p.altitude_m;
                    let z = e.pos[2];
                    if !(lo.sub(Num::Int(1)).le(z) && z.le(hi.add(Num::Int(1)))) {
                        continue;
                    }
                    let [a, b] = p.field_of_regard_deg;
                    let bearing = pymod(degrees(rel[0].f().atan2(rel[1].f())) + 360.0, 360.0);
                    let inside = (a.f() <= bearing && bearing <= b.f())
                        || (a.gt(b) && (bearing >= a.f() || bearing <= b.f()));
                    // The reference compares exactly, and so does the port.
                    #[allow(clippy::float_cmp)]
                    let whole = a.f() == 0.0 && b.f() == 360.0;
                    if !inside && !whole {
                        continue;
                    }
                    if p.moving_only && norm(&e.vel) < 1.0 {
                        continue;
                    }
                    if p.cued && st - last_seen.get(&e.id).copied().unwrap_or(-1e9) > 10.0 {
                        continue;
                    }
                    if let Some(chance) = e.flags.adsb_intermittent {
                        if chance.f() != 0.0 && key == "emission" && rng.random() < chance.f() {
                            continue;
                        }
                    }
                    let pd = p.pd_in_range.f() * (1.0 - dropout.f());
                    if rng.random() >= pd {
                        continue;
                    }
                    let los = unit(&rel);
                    let cross = unit(&[los[1].neg(), los[0], Num::Float(0.0)]);
                    let nz = p.noise;
                    let mut meas = add(&e.pos, &scale(&los, rng.gauss(0.0, nz.range_m.f())));
                    meas = add(&meas, &scale(&cross, rng.gauss(0.0, nz.cross_m.f())));
                    meas[2] = meas[2].add(Num::Float(rng.gauss(0.0, nz.height_m.f())));
                    if let Some((east, north)) = s.bias {
                        meas = add(&meas, &[east, north, Num::Float(0.0)]);
                    }
                    if let Some(off) = &e.flags.ais_spoof_offset_m {
                        if !off.is_empty() && key == "emission" {
                            let east = off.first().copied().unwrap_or(Num::Int(0));
                            let north = off.get(1).copied().unwrap_or(Num::Int(0));
                            meas = add(&meas, &[east, north, Num::Float(0.0)]);
                        }
                    }
                    if e.platform
                        .altitude_m
                        .is_some_and(|alt| nums_eq(alt, [0, 0]))
                    {
                        meas[2] = Num::Float(0.0);
                    }
                    let source = Num::Float(st).sub(skew);
                    let mut latency =
                        p.latency_s.mean.f() + rng.gauss(0.0, p.latency_s.jitter.f()).abs();
                    if rng.random() < p.out_of_order.f() {
                        latency += rng.uniform(0.5, 1.5);
                    }
                    let latency = Num::Float(latency).min2(Num::Float(MAX_LATENCY_S).sub(skew));
                    detections.push(DetectionLine {
                        sensor: s.id,
                        source_time: source.round(3),
                        receipt_time: Num::Float(st).add(latency).round(3),
                        measurement: meas,
                        calibration: s.calibration.clone(),
                        truth: Some(e.id.clone()),
                    });
                    if !p.cued {
                        last_seen.insert(e.id.clone(), st);
                    }
                }
                // False alarms.
                let lam = p.false_alarms_per_scan.mul(fa_mult);
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
                for _ in 0..k {
                    let rmax = bands
                        .values()
                        .copied()
                        .fold(None, |acc: Option<Num>, v| match acc {
                            None => Some(v),
                            Some(m) => Some(m.max2(v)),
                        })
                        .unwrap_or(Num::Int(0));
                    let ang = rng.uniform(0.0, 2.0 * std::f64::consts::PI);
                    let r = rng.uniform(0.05, 1.0) * rmax.f();
                    let [lo, hi] = p.altitude_m;
                    let z = if hi.gt(Num::Int(0)) {
                        rng.uniform(lo.f(), hi.min2(Num::Float(5000.0)).f())
                    } else {
                        0.0
                    };
                    let meas = [
                        s.pos[0].add(Num::Float(r * ang.cos())),
                        s.pos[1].add(Num::Float(r * ang.sin())),
                        Num::Float(z),
                    ];
                    let latency = Num::Float(
                        p.latency_s.mean.f() + rng.gauss(0.0, p.latency_s.jitter.f()).abs(),
                    )
                    .min2(Num::Float(MAX_LATENCY_S).sub(skew));
                    detections.push(DetectionLine {
                        sensor: s.id,
                        source_time: Num::Float(st).sub(skew).round(3),
                        receipt_time: Num::Float(st).add(latency).round(3),
                        measurement: meas,
                        calibration: s.calibration.clone(),
                        truth: None,
                    });
                    fa_total += 1;
                }
            }
        }
        t += dt;
    }
    detections.sort_by(|a, b| {
        a.receipt_time
            .f()
            .partial_cmp(&b.receipt_time.f())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.sensor.cmp(&b.sensor))
    });

    let mut detections_text = String::new();
    let mut detections_truth = String::new();
    for (i, d) in detections.iter().enumerate() {
        write_detection(&mut detections_text, d);
        let entity = d
            .truth
            .as_deref()
            .map_or_else(|| "null".to_string(), json_str);
        let _ = writeln!(
            detections_truth,
            "{{\"line\":{},\"entity\":{}}}",
            i + 1,
            entity
        );
    }
    event_lines.sort_by(|a, b| {
        a.t()
            .partial_cmp(&b.t())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut events_text = String::new();
    for ev in &event_lines {
        events_text.push_str(&ev.json());
        events_text.push('\n');
    }

    let mut by_class: Vec<(String, usize)> = Vec::new();
    for e in &built.entities {
        match by_class.iter_mut().find(|(c, _)| *c == e.cls.id) {
            Some((_, n)) => *n += 1,
            None => by_class.push((e.cls.id.clone(), 1)),
        }
    }
    let variant = format!(
        "{}{}",
        if built.full { "full" } else { "sample" },
        built
            .variant
            .as_deref()
            .map_or_else(String::new, |v| format!("-{v}"))
    );
    let counts = Counts {
        entities: built.entities.len(),
        truth_records,
        detections: detections.len(),
        false_alarms: fa_total,
        sensors: built.sensors.len(),
    };
    let metadata = serde_json::json!({
        "format": 1,
        "scenario": scen.id,
        "variant": variant,
        "seed": built.seed,
        "generator": GENERATOR,
        "catalogue_version": lib.catalogue.version,
        "classes_version": lib.classes.version,
        "sensors_version": lib.sensors.version,
        "scenarios_version": lib.scenarios.version,
        "duration_s": num_value(duration),
        "truth_tick_s": dt,
        "origin": {
            "name": lib.scenarios.origin.name,
            "lat": num_value(lib.scenarios.origin.lat),
            "lon": num_value(lib.scenarios.origin.lon),
            "alt_m": num_value(lib.scenarios.origin.alt_m),
        },
        "start_time_of_day": scen.start_time_of_day,
        "counts": {
            "entities": counts.entities,
            "entities_by_class": by_class.iter().map(|(c, n)| (c.clone(), serde_json::Value::from(*n))).collect::<serde_json::Map<_, _>>(),
            "truth_records": counts.truth_records,
            "detections": counts.detections,
            "false_alarms": counts.false_alarms,
            "sensors": counts.sensors,
        },
        "expected": scalar_value(&Scalar::Map(scen.expected.clone())),
        "provenance": {
            "policy": "docs/test-tracks/sourcing-and-legal.md",
            "inputs": ["classes.yaml", "sensors.yaml", "scenarios.yaml", "catalogue-*.yaml"],
        },
        "validation": {"passed": null, "report": "validation-report.json"},
    });
    Ok(GeneratedSet {
        scenario: scen.id.clone(),
        variant,
        seed: built.seed,
        truth,
        detections: detections_text,
        detections_truth,
        events: events_text,
        sensors_json: sensors_value(&built.sensors),
        metadata_json: metadata,
        counts,
    })
}

/// Generate one scenario's set: the committed sample reduction, or the full-size set.
///
/// # Errors
///
/// A scenario, base, point, route, class, platform or sensor type the library does not
/// hold; a sample set asked of a scenario with no `sample` block; an override or a
/// movement model the reference does not define.
pub fn generate(
    lib: &TrackLibrary,
    scenario_id: &str,
    full: bool,
    seed_override: Option<u64>,
    variant: Option<&str>,
) -> Result<GeneratedSet, TracksError> {
    let scen = lib
        .scenarios
        .scenarios
        .iter()
        .find(|s| s.id == scenario_id)
        .ok_or_else(|| TracksError::NoScenario(scenario_id.to_string()))?;
    let built = build_scenario(lib, scen, full, seed_override, variant)?;
    run(lib, built)
}

/// The committed sample reduction of a scenario.
///
/// # Errors
///
/// As [`generate`].
pub fn generate_sample(lib: &TrackLibrary, scenario_id: &str) -> Result<GeneratedSet, TracksError> {
    generate(lib, scenario_id, false, None, None)
}

impl GeneratedSet {
    /// Write the six files of a set into `dir`, as the reference lays them out.
    ///
    /// # Errors
    ///
    /// Any I/O failure, by path.
    pub fn write_to(&self, dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("truth.jsonl"), &self.truth)?;
        std::fs::write(dir.join("detections.jsonl"), &self.detections)?;
        std::fs::write(dir.join("detections-truth.jsonl"), &self.detections_truth)?;
        std::fs::write(dir.join("events.jsonl"), &self.events)?;
        std::fs::write(
            dir.join("sensors.json"),
            serde_json::to_string_pretty(&self.sensors_json)?,
        )?;
        std::fs::write(
            dir.join("metadata.json"),
            serde_json::to_string_pretty(&self.metadata_json)?,
        )?;
        Ok(())
    }
}
