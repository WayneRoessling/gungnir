// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point-cloud surface normal estimation (GAP-024): the direction of least variance in
//! each point's `k` nearest neighbours, which is the outward axis of the plane that
//! best fits the local neighbourhood. This is what `PointBuffer::normals` waits for
//! (`gungnir-data`, GAP-023) and what a point-to-plane registration step would read;
//! nothing here changes `CpuIcp`, which stays point-to-point until that step is built.
//!
//! Brute-force k-nearest-neighbour search, matching `cpu_reference`'s own stated
//! reasoning for its correspondence search: a spatial index is an optimisation for a
//! cloud large enough to need one, and the reference should stay legible.

use gungnir_data::pointcloud::PointBuffer;
use nalgebra::{Matrix3, Vector3};

use crate::FusionError;

/// Estimate one unit normal per point in `cloud`, each from its `k` nearest neighbours
/// (the point itself included, so `k` is the neighbourhood size rather than a count of
/// others).
///
/// **The sign is not resolved.** Principal component analysis gives an axis, not a
/// direction: nothing here knows which way is "outward" on an unoriented point cloud,
/// and guessing (by a viewpoint, say) is a further step this function deliberately does
/// not take. A caller that needs a consistent orientation must resolve one itself.
///
/// # Errors
///
/// [`FusionError::EmptyInput`] when `cloud` holds fewer than `k` points -- there is no
/// neighbourhood of that size to estimate from, for any point. [`FusionError::Degenerate`]
/// when some point's `k` nearest neighbours are coincident or exactly collinear, so the
/// local covariance has no single smallest-eigenvalue axis for a normal to be.
pub fn estimate_normals(cloud: &PointBuffer, k: usize) -> Result<Vec<[f32; 3]>, FusionError> {
    let n = cloud.positions.len();
    if k == 0 || n < k {
        return Err(FusionError::EmptyInput(format!(
            "{n} point(s), fewer than the {k} a neighbourhood needs"
        )));
    }
    (0..n)
        .map(|i| normal_at(cloud, i, k))
        .collect::<Result<Vec<_>, _>>()
}

/// The k-nearest-neighbour indices of `cloud.positions[i]`, itself included, nearest
/// first.
fn k_nearest(cloud: &PointBuffer, i: usize, k: usize) -> Vec<usize> {
    let p = cloud.positions[i];
    let mut by_distance: Vec<(usize, f32)> = cloud
        .positions
        .iter()
        .enumerate()
        .map(|(j, q)| {
            let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
            (j, d)
        })
        .collect();
    by_distance.sort_by(|a, b| a.1.total_cmp(&b.1));
    by_distance.truncate(k);
    by_distance.into_iter().map(|(j, _)| j).collect()
}

