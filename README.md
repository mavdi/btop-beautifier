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
- **Network:** loopback TCP. A sender thread is paced by a token bucket to hit the sine-shaped target throughput; a receiver thread drains. `--net-cap` is the peak per direction, so on `lo` you'll see both RX and TX trace the same waveform.
- **Patterns:** sum of K sines per channel (additive synthesis). Re-rolled with new randomly-chosen frequencies, amplitudes, and phases every `--reroll` seconds.

## Development

```bash
cargo test                            # unit tests
cargo test --test smoke -- --ignored  # full integration smoke (drives real CPU/mem/net)
```

The smoke test deliberately spikes CPU and allocates memory. **Don't run it on shared/CI machines** without thinking — that's why it's gated behind `--ignored`.

## License

MIT (or whatever the parent omarchy scripts use — adjust if needed).
