package api

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
)

func TestNormalizeAPIURLAndNetworkPolicy(t *testing.T) {
	tests := []struct {
		name     string
		raw      string
		allow    bool
		wantURL  string
		wantCode string
	}{
		{name: "loopback default", raw: "http://127.0.0.1:8080", wantURL: "http://127.0.0.1:8080"},
		{name: "loopback trailing slash", raw: "http://127.0.0.1:8080/", wantURL: "http://127.0.0.1:8080"},
		{name: "loopback ipv6", raw: "http://[::1]:8080", wantURL: "http://[::1]:8080"},
		{name: "localhost", raw: "http://localhost:8080", wantURL: "http://localhost:8080"},
		{name: "private http allowed", raw: "http://192.168.1.10:8080", allow: true, wantURL: "http://192.168.1.10:8080"},
		{name: "private ten network", raw: "http://10.20.30.40:8080", allow: true, wantURL: "http://10.20.30.40:8080"},
		{name: "private http needs consent", raw: "http://192.168.1.10:8080", wantCode: "network_consent_required"},
		{name: "local name http allowed", raw: "http://api.local:8080", allow: true, wantURL: "http://api.local:8080"},
		{name: "link local ipv6 http allowed", raw: "http://[fe80::1%25eth0]:8080", allow: true, wantURL: "http://[fe80::1%25eth0]:8080"},
		{name: "public https allowed", raw: "https://api.example.com", allow: true, wantURL: "https://api.example.com"},
		{name: "public https needs consent", raw: "https://api.example.com", wantCode: "network_consent_required"},
		{name: "public http rejected", raw: "http://api.example.com", allow: true, wantCode: "invalid_api_url"},
		{name: "public http rejected without consent", raw: "http://api.example.com", wantCode: "invalid_api_url"},
		{name: "credentials rejected", raw: "https://user:pass@api.example.com", allow: true, wantCode: "invalid_api_url"},
		{name: "path rejected", raw: "https://api.example.com/api", allow: true, wantCode: "invalid_api_url"},
		{name: "query rejected", raw: "https://api.example.com?x=1", allow: true, wantCode: "invalid_api_url"},
		{name: "fragment rejected", raw: "https://api.example.com#x", allow: true, wantCode: "invalid_api_url"},
		{name: "empty fragment rejected", raw: "https://api.example.com#", allow: true, wantCode: "invalid_api_url"},
		{name: "protocol relative rejected", raw: "//api.example.com", allow: true, wantCode: "invalid_api_url"},
		{name: "scheme trick rejected", raw: "http:////api.example.com", allow: true, wantCode: "invalid_api_url"},
		{name: "opaque scheme rejected", raw: "http:\\\\api.example.com", allow: true, wantCode: "invalid_api_url"},
		{name: "ftp rejected", raw: "ftp://api.example.com", allow: true, wantCode: "invalid_api_url"},
		{name: "empty rejected", raw: "  ", wantCode: "invalid_api_url"},
		{name: "backslash rejected", raw: "https://api.example.com\\@evil.com", allow: true, wantCode: "invalid_api_url"},
		{name: "public loopback name rejected", raw: "http://127.0.0.1.nip.io:8080", allow: true, wantCode: "invalid_api_url"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			normalized, err := normalizeAPIURL(tt.raw)
			if err == nil {
				err = validateServerAddress(normalized, tt.allow)
			}
			if tt.wantCode == "" {
				if err != nil {
					t.Fatalf("unexpected error: %v", err)
				}
				if normalized != tt.wantURL {
					t.Fatalf("normalized = %q, want %q", normalized, tt.wantURL)
				}
				return
			}
			if err == nil {
				t.Fatalf("expected %s, got success with %q", tt.wantCode, normalized)
			}
			var apiErr *Error
			if !errors.As(err, &apiErr) {
				t.Fatalf("error type = %T, want *Error", err)
			}
			if apiErr.Code != tt.wantCode {
				t.Fatalf("code = %q, want %q (message: %s)", apiErr.Code, tt.wantCode, apiErr.Message)
			}
			if apiErr.Message == "" {
				t.Fatal("validation error has no message")
			}
		})
	}
}

