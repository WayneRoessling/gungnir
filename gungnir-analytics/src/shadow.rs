// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Sensor dead spots: where the terrain hides what a sensor claims (GAP-246,
//! `docs/design/DN-34-sensor-dead-spots.md`).
//!
//! A field is a grid of cells over the sensors' reach, each cell a column of air at **one
//! stated height above the ground**. A cell is a **dead spot** when at least one searching
//! or tracking sensor claims it, by range, sector and elevation band, and none of those
//! sensors can see it because the ground between is in the way (D-146): a claim the map
//! makes and the terrain denies. A cell nobody claims is not a dead spot, because the
//! rings already say so; a cell with no ground under it is no data, because unknown is not
//! clear (DN-34 §4).
//!
//! **Two ways to the same answer.** [`shadow_field_reference`] is the definition: for every
//! claimed cell, a sampled line of sight from each claiming sensor to the probe. It costs
//! cells times sensors times samples a ray, so it is the oracle and a coarse-cell
//! fallback. [`shadow_field`] gets the same answer by a radial horizon sweep: one ray per
//! azimuth from each sensor, marched outward a terrain cell at a time keeping the running
//! highest elevation of the ground seen so far, so each cell is decided about once a
//! sensor (DN-34 §6.3). The two disagree only on cells within about a cell of a horizon
//! line, which `tests/shadow_geometry.rs` holds them to.
//!
//! **Geometric line of sight.** The placed terrain carries the Earth's curvature in its
//! ups, so a straight ray in the frame is the optical line of sight over a curved earth;
//! no refraction is added, which reports a little more shadow than a radar has (DN-34
//! §6.4). The probe height is added along the frame's up.

use crate::terrain::{fit_cell, HeightField, HeightFieldLineOfSight};
use crate::{CoverageVolume, LineOfSight};
use gungnir_model::SensorId;

/// The most cells a shadow field holds (DN-34 §9): about a 45 km square at 30 m.
pub const DEAD_SPOT_CELL_BUDGET: usize = 2_000_000;

/// What a shadow field was computed with, carried on the result (DN-12 §5 rule 4).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShadowParameters {
    /// The probe's height above the ground under it, metres.
    pub height_agl_m: f64,
    /// The side of a cell, metres.
    pub cell_m: f64,
}

/// One cell of the field (DN-34 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellSight {
    /// Claimed by a sensor's reach, but the DEM has no height here, or the cell lies off
    /// it: unknown, which is not clear.
    NoData,
    /// No active sensor's volume contains the probe.
    Unclaimed,
    /// Claimed by `claimed` sensors, seen by none: **the dead spot**.
    Shadow { claimed: u8 },
    /// Seen by exactly one sensor.
    Single { sensor: SensorId },
    /// Seen by two or more.
    Fused { seeing: u8 },
}

impl CellSight {
    /// Whether any active sensor claims this cell's probe.
    #[must_use]
    pub fn is_claimed(self) -> bool {
        matches!(
            self,
            CellSight::Shadow { .. } | CellSight::Single { .. } | CellSight::Fused { .. }
        )
    }
}

/// One sensor's own answer for one cell, so PN-10 can show that sensor's shadow alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sight {
    /// Not in this sensor's volume.
    Unclaimed,
    /// In its reach, over ground nobody measured.
    NoData,
    /// In its volume and hidden from it by the terrain.
    Hidden,
    /// In its volume and in its sight.
    Seen,
}

/// A shadow field: the cells, north row first, and each sensor's own sight.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowField {
    pub parameters: ShadowParameters,
    /// The cell size asked for when the budget made it coarser (DN-34 §9), else `None`.
    pub clamped_from_m: Option<f64>,
    /// ENU east and north of the south-west corner, metres.
    pub origin_enu: [f64; 2],
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<CellSight>,
    /// Each sensor's own sight, in the order the sensors were given.
    pub per_sensor: Vec<(SensorId, Vec<Sight>)>,
}

/// Areas, square metres, for the layer line, PN-11 and PN-16 (DN-34 §5).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShadowMeasure {
    /// Claimed by at least one sensor: dead spots, single-sensor and fused together.
    pub claimed_m2: f64,
    pub shadow_m2: f64,
    pub single_m2: f64,
    pub no_data_m2: f64,
}

