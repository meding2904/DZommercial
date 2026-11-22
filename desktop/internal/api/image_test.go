package api

import (
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
)

func TestImageLoadsAndCaches(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		w.Header().Set("Content-Type", "image/png")
		_, _ = w.Write([]byte{0x89, 0x50, 0x4e, 0x47})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Image("/api/images/good.png")
	if !response.OK {
		t.Fatalf("image failed: %#v", response.Error)
	}
	data, ok := response.Data.(string)
	if !ok || !strings.HasPrefix(data, "data:image/png;base64,") {
		t.Fatalf("image data = %#v", response.Data)
	}
	if response := c.Image("/api/images/good.png"); !response.OK {
		t.Fatalf("cached image failed: %#v", response.Error)
	}
	if hits.Load() != 1 {
		t.Fatalf("image was fetched %d times, want 1", hits.Load())
	}
}

func TestImageAllowedTypes(t *testing.T) {
	types := map[string]string{
		"/api/images/a.png":  "image/png",
		"/api/images/a.jpg":  "image/jpeg",
		"/api/images/a.webp": "image/webp",
		"/api/images/a.gif":  "image/gif",
		"/api/images/a.avif": "image/avif",
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", types[r.URL.Path]+"; charset=binary")
		_, _ = w.Write([]byte{1, 2, 3})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	for path, mediaType := range types {
		response := c.Image(path)
		if !response.OK {
			t.Fatalf("image %s failed: %#v", path, response.Error)
		}
		data, _ := response.Data.(string)
		if !strings.HasPrefix(data, "data:"+mediaType+";base64,") {
			t.Fatalf("image %s returned %q", path, data)
		}
	}
}

func TestImageRejectsInvalidRequests(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		switch r.URL.Path {
		case "/api/images/html":
			w.Header().Set("Content-Type", "text/html")
			_, _ = w.Write([]byte("<html>"))
		case "/api/images/svg":
			w.Header().Set("Content-Type", "image/svg+xml")
			_, _ = w.Write([]byte("<svg/>"))
		case "/api/images/missing":
			writeProblem(t, w, http.StatusNotFound, "not_found", "Not found.")
		default:
			w.Header().Set("Content-Type", "image/png")
			_, _ = w.Write([]byte{1})
		}
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	tests := []struct {
		path string
		code string
	}{
		{path: "/images/good.png", code: "invalid_image"},
		{path: "/api/other/image.png", code: "invalid_image"},
		{path: "/api/images/../secret", code: "invalid_path"},
		{path: "/api/images/%2e%2e/secret", code: "invalid_path"},
		{path: "/api/images/x\\y", code: "invalid_path"},
		{path: "/api/images/html", code: "image_failed"},
		{path: "/api/images/svg", code: "image_failed"},
		{path: "/api/images/missing", code: "image_failed"},
	}
	for _, tt := range tests {
		response := c.Image(tt.path)
		if response.OK || response.Error == nil || response.Error.Code != tt.code {
			t.Fatalf("image %q = %#v, want %s", tt.path, response, tt.code)
		}
	}
}

func TestImageSizeLimit(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "image/png")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(make([]byte, maxImageBytes+1))
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Image("/api/images/huge")
	if response.OK || response.Error == nil || response.Error.Code != "image_failed" {
		t.Fatalf("response = %#v, want image_failed", response)
	}
}

func TestImageCacheIsBounded(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		w.Header().Set("Content-Type", "image/png")
		_, _ = w.Write([]byte{1})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	for i := 0; i < imageCacheSize+1; i++ {
		path := fmt.Sprintf("/api/images/%d", i)
		if response := c.Image(path); !response.OK {
			t.Fatalf("image %d failed: %#v", i, response.Error)
		}
	}

	c.mu.RLock()
	size := len(c.imageCache)
	_, firstStillCached := c.imageCache["/api/images/0"]
	c.mu.RUnlock()
	if size != imageCacheSize {
		t.Fatalf("cache size = %d, want %d", size, imageCacheSize)
	}
	if firstStillCached {
		t.Fatal("oldest image was not evicted")
	}

	if response := c.Image("/api/images/0"); !response.OK {
		t.Fatalf("image reload failed: %#v", response.Error)
	}
	if hits.Load() != int32(imageCacheSize+2) {
		t.Fatalf("server hits = %d, want %d", hits.Load(), imageCacheSize+2)
	}
}

func TestImageCacheByteLimitEvicts(t *testing.T) {
	c, _ := testClient(t, "")
	c.imageCacheByteLimit = 100
	c.mu.RLock()
	generation := c.generation
	c.mu.RUnlock()

	first := "data:image/png;base64," + strings.Repeat("A", 60)
	second := "data:image/png;base64," + strings.Repeat("B", 60)
	c.storeImage("/api/images/one", first, generation)
	c.storeImage("/api/images/two", second, generation)

	c.mu.RLock()
	defer c.mu.RUnlock()
	if len(c.imageCache) != 1 {
		t.Fatalf("cache size = %d, want 1", len(c.imageCache))
	}
	if _, ok := c.imageCache["/api/images/one"]; ok {
		t.Fatal("oldest image was not evicted for the byte cap")
	}
	if _, ok := c.imageCache["/api/images/two"]; !ok {
		t.Fatal("new image was not cached")
	}
	if c.imageCacheBytes != int64(len(second)) {
		t.Fatalf("cache bytes = %d, want %d", c.imageCacheBytes, len(second))
	}
}

func TestStaleImageGenerationIsNotCached(t *testing.T) {
	c, _ := testClient(t, "")
	c.mu.RLock()
	generation := c.generation
	c.mu.RUnlock()

	if err := c.ClearSession(); err != nil {
		t.Fatalf("clear session: %v", err)
	}
	c.storeImage("/api/images/stale", "data:image/png;base64,AAAA", generation)

	c.mu.RLock()
	defer c.mu.RUnlock()
	if _, ok := c.imageCache["/api/images/stale"]; ok {
		t.Fatal("image from the old generation entered the cache")
	}
}
