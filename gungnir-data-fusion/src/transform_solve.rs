// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! CPU-side small linear solve (nalgebra) for the ICP transform-estimation step
//! (GAP-024). §3.4 step 5: the linear system is tiny, so reading back a small GPU buffer
//! and solving here each iteration is far simpler than implementing SVD in WGSL.
//!
//! The estimate is Kabsch's: with source and target centred on their centroids and the
//! cross-covariance `H = Σ sᵢ tᵢᵀ`, the rotation that best maps source onto target in the
//! least-squares sense is `R = V Uᵀ` from `H = U Σ Vᵀ`, with the sign of the last column of
//! `V` flipped when `det(V Uᵀ) < 0` so a reflection is never reported as a rotation.

use nalgebra::{Isometry3, Matrix3, Point3, Rotation3, Translation3, UnitQuaternion, Vector3};

use crate::FusionError;

/// The rotation part of the rigid transform from a cross-covariance matrix.
///
/// Translation is not recoverable from the cross-covariance alone, so the returned
/// isometry carries a **zero translation**; [`rigid_transform_between`] composes the
/// full transform from the centroids. Kept as the GPU path's read-back solve.
///
/// # Errors
///
/// `FusionError::Degenerate` when the covariance is not finite or the SVD does not
/// yield a proper rotation (all points collinear, for instance).
pub fn solve_rigid_transform(
    cross_covariance: &Matrix3<f32>,
) -> Result<Isometry3<f32>, FusionError> {
    if !cross_covariance.iter().all(|v| v.is_finite()) {
        return Err(FusionError::Degenerate(
            "the cross-covariance is not finite".into(),
        ));
    }
    let svd = cross_covariance.svd(true, true);
    let (Some(u), Some(v_t)) = (svd.u, svd.v_t) else {
        return Err(FusionError::Degenerate(
            "the cross-covariance has no singular value decomposition".into(),
        ));
    };
    let mut v = v_t.transpose();
    let mut rotation = v * u.transpose();
    if rotation.determinant() < 0.0 {
        // A reflection: flip the least significant axis.
        let mut column = v.column(2).into_owned();
        column = -column;
        v.set_column(2, &column);
        rotation = v * u.transpose();
    }
    if !rotation.iter().all(|x| x.is_finite()) || (rotation.determinant() - 1.0).abs() > 1e-3 {
        return Err(FusionError::Degenerate(
            "the solve did not yield a proper rotation".into(),
        ));
    }
    let rotation =
        UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(rotation));
    Ok(Isometry3::from_parts(Translation3::identity(), rotation))
}

/// The rigid transform mapping `source[i]` onto `target[i]` for paired points.
///
/// # Errors
///
/// `FusionError::EmptyInput` for fewer than three pairs or mismatched lengths;
/// `FusionError::Degenerate` as [`solve_rigid_transform`].
pub fn rigid_transform_between(
    source: &[[f32; 3]],
    target: &[[f32; 3]],
) -> Result<Isometry3<f32>, FusionError> {
    if source.len() != target.len() || source.len() < 3 {
        return Err(FusionError::EmptyInput(format!(
            "{} source and {} target points; three matched pairs are the least a rigid \
             transform can be solved from",
            source.len(),
            target.len()
        )));
    }
    #[allow(clippy::cast_precision_loss)]
    let n = source.len() as f32;
    let centroid = |points: &[[f32; 3]]| {
        points.iter().fold(Vector3::zeros(), |acc, p| {
            acc + Vector3::new(p[0], p[1], p[2])
        }) / n
    };
    let cs = centroid(source);
    let ct = centroid(target);
    let mut h = Matrix3::zeros();
    for (s, t) in source.iter().zip(target) {
        let ds = Vector3::new(s[0], s[1], s[2]) - cs;
        let dt = Vector3::new(t[0], t[1], t[2]) - ct;
        h += ds * dt.transpose();
    }
    let rotation = solve_rigid_transform(&h)?.rotation;
    let translation = ct - rotation * cs;
    Ok(Isometry3::from_parts(
        Translation3::from(translation),
        rotation,
    ))
}

/// Apply an isometry to a point.
#[must_use]
pub fn apply(transform: &Isometry3<f32>, p: [f32; 3]) -> [f32; 3] {
    let q = transform * Point3::new(p[0], p[1], p[2]);
    [q.x, q.y, q.z]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> Vec<[f32; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 3.0],
            [1.0, 1.0, 1.0],
            [2.0, -1.0, 0.5],
        ]
    }

    fn known() -> Isometry3<f32> {
        Isometry3::from_parts(
            Translation3::new(0.5, -1.5, 2.0),
            UnitQuaternion::from_euler_angles(0.1, -0.2, 0.7),
        )
    }

    #[test]
    fn a_known_transform_is_recovered_from_matched_pairs() {
        let source = cloud();
        let target: Vec<[f32; 3]> = source.iter().map(|p| apply(&known(), *p)).collect();
        let solved = rigid_transform_between(&source, &target).expect("solved");
        for (s, t) in source.iter().zip(&target) {
            let mapped = apply(&solved, *s);
            for i in 0..3 {
                assert!((mapped[i] - t[i]).abs() < 1e-4, "{mapped:?} vs {t:?}");
            }
        }
    }

    #[test]
    fn a_reflection_is_never_reported_as_a_rotation() {
        let source = cloud();
        let target: Vec<[f32; 3]> = source.iter().map(|p| [-p[0], p[1], p[2]]).collect();
        let solved = rigid_transform_between(&source, &target).expect("a best rotation exists");
        assert!((solved.rotation.to_rotation_matrix().matrix().determinant() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn too_few_or_non_finite_input_is_an_error_not_an_identity() {
        assert!(matches!(
            rigid_transform_between(&[[0.0; 3]], &[[0.0; 3]]),
            Err(FusionError::EmptyInput(_))
        ));
        assert!(matches!(
            solve_rigid_transform(&Matrix3::from_element(f32::NAN)),
            Err(FusionError::Degenerate(_))
        ));
    }
}
