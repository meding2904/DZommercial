package api

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
)

func TestRequestHeadersAndDefaultBody(t *testing.T) {
	var got http.Header
	var gotBody []byte
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got = r.Header.Clone()
		gotBody, _ = io.ReadAll(r.Body)
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	seedCSRF(c, "csrf-token")
	response := c.Request(http.MethodPost, "/api/echo", nil)
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if string(gotBody) != "{}" {
		t.Fatalf("body = %q, want {}", gotBody)
	}
	if got.Get("Accept") != "application/json" {
		t.Fatalf("Accept = %q", got.Get("Accept"))
	}
	if got.Get("X-Requested-With") != "azahar-web" {
		t.Fatalf("X-Requested-With = %q", got.Get("X-Requested-With"))
	}
	if got.Get("Content-Type") != "application/json" {
		t.Fatalf("Content-Type = %q", got.Get("Content-Type"))
	}
	if got.Get("X-CSRF-Token") != "csrf-token" {
		t.Fatalf("X-CSRF-Token = %q", got.Get("X-CSRF-Token"))
	}
	if !strings.Contains(got.Get("Cookie"), "azahar_csrf=csrf-token") {
		t.Fatalf("Cookie = %q", got.Get("Cookie"))
	}
	if ua := got.Get("User-Agent"); len(ua) < len("AzaharDesktop/2.0.0-test (") ||
		ua[:len("AzaharDesktop/2.0.0-test (")] != "AzaharDesktop/2.0.0-test (" {
		t.Fatalf("User-Agent = %q", ua)
	}
}

func TestRequestUsesConfiguredOrigin(t *testing.T) {
	var gotOrigin string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotOrigin = r.Header.Get("Origin")
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	t.Setenv("AZAHAR_API_URL", server.URL)
	t.Setenv("AZAHAR_ORIGIN", "http://admin.example")
	c, err := newClientWithSecrets("2.0.0-test", t.TempDir(), &memorySecrets{})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	if response := c.Request(http.MethodGet, "/api/ok", nil); !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if gotOrigin != "http://admin.example" {
		t.Fatalf("Origin = %q", gotOrigin)
	}
}

func TestRequestSendsJSONBody(t *testing.T) {
	var got map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewDecoder(r.Body).Decode(&got)
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	seedCSRF(c, "csrf-token")
	response := c.Request(http.MethodPost, "/api/echo", map[string]any{"name": "Amina"})
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if got["name"] != "Amina" {
		t.Fatalf("decoded body = %#v", got)
	}
}

func TestRequestDoesNotFollowRedirects(t *testing.T) {
	var targetHits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/start":
			http.Redirect(w, r, "/api/target", http.StatusFound)
		case "/api/target":
			targetHits.Add(1)
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		}
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodGet, "/api/start", nil)
	if response.OK {
		t.Fatal("redirect was followed")
	}
	if response.Error == nil || response.Error.Status != http.StatusFound {
		t.Fatalf("error = %#v, want status 302", response.Error)
	}
	if targetHits.Load() != 0 {
		t.Fatalf("redirect target was requested %d times", targetHits.Load())
	}
}

func TestRequestCsrfRetry(t *testing.T) {
	const firstToken = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	const secondToken = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

	var mu sync.Mutex
	csrfCalls := 0
	postCalls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/csrf":
			mu.Lock()
			csrfCalls++
			token := firstToken
			if csrfCalls > 1 {
				token = secondToken
			}
			mu.Unlock()
			http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: token, Path: "/"})
			writeJSON(t, w, http.StatusOK, map[string]any{"csrf_token": token})
		case "/api/login":
			mu.Lock()
			postCalls++
			call := postCalls
			mu.Unlock()
			if call == 1 && r.Header.Get("X-CSRF-Token") == firstToken {
				writeProblem(t, w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
				return
			}
			if r.Header.Get("X-CSRF-Token") != secondToken {
				writeProblem(t, w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
				return
			}
			writeJSON(t, w, http.StatusOK, map[string]any{"user": map[string]any{"name": "Amina"}})
		}
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/login", map[string]any{"email": "a@example.com"})
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	mu.Lock()
	defer mu.Unlock()
	if postCalls != 2 {
		t.Fatalf("POST calls = %d, want 2", postCalls)
	}
	if csrfCalls != 2 {
		t.Fatalf("CSRF calls = %d, want 2", csrfCalls)
	}
}

