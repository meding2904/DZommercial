package api

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestHealthResolvesOrigin(t *testing.T) {
	var mu sync.Mutex
	healthOrigin := ""
	requestOrigin := ""
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		if r.URL.Path == "/api/health" {
			healthOrigin = r.Header.Get("Origin")
		} else {
			requestOrigin = r.Header.Get("Origin")
		}
		mu.Unlock()
		if r.URL.Path == "/api/health" {
			writeJSON(t, w, http.StatusOK, map[string]any{
				"status":      "ok",
				"env":         "test",
				"origin":      "http://localhost:3000",
				"server_time": "2026-11-21T10:46:28Z",
			})
			return
		}
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	health := c.Health()
	if !health.OK || health.Status != http.StatusOK {
		t.Fatalf("health = %#v", health)
	}
	if health.Env != "test" || health.Origin != "http://localhost:3000" ||
		health.ServerTime != "2026-11-21T10:46:28Z" || health.Message != "" {
		t.Fatalf("health fields = %#v", health)
	}
	if c.Info().ResolvedOrigin != "http://localhost:3000" {
		t.Fatalf("resolved origin = %q", c.Info().ResolvedOrigin)
	}

	if response := c.Request(http.MethodGet, "/api/check", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	mu.Lock()
	defer mu.Unlock()
	if healthOrigin != "" {
		t.Fatalf("health request sent Origin %q", healthOrigin)
	}
	if requestOrigin != "http://localhost:3000" {
		t.Fatalf("request Origin = %q, want resolved origin", requestOrigin)
	}
}

func TestHealthBadStatus(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		writeProblem(t, w, http.StatusServiceUnavailable, "unavailable", "Try later.")
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	health := c.Health()
	if health.OK {
		t.Fatal("health reported ok for a 503")
	}
	if health.Status != http.StatusServiceUnavailable || health.Message == "" {
		t.Fatalf("health = %#v", health)
	}
	if c.Info().ResolvedOrigin != "" {
		t.Fatalf("resolved origin changed to %q", c.Info().ResolvedOrigin)
	}
}

func TestHealthNetworkFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	serverURL := server.URL
	server.Close()

	c, _ := testClient(t, serverURL)
	health := c.Health()
	if health.OK || health.Status != 0 || health.Message == "" {
		t.Fatalf("health = %#v, want a network failure with a message", health)
	}
}

func TestHealthTimeout(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		<-r.Context().Done()
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	c.timeout = 25 * time.Millisecond
	health := c.Health()
	if health.OK || health.Status != 0 || !strings.Contains(health.Message, "in time") {
		t.Fatalf("health = %#v, want a timeout message", health)
	}
}

func TestUnsafeRequestResolvesOriginFromHealth(t *testing.T) {
	var mu sync.Mutex
	origins := map[string]string{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		origins[r.URL.Path] = r.Header.Get("Origin")
		mu.Unlock()
		switch r.URL.Path {
		case "/api/health":
			writeJSON(t, w, http.StatusOK, map[string]any{
				"env":    "test",
				"origin": "http://reported.example",
			})
		case "/api/csrf":
			http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: "csrf-token", Path: "/"})
			writeJSON(t, w, http.StatusOK, map[string]any{"csrf_token": "csrf-token"})
		case "/api/action":
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		}
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/action", nil)
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}

	mu.Lock()
	defer mu.Unlock()
	if origin := origins["/api/health"]; origin != "" {
		t.Fatalf("health sent Origin %q", origin)
	}
	for _, path := range []string{"/api/csrf", "/api/action"} {
		if origin := origins[path]; origin != "http://reported.example" {
			t.Fatalf("%s sent Origin %q, want the health origin", path, origin)
		}
	}
}

