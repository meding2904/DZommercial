package httpapi

import (
	"math"
	"net/http"
	"strconv"
	"sync"
	"time"
)

type tokenBucket struct {
	tokens float64
	last   time.Time
}

type rateLimiter struct {
	mu      sync.Mutex
	buckets map[string]*tokenBucket
	limit   float64
	window  time.Duration
	stop    chan struct{}
	once    sync.Once
}

func newRateLimiter(limit int, window time.Duration) *rateLimiter {
	limiter := &rateLimiter{
		buckets: make(map[string]*tokenBucket),
		limit:   float64(limit),
		window:  window,
		stop:    make(chan struct{}),
	}
	go limiter.cleanupLoop()
	return limiter
}

func (l *rateLimiter) allow(key string, now time.Time) (bool, time.Duration) {
	l.mu.Lock()
	defer l.mu.Unlock()

	rate := l.limit / l.window.Seconds()
	bucket, ok := l.buckets[key]
	if !ok {
		bucket = &tokenBucket{tokens: l.limit, last: now}
		l.buckets[key] = bucket
	}

	elapsed := now.Sub(bucket.last).Seconds()
	if elapsed > 0 {
		bucket.tokens = math.Min(l.limit, bucket.tokens+elapsed*rate)
		bucket.last = now
	}
	if bucket.tokens >= 1 {
		bucket.tokens--
		return true, 0
	}
	wait := time.Duration(((1 - bucket.tokens) / rate) * float64(time.Second))
	if wait < time.Second {
		wait = time.Second
	}
	return false, wait
}

func (l *rateLimiter) cleanupLoop() {
	ticker := time.NewTicker(10 * time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-l.stop:
			return
		case now := <-ticker.C:
			l.cleanup(now)
		}
	}
}

func (l *rateLimiter) cleanup(now time.Time) {
	l.mu.Lock()
	defer l.mu.Unlock()
	for key, bucket := range l.buckets {
		if now.Sub(bucket.last) > 2*l.window {
			delete(l.buckets, key)
		}
	}
}

func (l *rateLimiter) close() {
	l.once.Do(func() { close(l.stop) })
}

func (s *Server) rateLimit(limiter *rateLimiter) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			allowed, wait := limiter.allow(clientIP(r, s.cfg.TrustProxy), time.Now())
			if !allowed {
				seconds := int(math.Ceil(wait.Seconds()))
				if seconds < 1 {
					seconds = 1
				}
				w.Header().Set("Retry-After", strconv.Itoa(seconds))
				writeError(w, http.StatusTooManyRequests, "rate_limited", "Too many requests. Try again later.")
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}

func (s *Server) adminMutationLimit(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet || r.Method == http.MethodHead {
			next.ServeHTTP(w, r)
			return
		}
		allowed, wait := s.adminLimiter.allow(clientIP(r, s.cfg.TrustProxy), time.Now())
		if !allowed {
			seconds := int(math.Ceil(wait.Seconds()))
			if seconds < 1 {
				seconds = 1
			}
			w.Header().Set("Retry-After", strconv.Itoa(seconds))
			writeError(w, http.StatusTooManyRequests, "rate_limited", "Too many requests. Try again later.")
			return
		}
		next.ServeHTTP(w, r)
	})
}
