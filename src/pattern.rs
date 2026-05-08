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
}