func TestUnsafeRequestFallsBackToLocalOriginForLoopback(t *testing.T) {
	var mu sync.Mutex
	origins := map[string]string{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		origins[r.URL.Path] = r.Header.Get("Origin")
		mu.Unlock()
		switch r.URL.Path {
		case "/api/health":
			writeProblem(t, w, http.StatusServiceUnavailable, "unavailable", "Try later.")
		case "/api/csrf":
			http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: "csrf-token", Path: "/"})
			writeJSON(t, w, http.StatusOK, map[string]any{"csrf_token": "csrf-token"})
		case "/api/action":
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		}
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/action", nil)
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	mu.Lock()
	defer mu.Unlock()
	for _, path := range []string{"/api/csrf", "/api/action"} {
		if origin := origins[path]; origin != fallbackOrigin {
			t.Fatalf("%s sent Origin %q, want %q", path, origin, fallbackOrigin)
		}
	}
}

func TestUnsafeRequestFailsClosedForRemoteWithoutOrigin(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	if err := os.MkdirAll(profile, 0o700); err != nil {
		t.Fatal(err)
	}
	writeSettingsFile(t, profile, Settings{
		APIURL:             "https://remote.example",
		NetworkAllowed:     true,
		NetworkAllowedHost: "remote.example",
	})
	t.Setenv("AZAHAR_API_URL", "")
	t.Setenv("AZAHAR_ORIGIN", "")
	c, err := newClientWithSecrets("2.0.0-test", profile, &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	var hits atomic.Int32
	var requestedPath atomic.Value
	c.httpClient = &http.Client{Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
		hits.Add(1)
		requestedPath.Store(r.URL.Path)
		return nil, errors.New("unreachable")
	})}

	response := c.Request(http.MethodPost, "/api/action", nil)
	if response.OK || response.Error == nil || response.Error.Code != "origin_unavailable" {
		t.Fatalf("response = %#v, want origin_unavailable", response)
	}
	if !strings.Contains(response.Error.Message, "Connection") {
		t.Fatalf("message = %q, want Connection guidance", response.Error.Message)
	}
	if hits.Load() != 1 || requestedPath.Load() != "/api/health" {
		t.Fatalf("requests = %d to %v, want only the health probe", hits.Load(), requestedPath.Load())
	}
}

func TestHealthUsesConfiguredOriginForDisplay(t *testing.T) {
	var mu sync.Mutex
	gotOrigin := ""
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		gotOrigin = r.Header.Get("Origin")
		mu.Unlock()
		writeJSON(t, w, http.StatusOK, map[string]any{"origin": "http://server.example"})
	}))
	defer server.Close()

	t.Setenv("AZAHAR_API_URL", server.URL)
	t.Setenv("AZAHAR_ORIGIN", "http://admin.example")
	c, err := newClientWithSecrets("2.0.0-test", t.TempDir(), &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	health := c.Health()
	if !health.OK || health.Origin != "http://admin.example" {
		t.Fatalf("health = %#v", health)
	}
	mu.Lock()
	defer mu.Unlock()
	if gotOrigin != "" {
		t.Fatalf("health request sent Origin %q", gotOrigin)
	}
}

func TestUnsafeRequestUsesConfiguredOriginWithoutHealth(t *testing.T) {
	var healthHits atomic.Int32
	var actionOrigin atomic.Value
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/health":
			healthHits.Add(1)
			writeJSON(t, w, http.StatusOK, map[string]any{"origin": "http://health.example"})
		case "/api/action":
			actionOrigin.Store(r.Header.Get("Origin"))
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		default:
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		}
	}))
	defer server.Close()

	t.Setenv("AZAHAR_API_URL", server.URL)
	t.Setenv("AZAHAR_ORIGIN", "http://admin.example")
	c, err := newClientWithSecrets("2.0.0-test", t.TempDir(), &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	seedCSRF(c, "csrf-token")
	if response := c.Request(http.MethodPost, "/api/action", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if healthHits.Load() != 0 {
		t.Fatalf("configured origin triggered %d health requests", healthHits.Load())
	}
	if actionOrigin.Load() != "http://admin.example" {
		t.Fatalf("action Origin = %v", actionOrigin.Load())
	}
}
