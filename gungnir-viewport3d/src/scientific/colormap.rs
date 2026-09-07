// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Standard colormaps -- palette shared with `gungnir_ui::theme`.
//!
//! **Viridis is built (2026-09-06, GAP-023)** as a nine-stop table with linear
//! interpolation, which is within a few units of the reference at every stop and is what
//! a scalar field on a mesh needs; turbo is not, and nothing asks for it.

/// Viridis at nine stops from 0 to 1 (matplotlib's table, rounded to 8 bits).
const VIRIDIS: [[u8; 3]; 9] = [
    [68, 1, 84],
    [72, 40, 120],
    [62, 74, 137],
    [49, 104, 142],
    [38, 130, 142],
    [31, 158, 137],
    [53, 183, 121],
    [109, 205, 89],
    [253, 231, 37],
];

/// Viridis for `t` in `[0, 1]`; a non-finite `t` reads as 0 rather than a random cell.
#[must_use]
pub fn viridis(t: f32) -> [u8; 3] {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Nine stops: the casts are exact.
    #[allow(clippy::cast_precision_loss)]
    let scaled = t * (VIRIDIS.len() - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (scaled.floor() as usize).min(VIRIDIS.len() - 2);
    #[allow(clippy::cast_precision_loss)]
    let f = scaled - i as f32;
    let (a, b) = (VIRIDIS[i], VIRIDIS[i + 1]);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mix = |k: usize| (f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * f).round() as u8;
    [mix(0), mix(1), mix(2)]
}

/// The colour of `value` on the viridis ramp between `min` and `max`.
///
/// # Errors
///
/// [`crate::ViewportError::MalformedMesh`] for a non-finite value or a range that is
/// not a range. **An error rather than a colour**, because any default would have been a
/// scalar field rendered in a single flat shade, which reads as a measurement rather
/// than as nothing.
pub fn scalar_to_rgba(value: f32, min: f32, max: f32) -> Result<[u8; 4], crate::ViewportError> {
    if !value.is_finite() || !min.is_finite() || !max.is_finite() {
        return Err(crate::ViewportError::MalformedMesh {
            reason: "a non-finite scalar cannot be coloured".into(),
        });
    }
    if max <= min {
        return Err(crate::ViewportError::MalformedMesh {
            reason: format!("the scalar range [{min}, {max}] is not a range"),
        });
    }
    let [r, g, b] = viridis((value - min) / (max - min));
    Ok([r, g, b, 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viridis_runs_from_purple_to_yellow() {
        assert_eq!(viridis(0.0), [68, 1, 84]);
        assert_eq!(viridis(1.0), [253, 231, 37]);
        let mid = viridis(0.5);
        assert!(mid[1] > mid[0], "the middle is teal: {mid:?}");
        assert_eq!(viridis(f32::NAN), viridis(0.0));
    }

    #[test]
    fn a_bad_scalar_or_range_is_an_error_not_a_flat_shade() {
        assert!(scalar_to_rgba(f32::NAN, 0.0, 1.0).is_err());
        assert!(scalar_to_rgba(0.5, 1.0, 1.0).is_err());
        assert_eq!(
            scalar_to_rgba(0.0, 0.0, 1.0).expect("low"),
            [68, 1, 84, 255]
        );
    }
}
