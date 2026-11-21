package httpapi

import (
	"bytes"
	"net/http"
	"sync"
	"time"
)

const (
	readCacheMaxEntries = 512
	readCacheMaxBody    = 256 << 10
	readCacheMaxBytes   = 32 << 20
)

type cacheEntry struct {
	status  int
	headers http.Header
	body    []byte
	expires time.Time
}

// readCache is a small in-process cache for public catalogue reads and the
// shop-wide admin reads. The TTLs are deliberately short: checkout always
// re-reads stock and price, so a stale product card cannot oversell. The cache
// exists to absorb traffic spikes and the demo libsql server's limited
// connection creation window, not to become a second source of truth.
type readCache struct {
	mu      sync.Mutex
	entries map[string]cacheEntry
	max     int
	bytes   int
}

func newReadCache(max int) *readCache {
	if max <= 0 {
		max = readCacheMaxEntries
	}
	return &readCache{entries: make(map[string]cacheEntry), max: max}
}

func (c *readCache) get(key string, now time.Time) (cacheEntry, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	entry, ok := c.entries[key]
	if !ok {
		return cacheEntry{}, false
	}
	if !entry.expires.After(now) {
		c.bytes -= len(entry.body)
		delete(c.entries, key)
		return cacheEntry{}, false
	}
	return entry, true
}

func (c *readCache) put(key string, entry cacheEntry) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if previous, ok := c.entries[key]; ok {
		c.bytes -= len(previous.body)
		delete(c.entries, key)
	}
	size := len(entry.body)
	for len(c.entries) > 0 && (len(c.entries) >= c.max || c.bytes+size > readCacheMaxBytes) {
		var oldestKey string
		var oldest time.Time
		for candidateKey, candidate := range c.entries {
			if oldest.IsZero() || candidate.expires.Before(oldest) {
				oldestKey, oldest = candidateKey, candidate.expires
			}
		}
		if oldestKey == "" {
			break
		}
		c.bytes -= len(c.entries[oldestKey].body)
		delete(c.entries, oldestKey)
	}
	c.entries[key] = entry
	c.bytes += size
}

func (c *readCache) clear() {
	c.mu.Lock()
	defer c.mu.Unlock()
	clear(c.entries)
	c.bytes = 0
}

func (c *readCache) size() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return len(c.entries)
}

type cacheRecorder struct {
	http.ResponseWriter
	status   int
	wrote    bool
	overflow bool
	body     bytes.Buffer
}

func (w *cacheRecorder) WriteHeader(status int) {
	if w.wrote {
		return
	}
	w.status = status
	w.wrote = true
	w.ResponseWriter.WriteHeader(status)
}

func (w *cacheRecorder) Write(body []byte) (int, error) {
	if !w.wrote {
		w.WriteHeader(http.StatusOK)
	}
	if !w.overflow && w.body.Len()+len(body) <= readCacheMaxBody {
		_, _ = w.body.Write(body)
	} else {
		w.overflow = true
	}
	return w.ResponseWriter.Write(body)
}

func (s *Server) cacheRead(ttl time.Duration) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.Method != http.MethodGet {
				next.ServeHTTP(w, r)
				return
			}
			key := r.URL.RequestURI()
			if entry, ok := s.readCache.get(key, time.Now()); ok {
				for name, values := range entry.headers {
					if name == "X-Request-Id" || name == "Date" {
						continue
					}
					w.Header()[name] = append([]string(nil), values...)
				}
				w.Header().Set("X-Request-Id", requestID(r))
				w.WriteHeader(entry.status)
				_, _ = w.Write(entry.body)
				return
			}

			recorder := &cacheRecorder{ResponseWriter: w, status: http.StatusOK}
			next.ServeHTTP(recorder, r)
			if recorder.status != http.StatusOK || recorder.overflow {
				return
			}
			headers := w.Header().Clone()
			headers.Del("X-Request-Id")
			headers.Del("Date")
			s.readCache.put(key, cacheEntry{
				status:  recorder.status,
				headers: headers,
				body:    append([]byte(nil), recorder.body.Bytes()...),
				expires: time.Now().Add(ttl),
			})
		})
	}
}

// invalidateReadCacheOnWrite runs before a mutation. The next read fills from
// the database, so a sale or a stock edit is visible immediately.
func (s *Server) invalidateReadCacheOnWrite(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.Method {
		case http.MethodPost, http.MethodPut, http.MethodPatch, http.MethodDelete:
			s.readCache.clear()
		}
		next.ServeHTTP(w, r)
	})
}
