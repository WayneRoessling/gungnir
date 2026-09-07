//! The observation model: truth in, detections out.
//!
//! This is the numbered procedure of `docs/test-tracks/sensor-models.md` §"How a
//! detection is produced", restricted to the parts the five engineering scenarios of
//! `docs/scenario-crate-narrative.md` need. The reference generator
//! `docs/test-tracks/tools/gen_tracks.py` additionally resolves signature classes,
//! cooperative and cued rules, fields of regard, and the radar-horizon rule from the
//! plan-07 YAML; those are properties of the TT-01 to TT-10 library rather than of the
//! five verification scenarios, and are not modelled here. What *is* modelled is
//! everything the verification rows read: detection probability, dropout, Gaussian
//! noise in the line-of-sight frame, per-instance bias, clock skew, latency, jitter,
//! out-of-order arrival, and Poisson false alarms.
//!
//! Every draw comes from the caller's `Rng` in a fixed order
//! (agentic-coding-standards.md §2.4), so a seeded generator produces the same
//! timeline every run.

use crate::Observation;
use gungnir_fusion_async::Detection;
use nalgebra::Vector3;
use rand::Rng;
use rand_distr::{Distribution, Normal, Poisson};

/// One simulated sensor. Field names and units follow `sensors.json` in
/// `docs/test-tracks/data-format.md` §5.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorModel {
    pub id: u32,
    /// Human-readable name, as written to `sensors.json`.
    pub name: String,
    /// Sensor type id from `sensor-models.md`, e.g. `radar.medium`.
    pub kind: String,
    /// ENU metres relative to the scenario origin.
    pub position: Vector3<f64>,
    /// Seconds between detection opportunities.
    pub update_period_s: f64,
    /// Offset of the first opportunity, so two sensors of the same rate are not
    /// aligned. `generation-method.md` §3 draws this per sensor.
    pub phase_offset_s: f64,
    /// Beyond this range there is no detection.
    pub range_m: f64,
    /// Probability of detection inside range, before dropout.
    pub pd_in_range: f64,
    /// Fraction of in-range opportunities lost to dropout.
    pub dropout: f64,
    /// One-sigma noise along the line of sight, metres.
    pub sigma_range_m: f64,
    /// One-sigma noise across the line of sight, metres.
    pub sigma_cross_m: f64,
    /// One-sigma vertical noise, metres.
    pub sigma_height_m: f64,
    /// Systematic registration bias added to every measurement, ENU metres. This is
    /// the quantity `gungnir_track_fusion::SensorRegistration` must recover.
    pub bias_m: Vector3<f64>,
    /// Mean transport delay from source time to receipt time, seconds.
    pub latency_mean_s: f64,
    /// One-sigma half-normal jitter added to the latency, seconds.
    pub jitter_s: f64,
    /// Fraction of detections delayed by a further 1 to 4 s, arriving out of order.
    pub out_of_order: f64,
    /// Mean false alarms per opportunity (Poisson).
    pub false_alarms_per_scan: f64,
    /// Lagging clock skew subtracted from source time, seconds. Never negative: a
    /// leading clock would be quarantined by the gateway rule that source time may
    /// not lead receipt time (`sensor-models.md` step 5).
    pub clock_skew_s: f64,
    /// Calibration baseline id, or `None` for an uncalibrated sensor.
    pub calibration: Option<String>,
}

