package httpapi

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"azahar/internal/config"
)

func newCSRFTestServer(t *testing.T) *Server {
	t.Helper()
	return &Server{cfg: config.Config{
		AppEnv:       "development",
		AppOrigin:    "http://localhost:3000",
		CookieSecure: false,
	}}
}

func TestCSRFTokenIsIssuedAsCookieAndBody(t *testing.T) {
	server := newCSRFTestServer(t)
	recorder := httptest.NewRecorder()
	server.handleCSRF(recorder, httptest.NewRequest(http.MethodGet, "/api/csrf", nil))

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusOK)
	}
	cookies := recorder.Result().Cookies()
	if len(cookies) != 1 || cookies[0].Name != csrfCookieName {
		t.Fatalf("cookies = %#v, want one %s cookie", cookies, csrfCookieName)
	}
	cookie := cookies[0]
	if !validCSRFToken(cookie.Value) {
		t.Fatalf("cookie value %q is not a 64 character hex token", cookie.Value)
	}
	if cookie.HttpOnly {
		t.Fatal("the CSRF cookie must stay readable by a page")
	}
	if cookie.SameSite != http.SameSiteLaxMode {
		t.Fatalf("SameSite = %v, want Lax", cookie.SameSite)
	}
	if cookie.Path != "/" {
		t.Fatalf("Path = %q, want /", cookie.Path)
	}

	var body struct {
		Token     string `json:"csrf_token"`
		ExpiresIn int    `json:"expires_in"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if body.Token != cookie.Value {
		t.Fatalf("body token %q does not match cookie %q", body.Token, cookie.Value)
	}
	if body.ExpiresIn != int(csrfCookieAge.Seconds()) {
		t.Fatalf("expires_in = %d, want %d", body.ExpiresIn, int(csrfCookieAge.Seconds()))
	}
}

func TestCSRFEndpointKeepsAValidCookie(t *testing.T) {
	server := newCSRFTestServer(t)
	existing := strings.Repeat("ab", csrfTokenBytes)
	request := httptest.NewRequest(http.MethodGet, "/api/csrf", nil)
	request.AddCookie(&http.Cookie{Name: csrfCookieName, Value: existing})
	recorder := httptest.NewRecorder()

	server.handleCSRF(recorder, request)

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusOK)
	}
	var body struct {
		Token string `json:"csrf_token"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if body.Token != existing {
		t.Fatalf("token = %q, want the existing cookie %q", body.Token, existing)
	}
}

func TestCSRFEndpointReplacesAMalformedCookie(t *testing.T) {
	server := newCSRFTestServer(t)
	request := httptest.NewRequest(http.MethodGet, "/api/csrf", nil)
	request.AddCookie(&http.Cookie{Name: csrfCookieName, Value: "not-a-token"})
	recorder := httptest.NewRecorder()

	server.handleCSRF(recorder, request)

	var body struct {
		Token string `json:"csrf_token"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if body.Token == "not-a-token" || !validCSRFToken(body.Token) {
		t.Fatalf("token = %q, want a fresh valid token", body.Token)
	}
}

func TestHealthReportsTheService(t *testing.T) {
	server := newCSRFTestServer(t)
	recorder := httptest.NewRecorder()

	server.handleHealth(recorder, httptest.NewRequest(http.MethodGet, "/api/health", nil))

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusOK)
	}
	var body struct {
		Status  string `json:"status"`
		Service string `json:"service"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if body.Status != "ok" || body.Service != "azahar-api" {
		t.Fatalf("health body = %#v", body)
	}
}
