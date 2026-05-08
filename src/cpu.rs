//! CPU driver: per-core pinned threads, busy-spin to target percent.

use crate::pattern::PatternState;
use std::hint::black_box;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TICK_MS: u64 = 100;

/// Compute how many milliseconds out of `tick_ms` to busy-spin to achieve `target_pct`.
/// Clamps target into [0, 100].
pub fn busy_ms_for_tick(target_pct: f64, tick_ms: u64) -> u64 {
    let p = target_pct.clamp(0.0, 100.0);
    ((p / 100.0) * (tick_ms as f64)).round() as u64
}

/// Spin pinned to `core_id`, driving CPU% per `pattern`. Returns when `pattern.is_stopped()`.
pub fn run_core(pattern: Arc<PatternState>, core_id: usize, num_cores: usize, peak: f64, affinity: Option<core_affinity::CoreId>) {
    if let Some(c) = affinity {
        let _ok = core_affinity::set_for_current(c);
        // If pinning fails we just continue unpinned.
    }
    while !pattern.is_stopped() {
        let tick_start = Instant::now();
        let snap = pattern.snapshot();
        let target = snap.cpu.target_for_core(pattern.elapsed_secs(), peak, core_id, num_cores);
        let busy_ms = busy_ms_for_tick(target, TICK_MS);
        busy_spin(Duration::from_millis(busy_ms));
        let elapsed = tick_start.elapsed();
        if elapsed < Duration::from_millis(TICK_MS) {
            thread::sleep(Duration::from_millis(TICK_MS) - elapsed);
        }
    }
}

/// Tight arithmetic loop the optimizer cannot remove.
fn busy_spin(dur: Duration) {
    let end = Instant::now() + dur;
    let mut x: u64 = 0xDEAD_BEEF;
    while Instant::now() < end {
        for _ in 0..1000 {
            x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407));
        }
        let _ = black_box(x);
    }
}

/// Spawn one pinned thread per core. Returns the join handles.
pub fn spawn_all(pattern: Arc<PatternState>, peak: f64) -> Vec<JoinHandle<()>> {
    let cores = core_affinity::get_core_ids().unwrap_or_default();
    let num_cores = cores.len().max(1);
    cores
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let pattern = Arc::clone(&pattern);
            thread::Builder::new()
                .name(format!("cpu-driver-{i}"))
                .spawn(move || run_core(pattern, i, num_cores, peak, Some(c)))
                .expect("failed to spawn cpu driver thread")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_ms_zero_for_zero_pct() {
        assert_eq!(busy_ms_for_tick(0.0, 100), 0);
    }

    #[test]
    fn busy_ms_full_for_100_pct() {
        assert_eq!(busy_ms_for_tick(100.0, 100), 100);
    }

    #[test]
    fn busy_ms_half_for_50_pct() {
        assert_eq!(busy_ms_for_tick(50.0, 100), 50);
    }

    #[test]
    fn busy_ms_clamps_above_100() {
        assert_eq!(busy_ms_for_tick(150.0, 100), 100);
    }

    #[test]
    fn busy_ms_clamps_below_0() {
        assert_eq!(busy_ms_for_tick(-10.0, 100), 0);
    }

    #[test]
    fn busy_ms_scales_with_tick() {
        assert_eq!(busy_ms_for_tick(25.0, 200), 50);
    }
}
