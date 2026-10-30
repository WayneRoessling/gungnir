// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The shadow field's geometry (GAP-246, `docs/design/DN-34-sensor-dead-spots.md` §11, the
//! four `gungnir-analytics` rows, drafted there and not yet agreed in
//! `docs/verification-capability-table.md`):
//!
//! - **a, the wall.** A wall of height *H* at distance *d* from a sensor at height
//!   *h<sub>s</sub>* on flat ground; for a probe at *h<sub>p</sub>* above the wall the
//!   shadow begins at *x* = *d*·(*h<sub>p</sub>* − *h<sub>s</sub>*)/(*H* − *h<sub>s</sub>*)
//!   from the sensor, the line through the sensor and the wall's top; at or below the wall
//!   every cell beyond it is shadow. The expected boundary is closed form, nothing of the
//!   crate's.
//! - **b, sweep against reference.** Random ridged terrain, several sensors and heights:
//!   every cell the sweep decides differently from the sampled reference lies beside a
//!   cell where the reference's own answer changes, and they disagree on under one percent.
//! - **c, monotonicity.** Raising the probe never adds a shadow cell; removing a sensor
//!   never turns a dead spot into a watched one; the field carries its height and cell.
//! - **d, agreement with the approach report.** An approach flown at the field's height over
//!   the same terrain and sensors: each sample's severity is the cell's under it, but where
//!   a neighbouring cell already has the sample's answer.

use gungnir_analytics::{
    combined_coverage, shadow_field, shadow_field_reference, CellSight, CoverageParameters,
    CoverageVolume, GapSeverity, HeightField, HeightFieldLineOfSight, ShadowField,
    ShadowParameters, Sight,
};
use gungnir_data::geospatial::{GridCrs, HeightGrid, TerrainMesh};
use gungnir_model::SensorId;

/// A `rows` by `columns` DEM of `cell` m cells in the local frame, its south-west corner at
/// the origin, with the height `h(east, north)` at each cell centre.
fn dem(rows: u32, columns: u32, cell: f64, h: impl Fn(f64, f64) -> f64) -> TerrainMesh {
    let mut heights = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let east = (f64::from(column) + 0.5) * cell;
            let north = (f64::from(rows - 1 - row) + 0.5) * cell;
            #[allow(clippy::cast_possible_truncation)]
            heights.push(h(east, north) as f32);
        }
    }
    TerrainMesh::from_grid(&HeightGrid {
        columns,
        rows,
        origin: [0.0, 0.0],
        cell_size: [cell, cell],
        nodata: None,
        crs: GridCrs::Unstated,
        vertical: None,
        heights,
    })
    .expect("a test grid")
    .placed()
}

/// An omnidirectional sensor seeing the whole hemisphere and below, out to `range_m`.
fn sensor(at: [f64; 3], range_m: f64) -> CoverageVolume {
    CoverageVolume {
        sensor_enu: at,
        max_range_m: range_m,
        min_elevation_rad: -std::f64::consts::FRAC_PI_2,
        max_elevation_rad: std::f64::consts::FRAC_PI_2,
        vertical: [0.0, 0.0, 1.0],
        azimuth: None,
    }
}

/// The field's cells due east of `sensor_e` along `north`, as (distance, cell).
fn east_of(field: &ShadowField, sensor_e: f64, north: f64, to_e: f64) -> Vec<(f64, CellSight)> {
    let cell = field.parameters.cell_m;
    let mut out = Vec::new();
    let mut e = sensor_e + cell / 2.0;
    while e < to_e {
        if let Some(c) = field.cell_at(e, north) {
            let centre = field.centre(index_at(field, e, north));
            out.push((centre[0] - sensor_e, c));
        }
        e += cell;
    }
    out
}

fn index_at(field: &ShadowField, east: f64, north: f64) -> usize {
    (0..field.cells.len())
        .min_by(|a, b| {
            let d = |i: usize| {
                let c = field.centre(i);
                (c[0] - east).powi(2) + (c[1] - north).powi(2)
            };
            d(*a).total_cmp(&d(*b))
        })
        .expect("a non-empty field")
}

