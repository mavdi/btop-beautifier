//! Memory driver: shape RSS via page-touch + madvise(MADV_DONTNEED).

use crate::pattern::PatternState;
use crate::TICK_MS;
use nix::sys::mman::{madvise, MmapAdvise};
use std::num::NonZeroUsize;
use std::ptr::NonNull;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const PAGE_STEP: usize = 4096;

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
