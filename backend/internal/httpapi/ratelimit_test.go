package httpapi

import (
	"testing"
	"time"
)

func TestRateLimiterAllowsBurstThenRefills(t *testing.T) {
	limiter := newRateLimiter(2, time.Minute)
	defer limiter.close()

	now := time.Now()
	if ok, _ := limiter.allow("127.0.0.1", now); !ok {
		t.Fatal("first request should pass")
	}
	if ok, _ := limiter.allow("127.0.0.1", now); !ok {
		t.Fatal("second request should pass")
	}
	ok, wait := limiter.allow("127.0.0.1", now)
	if ok {
		t.Fatal("third request should be limited")
	}
	if wait <= 0 {
		t.Fatal("limited request should report a wait")
	}
	if ok, _ := limiter.allow("10.0.0.2", now); !ok {
		t.Fatal("a different key should have its own bucket")
	}
	if ok, _ := limiter.allow("127.0.0.1", now.Add(31*time.Second)); !ok {
		t.Fatal("bucket should refill over time")
	}
}

func TestRateLimiterCleanup(t *testing.T) {
	limiter := newRateLimiter(1, time.Minute)
	defer limiter.close()

	start := time.Now()
	limiter.allow("old", start)
	limiter.allow("fresh", start.Add(3*time.Minute))
	limiter.cleanup(start.Add(3 * time.Minute))

	limiter.mu.Lock()
	defer limiter.mu.Unlock()
	if _, ok := limiter.buckets["old"]; ok {
		t.Fatal("idle bucket should be removed")
	}
	if _, ok := limiter.buckets["fresh"]; !ok {
		t.Fatal("recent bucket should be kept")
	}
}

func TestRedactedPath(t *testing.T) {
	if got := redactedPath("/api/orders/abc123"); got != "/api/orders/[redacted]" {
		t.Fatalf("unexpected path: %s", got)
	}
	if got := redactedPath("/api/products/rose-clay-mask"); got != "/api/products/rose-clay-mask" {
		t.Fatalf("unexpected path: %s", got)
	}
}
