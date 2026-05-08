# btop-beautifier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI binary that drives CPU (per-core, phase-shifted), memory, and loopback network in sum-of-sines patterns to make btop's graphs look beautiful.

**Architecture:** Single binary, std threads only (no async runtime). One thread per logical CPU core (pinned via `core_affinity`), one for memory shaping (`Vec<u8>` + `madvise(MADV_DONTNEED)`), two for loopback TCP (sender + receiver). All drivers read shared `PatternParams` via `Arc<RwLock<>>`, re-rolled every 15s by the main thread.

**Tech Stack:** Rust 2021 edition. Crates: `clap` (CLI), `core_affinity`, `nix` (madvise), `signal-hook` (SIGINT), `rand` + `rand_pcg` (seeded RNG), `humantime` (time parsing), `bytesize` (byte parsing).

**Spec:** `docs/superpowers/specs/2026-05-08-btop-beautifier-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| `Cargo.toml` | Crate manifest, dependencies, lib + bin targets |
| `.gitignore` | Rust ignores (`target/`, etc.) |
| `src/lib.rs` | Re-exports modules so `cargo test` discovers unit tests |
| `src/main.rs` | CLI parsing, signal handling, orchestration, status printer |
| `src/pattern.rs` | `PatternParams`, `PatternController`, sum-of-sines math, phase shift |
| `src/ratelimit.rs` | `TokenBucket` rate limiter |
| `src/cpu.rs` | `CpuDriver` — per-core pinned threads, busy-spin/sleep |
| `src/memory.rs` | `MemoryDriver` — Vec<u8> shaped via madvise + page touch |
| `src/net.rs` | `NetDriver` — TCP loopback sender+receiver, paced by token bucket |
| `tests/smoke.rs` | Integration test (gated `#[ignore]` so default `cargo test` doesn't drive load) |
| `README.md` | Usage, btop tip, dev/test notes |

Modules are loaded from `lib.rs` via `pub mod`, and `main.rs` uses `btop_beautifier::*`.

---

## Task 1: Project initialization