func TestRendererCannotRequestCSRFToken(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		writeJSON(t, w, http.StatusOK, map[string]any{"csrf_token": "secret-token"})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	paths := []string{
		"/api/csrf",
		"/api/csrf?x=1",
		"/api/csrf/",
		"/api/csrf;foo",
		"/api/csrf%3bfoo",
		"/api/csrf.",
		"/api/csrf%20",
		"/api/CSRF",
		"/api//csrf",
		"/api/%63srf",
	}
	for _, path := range paths {
		response := c.Request(http.MethodGet, path, nil)
		if response.OK || response.Error == nil || response.Error.Code != "invalid_path" {
			t.Fatalf("request %q = %#v, want invalid_path", path, response)
		}
	}
	if hits.Load() != 0 {
		t.Fatalf("CSRF endpoint was reached %d times from the renderer bridge", hits.Load())
	}
}

func TestRequestCsrfTokenFromBodyOnly(t *testing.T) {
	token := strings.Repeat("c", 64)
	var mu sync.Mutex
	gotHeader := ""
	gotCookie := ""
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/csrf" {
			writeJSON(t, w, http.StatusOK, map[string]any{"csrf_token": token})
			return
		}
		cookie, _ := r.Cookie(csrfCookieName)
		mu.Lock()
		gotHeader = r.Header.Get("X-CSRF-Token")
		if cookie != nil {
			gotCookie = cookie.Value
		}
		mu.Unlock()
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/action", nil)
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	mu.Lock()
	defer mu.Unlock()
	if gotHeader != token || gotCookie != token {
		t.Fatalf("header = %q, cookie = %q, want %q", gotHeader, gotCookie, token)
	}
}

func TestRequestCsrfUnavailable(t *testing.T) {
	var postHits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/health":
			writeJSON(t, w, http.StatusOK, map[string]any{})
		case "/api/csrf":
			writeJSON(t, w, http.StatusOK, map[string]any{})
		default:
			postHits.Add(1)
			writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
		}
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/action", nil)
	if response.OK || response.Error == nil || response.Error.Code != "csrf_unavailable" {
		t.Fatalf("response = %#v, want csrf_unavailable", response)
	}
	if postHits.Load() != 0 {
		t.Fatal("request was sent without a CSRF token")
	}
}

func TestRequestCsrfRetriesOnlyOnce(t *testing.T) {
	const firstToken = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	const secondToken = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

	var mu sync.Mutex
	csrfCalls := 0
	postCalls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/health" {
			writeJSON(t, w, http.StatusOK, map[string]any{})
			return
		}
		if r.URL.Path == "/api/csrf" {
			mu.Lock()
			csrfCalls++
			token := firstToken
			if csrfCalls > 1 {
				token = secondToken
			}
			mu.Unlock()
			http.SetCookie(w, &http.Cookie{Name: csrfCookieName, Value: token, Path: "/"})
			writeJSON(t, w, http.StatusOK, map[string]any{"csrf_token": token})
			return
		}
		mu.Lock()
		postCalls++
		mu.Unlock()
		writeProblem(t, w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/login", nil)
	if response.OK || response.Error == nil || response.Error.Code != "csrf_failed" {
		t.Fatalf("response = %#v, want csrf_failed", response)
	}
	mu.Lock()
	defer mu.Unlock()
	if postCalls != 2 {
		t.Fatalf("POST calls = %d, want 2", postCalls)
	}
	if csrfCalls != 2 {
		t.Fatalf("CSRF calls = %d, want 2", csrfCalls)
	}
}

func TestRequestBodyLimit(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodPost, "/api/echo", map[string]any{
		"value": strings.Repeat("a", maxRequestBodyBytes),
	})
	if response.OK || response.Error == nil || response.Error.Code != "payload_too_large" {
		t.Fatalf("response = %#v, want payload_too_large", response)
	}
	if hits.Load() != 0 {
		t.Fatal("oversized request reached the server")
	}
}

func TestResponseBodyLimit(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(make([]byte, maxResponseBytes+1))
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodGet, "/api/huge", nil)
	if response.OK || response.Error == nil || response.Error.Code != "response_too_large" {
		t.Fatalf("response = %#v, want response_too_large", response)
	}
}
