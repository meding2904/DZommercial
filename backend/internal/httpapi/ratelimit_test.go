package httpapi

import (
	"strconv"
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

func TestRateLimiterBucketMapStaysBounded(t *testing.T) {
	limiter := newRateLimiter(1, time.Minute)
	defer limiter.close()

	start := time.Now()
	limiter.mu.Lock()
	for i := 0; i < maxBuckets; i++ {
		key := "client-" + strconv.Itoa(i)
		limiter.buckets[key] = &tokenBucket{
			tokens: 1,
			last:   start.Add(time.Duration(i) * time.Second),
		}
	}
	limiter.mu.Unlock()

	now := start.Add(time.Duration(maxBuckets) * time.Second)
	if ok, _ := limiter.allow("new-client", now); !ok {
		t.Fatal("a new key should still work when the bucket map is full")
	}

	limiter.mu.Lock()
	defer limiter.mu.Unlock()
	if len(limiter.buckets) > maxBuckets {
		t.Fatalf("bucket map grew past its cap: got %d, want at most %d", len(limiter.buckets), maxBuckets)
	}
	if _, ok := limiter.buckets["client-0"]; ok {
		t.Fatal("the oldest bucket should be evicted")
	}
	if _, ok := limiter.buckets["new-client"]; !ok {
		t.Fatal("the new key should have a bucket")
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