impl ShadowMeasure {
    /// The measure in words, at `height_agl_m` (DN-34 §7 item 1).
    #[must_use]
    pub fn sentence(&self, height_agl_m: f64) -> String {
        let km2 = |m2: f64| m2 / 1.0e6;
        format!(
            "dead spots at {height_agl_m:.0} m above ground: {:.1} km\u{b2} of {:.1} km\u{b2} claimed; \
             {:.1} km\u{b2} single-sensor; {:.1} km\u{b2} no data",
            km2(self.shadow_m2),
            km2(self.claimed_m2),
            km2(self.single_m2),
            km2(self.no_data_m2)
        )
    }
}

/// The raster a field is computed on.
#[derive(Debug, Clone, Copy)]
struct Grid {
    origin: [f64; 2],
    cell: f64,
    rows: usize,
    columns: usize,
}

impl Grid {
    fn len(&self) -> usize {
        self.rows * self.columns
    }

    // A row index is below `rows`, which the budget bounds.
    #[allow(clippy::cast_precision_loss)]
    fn centre(&self, index: usize) -> [f64; 2] {
        let (row, column) = (index / self.columns, index % self.columns);
        let south = self.rows - 1 - row;
        [
            self.origin[0] + (column as f64 + 0.5) * self.cell,
            self.origin[1] + (south as f64 + 0.5) * self.cell,
        ]
    }

    // Checked against the grid's own size before it is converted.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn index_at(&self, east: f64, north: f64) -> Option<usize> {
        let x = ((east - self.origin[0]) / self.cell).floor();
        let y = ((north - self.origin[1]) / self.cell).floor();
        if !(x >= 0.0 && y >= 0.0 && x < self.columns as f64 && y < self.rows as f64) {
            return None;
        }
        let (column, south) = (x as usize, y as usize);
        Some((self.rows - 1 - south) * self.columns + column)
    }

    /// The cells whose centres lie within `reach` of `at` in east and north.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn within(&self, at: [f64; 3], reach: f64) -> impl Iterator<Item = usize> + '_ {
        // The indices along one axis whose centres lie in `a..=b`; empty when none do,
        // including a reach that lies wholly off the grid.
        let span = |start: f64, a: f64, b: f64, len: usize| -> std::ops::Range<usize> {
            let first = ((a - start) / self.cell - 0.5).ceil().max(0.0);
            let last = ((b - start) / self.cell - 0.5)
                .floor()
                .min(len as f64 - 1.0);
            if first.is_finite() && last.is_finite() && first <= last {
                first as usize..last as usize + 1
            } else {
                0..0
            }
        };
        let columns = span(self.origin[0], at[0] - reach, at[0] + reach, self.columns);
        let souths = span(self.origin[1], at[1] - reach, at[1] + reach, self.rows);
        let (width, rows) = (self.columns, self.rows);
        souths.flat_map(move |s| columns.clone().map(move |c| (rows - 1 - s) * width + c))
    }
}

/// The cell side a field over these sensors would have at `wanted` metres, made coarser if
/// it would hold more than [`DEAD_SPOT_CELL_BUDGET`] cells; and whether it was (DN-34 §9).
#[must_use]
pub fn dead_spot_cell_m(sensors: &[(SensorId, CoverageVolume)], wanted: f64) -> (f64, bool) {
    match reach_box(sensors) {
        Some([lo, hi]) => fit_cell(hi[0] - lo[0], hi[1] - lo[1], wanted, DEAD_SPOT_CELL_BUDGET),
        None => (wanted, false),
    }
}

/// The east-north box every sensor's range reaches into.
fn reach_box(sensors: &[(SensorId, CoverageVolume)]) -> Option<[[f64; 2]; 2]> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for (_, v) in sensors {
        let r = v.max_range_m;
        if !(r.is_finite() && r > 0.0 && v.sensor_enu.iter().all(|x| x.is_finite())) {
            continue;
        }
        lo = [
            lo[0].min(v.sensor_enu[0] - r),
            lo[1].min(v.sensor_enu[1] - r),
        ];
        hi = [
            hi[0].max(v.sensor_enu[0] + r),
            hi[1].max(v.sensor_enu[1] + r),
        ];
    }
    (lo[0] < hi[0] && lo[1] < hi[1]).then_some([lo, hi])
}