/// The unit normal at point `i`: the eigenvector of the smallest eigenvalue of the
/// covariance of its `k` nearest neighbours.
fn normal_at(cloud: &PointBuffer, i: usize, k: usize) -> Result<[f32; 3], FusionError> {
    let neighbours = k_nearest(cloud, i, k);
    #[allow(clippy::cast_precision_loss)]
    let centroid = neighbours
        .iter()
        .map(|&j| Vector3::from(cloud.positions[j]))
        .sum::<Vector3<f32>>()
        / neighbours.len() as f32;
    let covariance: Matrix3<f32> = neighbours
        .iter()
        .map(|&j| {
            let d = Vector3::from(cloud.positions[j]) - centroid;
            d * d.transpose()
        })
        .sum();
    if !covariance.iter().all(|v| v.is_finite()) {
        return Err(FusionError::Degenerate(format!(
            "point {i}'s neighbourhood covariance is not finite"
        )));
    }
    let eigen = covariance.symmetric_eigen();
    let mut by_value: Vec<(usize, f32)> = eigen.eigenvalues.iter().copied().enumerate().collect();
    by_value.sort_by(|a, b| a.1.total_cmp(&b.1));
    let min_index = by_value[0].0;
    let (lambda1, lambda2) = (by_value[1].1, by_value[2].1);
    // The eigenvector's own norm says nothing here: `symmetric_eigen` returns a unit
    // eigenvector for every eigenvalue, degenerate or not, so an arbitrary axis of a
    // coincident or collinear neighbourhood would pass a norm check undetected. What
    // actually distinguishes a well-posed local plane (a real 2D spread, however small
    // its out-of-plane residual `lambda0`) from a collinear one (spread in only one
    // direction, so `lambda1` is small too and no single axis is "the" normal) is
    // whether `lambda1` clears `lambda2` by enough to call the neighbourhood planar.
    if lambda2 < 1e-12 {
        return Err(FusionError::Degenerate(format!(
            "point {i}'s neighbourhood is coincident: no variance in any direction"
        )));
    }
    if lambda1 / lambda2 < 1e-6 {
        return Err(FusionError::Degenerate(format!(
            "point {i}'s neighbourhood is collinear: variance in only one direction, \
             so no single normal axis is defined"
        )));
    }
    let normal = eigen.eigenvectors.column(min_index).into_owned();
    Ok([normal.x, normal.y, normal.z])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same analytic surface `cpu_reference`'s tests use: `z = sin(1.3x + 0.7y) *
    /// 0.4` over a 6x6 grid at 0.5 m spacing, which gives a closed-form normal from its
    /// gradient to check the estimate against.
    fn grid() -> PointBuffer {
        let mut positions = Vec::new();
        for i in 0..6 {
            for j in 0..6 {
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (i as f32 * 0.5, j as f32 * 0.5);
                positions.push([x, y, (x * 1.3 + y * 0.7).sin() * 0.4]);
            }
        }
        PointBuffer {
            positions,
            ..PointBuffer::default()
        }
    }

    /// The graph `z = f(x, y)` has surface normal proportional to `(-df/dx, -df/dy, 1)`.
    #[allow(clippy::similar_names)]
    fn analytic_normal(x: f32, y: f32) -> Vector3<f32> {
        let d = 1.3 * x + 0.7 * y;
        let df_dx = 1.3 * d.cos() * 0.4;
        let df_dy = 0.7 * d.cos() * 0.4;
        Vector3::new(-df_dx, -df_dy, 1.0).normalize()
    }

    #[test]
    fn every_point_gets_a_unit_normal() {
        let normals = estimate_normals(&grid(), 9).expect("36 points, k=9, well posed");
        assert_eq!(normals.len(), 36);
        for n in &normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-4, "not a unit vector: {n:?}");
        }
    }

    #[test]
    fn an_interior_points_normal_agrees_with_the_surfaces_own_gradient() {
        // Interior points (not on the grid's outer ring) get a symmetric, full 3x3
        // neighbourhood at k=9, whose bias against the true gradient is entirely the
        // surface's own second-order curvature -- a first-order (planar) local fit
        // cannot see that term by construction. Cross-checked independently in Python
        // against the same analytic surface: worst case over this interior set is
        // ~0.9986 (about 3 degrees), at (1.5, 1.5) where curvature is locally
        // greatest. 0.995 (about 5.7 degrees) leaves headroom above that without
        // being loose enough to pass a real defect -- a wrong eigenvector or a sign/
        // axis mix-up misses by tens of degrees, not single digits.
        let cloud = grid();
        let normals = estimate_normals(&cloud, 9).expect("well posed");
        for i in 1..5 {
            for j in 1..5 {
                let idx = i * 6 + j;
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (i as f32 * 0.5, j as f32 * 0.5);
                let want = analytic_normal(x, y);
                let got = Vector3::from(normals[idx]);
                // PCA gives an axis, not a direction: compare magnitudes, not sign.
                let agreement = got.dot(&want).abs();
                assert!(
                    agreement > 0.995,
                    "point ({x}, {y}): estimated {got:?} vs analytic {want:?}, \
                     agreement {agreement}"
                );
            }
        }
    }

    #[test]
    fn every_points_normal_is_at_least_roughly_right() {
        // Including the boundary ring. A corner's 9 nearest neighbours reach two full
        // grid steps out in one direction to make up for having none beyond the edge,
        // which is a materially different (and more lopsided) neighbourhood than an
        // interior point's -- not a bug, just a worse-conditioned fit. Cross-checked
        // in Python: the worst case over the whole 6x6 grid is ~0.878 (about 29
        // degrees), at corner (2.5, 2.5). 0.85 (about 32 degrees) is this test's
        // floor precisely because a corner's asymmetric neighbourhood earns a looser
        // bound than the interior check above, not because the algorithm is in doubt.
        let cloud = grid();
        let normals = estimate_normals(&cloud, 9).expect("well posed");
        for i in 0..6 {
            for j in 0..6 {
                let idx = i * 6 + j;
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (i as f32 * 0.5, j as f32 * 0.5);
                let want = analytic_normal(x, y);
                let got = Vector3::from(normals[idx]);
                let agreement = got.dot(&want).abs();
                assert!(
                    agreement > 0.85,
                    "point ({x}, {y}): estimated {got:?} vs analytic {want:?}, \
                     agreement {agreement}"
                );
            }
        }
    }

    #[test]
    fn a_cloud_smaller_than_k_is_refused_rather_than_given_a_partial_neighbourhood() {
        let cloud = PointBuffer {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            ..PointBuffer::default()
        };
        assert!(matches!(
            estimate_normals(&cloud, 9),
            Err(FusionError::EmptyInput(_))
        ));
    }

    #[test]
    fn an_empty_cloud_is_refused_rather_than_returning_an_empty_vec_that_looks_like_success() {
        assert!(matches!(
            estimate_normals(&PointBuffer::default(), 8),
            Err(FusionError::EmptyInput(_))
        ));
    }

    #[test]
    fn k_of_zero_is_refused_rather_than_an_undefined_neighbourhood() {
        assert!(matches!(
            estimate_normals(&grid(), 0),
            Err(FusionError::EmptyInput(_))
        ));
    }

    #[test]
    fn coincident_points_are_degenerate_not_an_arbitrary_axis() {
        // Every neighbour identical to the query point: zero covariance, no single
        // smallest-eigenvalue axis to call a normal.
        let cloud = PointBuffer {
            positions: vec![[1.0, 1.0, 1.0]; 9],
            ..PointBuffer::default()
        };
        assert!(matches!(
            estimate_normals(&cloud, 9),
            Err(FusionError::Degenerate(_))
        ));
    }

    #[test]
    fn collinear_points_are_degenerate_not_an_arbitrary_axis() {
        // Every neighbour on one line: two of the three eigenvalues are zero, so the
        // "smallest" one is not unique and any normal reported would be arbitrary.
        #[allow(clippy::cast_precision_loss)]
        let positions = (0_i32..9).map(|i| [i as f32, 0.0, 0.0]).collect();
        let cloud = PointBuffer {
            positions,
            ..PointBuffer::default()
        };
        assert!(matches!(
            estimate_normals(&cloud, 9),
            Err(FusionError::Degenerate(_))
        ));
    }
}