/// Row a: the wall, both paths, a probe above the wall and one below it.
#[test]
fn a_walls_shadow_begins_where_the_line_over_its_top_says() {
    const CELL: f64 = 10.0;
    let (near, far, wall) = (600.0, 640.0, 100.0);
    let terrain = HeightField::resample(
        &dem(41, 201, CELL, |e, _| {
            if (near..=far).contains(&e) {
                wall
            } else {
                0.0
            }
        }),
        CELL,
    )
    .expect("resampled");
    let (se, sn, hs) = (105.0, 205.0, 20.0);
    let sensors = [(SensorId(1), sensor([se, sn, hs], 1_900.0))];
    // The DEM's surface runs through its cell centres: the last one before the wall is at
    // 0 m, the first on it at `wall`, so the wall's top begins at that first centre and
    // the ramp below it is a cell long. The line over the top grazes that edge.
    let d = near + CELL / 2.0 - se;

    for compute in [shadow_field, shadow_field_reference] {
        // Above the wall: seen past it, then shadow from the line over its top outward.
        let hp = 150.0;
        let field = compute(
            &sensors,
            &terrain,
            ShadowParameters {
                height_agl_m: hp,
                cell_m: CELL,
            },
        );
        let expected = d * (hp - hs) / (wall - hs);
        let line = east_of(&field, se, sn, 1_990.0);
        let first_shadow = line
            .iter()
            .find(|(_, c)| matches!(c, CellSight::Shadow { .. }))
            .map(|(x, _)| *x)
            .expect("a shadow beyond the wall");
        // A cell of quantisation, plus the ramp's half cell carried out along the line.
        let tolerance = CELL + (CELL / 2.0) * (hp - hs) / (wall - hs);
        assert!(
            (first_shadow - expected).abs() <= tolerance,
            "shadow begins at {first_shadow} m, the line over the wall says {expected} m"
        );
        for (x, c) in &line {
            if *x < expected - tolerance {
                assert!(
                    matches!(c, CellSight::Single { .. }),
                    "{x} m is in sight: {c:?}"
                );
            }
            if *x > expected + tolerance {
                assert!(
                    matches!(c, CellSight::Shadow { claimed: 1 }),
                    "{x} m is behind the wall: {c:?}"
                );
            }
        }

        // At or below the wall: every cell beyond it is shadow.
        let low = compute(
            &sensors,
            &terrain,
            ShadowParameters {
                height_agl_m: 60.0,
                cell_m: CELL,
            },
        );
        for (x, c) in east_of(&low, se, sn, 1_990.0) {
            if x > far - se + CELL {
                assert!(
                    matches!(c, CellSight::Shadow { .. }),
                    "{x} m is behind a wall taller than the probe: {c:?}"
                );
            }
        }
    }
}

/// The first and last dead-spot cells along a line of cells, as distances from the sensor.
fn shadow_span(line: &[(f64, CellSight)]) -> (f64, f64) {
    let mut shadows = line
        .iter()
        .filter(|(_, c)| matches!(c, CellSight::Shadow { .. }))
        .map(|(x, _)| *x);
    let first = shadows.next().expect("a shadow on the line");
    (first, shadows.next_back().unwrap_or(first))
}

/// Row a: a cone. Its apex `D` from a sensor at `hs`, height `H`, on flat ground, with the
/// sensor outside its base so the apex is the highest elevation the near face presents. A
/// probe at `hp` above the apex is hidden from `x = D(hp − hs)/(H − hs)` on, along the line
/// through the apex. The apex is a DEM cell centre, so the surface along that line is the
/// cone's own and one cell is the tolerance.
#[test]
fn a_cones_shadow_begins_where_the_line_over_its_apex_says() {
    const CELL: f64 = 10.0;
    let (apex_e, apex_n, height, radius) = (705.0, 205.0, 150.0, 100.0);
    let terrain = HeightField::resample(
        &dem(41, 201, CELL, |e, n| {
            let r = ((e - apex_e).powi(2) + (n - apex_n).powi(2)).sqrt();
            height * (1.0 - r / radius).max(0.0)
        }),
        CELL,
    )
    .expect("resampled");
    let (se, sn, hs, hp) = (105.0, 205.0, 20.0, 200.0);
    // The field's cells are laid out from the sensor's reach, so a range of 190.5 cells puts
    // a row of cell centres on the line through the apex. At 190 the centres sit half a cell
    // to either side, where a cone this steep is five metres lower than on the line.
    let sensors = [(SensorId(1), sensor([se, sn, hs], 1_905.0))];
    let expected = (apex_e - se) * (hp - hs) / (height - hs);
    for compute in [shadow_field, shadow_field_reference] {
        let field = compute(
            &sensors,
            &terrain,
            ShadowParameters {
                height_agl_m: hp,
                cell_m: CELL,
            },
        );
        let line = east_of(&field, se, sn, 1_990.0);
        let (first, _) = shadow_span(&line);
        assert!(
            (first - expected).abs() <= CELL,
            "the cone's shadow begins at {first} m, the line over its apex says {expected} m"
        );
        assert!(line
            .iter()
            .filter(|(x, _)| *x > expected + CELL)
            .all(|(_, c)| matches!(c, CellSight::Shadow { .. })));
    }
}

