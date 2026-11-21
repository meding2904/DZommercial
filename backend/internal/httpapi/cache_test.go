package httpapi

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestReadCacheServesTheSecondRead(t *testing.T) {
	server := newCSRFTestServer(t)
	server.readCache = newReadCache(8)
	calls := 0
	handler := server.cacheRead(time.Minute)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.Header().Set("X-Test", "one")
		_, _ = w.Write([]byte(`{"ok":true}`))
	}))

	first := httptest.NewRecorder()
	handler.ServeHTTP(first, httptest.NewRequest(http.MethodGet, "/api/products", nil))
	second := httptest.NewRecorder()
	handler.ServeHTTP(second, httptest.NewRequest(http.MethodGet, "/api/products", nil))

	if calls != 1 {
		t.Fatalf("handler calls = %d, want 1", calls)
	}
	if second.Body.String() != first.Body.String() || second.Header().Get("X-Test") != "one" {
		t.Fatalf("cached response did not match: %q / %q", second.Body.String(), second.Header().Get("X-Test"))
	}
}

func TestReadCacheSeparatesQueryStrings(t *testing.T) {
	server := newCSRFTestServer(t)
	server.readCache = newReadCache(8)
	calls := 0
	handler := server.cacheRead(time.Minute)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		_, _ = w.Write([]byte(r.URL.RawQuery))
	}))

	handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products?category=face", nil))
	handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products?category=body", nil))

	if calls != 2 {
		t.Fatalf("handler calls = %d, want 2", calls)
	}
}

func TestReadCacheDoesNotStoreErrorsOrEmptyBodies(t *testing.T) {
	server := newCSRFTestServer(t)
	server.readCache = newReadCache(8)
	calls := 0
	handler := server.cacheRead(time.Minute)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = w.Write([]byte("no"))
	}))

	handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products", nil))
	handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products", nil))

	if calls != 2 {
		t.Fatalf("handler calls = %d, want 2", calls)
	}
	if server.readCache.size() != 0 {
		t.Fatalf("cache size = %d, want 0", server.readCache.size())
	}
}

func TestReadCacheExpiresAndWritesClearIt(t *testing.T) {
	server := newCSRFTestServer(t)
	server.readCache = newReadCache(8)
	calls := 0
	cached := server.cacheRead(time.Millisecond)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		_, _ = w.Write([]byte("body"))
	}))
	write := server.invalidateReadCacheOnWrite(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))

	cached.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products", nil))
	if server.readCache.size() != 1 {
		t.Fatalf("cache size = %d, want 1", server.readCache.size())
	}
	time.Sleep(5 * time.Millisecond)
	cached.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products", nil))
	if calls != 2 {
		t.Fatalf("expired entry was served: calls = %d", calls)
	}

	cached.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/api/products", nil))
	write.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodPost, "/api/admin/sales", nil))
	if server.readCache.size() != 0 {
		t.Fatalf("write did not clear the cache: size = %d", server.readCache.size())
	}
}
