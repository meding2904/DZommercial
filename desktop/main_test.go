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

func TestRendererCannotFetchDirectly(t *testing.T) {
	script, err := os.ReadFile("frontend/app.js")
	if err != nil {
		t.Fatal(err)
	}
	source := string(script)
	for _, forbidden := range []string{
		"window.azahar",
		"azahar-asset",
		"fetch(",
		"XMLHttpRequest",
		"new WebSocket",
	} {
		if strings.Contains(source, forbidden) {
			t.Errorf("app.js still contains %q", forbidden)
		}
	}
}

func TestWailsBridgeSurfaceIsPresent(t *testing.T) {
	script, err := os.ReadFile("frontend/app.js")
	if err != nil {
		t.Fatal(err)
	}
	source := string(script)
	for _, method := range []string{
		"Info",
		"Health",
		"Request",
		"Image",
		"UpdateSettings",
		"ClearSession",
		"FirewallGuide",
	} {
		if !strings.Contains(source, "'"+method+"'") {
			t.Errorf("app.js does not call the %s binding", method)
		}
	}
}

func TestRendererBlocksDirectNavigation(t *testing.T) {
	script, err := os.ReadFile("frontend/app.js")
	if err != nil {
		t.Fatal(err)
	}
	source := string(script)
	for _, want := range []string{
		"api.openExternal(href)",
		"event.preventDefault()",
		"window.addEventListener('drop'",
		"window.addEventListener('dragover'",
		"window.open = () =>",
	} {
		if !strings.Contains(source, want) {
			t.Errorf("app.js is missing navigation guard %q", want)
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
	script, err := os.ReadFile("frontend/app.js")
	if err != nil {
		t.Fatal(err)
	}
	source := string(script)
	for _, want := range []string{
		"const sameHost = Boolean(previous && next && previous.origin === next.origin)",
		"id=\"app-server-state\"",
		"id=\"app-resolved-origin\"",
		"function resetTransientState()",
	} {
		if !strings.Contains(source, want) {
			t.Errorf("app.js is missing medium-bug fix %q", want)
		}
	}
	helperStart := strings.Index(source, "function resetTransientState()")
	helperEnd := strings.Index(source[helperStart:], "function setView")
	helper := source[helperStart : helperStart+helperEnd]
	for _, want := range []string{
		"state.counterQuery = ''",
		"state.render += 1",
		"clearTimeout(state.orderTimer)",
	} {
		if !strings.Contains(helper, want) {
			t.Errorf("resetTransientState is missing %q", want)
		}
	}
	if got := strings.Count(source, "\tresetTransientState();\n"); got != 2 {
		t.Errorf("resetTransientState calls = %d, want 2", got)
	}
	start := strings.Index(source, "document.getElementById('app-test')")
	endMarker := "document.getElementById('app-clear-session')"
	if start >= 0 {
		rest := source[start:]
		if end := strings.Index(rest, endMarker); end >= 0 {
			if strings.Contains(rest[:end], "renderConnection();") {
				t.Error("app-test must not re-render the connection form")
			}
		}
	}

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
