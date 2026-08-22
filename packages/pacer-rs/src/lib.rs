//! Sync retryer, with semantics aligned to TS `@tanstack/pacer` `AsyncRetryer`
//! (packages/core/ml/voxcpm/voxcpm.ts uses maxAttempts=3 / exponential /
//! baseWait=1000 / maxWait=2000 / jitter=0.3).
//!
//! Deliberately blocking rather than async: all current call sites (gradio_client,
//! stage_tts) run in a blocking environment such as spawn_blocking; an async variant
//! will be added later once async callers actually exist.
//!
//! ```
//! let r = pacer_rs::Retryer::new().max_attempts(2).base_wait_ms(1).max_wait_ms(2).jitter(0.0);
//! let mut n = 0;
//! let v = r.execute(|| { n += 1; if n < 2 { Err("x") } else { Ok(n) } }, |_, _| {}).unwrap();
//! assert_eq!(v, 2);
//! ```

use std::cell::Cell;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Retry executor: calls the operation repeatedly until success or `max_attempts` is exhausted.
///
/// Backoff strategy: exponential backoff before the k-th retry
/// `min(base_wait * 2^(k-1), max_wait)`, then multiplied by jitter — uniform random within
/// `[1-jitter, 1+jitter]` (jitter=0 disables it).
#[derive(Debug, Clone)]
pub struct Retryer {
    max_attempts: u32,
    base_wait_ms: u64,
    max_wait_ms: u64,
    jitter: f64,
}

impl Default for Retryer {
    /// Defaults match the parameters used by TS voxcpm.ts.
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_wait_ms: 1000,
            max_wait_ms: 2000,
            jitter: 0.3,
        }
    }
}

impl Retryer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total attempt count (including the first); minimum is 1.
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n.max(1);
        self
    }

    /// Delay before the first retry.
    pub fn base_wait_ms(mut self, ms: u64) -> Self {
        self.base_wait_ms = ms;
        self
    }

    /// Upper bound for backoff delay (applied before jitter).
    pub fn max_wait_ms(mut self, ms: u64) -> Self {
        self.max_wait_ms = ms;
        self
    }

    /// Jitter ratio (0..=1): wait time randomly multiplied by `[1-j, 1+j]`.
    pub fn jitter(mut self, ratio: f64) -> Self {
        self.jitter = ratio.clamp(0.0, 1.0);
        self
    }

    /// Run the operation; on failure, back off and retry per the strategy until success
    /// or attempts are exhausted (returns the error from the last attempt).
    ///
    /// `on_retry(failed_attempt, err)` fires after each failed attempt that still has retries remaining;
    /// `failed_attempt` starts at 1.
    pub fn execute<F, T, E>(&self, mut op: F, mut on_retry: impl FnMut(u32, &E)) -> Result<T, E>
    where
        F: FnMut() -> Result<T, E>,
    {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            match op() {
                Ok(v) => return Ok(v),
                Err(e) if attempt >= self.max_attempts => return Err(e),
                Err(e) => {
                    on_retry(attempt, &e);
                    thread::sleep(self.wait_after(attempt));
                }
            }
        }
    }

    /// Wait duration before the next retry after the given attempt fails.
    fn wait_after(&self, failed_attempt: u32) -> Duration {
        let exp_shift = (failed_attempt - 1).min(32);
        let exp = self
            .base_wait_ms
            .saturating_mul(1u64 << exp_shift)
            .min(self.max_wait_ms);
        let j = self.jitter.clamp(0.0, 1.0);
        let factor = 1.0 - j + 2.0 * j * rng_f64();
        Duration::from_millis(((exp as f64) * factor).max(0.0) as u64)
    }
}

/// Zero-dependency PRNG (xorshift64*), thread-local state, seeded from system nanosecond timestamp on first use.
fn rng_f64() -> f64 {
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0) };
    }
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                .max(1);
        }
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (r >> 11) as f64 / (1u64 << 53) as f64
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn succeeds_first_try_without_retry() {
        let r = Retryer::new().max_attempts(3);
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let v = r
            .execute(
                || {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, &str>(42)
                },
                |_, _| panic!("should not retry"),
            )
            .unwrap();
        assert_eq!(v, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn succeeds_on_later_attempt_with_backoff() {
        let r = Retryer::new()
            .max_attempts(3)
            .base_wait_ms(1)
            .max_wait_ms(2)
            .jitter(0.0);
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let retries = Arc::new(AtomicU32::new(0));
        let rc = retries.clone();
        let v = r
            .execute(
                || {
                    let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                    if n < 3 { Err("fail") } else { Ok(n) }
                },
                |attempt, _| {
                    rc.fetch_add(1, Ordering::SeqCst);
                    assert!(attempt == 1 || attempt == 2);
                },
            )
            .unwrap();
        assert_eq!(v, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(retries.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn exhausts_attempts_and_returns_last_error() {
        let r = Retryer::new().max_attempts(3).base_wait_ms(1).jitter(0.0);
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let err = r
            .execute(
                || {
                    let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                    Err::<(), _>(format!("err{n}"))
                },
                |_, _| {},
            )
            .unwrap_err();
        assert_eq!(err, "err3");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn max_attempts_one_never_retries() {
        let r = Retryer::new().max_attempts(1).base_wait_ms(1);
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let res: Result<(), _> = r.execute(|| { c.fetch_add(1, Ordering::SeqCst); Err("x") }, |_, _| {});
        assert!(res.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn exponential_backoff_capped_by_max_wait() {
        let r = Retryer::new()
            .base_wait_ms(1000)
            .max_wait_ms(2000)
            .jitter(0.0);
        // After failing attempt 1 → 1000ms; after failing attempt 2 → capped at 2000ms; attempt 10 → still 2000ms
        assert_eq!(r.wait_after(1), Duration::from_millis(1000));
        assert_eq!(r.wait_after(2), Duration::from_millis(2000));
        assert_eq!(r.wait_after(10), Duration::from_millis(2000));

        let r2 = Retryer::new().base_wait_ms(500).max_wait_ms(10_000).jitter(0.0);
        assert_eq!(r2.wait_after(3), Duration::from_millis(2000)); // 500*4
    }

    #[test]
    fn jitter_stays_within_bounds() {
        let r = Retryer::new().base_wait_ms(1000).max_wait_ms(2000).jitter(0.3);
        for k in 1..=5u32 {
            let d = r.wait_after(k).as_millis() as f64;
            // attempt 1 → base value 1000; from attempt 2 onwards already capped by max_wait at 2000
            let expected = if k == 1 { 1000.0 } else { 2000.0 };
            assert!(
                d >= expected * 0.7 && d <= expected * 1.3,
                "wait {d} out of [700, 2600] for attempt {k}"
            );
        }
    }

    #[test]
    fn jitter_clamped_and_attempts_floor() {
        // Out-of-range jitter gets clamped; too-small max_attempts gets raised to 1
        let r = Retryer::new().max_attempts(0).jitter(5.0);
        assert_eq!(r.max_attempts, 1);
        assert_eq!(r.jitter, 1.0);
    }

    #[test]
    fn rng_in_unit_range() {
        for _ in 0..1000 {
            let v = super::rng_f64();
            assert!((0.0..1.0).contains(&v));
        }
    }
}
