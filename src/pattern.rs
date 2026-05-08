//! Pattern math: sum-of-sines per channel, with per-core phase shift for CPU.

use rand::Rng;
use rand::SeedableRng;
use rand_pcg::Pcg64;

/// One sine component.
#[derive(Clone, Copy, Debug)]
pub struct Sine {
    pub amplitude: f64,
    pub frequency_hz: f64,
    pub phase_rad: f64,
}

/// Parameters for a single channel (CPU, memory, or network) at a moment in time.
#[derive(Clone, Debug)]
pub struct ChannelParams {
    pub sines: Vec<Sine>,
}

impl ChannelParams {
    /// Compute the raw sum of sines at time `t` (seconds since start).
    pub fn raw(&self, t: f64) -> f64 {
        self.sines
            .iter()
            .map(|s| s.amplitude * (2.0 * std::f64::consts::PI * s.frequency_hz * t + s.phase_rad).sin())
            .sum()
    }

    /// Theoretical maximum amplitude (sum of |amplitudes|).
    pub fn max_amp(&self) -> f64 {
        self.sines.iter().map(|s| s.amplitude.abs()).sum()
    }

    /// Normalized value in `[0, 1]` at time `t`.
    pub fn normalized(&self, t: f64) -> f64 {
        let max = self.max_amp();
        if max == 0.0 {
            return 0.5;
        }
        (self.raw(t) + max) / (2.0 * max)
    }

    /// Final value scaled to `[0, peak]`.
    pub fn target(&self, t: f64, peak: f64) -> f64 {
        self.normalized(t) * peak
    }

    /// Target value for a specific CPU core, with per-core phase shift.
    /// Each core adds `2*PI * core_id / num_cores` to all phases.
    pub fn target_for_core(&self, t: f64, peak: f64, core_id: usize, num_cores: usize) -> f64 {
        if num_cores == 0 {
            return self.target(t, peak);
        }
        let shift = 2.0 * std::f64::consts::PI * (core_id as f64) / (num_cores as f64);
        let raw: f64 = self
            .sines
            .iter()
            .map(|s| s.amplitude * (2.0 * std::f64::consts::PI * s.frequency_hz * t + s.phase_rad + shift).sin())
            .sum();
        let max = self.max_amp();
        if max == 0.0 {
            return 0.5 * peak;
        }
        ((raw + max) / (2.0 * max)) * peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build random ChannelParams from a seeded RNG.
    fn random_channel(rng: &mut Pcg64, k: usize) -> ChannelParams {
        let sines = (0..k)
            .map(|_| Sine {
                amplitude: rng.gen_range(0.3..1.0),
                frequency_hz: rng.gen_range(0.05..0.30),
                phase_rad: rng.gen_range(0.0..(2.0 * std::f64::consts::PI)),
            })
            .collect();
        ChannelParams { sines }
    }

    #[test]
    fn normalized_stays_in_unit_interval() {
        // Sweep many seeds, many t values, assert normalized in [0, 1] strictly.
        for seed in 0u64..20 {
            let mut rng = Pcg64::seed_from_u64(seed);
            let ch = random_channel(&mut rng, 3);
            for step in 0..1000 {
                let t = step as f64 * 0.1;
                let n = ch.normalized(t);
                assert!(
                    (0.0..=1.0).contains(&n),
                    "seed={seed} t={t} n={n} not in [0,1]",
                );
            }
        }
    }

    #[test]
    fn target_stays_within_peak() {
        let mut rng = Pcg64::seed_from_u64(42);
        let ch = random_channel(&mut rng, 3);
        let peak = 90.0;
        for step in 0..1000 {
            let t = step as f64 * 0.1;
            let v = ch.target(t, peak);
            assert!((0.0..=peak).contains(&v), "t={t} v={v} out of [0, {peak}]");
        }
    }

    #[test]
    fn empty_sines_returns_midpoint() {
        let ch = ChannelParams { sines: vec![] };
        assert_eq!(ch.normalized(0.0), 0.5);
        assert_eq!(ch.target(123.4, 100.0), 50.0);
    }

    #[test]
    fn per_core_phase_shift_is_uniform() {
        // With a single sine (freq=0.1, amp=1, phase=0) and 4 cores,
        // each core's signal should be phase-shifted by exactly 2*PI*i/4 vs core 0.
        let ch = ChannelParams {
            sines: vec![Sine {
                amplitude: 1.0,
                frequency_hz: 0.1,
                phase_rad: 0.0,
            }],
        };
        let num_cores = 4;
        let t = 1.234;
        let v0 = ch.target_for_core(t, 100.0, 0, num_cores);
        let v1 = ch.target_for_core(t, 100.0, 1, num_cores);
        let v2 = ch.target_for_core(t, 100.0, 2, num_cores);
        let v3 = ch.target_for_core(t, 100.0, 3, num_cores);

        // Core 0 and core 2 are 180 degrees out of phase: their normalized values
        // should sum to 1.0 (peak normalized space), so values sum to peak.
        assert!(
            ((v0 + v2) - 100.0).abs() < 1e-6,
            "core 0+2 should sum to peak (180 out of phase), got {} + {} = {}",
            v0,
            v2,
            v0 + v2,
        );
        // Same for cores 1 and 3.
        assert!(
            ((v1 + v3) - 100.0).abs() < 1e-6,
            "core 1+3 should sum to peak, got {} + {} = {}",
            v1,
            v3,
            v1 + v3,
        );
    }

    #[test]
    fn target_for_core_zero_equals_target() {
        // Core 0 with shift 2*PI*0/N == 0 should match the un-shifted target.
        let mut rng = Pcg64::seed_from_u64(7);
        let ch = random_channel(&mut rng, 3);
        let t = 5.0;
        assert!((ch.target_for_core(t, 90.0, 0, 8) - ch.target(t, 90.0)).abs() < 1e-9);
    }
}
