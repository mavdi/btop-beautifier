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
