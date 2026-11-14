package httpapi

import (
	"bytes"
	"encoding/base64"
	"image/png"
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
	if reason := store.verify(token, "AAAAA", "203.0.113.8", base.Add(time.Minute+time.Second)); reason != "expired" {
		t.Fatalf("verify expired = %q, want expired", reason)
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