/// Row a: a valley. A sensor at `hs` on a plateau whose rim is `d` away, the far side of the
/// rim falling at slope `s` to a floor `V` down and rising again at the same slope to the
/// plateau. A probe at `hp` above the ground is hidden where the line from the sensor over
/// the rim passes above it: from `x₁ = (hp − hs + s·d)/(s − hs/d)` on the near slope until
/// the far slope climbs back into sight at `x₂`, where `−V + s(x₂ − f) + hp − hs = −hs·x₂/d`
/// and `f` is the far slope's foot. Every kink is a DEM cell centre, so the surface is the
/// valley's own and one cell is the tolerance at each end.
#[test]
fn a_valleys_shadow_runs_between_the_rim_and_where_the_far_slope_rises_into_sight() {
    const CELL: f64 = 10.0;
    let (rim, floor_from, floor_to, far_rim, slope, depth) =
        (605.0, 905.0, 1_305.0, 1_605.0, 0.5, 150.0);
    let ground = move |e: f64| {
        if e <= rim || e >= far_rim {
            0.0
        } else if e < floor_from {
            -slope * (e - rim)
        } else if e <= floor_to {
            -depth
        } else {
            -depth + slope * (e - floor_to)
        }
    };
    let terrain =
        HeightField::resample(&dem(41, 201, CELL, |e, _| ground(e)), CELL).expect("resampled");
    let (se, sn, hs, hp) = (105.0, 205.0, 20.0, 10.0);
    let sensors = [(SensorId(1), sensor([se, sn, hs], 1_900.0))];
    let d = rim - se;
    let foot = floor_to - se;
    let enters = (hp - hs + slope * d) / (slope - hs / d);
    let leaves = (depth + slope * foot - hp + hs) / (slope + hs / d);
    assert!(
        enters > d && enters < floor_from - se,
        "the fixture hides the near slope"
    );
    assert!(
        leaves > foot && leaves < far_rim - se,
        "and the far slope rises into sight"
    );
    for compute in [shadow_field, shadow_field_reference] {
        let field = compute(
            &sensors,
            &terrain,
            ShadowParameters {
                height_agl_m: hp,
                cell_m: CELL,
            },
        );
        let line = east_of(&field, se, sn, 1_990.0);
        let (first, last) = shadow_span(&line);
        assert!(
            (first - enters).abs() <= CELL,
            "the valley's shadow begins at {first} m, the rim says {enters} m"
        );
        assert!(
            (last - leaves).abs() <= CELL,
            "the valley's shadow ends at {last} m, the rim says {leaves} m"
        );
        for (x, c) in &line {
            let inside = *x > enters + CELL && *x < leaves - CELL;
            let outside = *x < enters - CELL || *x > leaves + CELL;
            if inside {
                assert!(matches!(c, CellSight::Shadow { .. }), "{x} m: {c:?}");
            }
            if outside {
                assert!(matches!(c, CellSight::Single { .. }), "{x} m: {c:?}");
            }
        }
    }
}

/// A deterministic sequence in [0, 1), so the terrain is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        #[allow(clippy::cast_precision_loss)]
        let x = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        x
    }
}

