package main

import (
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
)

func TestSecurityHeaders(t *testing.T) {
	handler := securityHeaders(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	}))
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, httptest.NewRequest(http.MethodGet, "/", nil))
	headers := recorder.Header()
	for name, want := range map[string]string{
		"Content-Security-Policy":      "frame-ancestors 'none'",
		"X-Content-Type-Options":       "nosniff",
		"X-Frame-Options":              "DENY",
		"Referrer-Policy":              "no-referrer",
		"Cross-Origin-Opener-Policy":   "same-origin",
		"Cross-Origin-Resource-Policy": "same-origin",
	} {
		if !strings.Contains(headers.Get(name), want) {
			t.Errorf("%s = %q, want %q", name, headers.Get(name), want)
		}
	}
}

func TestRendererHasStrictNetworkPolicy(t *testing.T) {
	html, err := os.ReadFile("frontend/index.html")
	if err != nil {
		t.Fatal(err)
	}
	page := string(html)
	for _, want := range []string{
		"connect-src 'none'",
		"script-src 'self'",
		"img-src 'self' data:",
		"font-src 'self'",
		"frame-src 'self'",
		"object-src 'none'",
		"base-uri 'none'",
		"form-action 'none'",
	} {
		if !strings.Contains(page, want) {
			t.Errorf("index.html CSP is missing %q", want)
		}
	}
}

func TestOpenExternalRejectsUnsafeURLs(t *testing.T) {
	app := &App{}
	for _, raw := range []string{
		"",
		"mailto:admin@example.com",
		"file:///etc/passwd",
		"javascript:alert(1)",
		"https://user:pass@example.com",
		"https://",
		"https://:443",
	} {
		if err := app.OpenExternal(raw); err == nil {
			t.Errorf("OpenExternal(%q) was accepted", raw)
		}
	}
}

func TestExternalLinksAreSchemeChecked(t *testing.T) {
	source, err := os.ReadFile("main.go")
	if err != nil {
		t.Fatal(err)
	}
	text := string(source)
	if !strings.Contains(text, `parsed.Scheme != "http"`) ||
		!strings.Contains(text, `parsed.Scheme != "https"`) ||
		!strings.Contains(text, "parsed.User != nil") {
		t.Fatal("OpenExternal must reject non-http(s) and credential-bearing URLs")
	}
}

func TestWindowHardeningIsConfigured(t *testing.T) {
	source, err := os.ReadFile("main.go")
	if err != nil {
		t.Fatal(err)
	}
	text := string(source)
	for _, want := range []string{
		"DisableWebViewDrop: true",
		"frame-ancestors 'none'",
		"X-Content-Type-Options",
	} {
		if !strings.Contains(text, want) {
			t.Errorf("main.go is missing %q", want)
		}
	}
	if strings.Contains(text, "func (a *App) WindowBounds()") {
		t.Error("WindowBounds must not be bound to the renderer")
	}
}

func TestMediumBugFixesArePresent(t *testing.T) {
	client, err := os.ReadFile("internal/api/client.go")
	if err != nil {
		t.Fatal(err)
	}
	clientSource := string(client)
	if !strings.Contains(clientSource, "if err := c.saveSettings(next); err != nil {") {
		t.Error("UpdateSettings must save before mutating live state")
	}
	if strings.Contains(clientSource, "c.settings = previous") {
		t.Error("UpdateSettings still contains the rollback path that dropped session state")
	}
}

func TestLowRiskFrontendFixesArePresent(t *testing.T) {
	styles, err := os.ReadFile("frontend/styles.css")
	if err != nil {
		t.Fatal(err)
	}
	css := string(styles)
	for _, forbidden := range []string{"'Source Serif 4 Variable'", "@media print"} {
		if strings.Contains(css, forbidden) {
			t.Errorf("styles.css still contains %q", forbidden)
		}
	}
	for _, want := range []string{".receipt-total", ".receipt-actions", ".receipt-served"} {
		if !strings.Contains(css, want) {
			t.Errorf("styles.css lost on-screen rule %q", want)
		}
	}

	for _, name := range []string{
		"fraunces-latin-ext-wght-normal.woff2",
		"fraunces-latin-wght-normal.woff2",
		"inter-latin-ext-wght-normal.woff2",
		"inter-latin-wght-normal.woff2",
		"jetbrains-mono-latin-ext-wght-normal.woff2",
		"schibsted-grotesk-latin-ext-wght-normal.woff2",
		"source-serif-4-latin-ext-wght-normal.woff2",
		"source-serif-4-latin-wght-normal.woff2",
	} {
		path := "frontend/fonts/" + name
		if _, err := os.Stat(path); err == nil {
			t.Errorf("unused font %s still exists", name)
		} else if !os.IsNotExist(err) {
			t.Errorf("os.Stat(%s): %v", name, err)
		}
	}
	for _, name := range []string{
		"schibsted-grotesk-latin-wght-normal.woff2",
		"jetbrains-mono-latin-wght-normal.woff2",
	} {
		if _, err := os.Stat("frontend/fonts/" + name); err != nil {
			t.Errorf("kept font %s is missing: %v", name, err)
		}
	}
}