/// Whether `volume` reaches the column of air at `east`, `north` by range and sector
/// alone: what a cell with no ground to stand a probe on is judged by, so a claimed stretch
/// of missing DEM reads as no data rather than as unclaimed.
fn reaches_column(volume: &CoverageVolume, east: f64, north: f64) -> bool {
    let (de, dn) = (east - volume.sensor_enu[0], north - volume.sensor_enu[1]);
    if (de * de + dn * dn).sqrt() > volume.max_range_m {
        return false;
    }
    match (volume.azimuth, gungnir_model::bearing_rad(de, dn)) {
        (Some(sector), Some(bearing)) => sector.contains(bearing),
        _ => true,
    }
}

/// The field for these sensors over this terrain at this height, by the radial horizon
/// sweep (module documentation; DN-34 §6.3). Pure, like `combined_coverage`, so a property
/// test drives it without a registry.
///
/// The sensors are the ones that count: the caller passes searching and tracking sensors
/// only (`coverage_from_registry`), as DN-12 §5 rule 1 holds. A height or cell that is not
/// finite and positive gives an empty field, never a guessed one.
#[must_use]
pub fn shadow_field(
    sensors: &[(SensorId, CoverageVolume)],
    terrain: &HeightField,
    parameters: ShadowParameters,
) -> ShadowField {
    compute(sensors, terrain, parameters, Method::Sweep)
}

/// The same field by the definition: a sampled line of sight from each claiming sensor to
/// each claimed cell's probe (DN-34 §6.2). The oracle [`shadow_field`] is tested against,
/// and a legitimate path at coarse cells.
#[must_use]
pub fn shadow_field_reference(
    sensors: &[(SensorId, CoverageVolume)],
    terrain: &HeightField,
    parameters: ShadowParameters,
) -> ShadowField {
    compute(sensors, terrain, parameters, Method::Reference)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Sweep,
    Reference,
}

// Cell counts are bounded by the budget; a u8 count saturates by construction.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn compute(
    sensors: &[(SensorId, CoverageVolume)],
    terrain: &HeightField,
    parameters: ShadowParameters,
    method: Method,
) -> ShadowField {
    let empty = |parameters| ShadowField {
        parameters,
        clamped_from_m: None,
        origin_enu: [0.0, 0.0],
        rows: 0,
        columns: 0,
        cells: Vec::new(),
        per_sensor: sensors.iter().map(|(id, _)| (*id, Vec::new())).collect(),
    };
    let valid = |x: f64| x.is_finite() && x > 0.0;
    if !(valid(parameters.height_agl_m) && valid(parameters.cell_m)) {
        return empty(parameters);
    }
    let Some([lo, hi]) = reach_box(sensors) else {
        return empty(parameters);
    };
    let asked = parameters.cell_m;
    let (cell, clamped) = fit_cell(hi[0] - lo[0], hi[1] - lo[1], asked, DEAD_SPOT_CELL_BUDGET);
    let grid = Grid {
        origin: lo,
        cell,
        columns: (((hi[0] - lo[0]) / cell).ceil() as usize).max(1),
        rows: (((hi[1] - lo[1]) / cell).ceil() as usize).max(1),
    };
    let parameters = ShadowParameters {
        height_agl_m: parameters.height_agl_m,
        cell_m: cell,
    };

    // The probe over every cell: the ground under its centre plus the stated height, or
    // `NaN` where there is no ground.
    let probes: Vec<f64> = (0..grid.len())
        .map(|i| {
            let [e, n] = grid.centre(i);
            terrain
                .up_at(e, n)
                .map_or(f64::NAN, |g| g + parameters.height_agl_m)
        })
        .collect();

    let per_sensor: Vec<(SensorId, Vec<Sight>)> = sensors
        .iter()
        .map(|(id, volume)| {
            let mut sights = claims(volume, &grid, &probes);
            decide(volume, terrain, &grid, &probes, &mut sights, method);
            (*id, sights)
        })
        .collect();

    let cells = (0..grid.len())
        .map(|i| {
            let (mut claimed, mut seeing, mut no_data) = (0u32, 0u32, false);
            let mut only = None;
            for (id, sights) in &per_sensor {
                match sights[i] {
                    Sight::Unclaimed => {}
                    Sight::NoData => no_data = true,
                    Sight::Hidden => claimed += 1,
                    Sight::Seen => {
                        claimed += 1;
                        seeing += 1;
                        only = Some(*id);
                    }
                }
            }
            let small = |n: u32| n.min(u32::from(u8::MAX)) as u8;
            match (claimed, seeing) {
                (0, _) if no_data => CellSight::NoData,
                (0, _) => CellSight::Unclaimed,
                (c, 0) => CellSight::Shadow { claimed: small(c) },
                (_, 1) => only.map_or(CellSight::Unclaimed, |sensor| CellSight::Single { sensor }),
                (_, s) => CellSight::Fused { seeing: small(s) },
            }
        })
        .collect();

    ShadowField {
        parameters,
        clamped_from_m: clamped.then_some(asked),
        origin_enu: grid.origin,
        rows: grid.rows as u32,
        columns: grid.columns as u32,
        cells,
        per_sensor,
    }
}