/// Sums of ridges: each a tent of height `a` and half-width `w` along a line at angle `t`
/// through `(e, n)`.
fn ridged(seed: u64, size_m: f64, ridges: usize) -> impl Fn(f64, f64) -> f64 {
    let mut r = Lcg(seed);
    let shape: Vec<(f64, f64, f64, f64, f64)> = (0..ridges)
        .map(|_| {
            (
                r.next() * size_m,
                r.next() * size_m,
                r.next() * std::f64::consts::PI,
                40.0 + r.next() * 160.0,
                100.0 + r.next() * 400.0,
            )
        })
        .collect();
    move |e, n| {
        shape
            .iter()
            .map(|&(e0, n0, t, a, w)| {
                let across = (e - e0) * t.cos() + (n - n0) * t.sin();
                a * (1.0 - across.abs() / w).max(0.0)
            })
            .sum()
    }
}

/// Whether cell `i`, or any of its eight neighbours, has a different sight in `sights`
/// than `i` itself: a cell beside a horizon line.
fn beside_a_change(field: &ShadowField, sights: &[Sight], i: usize) -> bool {
    let (rows, columns) = (i64::from(field.rows), i64::from(field.columns));
    #[allow(clippy::cast_possible_wrap)]
    let (r, c) = ((i as i64) / columns, (i as i64) % columns);
    for dr in -1..=1 {
        for dc in -1..=1 {
            let (nr, nc) = (r + dr, c + dc);
            if nr < 0 || nc < 0 || nr >= rows || nc >= columns {
                continue;
            }
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let j = (nr * columns + nc) as usize;
            if sights[j] != sights[i] {
                return true;
            }
        }
    }
    false
}

