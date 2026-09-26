// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Three-vectors with the reference generator's numeric behaviour (GAP-016).
//!
//! Moved here from `gungnir-scenario`'s `tracks.rs` with the observation model
//! (docs/design/DN-32-re-observation-for-a-laydown.md §4): the generator's motion models
//! and the observation model both read them, and the parity the generator is held to is
//! a parity of every operation, so there is one copy of each.

// "CPython" is a proper noun and not an identifier; the lint would have it in backticks.
#![allow(clippy::doc_markdown)]

use crate::pynum::Num;
use std::ops::{Add, Div, Mul, Sub};

/// A position or velocity whose components remember whether Python held an `int`.
pub type V3 = [Num; 3];

/// Three floats as a [`V3`].
#[must_use]
pub fn f3(a: [f64; 3]) -> V3 {
    [Num::Float(a[0]), Num::Float(a[1]), Num::Float(a[2])]
}

/// Python's `math.sqrt(sum(x * x for x in v))`, the sum starting from the integer 0.
#[must_use]
pub fn norm(v: &V3) -> f64 {
    let sum = v.iter().fold(Num::Int(0), |acc, x| acc.add(x.mul(*x)));
    sum.f().sqrt()
}

#[must_use]
pub fn sub(a: &V3, b: &V3) -> V3 {
    [a[0].sub(b[0]), a[1].sub(b[1]), a[2].sub(b[2])]
}

#[must_use]
pub fn add(a: &V3, b: &V3) -> V3 {
    [a[0].add(b[0]), a[1].add(b[1]), a[2].add(b[2])]
}

#[must_use]
pub fn scale(a: &V3, k: f64) -> V3 {
    let k = Num::Float(k);
    [a[0].mul(k), a[1].mul(k), a[2].mul(k)]
}

/// The unit vector, or zero for a vector shorter than a nanometre, as the reference does.
#[must_use]
pub fn unit(v: &V3) -> V3 {
    let n = norm(v);
    if n > 1e-9 {
        [v[0].div(n.into()), v[1].div(n.into()), v[2].div(n.into())]
    } else {
        f3([0.0, 0.0, 0.0])
    }
}

/// `math.degrees`: CPython divides by `pi / 180`, and the last bit differs from a
/// multiplication by `180 / pi`.
#[must_use]
pub fn degrees(x: f64) -> f64 {
    x / (std::f64::consts::PI / 180.0)
}

/// Python's float `%` for a positive divisor.
#[must_use]
pub fn pymod(x: f64, y: f64) -> f64 {
    let m = x % y;
    if m != 0.0 && ((y < 0.0) != (m < 0.0)) {
        m + y
    } else if m == 0.0 {
        0.0f64.copysign(y)
    } else {
        m
    }
}

/// The 4/3-earth radar horizon between two heights, metres.
#[must_use]
pub fn radar_horizon_m(h1: Num, h2: Num) -> f64 {
    4120.0 * (h1.max2(Num::Float(0.0)).f().sqrt() + h2.max2(Num::Float(0.0)).f().sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_too_short_to_point_anywhere_has_no_direction() {
        assert_eq!(unit(&f3([1e-12, 0.0, 0.0])), f3([0.0, 0.0, 0.0]));
        let u = unit(&f3([3.0, 4.0, 0.0]));
        assert!((u[0].f() - 0.6).abs() < 1e-15 && (u[1].f() - 0.8).abs() < 1e-15);
    }

    #[test]
    fn python_modulo_takes_the_sign_of_the_divisor() {
        assert!((pymod(-1.0, 360.0) - 359.0).abs() < 1e-12);
        assert!((pymod(361.0, 360.0) - 1.0).abs() < 1e-12);
        assert!(pymod(0.0, 360.0).is_sign_positive());
    }

    #[test]
    fn the_horizon_grows_with_either_height_and_ignores_a_negative_one() {
        assert!(radar_horizon_m(Num::Int(100), Num::Int(0)) > 0.0);
        assert_eq!(
            radar_horizon_m(Num::Int(-5), Num::Int(0)).to_bits(),
            radar_horizon_m(Num::Int(0), Num::Int(0)).to_bits()
        );
    }
}