/// Which cells `volume` claims, before line of sight: `Hidden` for a claimed probe until
/// something sees it, `NoData` for a reached column with no ground, `Unclaimed` otherwise.
fn claims(volume: &CoverageVolume, grid: &Grid, probes: &[f64]) -> Vec<Sight> {
    let mut sights = vec![Sight::Unclaimed; grid.len()];
    if !(volume.max_range_m.is_finite() && volume.max_range_m > 0.0) {
        return sights;
    }
    for i in grid.within(volume.sensor_enu, volume.max_range_m) {
        let [e, n] = grid.centre(i);
        let up = probes[i];
        sights[i] = if up.is_nan() {
            if reaches_column(volume, e, n) {
                Sight::NoData
            } else {
                Sight::Unclaimed
            }
        } else if volume.covers([e, n, up]) {
            Sight::Hidden
        } else {
            Sight::Unclaimed
        };
    }
    sights
}

/// Decide, for every claimed cell, whether `volume`'s sensor sees its probe.
// Ray and step counts are bounded by the range over the cell, a few thousand.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn decide(
    volume: &CoverageVolume,
    terrain: &HeightField,
    grid: &Grid,
    probes: &[f64],
    sights: &mut [Sight],
    method: Method,
) {
    let s = volume.sensor_enu;
    let los = HeightFieldLineOfSight { field: terrain };
    let by_reference = |i: usize| {
        let [e, n] = grid.centre(i);
        los.visible(s, [e, n, probes[i]])
    };
    // A sensor below its own ground sees nothing: the reference's first sample says so.
    if terrain.up_at(s[0], s[1]).is_some_and(|g| s[2] < g) {
        return;
    }
    let mut reached = vec![false; grid.len()];
    if method == Method::Sweep {
        let reach = volume.max_range_m;
        // The ray is marched every half terrain cell, so no field cell is passed over, and
        // reads the ground at every crossing of a line of cell centres too, where the
        // surface bends and a peak sits, as the reference does.
        let step = (terrain.cell_m / 2.0).max(f64::EPSILON);
        // **The sweep's own error bound.** The ray that decides a cell runs `offset` to one
        // side of the true line at the cell, and proportionally less nearer the sensor, so
        // the ground it reads at the line's binding point can differ by the steepest slope
        // times that offset, plus half a step between the points it reads. Over the cell's
        // distance that is an error in elevation slope; a cell whose margin is inside it is
        // not the sweep's to decide, and the reference decides it.
        let gradient = terrain.max_gradient();
        // Adjacent rays no more than one field cell apart at the far edge.
        let rays = ((std::f64::consts::TAU * reach / grid.cell).ceil() as usize).max(16);
        let end = reach + step;
        let mut best = vec![f64::INFINITY; grid.len()];
        for ray in 0..rays {
            let bearing = std::f64::consts::TAU * ray as f64 / rays as f64;
            let (de, dn) = (bearing.sin(), bearing.cos());
            let mut horizon = f64::NEG_INFINITY;
            let mut previous = None;
            let mut crossings = terrain.crossings([s[0], s[1]], [de, dn], end).peekable();
            let mut k = 1usize;
            loop {
                let half = k as f64 * step;
                let d = match crossings.peek() {
                    Some(&c) if c < half => {
                        crossings.next();
                        c
                    }
                    _ => {
                        k += 1;
                        half
                    }
                };
                if d > end {
                    break;
                }
                let (e, n) = (s[0] + de * d, s[1] + dn * d);
                let here = grid.index_at(e, n);
                // A cell is decided where the ray enters it, so the horizon holds only the
                // ground before it.
                if let Some(i) = here.filter(|i| Some(*i) != previous) {
                    if matches!(sights[i], Sight::Hidden | Sight::Seen) {
                        let [ce, cn] = grid.centre(i);
                        let (ce, cn) = (ce - s[0], cn - s[1]);
                        let offset = (ce * dn - cn * de).abs();
                        let along = ce * de + cn * dn;
                        if along > 0.0 && offset < best[i] {
                            best[i] = offset;
                            let dc = (ce * ce + cn * cn).sqrt();
                            let slope = (probes[i] - s[2]) / dc;
                            let margin = gradient * (offset + step) / dc;
                            // Undecided until a closer ray is sure, or the reference says.
                            reached[i] = (slope - horizon).abs() > margin;
                            sights[i] = if slope >= horizon {
                                Sight::Seen
                            } else {
                                Sight::Hidden
                            };
                        }
                    }
                }
                previous = here;
                if let Some(h) = terrain.up_at(e, n) {
                    horizon = horizon.max((h - s[2]) / d);
                }
            }
        }
    }
    // The reference decides every cell the sweep did not: all of them for the reference
    // itself, and for the sweep the few its rays missed and those inside its error bound.
    for i in 0..grid.len() {
        if !reached[i] && matches!(sights[i], Sight::Hidden | Sight::Seen) {
            sights[i] = if by_reference(i) {
                Sight::Seen
            } else {
                Sight::Hidden
            };
        }
    }
}

