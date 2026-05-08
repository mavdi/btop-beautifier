pub mod pattern;
pub mod ratelimit;
pub mod cpu;
pub mod memory;
pub mod net;

/// Driver tick granularity. btop's default 2s sample averages ~20 ticks at 100ms.
pub const TICK_MS: u64 = 100;