impl SensorModel {
    /// A plain surveillance radar at `position`, the shape most scenarios want.
    /// Values follow the `radar.medium` row of `sensor-models.md`.
    #[must_use]
    pub fn radar_medium(id: u32, name: &str, position: Vector3<f64>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            kind: "radar.medium".to_owned(),
            position,
            update_period_s: 1.0,
            phase_offset_s: 0.0,
            range_m: 120_000.0,
            pd_in_range: 0.92,
            dropout: 0.04,
            sigma_range_m: 25.0,
            sigma_cross_m: 60.0,
            sigma_height_m: 150.0,
            bias_m: Vector3::zeros(),
            latency_mean_s: 0.25,
            jitter_s: 0.1,
            out_of_order: 0.02,
            false_alarms_per_scan: 0.08,
            clock_skew_s: 0.0,
            calibration: Some("cb-2026-09".to_owned()),
        }
    }

    /// The `radar.coastal` row: short range, heavy dropout, and the false-alarm rate
    /// that makes Scenario 2's clutter real.
    #[must_use]
    pub fn radar_coastal(id: u32, name: &str, position: Vector3<f64>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            kind: "radar.coastal".to_owned(),
            position,
            update_period_s: 2.5,
            phase_offset_s: 0.0,
            range_m: 40_000.0,
            pd_in_range: 0.8,
            dropout: 0.15,
            sigma_range_m: 15.0,
            sigma_cross_m: 60.0,
            sigma_height_m: 0.0,
            bias_m: Vector3::zeros(),
            latency_mean_s: 0.4,
            jitter_s: 0.2,
            out_of_order: 0.03,
            false_alarms_per_scan: 2.0,
            clock_skew_s: 0.0,
            calibration: Some("cb-2026-09".to_owned()),
        }
    }

    /// The `isr-video` row: slow, late, and badly out of order -- the source that
    /// makes Scenario 3's out-of-sequence handling mean something.
    #[must_use]
    pub fn isr_video(id: u32, name: &str, position: Vector3<f64>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            kind: "isr-video".to_owned(),
            position,
            update_period_s: 3.0,
            phase_offset_s: 0.0,
            range_m: 12_000.0,
            pd_in_range: 0.8,
            dropout: 0.15,
            sigma_range_m: 25.0,
            sigma_cross_m: 25.0,
            sigma_height_m: 10.0,
            bias_m: Vector3::zeros(),
            latency_mean_s: 2.5,
            jitter_s: 1.0,
            out_of_order: 0.35,
            false_alarms_per_scan: 0.02,
            clock_skew_s: 0.0,
            calibration: None,
        }
    }

    /// The `acoustic` row: coarse, very late, and clock-skewed under attack.
    #[must_use]
    pub fn acoustic(id: u32, name: &str, position: Vector3<f64>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            kind: "acoustic".to_owned(),
            position,
            update_period_s: 5.0,
            phase_offset_s: 0.0,
            range_m: 4_000.0,
            pd_in_range: 0.6,
            dropout: 0.3,
            sigma_range_m: 800.0,
            sigma_cross_m: 400.0,
            sigma_height_m: 300.0,
            bias_m: Vector3::zeros(),
            latency_mean_s: 2.5,
            jitter_s: 1.0,
            out_of_order: 0.25,
            false_alarms_per_scan: 0.05,
            clock_skew_s: 0.0,
            calibration: None,
        }
    }

    /// Opportunity times over `[0, duration_s]`, starting at the phase offset.
    pub(crate) fn opportunities(&self, duration_s: f64) -> impl Iterator<Item = f64> + '_ {
        let period = self.update_period_s;
        let start = self.phase_offset_s;
        std::iter::successors(Some(start), move |t| {
            let next = t + period;
            (next <= duration_s).then_some(next)
        })
    }

    /// Probability this sensor reports an in-range, unoccluded entity at one
    /// opportunity: `sensor-models.md` step 3.
    #[must_use]
    pub fn effective_pd(&self) -> f64 {
        self.pd_in_range * (1.0 - self.dropout)
    }

    /// Measurement noise in the line-of-sight frame at `truth`, mapped back into ENU
    /// and offset by the instance bias: `sensor-models.md` step 4.
    ///
    /// The line-of-sight frame is built from the horizontal bearing to the target, so
    /// `sigma_range_m` acts along the boresight and `sigma_cross_m` across it. A
    /// target directly overhead has no defined horizontal bearing; there the two
    /// horizontal sigmas are interchangeable and the east axis is used, which is
    /// exactly the degenerate case, not an approximation of a different one.
    fn noisy_measurement<R: Rng + ?Sized>(&self, truth: Vector3<f64>, rng: &mut R) -> Vector3<f64> {
        let los = truth - self.position;
        let horizontal = (los.x * los.x + los.y * los.y).sqrt();
        let (ux, uy) = if horizontal > 1e-9 {
            (los.x / horizontal, los.y / horizontal)
        } else {
            (1.0, 0.0)
        };
        let along = draw_normal(self.sigma_range_m, rng);
        let across = draw_normal(self.sigma_cross_m, rng);
        let vertical = draw_normal(self.sigma_height_m, rng);
        Vector3::new(
            truth.x + along * ux - across * uy + self.bias_m.x,
            truth.y + along * uy + across * ux + self.bias_m.y,
            truth.z + vertical + self.bias_m.z,
        )
    }

    /// Receipt time for an observation made at `source_time`: `sensor-models.md`
    /// step 5. Latency, half-normal jitter, and the out-of-order tail.
    fn receipt_time<R: Rng + ?Sized>(&self, source_time: f64, rng: &mut R) -> f64 {
        let jitter = draw_normal(self.jitter_s, rng).abs();
        let mut receipt = source_time + self.latency_mean_s + jitter;
        if rng.gen::<f64>() < self.out_of_order {
            receipt += rng.gen_range(1.0..4.0);
        }
        receipt
    }

    /// One opportunity against one entity. `None` when the sensor does not report.
    pub(crate) fn observe<R: Rng + ?Sized>(
        &self,
        truth: Vector3<f64>,
        entity_id: &str,
        opportunity_s: f64,
        rng: &mut R,
    ) -> Option<Observation> {
        // Step 2: range gate. Drawn before the Pd coin so that an out-of-range entity
        // consumes no randomness and adding a distant entity cannot shift the stream.
        if (truth - self.position).norm() > self.range_m {
            return None;
        }
        // Step 3.
        if rng.gen::<f64>() >= self.effective_pd() {
            return None;
        }
        let measurement = self.noisy_measurement(truth, rng);
        let source_time = opportunity_s - self.clock_skew_s;
        let receipt_time_s = self.receipt_time(source_time, rng);
        Some(Observation {
            detection: Detection {
                sensor_id: self.id,
                timestamp_s: source_time,
                measurement,
            },
            receipt_time_s,
            truth_entity: Some(entity_id.to_owned()),
            calibration: self.calibration.clone(),
        })
    }

    /// One opportunity's false alarms: `sensor-models.md` step 6. Poisson in count,
    /// uniform in the sphere of the sensor's range band.
    pub(crate) fn false_alarms<R: Rng + ?Sized>(
        &self,
        opportunity_s: f64,
        rng: &mut R,
    ) -> Vec<Observation> {
        if self.false_alarms_per_scan <= 0.0 {
            return Vec::new();
        }
        let count = draw_poisson(self.false_alarms_per_scan, rng);
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            // Uniform in the ball, by the standard cube-root radius draw: uniform in
            // radius alone would pile clutter at the sensor, which would make the
            // statistical self-check pass while the spatial distribution was wrong.
            let u: f64 = rng.gen();
            let radius = self.range_m * u.cbrt();
            let cos_theta = rng.gen_range(-1.0..1.0_f64);
            let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
            let phi = rng.gen_range(0.0..std::f64::consts::TAU);
            let offset = Vector3::new(
                radius * sin_theta * phi.cos(),
                radius * sin_theta * phi.sin(),
                radius * cos_theta,
            );
            let source_time = opportunity_s - self.clock_skew_s;
            let receipt_time_s = self.receipt_time(source_time, rng);
            out.push(Observation {
                detection: Detection {
                    sensor_id: self.id,
                    timestamp_s: source_time,
                    measurement: self.position + offset,
                },
                receipt_time_s,
                // A false alarm has no truth entity. Its provenance is identical to a
                // real detection, because the sensor cannot tell the difference
                // (data-format.md §3).
                truth_entity: None,
                calibration: self.calibration.clone(),
            });
        }
        out
    }
}