**Files:**
- Create: `.gitignore`
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`

- [ ] **Step 1: Initialize git repo**

Run from `/home/mavdi/Documents/work/omarchy/scripts/btop-beautifier`:

```bash
git init
git add docs/
git commit -m "docs: add btop-beautifier design spec"
```

Expected: a single commit with the existing spec/plan docs.

- [ ] **Step 2: Create `.gitignore`**

Create `.gitignore` with:

```
/target
**/*.rs.bk
Cargo.lock
```

(Note: `Cargo.lock` is intentionally ignored because this is a binary that ships through the omarchy scripts directory, not a published library — keep `Cargo.lock` ignored for a more flexible distribution model. If the user later wants reproducible builds, they can remove this line.)

- [ ] **Step 3: Create `Cargo.toml`**

Create `Cargo.toml`:

```toml
[package]
name = "btop-beautifier"
version = "0.1.0"
edition = "2021"
description = "Drive CPU, memory, and loopback network in sine-wave patterns to make btop look beautiful"

[[bin]]
name = "btop-beautifier"
path = "src/main.rs"

[lib]
name = "btop_beautifier"
path = "src/lib.rs"

[dependencies]
clap = { version = "4.5", features = ["derive"] }
core_affinity = "0.8"
nix = { version = "0.29", features = ["mman"] }
signal-hook = "0.3"
rand = "0.8"
rand_pcg = "0.3"
humantime = "2.1"
bytesize = "1.3"

[profile.release]
opt-level = 3
lto = "thin"
```

- [ ] **Step 4: Create stub `src/lib.rs`**

Create `src/lib.rs`:

```rust
pub mod pattern;
pub mod ratelimit;
pub mod cpu;
pub mod memory;
pub mod net;
```

- [ ] **Step 5: Create stub `src/main.rs`**

Create `src/main.rs`:

```rust
fn main() {
    println!("btop-beautifier (stub)");
}
```

- [ ] **Step 6: Create stub source files so `cargo build` compiles**

Create empty stubs (each file just needs to exist; functions added in later tasks):

`src/pattern.rs`:
```rust
// Pattern math; populated in Task 2-4.
```

`src/ratelimit.rs`:
```rust
// Token bucket rate limiter; populated in Task 5.
```

`src/cpu.rs`:
```rust
// CPU driver; populated in Task 6.
```

`src/memory.rs`:
```rust
// Memory driver; populated in Task 7.
```

`src/net.rs`:
```rust
// Network driver; populated in Task 8.
```

- [ ] **Step 7: Verify `cargo build` succeeds**

Run: `cargo build`
Expected: warnings about empty modules are fine; no errors. Crates download and compile.

- [ ] **Step 8: Commit**

```bash
git add .gitignore Cargo.toml src/
git commit -m "chore: scaffold cargo project with deps"
```

---

## Task 2: Pattern math — sum-of-sines normalization

Implements the `PatternParams` data struct and the per-channel value computation: sum a few sines, normalize the result into `[0, 1]`, scale to a peak.

**Files:**
- Modify: `src/pattern.rs`

- [ ] **Step 1: Write failing tests**

Replace `src/pattern.rs` contents with the test scaffolding (impl will follow in step 3):

```rust
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
```

- [ ] **Step 2: Run tests — expect PASS** (the impl is included alongside the tests in this single step for clarity)

Run: `cargo test --lib pattern::tests`
Expected: 3 passing tests.

(Note for the implementer: this task collapses test+impl because the math is short enough that splitting them into separate steps adds noise. The actual TDD discipline starts in Task 3 where we add new behavior to existing code.)

- [ ] **Step 3: Commit**

```bash
git add src/pattern.rs
git commit -m "feat(pattern): add sum-of-sines ChannelParams with normalization"
```

---

## Task 3: Pattern — per-core CPU phase shift

The CPU channel's pattern is the same `ChannelParams`, but each core adds its own phase offset to produce a traveling-wave visual.

**Files:**
- Modify: `src/pattern.rs`

- [ ] **Step 1: Write failing test**

Append to the `mod tests` block in `src/pattern.rs` (above the closing `}`):

```rust
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
```

- [ ] **Step 2: Run test — expect FAIL** (`target_for_core` not defined)

Run: `cargo test --lib pattern::tests::per_core_phase_shift_is_uniform`
Expected: compile error — `no method named target_for_core`.

- [ ] **Step 3: Implement `target_for_core`**

Add this method inside `impl ChannelParams { ... }` in `src/pattern.rs`:

```rust
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
```

- [ ] **Step 4: Run tests — expect PASS**

Run: `cargo test --lib pattern::tests`
Expected: 5 passing tests.

- [ ] **Step 5: Commit**

```bash
git add src/pattern.rs
git commit -m "feat(pattern): add per-core phase shift for CPU traveling wave"
```

---

## Task 4: Pattern — `PatternState` shared across drivers + reroll

Build the `PatternState` that holds the current per-channel params and a stop flag, plus a function to re-roll all channels using a seeded RNG.

**Files:**
- Modify: `src/pattern.rs`

- [ ] **Step 1: Write failing test**

Append inside `mod tests`:

```rust
    use std::sync::Arc;

    #[test]
    fn pattern_state_reroll_changes_params() {
        let state = Arc::new(PatternState::new(123));
        let cpu_before = state.snapshot().cpu.clone();
        state.reroll();
        let cpu_after = state.snapshot().cpu.clone();
        // Params should differ (with extreme probability for seeded RNG).
        let same = cpu_before.sines.len() == cpu_after.sines.len()
            && cpu_before
                .sines
                .iter()
                .zip(cpu_after.sines.iter())
                .all(|(a, b)| (a.frequency_hz - b.frequency_hz).abs() < 1e-9);
        assert!(!same, "reroll should change params");
    }

    #[test]
    fn pattern_state_seeded_is_reproducible() {
        let a = Arc::new(PatternState::new(999));
        let b = Arc::new(PatternState::new(999));
        let pa = a.snapshot();
        let pb = b.snapshot();
        assert_eq!(pa.cpu.sines.len(), pb.cpu.sines.len());
        for (x, y) in pa.cpu.sines.iter().zip(pb.cpu.sines.iter()) {
            assert!((x.frequency_hz - y.frequency_hz).abs() < 1e-9);
            assert!((x.amplitude - y.amplitude).abs() < 1e-9);
            assert!((x.phase_rad - y.phase_rad).abs() < 1e-9);
        }
    }

    #[test]
    fn pattern_state_stop_flag_default_false() {
        let state = PatternState::new(0);
        assert!(!state.is_stopped());
        state.stop();
        assert!(state.is_stopped());
    }
```

- [ ] **Step 2: Run tests — expect FAIL** (`PatternState` not defined)

Run: `cargo test --lib pattern::tests::pattern_state_reroll_changes_params`
Expected: compile error.

- [ ] **Step 3: Implement `PatternState` and `AllChannels`**

Append to `src/pattern.rs` (outside the test module):

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Instant;

/// All channels' current parameters.
#[derive(Clone, Debug)]
pub struct AllChannels {
    pub cpu: ChannelParams,
    pub memory: ChannelParams,
    pub network: ChannelParams,
}

/// Shared state read by all drivers and written by the pattern controller.
pub struct PatternState {
    params: RwLock<AllChannels>,
    stop: AtomicBool,
    started_at: Instant,
    rng: Mutex<Pcg64>,
}

impl PatternState {
    pub fn new(seed: u64) -> Self {
        let mut rng = Pcg64::seed_from_u64(seed);
        let params = AllChannels {
            cpu: roll_channel(&mut rng, 3, 0.05..0.30, 0.3..1.0),
            memory: roll_channel(&mut rng, rng.gen_range(1..=2), 0.02..0.10, 0.5..1.0),
            network: roll_channel(&mut rng, 2, 0.05..0.20, 0.3..1.0),
        };
        Self {
            params: RwLock::new(params),
            stop: AtomicBool::new(false),
            started_at: Instant::now(),
            rng: Mutex::new(rng),
        }
    }

    /// Snapshot current params (cheap clone — reads under read-lock).
    pub fn snapshot(&self) -> AllChannels {
        self.params.read().expect("pattern lock poisoned").clone()
    }

    /// Re-roll all channels using the seeded RNG.
    pub fn reroll(&self) {
        let mut rng = self.rng.lock().expect("rng lock poisoned");
        let new_params = AllChannels {
            cpu: roll_channel(&mut rng, 3, 0.05..0.30, 0.3..1.0),
            memory: roll_channel(&mut rng, rng.gen_range(1..=2), 0.02..0.10, 0.5..1.0),
            network: roll_channel(&mut rng, 2, 0.05..0.20, 0.3..1.0),
        };
        *self.params.write().expect("pattern lock poisoned") = new_params;
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

fn roll_channel(
    rng: &mut Pcg64,
    k: usize,
    freq_range: std::ops::Range<f64>,
    amp_range: std::ops::Range<f64>,
) -> ChannelParams {
    let two_pi = 2.0 * std::f64::consts::PI;
    let sines = (0..k)
        .map(|_| Sine {
            amplitude: rng.gen_range(amp_range.clone()),
            frequency_hz: rng.gen_range(freq_range.clone()),
            phase_rad: rng.gen_range(0.0..two_pi),
        })
        .collect();
    ChannelParams { sines }
}
```

- [ ] **Step 4: Run tests — expect PASS**

Run: `cargo test --lib pattern::tests`
Expected: 8 passing tests.

- [ ] **Step 5: Commit**

```bash
git add src/pattern.rs
git commit -m "feat(pattern): add PatternState with reroll + stop flag"
```

---

## Task 5: Token bucket rate limiter

Used by `NetDriver` (and conceptually by `MemoryDriver` if needed) to pace work to a target rate.

**Files:**
- Modify: `src/ratelimit.rs`

- [ ] **Step 1: Write failing tests**

Replace `src/ratelimit.rs` contents:

```rust
//! Token bucket rate limiter.

use std::time::{Duration, Instant};

/// Simple token bucket. Caller pulls tokens; bucket refills at `rate` tokens/sec.
pub struct TokenBucket {
    rate: f64,        // tokens per second
    capacity: f64,    // max tokens (burst budget)
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(rate: f64, capacity: f64) -> Self {
        Self {
            rate,
            capacity,
            tokens: capacity,
            last_refill: Instant::now(),
        }
    }

    /// Update the rate. Existing tokens preserved.
    pub fn set_rate(&mut self, rate: f64) {
        self.refill();
        self.rate = rate;
    }

    /// Refill tokens based on elapsed time.
    fn refill(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + dt * self.rate).min(self.capacity);
        self.last_refill = now;
    }

    /// Try to take up to `want` tokens; returns the number actually taken (0 if empty).
    pub fn take(&mut self, want: f64) -> f64 {
        self.refill();
        let taken = want.min(self.tokens).max(0.0);
        self.tokens -= taken;
        taken
    }

    /// Sleep until at least `want` tokens are available, then take them.
    pub fn take_blocking(&mut self, want: f64) {
        loop {
            self.refill();
            if self.tokens >= want {
                self.tokens -= want;
                return;
            }
            let need = want - self.tokens;
            let wait = Duration::from_secs_f64(need / self.rate.max(1.0));
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn refills_at_target_rate_within_tolerance() {
        // 1000 tokens/sec, 1000 capacity. Drain it, wait 500ms, expect ~500 tokens back.
        let mut bucket = TokenBucket::new(1000.0, 1000.0);
        let _ = bucket.take(1000.0);
        std::thread::sleep(Duration::from_millis(500));
        let got = bucket.take(1000.0);
        assert!(
            got > 400.0 && got < 600.0,
            "expected ~500 tokens after 500ms at 1000/s, got {got}",
        );
    }

    #[test]
    fn cannot_exceed_capacity() {
        let mut bucket = TokenBucket::new(1_000_000.0, 100.0);
        std::thread::sleep(Duration::from_millis(50));
        let got = bucket.take(1_000_000.0);
        assert!(got <= 100.0, "should not exceed capacity 100, got {got}");
    }

    #[test]
    fn set_rate_changes_refill() {
        let mut bucket = TokenBucket::new(100.0, 100.0);
        let _ = bucket.take(100.0);
        bucket.set_rate(10_000.0);
        std::thread::sleep(Duration::from_millis(50));
        let got = bucket.take(10_000.0);
        // After 50ms at 10000/s = 500 tokens, capped at capacity 100.
        assert!(got <= 100.0 && got > 50.0, "expected ~100 (capacity), got {got}");
    }
}
```

- [ ] **Step 2: Run tests — expect PASS**

Run: `cargo test --lib ratelimit::tests`
Expected: 3 passing tests.

- [ ] **Step 3: Commit**

```bash
git add src/ratelimit.rs
git commit -m "feat(ratelimit): add token bucket"
```

---

## Task 6: CPU driver

Per-core pinned thread that busy-spins/sleeps to hit the target CPU% from the pattern.

**Files:**
- Modify: `src/cpu.rs`

- [ ] **Step 1: Write failing tests for the pure helper**

Replace `src/cpu.rs` contents:

```rust
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
```

- [ ] **Step 2: Run tests — expect PASS**

Run: `cargo test --lib cpu::tests`
Expected: 6 passing tests.

- [ ] **Step 3: Sanity-check the busy-spin manually (optional, no commit gate)**

Optional: write a small ad-hoc test (don't keep it) that calls `busy_spin(Duration::from_millis(50))` and asserts elapsed is between 45 and 80ms. Skip if the previous unit tests passed cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/cpu.rs
git commit -m "feat(cpu): per-core driver with busy-spin and affinity pinning"
```

---

## Task 7: Memory driver

Allocates `mem_cap` bytes once, releases all pages via `madvise(MADV_DONTNEED)`, then shapes RSS by re-touching pages and madvise-ing the unused tail.

**Files:**
- Modify: `src/memory.rs`

- [ ] **Step 1: Write failing tests for the pure helper**

Replace `src/memory.rs` contents:

```rust
//! Memory driver: shape RSS via page-touch + madvise(MADV_DONTNEED).

use crate::pattern::PatternState;
use nix::sys::mman::{madvise, MmapAdvise};
use std::num::NonZeroUsize;
use std::ptr::NonNull;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const PAGE_STEP: usize = 4096;
const TICK_MS: u64 = 100;

/// Compute the target filled length in bytes for a given normalized signal in [0, 1].
pub fn target_len_bytes(normalized: f64, cap: usize) -> usize {
    let n = normalized.clamp(0.0, 1.0);
    ((n * cap as f64) as usize).min(cap)
}

/// Run the memory driver until `pattern.is_stopped()`.
pub fn run(pattern: Arc<PatternState>, cap: usize) -> std::io::Result<()> {
    if cap == 0 {
        return Ok(());
    }
    let mut buf: Vec<u8> = vec![0u8; cap];
    // Release initial RSS.
    advise_dontneed(&mut buf[..]);
    let mut filled_len: usize = 0;
    let mut counter: u8 = 1;
    while !pattern.is_stopped() {
        let snap = pattern.snapshot();
        let normalized = snap.memory.normalized(pattern.elapsed_secs());
        let target = target_len_bytes(normalized, cap);
        if target > filled_len {
            // Touch pages [filled_len..target] to fault them in.
            let mut offset = (filled_len + PAGE_STEP - 1) / PAGE_STEP * PAGE_STEP;
            // ensure we touch the boundary too
            if offset >= target {
                offset = filled_len;
            }
            while offset < target {
                buf[offset] = counter;
                offset = offset.saturating_add(PAGE_STEP);
            }
            counter = counter.wrapping_add(1);
            filled_len = target;
        } else if target < filled_len {
            advise_dontneed(&mut buf[target..filled_len]);
            filled_len = target;
        }
        thread::sleep(Duration::from_millis(TICK_MS));
    }
    Ok(())
}

fn advise_dontneed(slice: &mut [u8]) {
    if slice.is_empty() {
        return;
    }
    let len = match NonZeroUsize::new(slice.len()) {
        Some(l) => l,
        None => return,
    };
    let ptr = match NonNull::new(slice.as_mut_ptr() as *mut std::ffi::c_void) {
        Some(p) => p,
        None => return,
    };
    // SAFETY: `ptr` and `len` come from a valid &mut [u8].
    unsafe {
        let _ = madvise(ptr, len.get(), MmapAdvise::MADV_DONTNEED);
    }
}

pub fn spawn(pattern: Arc<PatternState>, cap: usize) -> JoinHandle<()> {
    thread::Builder::new()
        .name("mem-driver".into())
        .spawn(move || {
            if let Err(e) = run(pattern, cap) {
                eprintln!("[btop-beautifier] memory driver error: {e}");
            }
        })
        .expect("failed to spawn memory driver thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_len_zero() {
        assert_eq!(target_len_bytes(0.0, 1_000_000), 0);
    }

    #[test]
    fn target_len_full() {
        assert_eq!(target_len_bytes(1.0, 1_000_000), 1_000_000);
    }

    #[test]
    fn target_len_half() {
        assert_eq!(target_len_bytes(0.5, 1_000_000), 500_000);
    }

    #[test]
    fn target_len_clamps_above_one() {
        assert_eq!(target_len_bytes(2.5, 100), 100);
    }

    #[test]
    fn target_len_clamps_below_zero() {
        assert_eq!(target_len_bytes(-0.5, 100), 0);
    }
}
```

- [ ] **Step 2: Run tests — expect PASS**

Run: `cargo test --lib memory::tests`
Expected: 5 passing tests.

- [ ] **Step 3: Commit**

```bash
git add src/memory.rs
git commit -m "feat(memory): RSS driver via Vec<u8> + madvise + page touch"
```

---

## Task 8: Network driver

Loopback TCP. Sender thread paces writes via token bucket. Receiver thread drains.

**Files:**
- Modify: `src/net.rs`

- [ ] **Step 1: Write the implementation**

(No pure helpers worth unit-testing here — the rate logic lives in `TokenBucket` which is already tested. Network behavior is verified in the smoke test.)

Replace `src/net.rs` contents:

```rust
//! Network driver: loopback TCP, sender paced by token bucket.

use crate::pattern::PatternState;
use crate::ratelimit::TokenBucket;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TICK_MS: u64 = 100;
const CHUNK_SIZE: usize = 64 * 1024;

/// Spawn the network driver: returns (listener_thread, sender_thread).
/// `peak_bytes_per_sec` is the maximum throughput at sine peak.
pub fn spawn(pattern: Arc<PatternState>, peak_bytes_per_sec: f64) -> std::io::Result<(JoinHandle<()>, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;

    let receiver_pattern = Arc::clone(&pattern);
    let receiver_thread = thread::Builder::new()
        .name("net-receiver".into())
        .spawn(move || {
            run_receiver(listener, receiver_pattern);
        })?;

    let sender_pattern = Arc::clone(&pattern);
    let sender_thread = thread::Builder::new()
        .name("net-sender".into())
        .spawn(move || {
            run_sender(addr.to_string(), sender_pattern, peak_bytes_per_sec);
        })?;

    Ok((receiver_thread, sender_thread))
}

fn run_receiver(listener: TcpListener, pattern: Arc<PatternState>) {
    listener.set_nonblocking(true).ok();
    // Wait for the sender to connect (with stop check).
    let stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if pattern.is_stopped() {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("[btop-beautifier] net receiver accept error: {e}");
                return;
            }
        }
    };

    if let Err(e) = stream.set_read_timeout(Some(Duration::from_millis(200))) {
        eprintln!("[btop-beautifier] net receiver set_read_timeout error: {e}");
        return;
    }
    let mut stream = stream;
    let mut buf = vec![0u8; CHUNK_SIZE];
    while !pattern.is_stopped() {
        match stream.read(&mut buf) {
            Ok(0) => return, // EOF
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => return,
        }
    }
}

fn run_sender(addr: String, pattern: Arc<PatternState>, peak_bps: f64) {
    let mut stream = match TcpStream::connect(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[btop-beautifier] net sender connect error: {e}");
            return;
        }
    };
    if let Err(e) = stream.set_write_timeout(Some(Duration::from_millis(500))) {
        eprintln!("[btop-beautifier] net sender set_write_timeout error: {e}");
        return;
    }
    // Capacity is 1 second worth at peak.
    let mut bucket = TokenBucket::new(peak_bps, peak_bps);
    let chunk = vec![0u8; CHUNK_SIZE];

    while !pattern.is_stopped() {
        let tick_start = Instant::now();
        let snap = pattern.snapshot();
        let target_bps = snap.network.target(pattern.elapsed_secs(), peak_bps);
        bucket.set_rate(target_bps);

        // Compute how many bytes to send this tick.
        let bytes_this_tick = (target_bps * (TICK_MS as f64) / 1000.0) as usize;
        let mut sent = 0usize;
        while sent < bytes_this_tick && !pattern.is_stopped() {
            let want = (bytes_this_tick - sent).min(CHUNK_SIZE) as f64;
            let granted = bucket.take(want);
            let n = granted as usize;
            if n == 0 {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            if stream.write_all(&chunk[..n]).is_err() {
                return;
            }
            sent += n;
        }
        // Sleep the rest of the tick.
        let elapsed = tick_start.elapsed();
        if elapsed < Duration::from_millis(TICK_MS) {
            thread::sleep(Duration::from_millis(TICK_MS) - elapsed);
        }
    }

    // Clean shutdown so receiver exits.
    let _ = stream.shutdown(std::net::Shutdown::Both);
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: clean build (warnings ok).

- [ ] **Step 3: Commit**

```bash
git add src/net.rs
git commit -m "feat(net): loopback TCP driver paced by token bucket"
```

---

## Task 9: CLI parsing, signal handling, and orchestration in `main.rs`

Tie everything together: parse CLI args, spawn pattern controller (re-roll loop), spawn drivers, print status, handle SIGINT and `--duration`.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace `src/main.rs`**

```rust
use bytesize::ByteSize;
use clap::Parser;
use signal_hook::consts::SIGINT;
use signal_hook::flag::register;
use std::io::Write;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use btop_beautifier::cpu;
use btop_beautifier::memory;
use btop_beautifier::net;
use btop_beautifier::pattern::PatternState;

/// Drive CPU, memory, and loopback network in sine-wave patterns to make btop look beautiful.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Stop cleanly after this duration (e.g. `60s`, `2m`). Runs forever if omitted.
    #[arg(long, value_parser = parse_duration)]
    duration: Option<Duration>,

    /// Maximum RSS amplitude (bytes), e.g. `4G`, `512M`.
    #[arg(long, value_parser = parse_bytesize, default_value = "4G")]
    mem_cap: u64,

    /// Maximum loopback throughput per direction (bytes/sec), e.g. `50M`, `200M`.
    #[arg(long, value_parser = parse_bytesize, default_value = "50M")]
    net_cap: u64,

    /// Maximum per-core CPU percent at sine peak (0..=100).
    #[arg(long, default_value_t = 90.0)]
    cpu_peak: f64,

    /// Re-roll interval for sine parameters.
    #[arg(long, value_parser = parse_duration, default_value = "15s")]
    reroll: Duration,

    /// RNG seed for reproducible patterns.
    #[arg(long)]
    seed: Option<u64>,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

fn parse_bytesize(s: &str) -> Result<u64, String> {
    ByteSize::from_str(s).map(|b| b.as_u64()).map_err(|e| e.to_string())
}

fn main() {
    let args = Args::parse();
    let cpu_peak = args.cpu_peak.clamp(0.0, 100.0);
    let mem_cap = args.mem_cap as usize;
    let net_cap_bps = args.net_cap as f64;
    let seed = args.seed.unwrap_or_else(|| {
        use std::time::SystemTime;
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xC0FFEE_F00D)
    });

    let state = Arc::new(PatternState::new(seed));

    // Signal handling: register SIGINT to flip a stop flag in the state.
    let sigint_flag = Arc::new(AtomicBool::new(false));
    if let Err(e) = register(SIGINT, Arc::clone(&sigint_flag)) {
        eprintln!("[btop-beautifier] failed to register SIGINT handler: {e}");
        std::process::exit(1);
    }

    let num_cores = core_affinity::get_core_ids().map(|c| c.len()).unwrap_or(1);
    println!(
        "[btop-beautifier] Driving CPU ({} cores), Memory (cap {}), Network (loopback, cap {}/s).",
        num_cores,
        ByteSize::b(mem_cap as u64),
        ByteSize::b(args.net_cap),
    );
    println!(
        "[btop-beautifier] Tip: press 'b' inside btop to cycle to 'lo' \u{2014} that's where the network waveform shows."
    );
    println!("[btop-beautifier] Seed: {seed}");

    // Spawn drivers.
    let cpu_handles = cpu::spawn_all(Arc::clone(&state), cpu_peak);
    let mem_handle = memory::spawn(Arc::clone(&state), mem_cap);
    let net_handles = match net::spawn(Arc::clone(&state), net_cap_bps) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("[btop-beautifier] failed to start net driver: {e} (continuing without network)");
            None
        }
    };

    // Reroll + status loop on main thread.
    let started = Instant::now();
    let mut last_reroll = Instant::now();
    let mut last_status = Instant::now();
    let stdout = std::io::stdout();

    loop {
        if sigint_flag.load(Ordering::Acquire) {
            break;
        }
        if let Some(d) = args.duration {
            if started.elapsed() >= d {
                break;
            }
        }
        if last_reroll.elapsed() >= args.reroll {
            state.reroll();
            last_reroll = Instant::now();
        }
        if last_status.elapsed() >= Duration::from_secs(1) {
            print_status(&state, started.elapsed(), args.reroll, last_reroll.elapsed());
            let _ = stdout.lock().flush();
            last_status = Instant::now();
        }
        thread::sleep(Duration::from_millis(100));
    }

    // Initiate shutdown.
    println!("\n[btop-beautifier] stopping...");
    state.stop();

    // Join with timeout (best-effort).
    join_with_timeout(cpu_handles, Duration::from_secs(2));
    join_with_timeout(vec![mem_handle], Duration::from_secs(2));
    if let Some((r, s)) = net_handles {
        join_with_timeout(vec![r, s], Duration::from_secs(2));
    }

    println!(
        "[btop-beautifier] stopped cleanly after {}s",
        started.elapsed().as_secs()
    );
}

fn print_status(state: &PatternState, elapsed: Duration, reroll: Duration, since_reroll: Duration) {
    let snap = state.snapshot();
    let cpu_freqs: Vec<String> = snap.cpu.sines.iter().map(|s| format!("{:.2}", s.frequency_hz)).collect();
    let mem_freqs: Vec<String> = snap.memory.sines.iter().map(|s| format!("{:.2}", s.frequency_hz)).collect();
    let net_freqs: Vec<String> = snap.network.sines.iter().map(|s| format!("{:.2}", s.frequency_hz)).collect();
    let next_in = reroll.as_secs().saturating_sub(since_reroll.as_secs());
    print!(
        "\r[btop-beautifier] t={:>3}s  cpu f=[{}]Hz  mem f=[{}]Hz  net f=[{}]Hz  next reroll in {:>2}s   ",
        elapsed.as_secs(),
        cpu_freqs.join(","),
        mem_freqs.join(","),
        net_freqs.join(","),
        next_in,
    );
}

fn join_with_timeout(handles: Vec<thread::JoinHandle<()>>, timeout: Duration) {
    // Naive: spin a watchdog that gives up after timeout. We can't actually cancel, so if a thread
    // doesn't observe the stop flag in time we leak it (process exit cleans up).
    let deadline = Instant::now() + timeout;
    for h in handles {
        loop {
            if h.is_finished() {
                let _ = h.join();
                break;
            }
            if Instant::now() > deadline {
                eprintln!("[btop-beautifier] thread join timeout — forcing exit");
                std::process::exit(0);
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}
```

- [ ] **Step 2: Verify build + help output**

Run: `cargo build`
Expected: clean build.

Run: `cargo run -- --help`
Expected: clap-generated help text listing `--duration`, `--mem-cap`, `--net-cap`, `--cpu-peak`, `--reroll`, `--seed`.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): CLI, signal handling, orchestration"
```

---

## Task 10: Smoke test

Integration test that runs the binary briefly with conservative caps to catch regressions.

**Files:**
- Create: `tests/smoke.rs`

- [ ] **Step 1: Create the test**

Create `tests/smoke.rs`:

```rust
//! Smoke test: runs the binary briefly with tiny caps. Marked #[ignore] so it
//! doesn't run on shared CI by default — opt in with `cargo test -- --ignored`.

use std::process::Command;

fn binary_path() -> std::path::PathBuf {
    let mut p = std::env::current_exe().expect("test binary path");
    // tests/smoke-XXXXXX -> drop the test binary name and the deps/ folder
    p.pop(); // remove smoke-XXX
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("btop-beautifier");
    p
}

#[test]
#[ignore]
fn runs_briefly_and_exits_clean() {
    // Build first so the binary exists.
    let build = Command::new("cargo")
        .args(["build", "--bin", "btop-beautifier"])
        .status()
        .expect("cargo build");
    assert!(build.success(), "cargo build failed");

    let bin = binary_path();
    assert!(bin.exists(), "binary not found at {}", bin.display());

    let output = Command::new(&bin)
        .args([
            "--duration", "3s",
            "--cpu-peak", "20",
            "--mem-cap", "50M",
            "--net-cap", "5M",
            "--reroll", "1s",
            "--seed", "1",
        ])
        .output()
        .expect("run binary");

    assert!(
        output.status.success(),
        "binary exited non-zero: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Driving CPU"), "missing startup banner: {}", stdout);
    assert!(stdout.contains("stopped cleanly"), "missing exit message: {}", stdout);
}
```

- [ ] **Step 2: Run the smoke test**

Run: `cargo test --test smoke -- --ignored --nocapture`
Expected: PASS. Watch for the startup banner and exit message in the output.

- [ ] **Step 3: Verify default `cargo test` does NOT run the smoke**

Run: `cargo test`
Expected: passes; smoke test is listed as ignored.

- [ ] **Step 4: Commit**

```bash
git add tests/smoke.rs
git commit -m "test: add gated smoke test"
```

---

## Task 11: README

Document usage, the btop loopback tip, and the dev/test note.

**Files:**
- Create: `README.md`

- [ ] **Step 1: Create README**

Create `README.md`:

```markdown
# btop-beautifier

A small Rust CLI that drives CPU, memory, and loopback network in sine-wave patterns so [btop](https://github.com/aristocratos/btop) renders beautiful, organic-looking graphs. Useful for demos, screen recordings, or pure visual delight.

## Build

```bash
cargo build --release
```

The binary lands at `target/release/btop-beautifier`.

## Usage

```bash
btop-beautifier                              # run forever, conservative defaults
btop-beautifier --duration 60s               # stop cleanly after 60 seconds
btop-beautifier --cpu-peak 100 --mem-cap 8G  # crank it up
btop-beautifier --seed 42                    # reproducible patterns (handy for screen recordings)
```

Run it in one terminal, btop in another. Hit Ctrl+C to stop.

### Flags

| Flag | Default | Meaning |
|------|---------|---------|
| `--duration` | none | Run for this long, then exit cleanly. Accepts e.g. `30s`, `2m`. |
| `--mem-cap` | `4G` | Peak RSS the memory waveform reaches. Accepts e.g. `512M`, `8G`. |
| `--net-cap` | `50M` | Peak loopback throughput per direction (bytes/sec). |
| `--cpu-peak` | `90` | Peak per-core CPU % at sine maximum (clamped to 100). |
| `--reroll` | `15s` | How often to randomise sine parameters. |
| `--seed N` | random | RNG seed for reproducible patterns. |

### btop tip — see the network waveform

btop hides loopback by default. Press `b` inside btop to cycle the network interface to `lo`, or set `net_iface = "lo"` in `~/.config/btop/btop.conf`.

## How it works

- **CPU:** one pinned thread per logical core. Each tick (100ms), the thread busy-spins for `target%` of the tick and sleeps the rest. Each core uses the same sine parameters but with a per-core phase offset, producing a traveling-wave visual across cores.
- **Memory:** allocates `--mem-cap` zeroed bytes at startup, immediately releases them via `madvise(MADV_DONTNEED)`, then shapes RSS by re-touching pages (grow) and madvising the unused tail (shrink).
- **Network:** loopback TCP. A sender thread is paced by a token bucket to hit the sine-shaped target throughput; a receiver thread drains.
- **Patterns:** sum of K sines per channel (additive synthesis). Re-rolled with new randomly-chosen frequencies, amplitudes, and phases every `--reroll` seconds.

## Development

```bash
cargo test                            # unit tests
cargo test --test smoke -- --ignored  # full integration smoke (drives real CPU/mem/net)
```

The smoke test deliberately spikes CPU and allocates memory. **Don't run it on shared/CI machines** without thinking — that's why it's gated behind `--ignored`.

## License

MIT (or whatever the parent omarchy scripts use — adjust if needed).
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add README"
```

---

## Final verification

- [ ] **Run all tests:**

```bash
cargo test
cargo test --test smoke -- --ignored
```

Expected: all pass.

- [ ] **Build release:**

```bash
cargo build --release
```

Expected: clean release build.

- [ ] **Visual check (manual, optional):**

```bash
# Terminal 1
btop
# (press 'b' to cycle to lo)

# Terminal 2
./target/release/btop-beautifier --duration 30s --seed 42
```

Expected: per-core CPU graphs trace a traveling sine wave; memory graph slowly rises and falls; loopback network graph oscillates.

- [ ] **All commits clean, working tree empty:**

```bash
git status
git log --oneline
```

Expected: ~11 commits, clean tree.