/// Row b: the sweep against the reference, over three terrains, three heights and three
/// sensors.
#[test]
fn the_sweep_agrees_with_the_reference_but_beside_a_horizon() {
    const CELL: f64 = 25.0;
    for seed in [1, 7, 42] {
        let terrain = HeightField::resample(&dem(120, 120, CELL, ridged(seed, 3_000.0, 9)), CELL)
            .expect("resampled");
        let sensors = [
            (SensorId(1), sensor([700.0, 800.0, 60.0], 1_400.0)),
            (SensorId(2), sensor([2_200.0, 1_500.0, 120.0], 1_200.0)),
            (SensorId(3), sensor([1_500.0, 2_400.0, 30.0], 1_000.0)),
        ];
        for height in [30.0, 100.0, 300.0] {
            let parameters = ShadowParameters {
                height_agl_m: height,
                cell_m: CELL,
            };
            let sweep = shadow_field(&sensors, &terrain, parameters);
            let reference = shadow_field_reference(&sensors, &terrain, parameters);
            assert_eq!(sweep.parameters, reference.parameters);
            let (mut claimed, mut disagree) = (0usize, 0usize);
            for ((_, fast), (_, slow)) in sweep.per_sensor.iter().zip(&reference.per_sensor) {
                for i in 0..fast.len() {
                    if matches!(slow[i], Sight::Hidden | Sight::Seen) {
                        claimed += 1;
                    }
                    if fast[i] != slow[i] {
                        disagree += 1;
                        assert!(
                            beside_a_change(&reference, slow, i),
                            "seed {seed} at {height} m: cell {i} at {:?} disagrees ({:?} against \
                             the reference's {:?}) away from any horizon",
                            reference.centre(i),
                            fast[i],
                            slow[i]
                        );
                    }
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let fraction = disagree as f64 / claimed.max(1) as f64;
            assert!(
                fraction < 0.01,
                "seed {seed} at {height} m: {disagree} of {claimed} claimed cells disagree"
            );
            assert!(
                sweep.measure().shadow_m2 > 0.0,
                "seed {seed} at {height} m: ridged terrain should hide something"
            );
        }
    }
}

/// Row c: the properties.
#[test]
fn shadow_shrinks_as_the_probe_rises_and_a_lost_sensor_never_clears_one() {
    const CELL: f64 = 25.0;
    let terrain =
        HeightField::resample(&dem(80, 80, CELL, ridged(3, 2_000.0, 7)), CELL).expect("resampled");
    // Ranges past the whole grid, so a higher probe cannot leave a sensor's reach.
    let a = (SensorId(1), sensor([500.0, 600.0, 40.0], 5_000.0));
    let b = (SensorId(2), sensor([1_500.0, 1_400.0, 40.0], 5_000.0));
    let at = |sensors: &[(SensorId, CoverageVolume)], h: f64| {
        shadow_field(
            sensors,
            &terrain,
            ShadowParameters {
                height_agl_m: h,
                cell_m: CELL,
            },
        )
    };

    let mut previous = at(&[a, b], 20.0);
    assert!((previous.parameters.height_agl_m - 20.0).abs() < f64::EPSILON);
    assert!((previous.parameters.cell_m - CELL).abs() < f64::EPSILON);
    assert_eq!(previous.clamped_from_m, None);
    for h in [50.0, 100.0, 200.0, 400.0] {
        let higher = at(&[a, b], h);
        for (i, (low, high)) in previous.cells.iter().zip(&higher.cells).enumerate() {
            if matches!(high, CellSight::Shadow { .. }) {
                assert!(
                    matches!(low, CellSight::Shadow { .. }),
                    "cell {i} is a dead spot at {h} m and was not one lower: {low:?}"
                );
            }
        }
        assert!(higher.measure().shadow_m2 <= previous.measure().shadow_m2);
        previous = higher;
    }

    let both = at(&[a, b], 100.0);
    let one = at(&[a], 100.0);
    // The two fields span different boxes, so cells are matched by place, not by index.
    for (i, with) in both.cells.iter().enumerate() {
        let [e, n] = both.centre(i);
        let Some(without) = one.cell_at(e, n) else {
            continue;
        };
        let without = &without;
        if matches!(with, CellSight::Shadow { .. }) {
            assert!(
                matches!(without, CellSight::Shadow { .. } | CellSight::Unclaimed),
                "losing a sensor made dead spot {i} watched: {without:?}"
            );
        }
        if *with
            == (CellSight::Single {
                sensor: SensorId(2),
            })
        {
            assert!(
                matches!(without, CellSight::Shadow { .. } | CellSight::Unclaimed),
                "what only the lost sensor saw is no longer watched: {without:?}"
            );
        }
    }
    // Each sensor's own shadow is in the field, and one it was not computed with is not.
    assert!(both.for_sensor(SensorId(1)).is_some());
    assert!(both.for_sensor(SensorId(9)).is_none());
    // No sensors, or a height that is not one, is an empty field, never a guessed one.
    assert!(at(&[], 100.0).cells.is_empty());
    assert!(at(&[a], f64::NAN).cells.is_empty());
}

/// Row d: an approach flown at the field's height along the ground's own profile, and the
/// cells under it.
#[test]
fn the_approach_report_agrees_with_the_cells_under_it() {
    const CELL: f64 = 25.0;
    let terrain = HeightField::resample(&dem(100, 100, CELL, ridged(11, 2_500.0, 8)), CELL)
        .expect("resampled");
    let sensors = [
        (SensorId(1), sensor([600.0, 700.0, 50.0], 2_000.0)),
        (SensorId(2), sensor([1_900.0, 900.0, 80.0], 2_000.0)),
    ];
    let height = 100.0;
    let field = shadow_field(
        &sensors,
        &terrain,
        ShadowParameters {
            height_agl_m: height,
            cell_m: CELL,
        },
    );
    // Along two lines across the sector, a sample every cell at the stated height above
    // the ground under it. Each is its own one-point approach, so the report's answer for
    // it is read by index: a polyline's samples are spaced along its own climbing and
    // falling length, which is not the map distance, and would not land where asked.
    let lines = [
        ([100.0, 1_250.0], [2_400.0, 1_250.0]),
        ([1_250.0, 100.0], [1_250.0, 2_400.0]),
    ];
    let as_cell = |s: Option<GapSeverity>, c: CellSight| {
        matches!(
            (s, c),
            (
                Some(GapSeverity::Uncovered),
                CellSight::Shadow { .. } | CellSight::Unclaimed
            ) | (Some(GapSeverity::SingleSensor), CellSight::Single { .. })
                | (None, CellSight::Fused { .. })
        )
    };
    let mut checked = 0usize;
    for (from, to) in lines {
        #[allow(clippy::cast_precision_loss)]
        let placed: Vec<Vec<[f64; 3]>> = (0..=92)
            .map(|k| {
                let along = f64::from(k) * CELL;
                let t = along / 2_300.0;
                let (e, n) = (
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                );
                vec![[e, n, terrain.up_at(e, n).expect("on the DEM") + height]]
            })
            .collect();
        let approaches: Vec<&[[f64; 3]]> = placed.iter().map(Vec::as_slice).collect();
        let report = combined_coverage(
            &sensors,
            &HeightFieldLineOfSight { field: &terrain },
            &approaches,
            CoverageParameters {
                sample_spacing_m: CELL,
                terrain_masking_applied: true,
            },
        );
        for (index, sample) in placed.iter().enumerate() {
            let [pe, pn, _] = sample[0];
            let s = report
                .gaps
                .iter()
                .find(|g| g.approach == index)
                .map(|g| g.severity);
            let here = field.cell_at(pe, pn).expect("in the field");
            if !as_cell(s, here) {
                // Only where the sample sits beside a cell that already has its answer.
                let near = [-CELL, 0.0, CELL].iter().any(|de| {
                    [-CELL, 0.0, CELL].iter().any(|dn| {
                        field
                            .cell_at(pe + de, pn + dn)
                            .is_some_and(|c| as_cell(s, c))
                    })
                });
                assert!(
                    near,
                    "sample at {pe:.0}, {pn:.0}: {s:?} against the cell's {here:?}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 150, "{checked} samples compared");
}

/// GAP-249: two fields laid over one extent are one grid, and the comparison says what a
/// change clears and what it darkens. A sensor below a wall leaves the ground behind it
/// dark; the same sensor raised above the wall clears it and darkens nothing; and two fields
/// over different extents are refused rather than compared.
#[test]
fn a_raised_mast_clears_what_the_wall_hid_and_the_comparison_says_so() {
    use gungnir_analytics::{
        compare_fields, dead_spot_extent, shadow_field_over, union_extent, CellChange,
    };
    const CELL: f64 = 10.0;
    let terrain = HeightField::resample(
        &dem(41, 201, CELL, |e, _| {
            if (600.0..=640.0).contains(&e) {
                100.0
            } else {
                0.0
            }
        }),
        CELL,
    )
    .expect("resampled");
    let low = [(SensorId(1), sensor([105.0, 205.0, 20.0], 1_900.0))];
    let raised = [(SensorId(1), sensor([105.0, 205.0, 400.0], 1_900.0))];
    let extent = union_extent(dead_spot_extent(&low), dead_spot_extent(&raised)).expect("a reach");
    let parameters = ShadowParameters {
        height_agl_m: 60.0,
        cell_m: CELL,
    };
    let before = shadow_field_over(&low, &terrain, parameters, extent);
    let after = shadow_field_over(&raised, &terrain, parameters, extent);
    let change = compare_fields(&before, &after).expect("one grid");
    assert!(
        change.cleared_m2 > 1.0e5,
        "{} m² cleared",
        change.cleared_m2
    );
    assert!(
        change.darkened_m2 < f64::EPSILON,
        "{} m² darkened",
        change.darkened_m2
    );
    assert!(change.watched_after_m2 > change.watched_before_m2);
    for (i, c) in change.cells.iter().enumerate() {
        let was_dark = matches!(before.cells[i], CellSight::Shadow { .. });
        let is_dark = matches!(after.cells[i], CellSight::Shadow { .. });
        let expected = match (was_dark, is_dark) {
            (true, false) => CellChange::Cleared,
            (false, true) => CellChange::Darkened,
            _ => CellChange::Same,
        };
        assert_eq!(*c, expected, "cell {i}");
    }
    // Lowered back below the wall, the change darkens what the raise cleared.
    let back = compare_fields(&after, &before).expect("one grid");
    assert!((back.darkened_m2 - change.cleared_m2).abs() < f64::EPSILON);
    // Over their own reaches the two are different grids, and are not compared.
    let elsewhere = [(SensorId(1), sensor([900.0, 205.0, 20.0], 1_000.0))];
    let other = shadow_field(&elsewhere, &terrain, parameters);
    assert!(compare_fields(&before, &other).is_none());
}
