// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! CPython's `random.Random`, reproduced bit for bit (GAP-016).
//!
//! `docs/test-tracks/tools/gen_tracks.py` is the reference generator, and its every
//! draw comes from `random.Random(seed)`: `random()`, `uniform()`, `gauss()`. Regenerating
//! a sample set from Rust and comparing it byte for byte with the reference therefore
//! needs the same generator, not merely a good one. This is MT19937 with CPython's
//! seeding (`init_by_array` over the integer's 32-bit words), its 53-bit `random()`, its
//! `uniform()`, and its polar `gauss()` with the cached second value, checked against
//! vectors taken from CPython 3.12.10 on 2026-09-06.
//!
//! `gauss()` calls `ln`, `sqrt`, `cos` and `sin`; the first two are correctly rounded
//! everywhere, the last two follow the platform's libm, which is the same one CPython
//! links on the same machine. A cross-platform last-bit difference in `gauss()` is
//! therefore a libm difference, not a generator one, and the test tolerance says so.

// "CPython" is a proper noun and not an identifier; the lint would have it in backticks.
#![allow(clippy::doc_markdown)]

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

/// `random.Random`, seeded the way CPython seeds it from an integer.
#[derive(Debug, Clone)]
pub struct PythonRandom {
    state: [u32; N],
    index: usize,
    gauss_next: Option<f64>,
}

impl PythonRandom {
    /// `random.Random(seed)` for a non-negative integer seed of any width.
    #[must_use]
    pub fn new(seed: u128) -> Self {
        // CPython splits |seed| into 32-bit words, least significant first, with at
        // least one word, and feeds them to `init_by_array`.
        let mut key = Vec::new();
        let mut rest = seed;
        loop {
            #[allow(clippy::cast_possible_truncation)]
            key.push((rest & 0xffff_ffff) as u32);
            rest >>= 32;
            if rest == 0 {
                break;
            }
        }
        Self::from_key(&key)
    }

    fn init_genrand(seed: u32) -> [u32; N] {
        let mut mt = [0u32; N];
        mt[0] = seed;
        for i in 1..N {
            mt[i] = 1_812_433_253u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(u32::try_from(i).unwrap_or(u32::MAX));
        }
        mt
    }

