//! Token bucket rate limiter.

use std::time::Instant;

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