impl ShadowField {
    /// The cell under an east and north, if the field covers it.
    #[must_use]
    pub fn cell_at(&self, east: f64, north: f64) -> Option<CellSight> {
        self.grid().index_at(east, north).map(|i| self.cells[i])
    }

    /// The centre of cell `index`, north row first.
    #[must_use]
    pub fn centre(&self, index: usize) -> [f64; 2] {
        self.grid().centre(index)
    }

    fn grid(&self) -> Grid {
        Grid {
            origin: self.origin_enu,
            cell: self.parameters.cell_m,
            rows: self.rows as usize,
            columns: self.columns as usize,
        }
    }

    /// The areas of each kind of cell.
    #[must_use]
    pub fn measure(&self) -> ShadowMeasure {
        measure_cells(&self.cells, self.parameters.cell_m)
    }
}

/// The areas of each kind of cell in `cells`, each `cell_m` on a side: a field's own, or
/// one sensor's from [`ShadowField::for_sensor`].
#[must_use]
pub fn measure_cells(cells: &[CellSight], cell_m: f64) -> ShadowMeasure {
    let area = cell_m * cell_m;
    let mut m = ShadowMeasure::default();
    for cell in cells {
        match cell {
            CellSight::NoData => m.no_data_m2 += area,
            CellSight::Unclaimed => {}
            CellSight::Shadow { .. } => {
                m.shadow_m2 += area;
                m.claimed_m2 += area;
            }
            CellSight::Single { .. } => {
                m.single_m2 += area;
                m.claimed_m2 += area;
            }
            CellSight::Fused { .. } => m.claimed_m2 += area,
        }
    }
    m
}

impl ShadowField {
    /// One sensor's own shadow as cells (DN-34 §7 item 6): hidden from it is a dead spot,
    /// its no data is no data, and everything else is drawn as nothing. `None` for a sensor
    /// this field was not computed with.
    #[must_use]
    pub fn for_sensor(&self, sensor: SensorId) -> Option<Vec<CellSight>> {
        let (_, sights) = self.per_sensor.iter().find(|(id, _)| *id == sensor)?;
        Some(
            sights
                .iter()
                .map(|s| match s {
                    Sight::Hidden => CellSight::Shadow { claimed: 1 },
                    Sight::NoData => CellSight::NoData,
                    Sight::Seen | Sight::Unclaimed => CellSight::Unclaimed,
                })
                .collect(),
        )
    }
}
