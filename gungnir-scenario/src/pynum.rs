// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The arithmetic the reference generator's numbers obey (GAP-016).
//!
//! `gen_tracks.py` is Python, and a Python number is an `int` or a `float` depending on
//! where it came from: a YAML `0` stays an integer through `min`, `max`, `abs` and `round`,
//! and is written to JSON as `0`; the same value after one multiplication by a float is
//! `0.0`. Byte-for-byte parity therefore needs a number that remembers which it is, and a
//! writer that prints a float the way `repr(float)` does. Nothing else in the workspace
//! wants either; they are private to this crate's generator.

// "CPython" is a proper noun and not an identifier; the lint would have it in backticks.
#![allow(clippy::doc_markdown)]

use std::cmp::Ordering;

/// A Python number: an integer or a float, with Python's promotion rules.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    /// Python's `float(x)`: the nearest double, which is exact for every integer the
    /// YAML holds.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn f(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(x) => x,
        }
    }

    #[must_use]
    pub fn abs(self) -> Num {
        match self {
            Num::Int(a) => Num::Int(a.wrapping_abs()),
            Num::Float(x) => Num::Float(x.abs()),
        }
    }

    fn cmp_num(self, o: Num) -> Ordering {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => a.cmp(&b),
            _ => self.f().partial_cmp(&o.f()).unwrap_or(Ordering::Equal),
        }
    }

    #[must_use]
    pub fn lt(self, o: Num) -> bool {
        self.cmp_num(o) == Ordering::Less
    }

    #[must_use]
    pub fn gt(self, o: Num) -> bool {
        self.cmp_num(o) == Ordering::Greater
    }

    #[must_use]
    pub fn le(self, o: Num) -> bool {
        self.cmp_num(o) != Ordering::Greater
    }

    #[must_use]
    pub fn ge(self, o: Num) -> bool {
        self.cmp_num(o) != Ordering::Less
    }

    /// Python's two-argument `max`: the second only when it is strictly greater.
    #[must_use]
    pub fn max2(self, o: Num) -> Num {
        if o.gt(self) {
            o
        } else {
            self
        }
    }

    /// Python's two-argument `min`: the second only when it is strictly less.
    #[must_use]
    pub fn min2(self, o: Num) -> Num {
        if o.lt(self) {
            o
        } else {
            self
        }
    }

    /// Python's `round(x, n)`: an integer stays an integer; a float is correctly
    /// rounded to `n` decimals on its exact binary value, ties to even.
    #[must_use]
    pub fn round(self, digits: usize) -> Num {
        match self {
            Num::Int(a) => Num::Int(a),
            Num::Float(x) => Num::Float(round_float(x, digits)),
        }
    }

    #[must_use]
    pub fn is_int(self) -> bool {
        matches!(self, Num::Int(_))
    }

    /// Python's `int(x)`: truncation toward zero.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn as_i64_lossy(self) -> i64 {
        match self {
            Num::Int(i) => i,
            Num::Float(x) => x.trunc() as i64,
        }
    }
}

impl std::ops::Add for Num {
    type Output = Num;
    fn add(self, o: Num) -> Num {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => Num::Int(a.wrapping_add(b)),
            _ => Num::Float(self.f() + o.f()),
        }
    }
}

impl std::ops::Sub for Num {
    type Output = Num;
    fn sub(self, o: Num) -> Num {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => Num::Int(a.wrapping_sub(b)),
            _ => Num::Float(self.f() - o.f()),
        }
    }
}

impl std::ops::Mul for Num {
    type Output = Num;
    fn mul(self, o: Num) -> Num {
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => Num::Int(a.wrapping_mul(b)),
            _ => Num::Float(self.f() * o.f()),
        }
    }
}

/// Python's `/`: always a float.
impl std::ops::Div for Num {
    type Output = Num;
    fn div(self, o: Num) -> Num {
        Num::Float(self.f() / o.f())
    }
}

impl std::ops::Neg for Num {
    type Output = Num;
    fn neg(self) -> Num {
        match self {
            Num::Int(a) => Num::Int(a.wrapping_neg()),
            Num::Float(x) => Num::Float(-x),
        }
    }
}

impl From<f64> for Num {
    fn from(x: f64) -> Self {
        Num::Float(x)
    }
}

impl From<i64> for Num {
    fn from(i: i64) -> Self {
        Num::Int(i)
    }
}