    fn from_key(key: &[u32]) -> Self {
        let mut mt = Self::init_genrand(19_650_218);
        let mut i = 1usize;
        let mut j = 0usize;
        let rounds = N.max(key.len());
        for _ in 0..rounds {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(u32::try_from(j).unwrap_or(u32::MAX));
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(u32::try_from(i).unwrap_or(u32::MAX));
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        Self {
            state: mt,
            index: N,
            gauss_next: None,
        }
    }

    fn twist(&mut self) {
        let mt = &mut self.state;
        for kk in 0..N {
            let y = (mt[kk] & UPPER_MASK) | (mt[(kk + 1) % N] & LOWER_MASK);
            let mut v = mt[(kk + M) % N] ^ (y >> 1);
            if y & 1 == 1 {
                v ^= MATRIX_A;
            }
            mt[kk] = v;
        }
        self.index = 0;
    }

    /// One tempered 32-bit output: `getrandbits(32)`.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// `random()`: 53 random bits as a float in `[0, 1)`.
    pub fn random(&mut self) -> f64 {
        let a = f64::from(self.next_u32() >> 5);
        let b = f64::from(self.next_u32() >> 6);
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `uniform(a, b)`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    /// `gauss(mu, sigma)`: the polar method with CPython's cached second draw.
    pub fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        let z = if let Some(z) = self.gauss_next.take() {
            z
        } else {
            let x2pi = self.random() * std::f64::consts::TAU;
            let g2rad = (-2.0 * (1.0 - self.random()).ln()).sqrt();
            self.gauss_next = Some(x2pi.sin() * g2rad);
            x2pi.cos() * g2rad
        };
        mu + z * sigma
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors from CPython 3.12.10: for each seed, four `random()`, one
    /// `uniform(30, 180)`, four `gauss(0, 1)`, three `getrandbits(32)`, in that order.
    type Vector = (u128, [f64; 4], f64, [f64; 4], [u32; 3]);

    const VECTORS: &[Vector] = &[
        (
            0,
            [
                0.844_421_851_525_048_1,
                0.757_954_402_940_302_5,
                0.420_571_580_830_845,
                0.258_916_750_292_963_35,
            ],
            106.691_208_205_291_28,
            [
                -1.447_123_126_427_365_8,
                0.984_339_551_246_648_1,
                -0.374_076_846_986_729_05,
                1.074_650_385_779_055_5,
            ],
            [2_505_606_783, 3_829_653_368, 3_900_315_155],
        ),
        (
            42,
            [
                0.639_426_798_457_883_7,
                0.025_010_755_222_666_936,
                0.275_029_318_369_119_26,
                0.223_210_738_148_822_75,
            ],
            140.470_682_124_601_86,
            [
                -0.938_051_221_433_234,
                -1.890_670_808_223_996,
                0.894_589_959_661_402_2,
                0.543_876_017_917_679_9,
            ],
            [127_978_094, 402_418_010, 939_042_955],
        ),
        (
            2026,
            [
                0.119_119_884_963_963_09,
                0.502_515_755_231_250_6,
                0.511_822_712_773_071,
                0.860_000_587_649_275_4,
            ],
            45.395_527_576_043_975,
            [
                0.226_486_885_845_777_28,
                1.336_579_614_788_076,
                -1.639_760_812_474_369_4,
                -0.608_559_451_512_835_6,
            ],
            [2_352_832_278, 3_618_087_884, 3_137_641_081],
        ),
        (
            1042,
            [
                0.249_510_758_714_099_1,
                0.355_161_473_564_355_1,
                0.935_116_598_983_996_3,
                0.884_655_090_417_999_7,
            ],
            103.394_690_520_138_74,
            [
                1.687_519_008_220_225_4,
                -0.031_125_741_511_383_97,
                -0.608_887_064_208_367,
                0.072_717_088_325_044_55,
            ],
            [2_689_532_673, 737_527_765, 3_167_983_990],
        ),
        (
            1_099_511_627_783,
            [
                0.613_703_777_993_651_1,
                0.814_916_297_330_948_7,
                0.945_011_508_759_287_3,
                0.570_390_699_100_456_1,
            ],
            101.388_614_830_573_4,
            [
                0.493_239_312_192_823_74,
                -0.166_683_842_572_623_06,
                -1.129_799_710_861_761_5,
                1.125_973_855_905_228_6,
            ],
            [1_688_693_473, 1_902_582_600, 1_930_265_598],
        ),
    ];

    #[test]
    #[allow(clippy::float_cmp)]
    fn random_uniform_and_getrandbits_match_cpython_exactly() {
        for (seed, randoms, uniform, _, bits) in VECTORS {
            let mut r = PythonRandom::new(*seed);
            for expected in randoms {
                assert_eq!(r.random(), *expected, "seed {seed}");
            }
            assert_eq!(r.uniform(30.0, 180.0), *uniform, "seed {seed}");
            // Skip the four gauss draws: eight random() calls behind them.
            for _ in 0..4 {
                r.gauss(0.0, 1.0);
            }
            for expected in bits {
                assert_eq!(r.next_u32(), *expected, "seed {seed}");
            }
        }
    }

    #[test]
    fn gauss_matches_cpython_to_libm_precision() {
        for (seed, _, _, gausses, _) in VECTORS {
            let mut r = PythonRandom::new(*seed);
            for _ in 0..5 {
                r.random();
            }
            for expected in gausses {
                let got = r.gauss(0.0, 1.0);
                assert!(
                    (got - expected).abs() <= 1e-14 * expected.abs().max(1.0),
                    "seed {seed}: {got} vs {expected}"
                );
            }
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn a_wide_seed_uses_every_word() {
        // The two seeds differ only above bit 32; a seeding that dropped the high word
        // would make them equal.
        let a = PythonRandom::new(7).random();
        let b = PythonRandom::new((1u128 << 40) + 7).random();
        assert_ne!(a, b);
    }
}
