// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **Re-observation matches the recording statistically** -- the second of
//! `docs/design/DN-32-re-observation-for-a-laydown.md` §10's rows, a Draft row of
//! `docs/verification-capability-table.md` §2.
//!
//! Method: re-observe each of the ten committed sample sets' truth with **its own
//! sensors at their own positions**, the recording's sensor-specific events applied by
//! identifier ([`SensorEvents::ById`]) as the generator applied them. Criterion: per
//! sensor, the detection count within 2σ of the recording's; per axis, the
//! measurement-residual variance within 2σ of the model's noise -- the same terms as §1's
//! statistical self-check (`gungnir-scenario/tests/statistical_self_check.rs`).
//!
//! A *detection* is a detection of a recorded target. False alarms are held to the
//! configured clutter rate instead, pooled, as the self-check holds its own: they read
//! nothing of the truth, so the recording's count is one draw from the same rate, and
//! [`false_alarms_run_at_the_configured_rate`] says why comparing with that one draw
//! would be wrong (DN-32 §12).
//!
//! **Why statistics and not lines.** A re-observation cannot reproduce the recording's
//! `detections.jsonl`, even here with the recording's own sensors in their own places
//! (DN-32 §7): the generator drew motion and observation from one interleaved stream, and
//! a re-observation makes none of the motion draws. Equal statistics is the correct
//! expectation.
//!
//! **How the 2σ is made honest**, on the self-check's own pattern.
//!
//! * The re-observed side is pooled over [`SEEDS`] fixed seeds, so its own spread
//!   shrinks by √K and the outcome is reproducible on every machine.
//! * The recording is one realisation and cannot be pooled. A count's variance is
//!   bounded by its mean -- a sum of independent Bernoulli trials, and a Poisson count of
//!   false alarms, each have variance at most their mean -- so σ for the difference is
//!   taken as √(M̄(1 + 1/K)), M̄ the pooled mean. That is conservative, never generous:
//!   it can only make a real difference harder to see, not a chance one easier to fail.
//! * The residual row compares against the model's own noise, which is exact, and pools
//!   every sensor's residuals **normalised by that sensor's σ on that axis** into one
//!   test per axis, so three comparisons are made rather than two hundred -- two hundred
//!   tests at 2σ would each fail about one time in twenty by construction.

// Counts converted to f64 for statistics; every one is far inside f64's exact integers.
#![allow(clippy::cast_precision_loss)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gungnir_sensor_sim::pynum::Num;
use gungnir_sensor_sim::{
    reobserve, EntitiesFile, EnvironmentFile, PlacedSensor, Recording, SensorEvents, SensorParams,
    TruthRecord,
};

/// Fixed: the same answer on every machine and every run.
const SEEDS: [u64; 12] = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233];

/// The row's tolerance, in standard deviations.
const SIGMAS: f64 = 2.0;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/tracks/samples")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_str(&read(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn lines<T: serde::de::DeserializeOwned>(path: &Path) -> Vec<T> {
    read(path)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .collect()
}

#[derive(serde::Deserialize)]
struct Metadata {
    scenario: String,
    seed: u64,
    duration_s: f64,
    truth_tick_s: f64,
}

#[derive(serde::Deserialize)]
struct SensorsFile {
    sensors: Vec<SensorEntry>,
}

#[derive(serde::Deserialize)]
struct SensorEntry {
    id: i64,
    pos: [f64; 3],
    params: SensorParams,
}

#[derive(serde::Deserialize)]
struct DetectionLine {
    sensor: i64,
}

#[derive(serde::Deserialize)]
struct TruthLink {
    entity: Option<String>,
}

/// One committed set, read as a recording plus what its own sensors recorded.
struct Set {
    name: String,
    recording: Recording,
    sensors: Vec<PlacedSensor>,
    /// Per sensor: (detections of a target, false alarms) in the committed files.
    recorded: BTreeMap<i64, (usize, usize)>,
}

