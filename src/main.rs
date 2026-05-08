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
