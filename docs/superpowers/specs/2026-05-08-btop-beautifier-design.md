# btop-beautifier — Design

**Date:** 2026-05-08
**Status:** Approved, ready for implementation planning
**Language:** Rust

## Purpose

A CLI binary that synthetically drives CPU, memory, and network metrics in deliberate sine-wave patterns so that `btop` (running alongside it) renders beautiful, organic-looking graphs. Intended for demos, screen recordings, and pure visual delight.

## Scope

**In scope**
- Drive **CPU** (per-core, with phase-shifted sine traveling-wave effect)
- Drive **memory** (RSS grows/shrinks following a slower sine waveform)
- Drive **network** (loopback TCP traffic paced to a sine-shaped throughput)
- Pattern: **sum of K sines** per channel (additive synthesis), parameters re-rolled at a configurable interval
- Conservative resource defaults overridable via CLI flags
- Clean shutdown on SIGINT and on `--duration` expiry

**Out of scope**
- Disk I/O simulation
- GPU simulation
- Driving any external network interface (loopback only — uses no external bandwidth)
- TUI / interactive controls (no key bindings; status output only)
- Auto-launching btop or auto-configuring btop's interface selection

## User Experience

### Invocation

```
btop-beautifier [--duration 60s] [--mem-cap 4G] [--net-cap 50M]
                [--cpu-peak 90] [--reroll 15s] [--seed N]
```

All flags optional. With no flags: runs forever until SIGINT, with conservative defaults.

### Defaults

| Flag | Default | Meaning |
|------|---------|---------|
| `--duration` | none (run forever) | Stop cleanly after N seconds |
| `--mem-cap` | `4G` | Maximum RSS amplitude in bytes (peak of memory waveform) |
| `--net-cap` | `50M` | Maximum throughput per direction in bytes/sec (peak of network waveform) |
| `--cpu-peak` | `90` | Maximum per-core CPU % at sine peak (clamped to 100) |
| `--reroll` | `15s` | Interval between sine-parameter re-rolls |
| `--seed` | random | RNG seed for reproducible patterns |

### Status output

On startup:
```
[btop-beautifier] Driving CPU (8 cores), Memory (cap 4 GB), Network (loopback, cap 50 MB/s).
[btop-beautifier] Tip: press 'b' inside btop to cycle to 'lo' — that's where the network waveform shows.
```

While running, prints a single status line every second (overwrites in place using `\r`):
```
[btop-beautifier] t=0:23  cpu f=[0.12,0.18,0.27]Hz  mem f=[0.08]Hz  net f=[0.15,0.22]Hz  next reroll in 7s
```

On exit:
```
[btop-beautifier] stopped cleanly after 60s
```

### btop-side requirement

btop does not show loopback by default. The user must press `b` inside btop to cycle the network interface to `lo` (or set `net_iface = "lo"` in `~/.config/btop/btop.conf`). The startup tip mentions this. We do not modify btop's config.

## Architecture

Single Rust binary, std threads, blocking I/O. No async runtime.

### Module layout

```
src/
├── main.rs          orchestrator: CLI parsing, spawn drivers, signal handling, status loop
├── pattern.rs       PatternController + PatternParams (the math)
├── cpu.rs           CpuDriver — one pinned thread per logical core
├── memory.rs        MemoryDriver — RSS shaping via Vec<u8> + madvise
├── net.rs           NetDriver — loopback TCP sender + receiver
└── ratelimit.rs     token-bucket rate limiter (used by net + memory page-touch pacing)
```

### Component summary

| Component | Threads | Responsibility |
|-----------|---------|----------------|
| `PatternController` | main | Re-roll sine parameters every `--reroll` seconds; expose current `PatternParams` via `Arc<RwLock<>>` |
| `CpuDriver` | N (one per logical core, pinned) | Each tick: read pattern, compute target % for this core (with phase offset), busy-spin / sleep to hit it |
| `MemoryDriver` | 1 | Each tick: compute target RSS, touch pages or madvise(DONTNEED) on tail to grow/shrink |
| `NetDriver` | 2 (sender + receiver) | TCP loopback on a random port; sender paces writes via token bucket to hit target throughput; receiver drains |
| `StatusPrinter` | main | Print status line every 1s; print startup banner and exit message |

### Concurrency model