fn load(dir: &Path) -> Set {
    let meta: Metadata = json(&dir.join("metadata.json"));
    let entities: EntitiesFile = json(&dir.join("entities.json"));
    let environment: EnvironmentFile = json(&dir.join("environment.json"));
    let truth: Vec<TruthRecord> = lines(&dir.join("truth.jsonl"));
    let sensors: SensorsFile = json(&dir.join("sensors.json"));
    let detections: Vec<DetectionLine> = lines(&dir.join("detections.jsonl"));
    let links: Vec<TruthLink> = lines(&dir.join("detections-truth.jsonl"));
    assert_eq!(detections.len(), links.len(), "{}", dir.display());
    let mut recorded: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
    for (d, l) in detections.iter().zip(&links) {
        let e = recorded.entry(d.sensor).or_default();
        if l.entity.is_some() {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
    }
    Set {
        name: dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned(),
        recording: Recording {
            scenario: meta.scenario,
            seed: meta.seed,
            duration_s: meta.duration_s,
            tick_s: meta.truth_tick_s,
            truth,
            entities: entities.entities,
            environment: environment.events,
        },
        sensors: sensors
            .sensors
            .into_iter()
            .map(|s| PlacedSensor {
                id: s.id,
                model: s.params,
                position: s.pos,
            })
            .collect(),
        recorded,
    }
}

fn sets() -> Vec<Set> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(samples())
        .expect("testdata/tracks/samples exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let sets: Vec<Set> = dirs.iter().map(|d| load(d)).collect();
    assert!(sets.len() >= 10, "only {} sample sets", sets.len());
    sets
}

/// The tick a scan at `st` saw: the first at or after it, accumulated as the generator
/// accumulates its ticks.
fn tick_seen(tick_s: f64, st: f64) -> i64 {
    let mut t = 0.0f64;
    while t < st {
        t += tick_s;
    }
    // Whole milliseconds, as `truth.jsonl` rounds `t`.
    #[allow(clippy::cast_possible_truncation)]
    let ms = (t * 1000.0).round() as i64;
    ms
}

/// Every set re-observed under every seed, pooled per sensor: (detections of a target,
/// false alarms, scans), summed over [`SEEDS`].
fn pooled(set: &Set) -> HashMap<i64, (usize, usize, usize)> {
    let mut pooled: HashMap<i64, (usize, usize, usize)> = HashMap::new();
    for seed in SEEDS {
        let mut recording = set.recording.clone();
        recording.seed = set.recording.seed.wrapping_mul(1_000).wrapping_add(seed);
        let run = reobserve(&recording, &set.sensors, SensorEvents::ById, "recorded")
            .unwrap_or_else(|e| panic!("{}: {e}", set.name));
        assert_eq!(run.sensor_events_not_applied, 0);
        for t in run.per_sensor {
            let e = pooled.entry(t.sensor).or_default();
            e.0 += t.detections;
            e.1 += t.false_alarms;
            e.2 += t.scans;
        }
    }
    pooled
}

/// **Per sensor, the detection count within 2σ of the recording's.** A detection here is
/// a detection of a recorded target: that is the count a laydown's geometry moves, and
/// the one the recording's truth fixes.
#[test]
fn per_sensor_detection_counts_are_within_two_sigma_of_the_recording() {
    let k = SEEDS.len() as f64;
    let mut worst = (0.0f64, String::new());
    let mut failures = Vec::new();
    let mut compared = 0usize;
    for set in sets() {
        let pooled = pooled(&set);
        for s in &set.sensors {
            let recorded = set.recorded.get(&s.id).map_or(0, |r| r.0);
            let sum = pooled.get(&s.id).map_or(0, |p| p.0);
            let mean = sum as f64 / k;
            let sigma = (mean * (1.0 + 1.0 / k)).sqrt();
            let label = format!("{} sensor {} detections", set.name, s.id);
            compared += 1;
            if sigma == 0.0 {
                if recorded != 0 {
                    failures.push(format!(
                        "{label}: the recording has {recorded} and no re-observation has any"
                    ));
                }
                continue;
            }
            let z = (recorded as f64 - mean).abs() / sigma;
            println!("{label}: recorded {recorded}, re-observed mean {mean:.2}, {z:.2} sigma");
            if z > worst.0 {
                worst = (z, label.clone());
            }
            if z >= SIGMAS {
                failures.push(format!(
                    "{label}: recorded {recorded}, re-observed mean {mean:.2}, {z:.2} sigma"
                ));
            }
        }
    }
    println!(
        "{compared} comparisons; worst {:.2} sigma ({})",
        worst.0, worst.1
    );
    assert!(compared >= 50, "only {compared} comparisons made");
    assert!(
        failures.is_empty(),
        "per-sensor detection counts beyond {SIGMAS} sigma of the recording's:\n  {}",
        failures.join("\n  ")
    );
}

/// **False alarms against the configured rate, pooled**, as §1's self-check tests its
/// clutter. Not against the recording: a false alarm is drawn about the sensor and reads
/// nothing of the truth or of where anything stands, so the recording's own count is one
/// draw from the same rate and says nothing a re-observation could disagree with -- and
/// one of them is a tail draw (TT-08's sensor 13 recorded 21 against the 36 its model
/// expects, 2.5σ low), which a per-sensor comparison with the recording would read as a
/// fault in the re-observation. Sensors an electronic-attack window names are left out:
/// their rate changes inside the window.
#[test]
fn false_alarms_run_at_the_configured_rate() {
    let (mut observed, mut expected) = (0.0f64, 0.0f64);
    for set in sets() {
        let pooled = pooled(&set);
        let jammed: std::collections::HashSet<i64> = set
            .recording
            .environment
            .iter()
            .filter_map(|e| match e {
                gungnir_sensor_sim::EnvironmentEvent::EaDropout { sensor, .. } => Some(*sensor),
                _ => None,
            })
            .collect();
        for s in &set.sensors {
            if jammed.contains(&s.id) {
                continue;
            }
            let (_, fa, scans) = pooled.get(&s.id).copied().unwrap_or((0, 0, 0));
            observed += fa as f64;
            expected += scans as f64 * s.model.false_alarms_per_scan.f();
        }
    }
    assert!(expected > 1000.0, "only {expected} false alarms expected");
    let z = (observed - expected).abs() / expected.sqrt();
    println!("false alarms: {observed} observed, {expected:.1} expected, {z:.2} sigma");
    assert!(
        z < SIGMAS,
        "false alarms {observed} against {expected:.1} expected: {z:.2} sigma"
    );
}

/// Residuals in the frame the noise is drawn in: along the line of sight, across it
/// horizontally, and in height (`gungnir_sensor_sim::observe`).
#[test]
fn measurement_residual_variance_per_axis_is_within_two_sigma_of_the_model() {
    // Per axis: the sum of squared normalised residuals, and how many.
    let mut sums = [0.0f64; 3];
    let mut counts = [0usize; 3];
    for set in sets() {
        let tick = set.recording.tick_s;
        let truth: HashMap<(&str, i64), &TruthRecord> = set
            .recording
            .truth
            .iter()
            .map(|r| {
                #[allow(clippy::cast_possible_truncation)]
                let ms = (r.t * 1000.0).round() as i64;
                ((r.entity.as_str(), ms), r)
            })
            .collect();
        let entities: HashMap<&str, &gungnir_sensor_sim::EntityRecord> = set
            .recording
            .entities
            .iter()
            .map(|e| (e.id.as_str(), e))
            .collect();
        let by_id: HashMap<i64, &PlacedSensor> = set.sensors.iter().map(|s| (s.id, s)).collect();
        for seed in SEEDS {
            let mut recording = set.recording.clone();
            recording.seed = set.recording.seed.wrapping_mul(1_000).wrapping_add(seed);
            let run = reobserve(&recording, &set.sensors, SensorEvents::ById, "recorded")
                .unwrap_or_else(|e| panic!("{}: {e}", set.name));
            for o in &run.observations {
                let Some(id) = o.truth() else { continue };
                let s = by_id[&o.sensor()];
                let entity = entities[id];
                let record = truth[&(id, tick_seen(tick, o.scan_time_s()))];
                let p = &s.model;
                let pos = record.pos.map(Num::f);
                let m = o.measurement().map(Num::f);
                let mut r = [m[0] - pos[0], m[1] - pos[1], m[2] - pos[2]];
                if let Some(b) = p.bias_m {
                    r[0] -= b.east.f();
                    r[1] -= b.north.f();
                }
                if p.signature_key == "emission" {
                    if let Some(off) = entity.ais_spoof_offset_m.as_deref() {
                        r[0] -= off.first().map_or(0.0, |n| n.f());
                        r[1] -= off.get(1).map_or(0.0, |n| n.f());
                    }
                }
                let rel = [
                    pos[0] - s.position[0],
                    pos[1] - s.position[1],
                    pos[2] - s.position[2],
                ];
                let n = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
                let los = [rel[0] / n, rel[1] / n, rel[2] / n];
                let h2 = los[0] * los[0] + los[1] * los[1];
                if h2 < 1e-6 {
                    // Straight overhead: the horizontal split is undefined.
                    continue;
                }
                let h = h2.sqrt();
                let cross = [-los[1] / h, los[0] / h];
                let n_range = (r[0] * los[0] + r[1] * los[1]) / h2;
                let n_cross = r[0] * cross[0] + r[1] * cross[1];
                let n_height = r[2] - los[2] * n_range;
                let sigmas = [
                    p.noise.range_m.f(),
                    p.noise.cross_m.f(),
                    p.noise.height_m.f(),
                ];
                for (axis, value) in [n_range, n_cross, n_height].into_iter().enumerate() {
                    // A surface platform reports zero height whatever the draw, and an
                    // axis with no noise has nothing to measure.
                    if sigmas[axis] <= 0.0 || (axis == 2 && entity.surface) {
                        continue;
                    }
                    let z = value / sigmas[axis];
                    sums[axis] += z * z;
                    counts[axis] += 1;
                }
            }
        }
    }
    let mut failures = Vec::new();
    for (axis, name) in ["range", "cross-range", "height"].iter().enumerate() {
        let n = counts[axis] as f64;
        assert!(n > 1000.0, "only {n} {name} residuals");
        // The mean of squares of zero-mean unit normals has variance 2/n.
        let variance = sums[axis] / n;
        let z = (variance - 1.0).abs() / (2.0 / n).sqrt();
        println!(
            "{name}: {} residuals, normalised variance {variance:.4}, {z:.2} sigma",
            counts[axis]
        );
        if z >= SIGMAS {
            failures.push(format!(
                "{name}: normalised variance {variance:.4}, {z:.2} sigma"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "residual variance beyond {SIGMAS} sigma of the model's noise:\n  {}",
        failures.join("\n  ")
    );
}