func TestUpdateSettingsRequiresNetworkConsent(t *testing.T) {
	c, _ := testClient(t, "")
	_, err := c.UpdateSettings("https://api.example.com", "", false)
	if err == nil {
		t.Fatal("expected consent error")
	}
	var apiErr *Error
	if !errors.As(err, &apiErr) || apiErr.Code != "network_consent_required" {
		t.Fatalf("error = %v, want network_consent_required", err)
	}
	if got := c.Info().APIURL; got != defaultAPIURL {
		t.Fatalf("settings changed after rejected update: %q", got)
	}

	_, err = c.UpdateSettings("http://api.example.com", "", true)
	if err == nil {
		t.Fatal("expected public http to be rejected")
	}
	if !errors.As(err, &apiErr) || apiErr.Code != "invalid_api_url" {
		t.Fatalf("error = %v, want invalid_api_url", err)
	}
}

func TestNormalizeOrigin(t *testing.T) {
	tests := map[string]string{
		"":                              "",
		"http://localhost:3000":         "http://localhost:3000",
		"http://localhost:3000/":        "http://localhost:3000",
		"https://admin.example.com/app": "https://admin.example.com",
		"https://admin.example.com:443": "https://admin.example.com",
	}
	for input, want := range tests {
		got, err := normalizeOrigin(input)
		if err != nil {
			t.Fatalf("normalizeOrigin(%q): %v", input, err)
		}
		if got != want {
			t.Fatalf("normalizeOrigin(%q) = %q, want %q", input, got, want)
		}
	}
	for _, input := range []string{"//admin.example.com", "ftp://admin.example.com", "https://user@admin.example.com"} {
		if _, err := normalizeOrigin(input); err == nil {
			t.Fatalf("normalizeOrigin(%q) succeeded", input)
		}
	}
}

func TestRequestRejectsUnsafePaths(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	paths := []string{
		"",
		"/api",
		"/api/../admin",
		"/api/%2e%2e/admin",
		"/api/%2E%2E/admin",
		"/api/..%2f..%2fetc",
		"/api/%2f%2fevil.com",
		"/api/%252e%252e/admin",
		"/api/x%5Cy",
		"/api/x\\y",
		"/api/x\x00y",
		"//evil.com/api/x",
		"http://evil.com/api/x",
		"/other/x",
		"/api/" + strings.Repeat("a", maxAPIPathLength),
	}
	for _, path := range paths {
		response := c.Request(http.MethodGet, path, nil)
		if response.OK {
			t.Fatalf("path %q was accepted", path)
		}
		if response.Error == nil || response.Error.Code != "invalid_path" {
			t.Fatalf("path %q code = %#v, want invalid_path", path, response.Error)
		}
	}
	if hits.Load() != 0 {
		t.Fatalf("unsafe paths reached the server %d times", hits.Load())
	}
}

func TestRequestAllowsQueryStrings(t *testing.T) {
	var gotQuery string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		writeJSON(t, w, http.StatusOK, map[string]any{"ok": true})
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request(http.MethodGet, "/api/ok?q=%2e%2e%2f&page=2", nil)
	if !response.OK {
		t.Fatalf("request failed: %#v", response.Error)
	}
	if want := "q=%2e%2e%2f&page=2"; gotQuery != want {
		t.Fatalf("query = %q, want %q", gotQuery, want)
	}
}

func TestRequestRejectsUnknownMethod(t *testing.T) {
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
	}))
	defer server.Close()

	c, _ := testClient(t, server.URL)
	response := c.Request("TRACE", "/api/ok", nil)
	if response.OK || response.Error == nil || response.Error.Code != "invalid_method" {
		t.Fatalf("response = %#v, want invalid_method", response)
	}
	if hits.Load() != 0 {
		t.Fatal("unknown method reached the server")
	}
}
