package httpapi

import (
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"image/png"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestCaptchaStoreVerifiesOnce(t *testing.T) {
	store := newCaptchaStore(time.Minute)
	defer store.close()

	base := time.Now()
	token, image, ttl, err := store.issue("203.0.113.7", base)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if token == "" || image == "" || ttl <= 0 {
		t.Fatalf("issue returned empty values: %q %q %v", token, image, ttl)
	}

	store.mu.Lock()
	code := store.entries[token].code
	store.mu.Unlock()

	if reason := store.verify(token, code, "203.0.113.7", base.Add(time.Second)); reason != "fast" {
		t.Fatalf("verify too fast = %q, want fast", reason)
	}
	if reason := store.verify(token, strings.ToLower(code), "203.0.113.7", base.Add(3*time.Second)); reason != "" {
		t.Fatalf("verify correct = %q, want success", reason)
	}
	if reason := store.verify(token, code, "203.0.113.7", base.Add(3*time.Second)); reason != "expired" {
		t.Fatalf("verify reused = %q, want expired", reason)
	}
}

func TestCaptchaStoreRejectsWrongCodeAndExpiry(t *testing.T) {
	store := newCaptchaStore(time.Minute)
	defer store.close()

	base := time.Now()
	token, _, _, err := store.issue("203.0.113.8", base)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if reason := store.verify(token, "AAAAA", "203.0.113.8", base.Add(3*time.Second)); reason != "wrong" {
		t.Fatalf("verify wrong = %q, want wrong", reason)
	}
	if reason := store.verify(token, "AAAAA", "203.0.113.8", base.Add(4*time.Second)); reason != "expired" {
		t.Fatalf("verify after wrong = %q, want expired", reason)
	}

	expiredToken, _, _, err := store.issue("203.0.113.8", base)
	if err != nil {
		t.Fatalf("issue expired: %v", err)
	}
	if reason := store.verify(expiredToken, "AAAAA", "203.0.113.8", base.Add(time.Minute+time.Second)); reason != "expired" {
		t.Fatalf("verify expired = %q, want expired", reason)
	}
}

func TestCaptchaStoreEvictsOldestEntryWhenFull(t *testing.T) {
	store := newCaptchaStore(time.Minute)
	defer store.close()

	base := time.Now()
	store.mu.Lock()
	for index := 0; index < captchaMaxEntries; index++ {
		store.entries[fmt.Sprintf("full-%d", index)] = captchaEntry{
			code:    "AAAAA",
			ip:      "203.0.113.9",
			issued:  base.Add(time.Duration(index) * time.Second),
			expires: base.Add(time.Hour),
		}
	}
	store.mu.Unlock()

	token, _, _, err := store.issue("203.0.113.9", base)
	if err != nil {
		t.Fatalf("issue into full store: %v", err)
	}

	store.mu.Lock()
	defer store.mu.Unlock()
	if len(store.entries) != captchaMaxEntries {
		t.Fatalf("store size = %d, want %d", len(store.entries), captchaMaxEntries)
	}
	if _, ok := store.entries["full-0"]; ok {
		t.Fatal("oldest entry was not evicted")
	}
	if _, ok := store.entries[token]; !ok {
		t.Fatal("new entry was not stored")
	}
}

func TestCheckoutChallengeRequiresBrowserHeader(t *testing.T) {
	server := &Server{captcha: newCaptchaStore(time.Minute)}
	defer server.captcha.close()

	request := httptest.NewRequest(http.MethodGet, "/api/checkout/challenge", nil)
	recorder := httptest.NewRecorder()
	server.handleCheckoutChallenge(recorder, request)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("missing header status = %d, want %d", recorder.Code, http.StatusForbidden)
	}

	request.Header.Set("X-Requested-With", "other")
	recorder = httptest.NewRecorder()
	server.handleCheckoutChallenge(recorder, request)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("wrong header status = %d, want %d", recorder.Code, http.StatusForbidden)
	}

	request.Header.Set("X-Requested-With", "azahar-web")
	recorder = httptest.NewRecorder()
	server.handleCheckoutChallenge(recorder, request)
	if recorder.Code != http.StatusOK {
		t.Fatalf("valid header status = %d, want %d", recorder.Code, http.StatusOK)
	}
}

func TestChallengeHeaderGateRunsBeforeLimiter(t *testing.T) {
	server := &Server{}
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})
	handler := server.requireChallengeHeader(next)

	request := httptest.NewRequest(http.MethodGet, "/api/checkout/challenge", nil)
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("missing header status = %d, want %d", recorder.Code, http.StatusForbidden)
	}

	request.Header.Set("X-Requested-With", "azahar-web")
	recorder = httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusNoContent {
		t.Fatalf("valid header status = %d, want %d", recorder.Code, http.StatusNoContent)
	}
}

func TestTurnstileVerification(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("method = %s, want POST", r.Method)
		}
		if got := r.FormValue("secret"); got != "turnstile-secret" {
			t.Errorf("secret = %q", got)
		}
		if got := r.FormValue("remoteip"); got != "203.0.113.9" {
			t.Errorf("remoteip = %q", got)
		}
		w.Header().Set("Content-Type", "application/json")
		if r.FormValue("response") == "good-token" {
			_, _ = w.Write([]byte(`{"success":true,"error-codes":[]}`))
			return
		}
		_, _ = w.Write([]byte(`{"success":false,"error-codes":["invalid-input-response"]}`))
	}))
	defer server.Close()

	originalURL, originalClient := turnstileVerifyURL, turnstileHTTPClient
	turnstileVerifyURL = server.URL
	turnstileHTTPClient = server.Client()
	defer func() {
		turnstileVerifyURL = originalURL
		turnstileHTTPClient = originalClient
	}()

	ok, err := verifyTurnstile(context.Background(), "turnstile-secret", "good-token", "203.0.113.9")
	if err != nil || !ok {
		t.Fatalf("verify good token = %v, %v", ok, err)
	}
	ok, err = verifyTurnstile(context.Background(), "turnstile-secret", "bad-token", "203.0.113.9")
	if err != nil {
		t.Fatalf("verify bad token: %v", err)
	}
	if ok {
		t.Fatal("bad token was accepted")
	}
}

func TestCaptchaImageIsAPNGOfTheExpectedSize(t *testing.T) {
	data := captchaImage("ABCDE")
	if !strings.HasPrefix(data, "data:image/png;base64,") {
		t.Fatalf("image prefix = %q", data[:min(len(data), 30)])
	}
	raw, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(data, "data:image/png;base64,"))
	if err != nil {
		t.Fatalf("decode image: %v", err)
	}
	decoded, err := png.Decode(bytes.NewReader(raw))
	if err != nil {
		t.Fatalf("decode png: %v", err)
	}
	if decoded.Bounds().Dx() != 220 || decoded.Bounds().Dy() != 64 {
		t.Fatalf("image size = %dx%d, want 220x64", decoded.Bounds().Dx(), decoded.Bounds().Dy())
	}
}