- All drivers share `Arc<PatternState>` where `PatternState { params: RwLock<PatternParams>, stop: AtomicBool, started_at: Instant }`.
- Drivers loop on `while !stop.load(Acquire)`. Read-lock `params` once per tick (cheap, contention-free for read-only access).
- `PatternController` write-locks briefly (microseconds) at re-roll boundaries.
- SIGINT handler (via `signal-hook`) sets `stop = true`. Main waits up to 2s for joins, then exits.

### Crates

- `clap` (CLI)
- `core_affinity` (thread pinning)
- `nix` (`madvise(MADV_DONTNEED)` on Linux)
- `signal-hook` (SIGINT handling)
- `rand` + `rand_pcg` (seeded reproducible RNG)
- `humantime` (parse `--duration 60s`, `--reroll 15s`)
- `bytesize` (parse `--mem-cap 4G`, `--net-cap 50M`)

No tokio. No async-std.

## Visual Algorithm

### Per-channel waveform

Each channel (CPU, Mem, Net) computes a normalized signal in [0, 1]:

```
raw(t)  = Σ_{k=1..K} a_k · sin(2π · f_k · t + φ_k)
norm(t) = (raw(t) + max_amp) / (2 · max_amp)        # rescale to [0, 1]
target(t) = norm(t) · channel_peak                   # final value
```

Where `max_amp = Σ a_k` (the theoretical max of the sum). This guarantees `target(t) ∈ [0, channel_peak]` always.

### Per-channel parameter ranges

| Param | CPU | Memory | Network |
|-------|-----|--------|---------|
| K (number of sines) | 3 | 1–2 | 2 |
| Frequency range | 0.05–0.30 Hz | 0.02–0.10 Hz | 0.05–0.20 Hz |
| Amplitude range | 0.3–1.0 (relative) | 0.5–1.0 | 0.3–1.0 |
| Channel peak | `--cpu-peak` (90 default) | `--mem-cap` (4G default) | `--net-cap` (50M default) |

Memory uses lower frequencies (slower visual drift) because RSS changes are more dramatic visually than CPU spikes; rapid mem oscillation looks frantic, not beautiful.

### Per-core CPU phase shift

For core `i` of `N` cores, the CPU pattern uses `φ_k + 2π · i / N` as the phase. With 8 cores and a single dominant sine, this produces an exact "wave traveling top to bottom across cores" visual on btop.

### Re-roll

Every `--reroll` seconds (default 15s):
1. Acquire write lock on `PatternParams`.
2. For each channel: pick new K (within range), new frequencies, new amplitudes, new base phases via the seeded RNG.
3. Release lock.

Re-rolls are abrupt by design (the visible waveform morphs immediately). If smoothing is desired, that's a v2 feature.

### Tick granularity

All drivers tick every **100ms**. btop's default `update_ms = 2000` (2s) averages 20 ticks per displayed sample → curves look smooth, not stepped.

## Driver Details

### CpuDriver (per core)

```
loop:
    if stop: break
    target_pct = pattern.cpu_target(core_id, now)    # 0..cpu_peak
    busy_ms = 100 * (target_pct / 100)
    sleep_ms = 100 - busy_ms
    busy_spin_for(busy_ms)                            # tight arithmetic loop
    sleep_for(sleep_ms)
```

Pinned via `core_affinity::set_for_current(...)`. Busy-spin uses a volatile arithmetic loop that the optimizer cannot remove (`black_box`).

### MemoryDriver

**Startup:** allocate `vec![0u8; mem_cap]` (full length, zero-initialized). This commits all pages briefly — RSS spikes to `mem_cap` for a moment. Immediately call `madvise(buf[..], MADV_DONTNEED)` to release all pages back to the kernel. The `Vec<u8>` retains its full length, but RSS drops to near zero. From here on, we shape RSS by touching pages (re-faulting them in) or madvise-ing them away.

Maintain a current `filled_len: usize` (initially 0).

```
loop:
    if stop: break
    target_len = pattern.mem_target(now)              # 0..mem_cap bytes
    if target_len > filled_len:
        # Touch one byte every 4096 bytes to fault pages in
        for offset in (filled_len..target_len).step_by(4096):
            buf[offset] = counter_byte                 # any non-zero write commits the page
        filled_len = target_len
    elif target_len < filled_len:
        madvise(buf[target_len..filled_len], MADV_DONTNEED)
        filled_len = target_len
    sleep 100ms
```

`MADV_DONTNEED` immediately reclaims those pages from RSS without resizing the Vec. On next growth, kernel re-faults zero pages — fast enough for our cadence. Because the Vec has full `len()` from startup, all index accesses `buf[offset]` are safe — no `unsafe`, no UB.

