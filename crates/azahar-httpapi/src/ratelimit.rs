//! Token-bucket rate limiting, ported from `backend/internal/httpapi/ratelimit.go`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_BUCKETS: usize = 10_000;

#[derive(Debug, Clone, Copy)]
struct TokenBucket {
    tokens: f64,
    last: Instant,
}

#[derive(Debug)]
struct Inner {
    buckets: HashMap<String, TokenBucket>,
    last_cleanup: Instant,
}

#[derive(Debug)]
pub struct RateLimiter {
    inner: Mutex<Inner>,
    limit: f64,
    window: Duration,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner {
                // The cap is 10k; starting at a realistic working set avoids
                // repeated rehashing while a burst fills the map.
                buckets: HashMap::with_capacity(MAX_BUCKETS / 8),
                last_cleanup: Instant::now(),
            }),
            limit: f64::from(limit),
            window,
        }
    }

    /// Returns whether the request is allowed and, if not, how long the caller
    /// should wait. The minimum wait is one second, matching Retry-After.
    pub fn allow(&self, key: &str, now: Instant) -> (bool, Duration) {
        if self.limit <= 0.0 || !self.limit.is_finite() {
            return (false, Duration::from_secs(1));
        }
        let rate = self.limit / self.window.as_secs_f64();
        let mut inner = self.lock();

        // Go runs a 10-minute cleanup goroutine. Doing it inline on the first
        // request after ten minutes keeps the same retention without a task.
        if now.saturating_duration_since(inner.last_cleanup) >= Duration::from_secs(600) {
            let retention = self.window * 2;
            inner
                .buckets
                .retain(|_, bucket| now.saturating_duration_since(bucket.last) <= retention);
            inner.last_cleanup = now;
        }

        if let Some(bucket) = inner.buckets.get_mut(key) {
            return consume(bucket, self.limit, rate, now);
        }

        if inner.buckets.len() >= MAX_BUCKETS {
            // Evict in batches instead of rescanning all 10k buckets for every
            // new address once the cap is reached. The victims are still the
            // least recently seen buckets; only the moment of removal moves.
            evict_oldest_batch(&mut inner.buckets);
        }
        let bucket = inner.buckets.entry(key.to_owned()).or_insert(TokenBucket {
            tokens: self.limit,
            last: now,
        });
        consume(bucket, self.limit, rate, now)
    }

    /// Drop buckets that have been idle for more than two windows.
    pub fn cleanup(&self, now: Instant) {
        let mut inner = self.lock();
        let retention = self.window * 2;
        inner
            .buckets
            .retain(|_, bucket| now.saturating_duration_since(bucket.last) <= retention);
        inner.last_cleanup = now;
    }

    /// Go's limiter stops a cleanup goroutine; this port cleans inline, so
    /// shutdown has nothing to join.
    pub fn close(&self) {}

    pub fn size(&self) -> usize {
        self.lock().buckets.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn consume(bucket: &mut TokenBucket, limit: f64, rate: f64, now: Instant) -> (bool, Duration) {
    let elapsed = now.saturating_duration_since(bucket.last).as_secs_f64();
    if elapsed > 0.0 {
        bucket.tokens = limit.min(bucket.tokens + elapsed * rate);
        bucket.last = now;
    }
    if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        return (true, Duration::ZERO);
    }

    let wait = Duration::from_secs_f64((1.0 - bucket.tokens) / rate);
    (false, wait.max(Duration::from_secs(1)))
}

/// Removes the least recently seen eighth of the map in one pass.
///
/// The map is O(n) to scan, so doing that for every insertion at the cap would
/// make a churn of unique addresses quadratic. Selecting one batch per `n/8`
/// insertions keeps the amortised cost O(1) per insertion. Only the selected
/// keys are cloned; the scan itself keeps borrowed keys.
fn evict_oldest_batch(buckets: &mut HashMap<String, TokenBucket>) {
    let victims = {
        let batch = (buckets.len() / 8).max(1);
        let mut candidates: Vec<(Instant, &str)> = buckets
            .iter()
            .map(|(key, bucket)| (bucket.last, key.as_str()))
            .collect();
        if candidates.len() > batch {
            candidates.select_nth_unstable_by_key(batch - 1, |(last, _)| *last);
            candidates.truncate(batch);
        }
        candidates
            .iter()
            .map(|(_, key)| (*key).to_owned())
            .collect::<Vec<String>>()
    };
    for key in victims {
        buckets.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_the_budget_then_refills() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        let start = Instant::now();
        for _ in 0..3 {
            assert!(limiter.allow("ip", start).0);
        }
        let (allowed, wait) = limiter.allow("ip", start);
        assert!(!allowed);
        assert_eq!(wait, Duration::from_secs(20));

        assert!(limiter.allow("ip", start + Duration::from_secs(20)).0);
        assert!(limiter.allow("other", start).0);
    }

    #[test]
    fn evicts_old_buckets_in_batches_at_the_cap() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let start = Instant::now();
        for index in 0..MAX_BUCKETS {
            limiter.allow(
                &format!("ip-{index}"),
                start + Duration::from_millis(index as u64),
            );
        }
        assert_eq!(limiter.size(), MAX_BUCKETS);

        let batch = MAX_BUCKETS / 8;
        assert!(limiter.allow("new", start + Duration::from_secs(1)).0);
        assert!(limiter.size() <= MAX_BUCKETS);
        assert!(limiter.size() > MAX_BUCKETS - batch);

        // A second full batch keeps the map bounded and does not scan it once
        // per insertion.
        for index in 0..batch {
            limiter.allow(
                &format!("next-{index}"),
                start + Duration::from_secs(2 + index as u64),
            );
        }
        assert!(limiter.size() <= MAX_BUCKETS);
    }

    #[test]
    fn zero_limit_denies_without_panicking() {
        let limiter = RateLimiter::new(0, Duration::from_secs(60));
        assert_eq!(
            limiter.allow("ip", Instant::now()),
            (false, Duration::from_secs(1))
        );
    }

    #[test]
    fn cleanup_keeps_recent_buckets() {
        let limiter = RateLimiter::new(5, Duration::from_secs(10));
        let start = Instant::now();
        limiter.allow("old", start);
        limiter.allow("new", start + Duration::from_secs(30));
        limiter.cleanup(start + Duration::from_secs(30));
        assert_eq!(limiter.size(), 1);
    }
}