/// A zero-mean normal draw. `sigma == 0` is an exact zero rather than a degenerate
/// distribution, because several sensor rows have a zero height sigma.
fn draw_normal<R: Rng + ?Sized>(sigma: f64, rng: &mut R) -> f64 {
    if sigma <= 0.0 {
        return 0.0;
    }
    match Normal::new(0.0, sigma) {
        Ok(dist) => dist.sample(rng),
        // Unreachable for a finite positive sigma; falling back to no noise is the
        // conservative choice and keeps the generator total.
        Err(_) => 0.0,
    }
}

/// The largest false-alarm count one opportunity may produce. A Poisson draw is
/// unbounded above; a pathological lambda would otherwise turn a configuration
/// mistake into an enormous allocation.
const MAX_FALSE_ALARMS_PER_SCAN: f64 = 10_000.0;

fn draw_poisson<R: Rng + ?Sized>(lambda: f64, rng: &mut R) -> usize {
    match Poisson::new(lambda) {
        Ok(dist) => {
            let n: f64 = dist.sample(rng);
            // Poisson samples are non-negative integers held in an f64; after the
            // clamp the value is in [0, MAX_FALSE_ALARMS_PER_SCAN] and integral, so
            // the conversion is exact and neither truncates nor loses a sign.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let count = n.clamp(0.0, MAX_FALSE_ALARMS_PER_SCAN) as usize;
            count
        }
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn opportunities_respect_period_and_offset() {
        let mut s = SensorModel::radar_medium(1, "R1", Vector3::zeros());
        s.update_period_s = 2.0;
        s.phase_offset_s = 0.5;
        let times: Vec<f64> = s.opportunities(6.0).collect();
        assert_eq!(times, vec![0.5, 2.5, 4.5]);
    }

    #[test]
    fn out_of_range_never_reports() {
        let s = SensorModel::radar_medium(1, "R1", Vector3::zeros());
        let mut rng = StdRng::seed_from_u64(7);
        let far = Vector3::new(s.range_m * 2.0, 0.0, 0.0);
        for _ in 0..1000 {
            assert!(s.observe(far, "T-001", 0.0, &mut rng).is_none());
        }
    }

    /// Noise must be centred on truth: a biased sensor model would make every
    /// downstream filter look biased.
    #[test]
    fn measurement_noise_is_zero_mean() {
        let s = SensorModel::radar_medium(1, "R1", Vector3::zeros());
        let mut rng = StdRng::seed_from_u64(11);
        let truth = Vector3::new(10_000.0, 0.0, 3_000.0);
        let n = 20_000;
        let mut sum = Vector3::zeros();
        for _ in 0..n {
            sum += s.noisy_measurement(truth, &mut rng) - truth;
        }
        let mean = sum / f64::from(n);
        // Standard error of the mean is sigma/sqrt(n); 60/sqrt(20000) is about 0.42,
        // so 3 m is comfortably outside the noise and well inside a real bias.
        assert!(mean.norm() < 3.0, "mean offset {mean:?}");
    }

    /// The instance bias must survive into the measurement -- this is the quantity
    /// the sensor-registration row has to recover.
    #[test]
    fn instance_bias_offsets_the_mean() {
        let mut s = SensorModel::radar_medium(1, "R1", Vector3::zeros());
        s.bias_m = Vector3::new(120.0, -80.0, 15.0);
        let mut rng = StdRng::seed_from_u64(13);
        let truth = Vector3::new(10_000.0, 0.0, 3_000.0);
        let n = 20_000;
        let mut sum = Vector3::zeros();
        for _ in 0..n {
            sum += s.noisy_measurement(truth, &mut rng) - truth;
        }
        let mean = sum / f64::from(n);
        assert!((mean - s.bias_m).norm() < 3.0, "recovered {mean:?}");
    }

    /// Receipt time never precedes source time, which is the gateway's rule.
    #[test]
    fn receipt_never_precedes_source() {
        let s = SensorModel::isr_video(4, "V1", Vector3::zeros());
        let mut rng = StdRng::seed_from_u64(17);
        for _ in 0..5000 {
            assert!(s.receipt_time(100.0, &mut rng) >= 100.0);
        }
    }
}
