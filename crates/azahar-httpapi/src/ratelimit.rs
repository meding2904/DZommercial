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
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    limit: f64,
    window: Duration,
    last_cleanup: Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            limit: f64::from(limit),
            window,
            last_cleanup: Mutex::new(Instant::now()),
        }
    }

    /// Returns whether the request is allowed and, if not, how long the caller
    /// should wait. The minimum wait is one second, matching Retry-After.
    pub fn allow(&self, key: &str, now: Instant) -> (bool, Duration) {
        if self.limit <= 0.0 || !self.limit.is_finite() {
            return (false, Duration::from_secs(1));
        }
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Go runs a 10-minute cleanup goroutine. Doing it inline on the first
        // request after ten minutes keeps the same retention without a task.
        let mut last_cleanup = self
            .last_cleanup
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if now.saturating_duration_since(*last_cleanup) >= Duration::from_secs(600) {
            let retention = self.window * 2;
            buckets.retain(|_, bucket| now.saturating_duration_since(bucket.last) <= retention);
            *last_cleanup = now;
        }
        drop(last_cleanup);
        let rate = self.limit / self.window.as_secs_f64();

        if !buckets.contains_key(key)
            && buckets.len() >= MAX_BUCKETS
            && let Some(oldest) = buckets
                .iter()
                .min_by_key(|(_, bucket)| bucket.last)
                .map(|(key, _)| key.clone())
        {
            buckets.remove(&oldest);
        }

        let bucket = buckets.entry(key.to_string()).or_insert(TokenBucket {
            tokens: self.limit,
            last: now,
        });

        let elapsed = now.saturating_duration_since(bucket.last).as_secs_f64();
        if elapsed > 0.0 {
            bucket.tokens = self.limit.min(bucket.tokens + elapsed * rate);
            bucket.last = now;
        }
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            return (true, Duration::ZERO);
        }

        let wait = Duration::from_secs_f64((1.0 - bucket.tokens) / rate);
        (false, wait.max(Duration::from_secs(1)))
    }

    /// Drop buckets that have been idle for more than two windows.
    pub fn cleanup(&self, now: Instant) {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let retention = self.window * 2;
        buckets.retain(|_, bucket| now.saturating_duration_since(bucket.last) <= retention);
    }

    /// Go's limiter stops a cleanup goroutine; this port cleans inline, so
    /// shutdown has nothing to join.
    pub fn close(&self) {}

    pub fn size(&self) -> usize {
        self.buckets
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .len()
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
    fn evicts_the_oldest_bucket_at_the_cap() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let start = Instant::now();
        for index in 0..MAX_BUCKETS {
            limiter.allow(
                &format!("ip-{index}"),
                start + Duration::from_millis(index as u64),
            );
        }
        assert_eq!(limiter.size(), MAX_BUCKETS);
        limiter.allow("new", start + Duration::from_secs(1));
        assert_eq!(limiter.size(), MAX_BUCKETS);
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