/// `round(x, digits)` for a float. Rust's fixed-precision formatting is correctly
/// rounded on the exact binary value, ties to even, which is what CPython's `round`
/// computes; the result is read back as the nearest double, as CPython does.
#[must_use]
pub fn round_float(x: f64, digits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let text = format!("{x:.digits$}");
    let rounded: f64 = text.parse().unwrap_or(x);
    // `format!` prints "-0.0" for a negative value that rounds to zero, and Python's
    // round keeps the sign too; `parse` preserves it.
    rounded
}

/// Python's `repr(float)`: the shortest digits that round-trip, laid out in fixed
/// notation when the decimal exponent is in `(-4, 16]` and in `d.ddde±XX` otherwise, with
/// `.0` appended to an integral fixed value.
#[must_use]
pub fn repr_float(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // `{:e}` gives the shortest round-trip digits as `d.ddde<exp>`.
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // Position of the decimal point relative to the digit string: value = 0.d1d2.. × 10^decpt.
    let decpt = exp + 1;
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if decpt > -4 && decpt <= 16 {
        let n = i32::try_from(digits.len()).unwrap_or(i32::MAX);
        if decpt <= 0 {
            format!(
                "0.{}{}",
                "0".repeat(usize::try_from(-decpt).unwrap_or(0)),
                digits
            )
        } else if decpt >= n {
            format!(
                "{}{}.0",
                digits,
                "0".repeat(usize::try_from(decpt - n).unwrap_or(0))
            )
        } else {
            let at = usize::try_from(decpt).unwrap_or(0);
            format!("{}.{}", &digits[..at], &digits[at..])
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let e = decpt - 1;
        let mantissa = if rest.is_empty() {
            first.to_string()
        } else {
            format!("{first}.{rest}")
        };
        format!("{mantissa}e{}{:02}", if e < 0 { "-" } else { "+" }, e.abs())
    };
    format!("{sign}{body}")
}

/// A number as `json.dumps` writes it.
#[must_use]
pub fn json_num(n: Num) -> String {
    match n {
        Num::Int(i) => i.to_string(),
        Num::Float(x) => repr_float(x),
    }
}

/// A string as `json.dumps` writes it with `ensure_ascii`: every non-ASCII character as
/// `\uXXXX`, the two-byte escapes for the characters JSON names, and `\u00XX` for the
/// rest of the control range.
#[must_use]
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                use std::fmt::Write as _;
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    // Writing into a `String` cannot fail.
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_cpython_on_the_cases_that_differ_from_rust() {
        assert_eq!(repr_float(1234.6), "1234.6");
        assert_eq!(repr_float(12.0), "12.0");
        assert_eq!(repr_float(0.3), "0.3");
        assert_eq!(repr_float(1e-5), "1e-05");
        assert_eq!(repr_float(1e16), "1e+16");
        assert_eq!(repr_float(1e15), "1000000000000000.0");
        assert_eq!(repr_float(0.0001), "0.0001");
        assert_eq!(repr_float(-0.0), "-0.0");
        assert_eq!(repr_float(2.5), "2.5");
        assert_eq!(repr_float(-79953.9), "-79953.9");
        assert_eq!(
            repr_float(123_456_789_012_345_680.0),
            "1.2345678901234568e+17"
        );
    }

    #[test]
    fn round_keeps_an_integer_and_rounds_a_float_like_cpython() {
        assert_eq!(Num::Int(6).round(2), Num::Int(6));
        assert_eq!(Num::Float(1234.56789).round(1), Num::Float(1234.6));
        assert_eq!(Num::Float(2.675).round(2), Num::Float(2.67));
        assert_eq!(Num::Float(0.125).round(2), Num::Float(0.12));
        assert_eq!(json_num(Num::Float(0.1 + 0.2).round(2)), "0.3");
        assert_eq!(json_num(Num::Float(-0.04).round(1)), "-0.0");
    }

    #[test]
    fn min_and_max_keep_the_first_argument_on_a_tie_and_its_type() {
        assert_eq!(Num::Float(0.0).max2(Num::Int(0)), Num::Float(0.0));
        assert_eq!(Num::Int(0).max2(Num::Float(0.0)), Num::Int(0));
        assert_eq!(Num::Int(6).min2(Num::Float(9.5)), Num::Int(6));
        assert_eq!(Num::Int(6).min2(Num::Float(3.5)), Num::Float(3.5));
        assert!((Num::Int(2) + Num::Int(3)).is_int());
        assert!(!(Num::Int(2) + Num::Float(3.0)).is_int());
    }

    #[test]
    fn strings_escape_the_way_json_dumps_does() {
        assert_eq!(json_str("R1 ridge radar"), "\"R1 ridge radar\"");
        assert_eq!(json_str("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_str("é"), "\"\\u00e9\"");
    }
}