### NetDriver

```
listener thread:
    bind 127.0.0.1:0
    accept one connection
    read into 64KB scratch buffer in a loop until EOF or stop

sender thread:
    connect to listener address
    bucket = TokenBucket::new(net_cap_bytes_per_sec)
    loop:
        if stop: break
        target_bps = pattern.net_target(now)           # 0..net_cap
        bucket.set_rate(target_bps)
        bytes_this_tick = bucket.take(some chunk)
        send chunk
        sleep until next 100ms boundary
```

Both threads exit cleanly on stop. Sender shuts down the connection so the receiver's read returns 0 and exits its loop.

## Error Handling

- **Failed to pin a core** (e.g., affinity not supported): log a warning, run the CPU thread unpinned. Don't abort.
- **Failed to allocate memory cap**: log error, exit 1 with message ("requested --mem-cap NG exceeds available, try a smaller value").
- **Failed to bind loopback port**: extremely unlikely; log error, exit 1.
- **madvise fails**: log warning once, continue (memory waveform will look stair-stepped on the down-slope but binary keeps running).
- **SIGINT during startup** (before all drivers spawned): set stop, join what exists, exit 0.

No panics in steady-state code. All `Result`s handled.

## Cleanup on Exit

Triggered by SIGINT or `--duration` expiry:

1. `stop.store(true, Release)`.
2. NetDriver sender shuts down its connection.
3. Wait up to 2 seconds for all driver threads to join.
4. Drop the MemoryDriver buffer (full deallocation).
5. Print exit message.
6. Exit 0.

If joins time out (shouldn't happen with 100ms tick): log warning, exit anyway. OS will clean up.

## Testing

Three layers:

### 1. Unit tests
- `pattern.rs`: sum-of-sines stays in `[0, peak]` after normalization for a sweep of seeds.
- `pattern.rs`: phase shift formula produces correct values for known core counts.
- `ratelimit.rs`: token bucket honors target rate within ±5% over a 5s synthetic run.
- `memory.rs`: target-len computation is monotonic in target sine value (mock the sine).

### 2. Smoke test (integration)
- `cargo test --test smoke -- --ignored` runs the full binary with `--duration 5s --cpu-peak 30 --mem-cap 100M --net-cap 10M`. Asserts: process exits 0; status output appears; no panics in stderr.
- Marked `#[ignore]` so default `cargo test` doesn't actually drive system load.

### 3. Manual visual verification
- Run `cargo run --release -- --duration 30s --seed 42` alongside btop.
- Visually confirm: per-core traveling wave on CPU; smooth memory rise/fall; loopback throughput sine on btop's net graph (after pressing `b` to cycle to lo).
- This step is documented in README; no automation. The whole project is "looks right to a human."

### Gotcha: don't run smoke tests in CI on shared machines
The smoke test deliberately spikes CPU. Document in README that it's for local dev only.

## Performance & Resource Budget

| Resource | Budget |
|----------|--------|
| Per-CPU thread overhead (when target = 0) | < 0.5% — sleeps almost the whole tick |
| Per-CPU thread overhead (target = peak) | exactly target % (that's the point) |
| MemoryDriver thread CPU | negligible (< 1% of one core) |
| NetDriver threads CPU | ~5–10% of one core at 50 MB/s loopback |
| RSS overhead beyond `--mem-cap` | ~10 MB (binary, threads, Vec metadata) |
| External network | **zero** (loopback only) |

## Open Questions

None at design time. All resolved during brainstorming:
- Channel selection: CPU + Memory + Network (no disk, no GPU)
- Pattern style: additive sum-of-sines
- Per-core: phase-shifted (traveling wave)
- Run model: forever OR `--duration`, SIGINT to stop
- Resource policy: conservative defaults + flags

## Future / v2 Ideas (not for this implementation)

- Smooth re-rolls (interpolate parameters over 1–2s instead of jumping)
- Disk I/O channel
- GPU channel (would need vendor-specific shaders or compute kernels)
- Preset library (`--preset calm|wavy|frantic|chaos`)
- Auto-detect btop running and warn if `net_iface` isn't `lo`
- TUI mode with live parameter editing

## File Output Summary

The implementation will produce:
- `Cargo.toml` (with the crates listed above)
- `src/main.rs`, `src/pattern.rs`, `src/cpu.rs`, `src/memory.rs`, `src/net.rs`, `src/ratelimit.rs`
- `tests/smoke.rs`
- `README.md` (usage, btop tip, dev/test note)
