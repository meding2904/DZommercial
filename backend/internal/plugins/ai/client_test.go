package ai

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestClientCompletesChat(t *testing.T) {
	var gotAuth, gotModel string
	var gotTemperature float64
	var gotMaxTokens int

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/chat/completions" {
			t.Errorf("unexpected request %s %s", r.Method, r.URL.Path)
		}
		gotAuth = r.Header.Get("Authorization")
		var payload struct {
			Model       string  `json:"model"`
			Temperature float64 `json:"temperature"`
			MaxTokens   int     `json:"max_tokens"`
			Messages    []struct {
				Role    string `json:"role"`
				Content string `json:"content"`
			} `json:"messages"`
		}
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Errorf("decode request: %v", err)
		}
		gotModel = payload.Model
		gotTemperature = payload.Temperature
		gotMaxTokens = payload.MaxTokens

		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"  A clear answer.  "}}]}`)
	}))
	defer server.Close()

	client, err := newClient(server.URL, "sk-test-secret", "gpt-4o-mini", server.Client())
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	answer, err := client.complete(context.Background(), "system prompt", "user prompt")
	if err != nil {
		t.Fatalf("complete: %v", err)
	}
	if answer != "A clear answer." {
		t.Fatalf("answer = %q, want trimmed content", answer)
	}
	if gotAuth != "Bearer sk-test-secret" {
		t.Fatalf("authorization = %q", gotAuth)
	}
	if gotModel != "gpt-4o-mini" {
		t.Fatalf("model = %q", gotModel)
	}
	if gotTemperature != 0.3 || gotMaxTokens != 500 {
		t.Fatalf("temperature = %v, max_tokens = %d", gotTemperature, gotMaxTokens)
	}
}

func TestClientRejectsMissingConfigurationBeforeRequest(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	if _, err := newClient(server.URL, "", "gpt-4o-mini", server.Client()); !errors.Is(err, errNotConfigured) {
		t.Fatalf("missing key error = %v, want errNotConfigured", err)
	}
	if _, err := newClient(server.URL, "sk-key", "", server.Client()); !errors.Is(err, errNotConfigured) {
		t.Fatalf("missing model error = %v, want errNotConfigured", err)
	}

	settings := map[string]string{
		"enabled":  "false",
		"api_key":  "sk-key",
		"model":    "gpt-4o-mini",
		"language": "en",
	}
	if _, err := configFromSettings(settings); !errors.Is(err, errDisabled) {
		t.Fatalf("disabled error = %v, want errDisabled", err)
	}

	settings["enabled"] = "true"
	delete(settings, "api_key")
	if _, err := configFromSettings(settings); !errors.Is(err, errNotConfigured) {
		t.Fatalf("missing settings error = %v, want errNotConfigured", err)
	}

	settings["api_key"] = "sk-key"
	delete(settings, "model")
	if _, err := configFromSettings(settings); !errors.Is(err, errNotConfigured) {
		t.Fatalf("missing model error = %v, want errNotConfigured", err)
	}

	if calls != 0 {
		t.Fatalf("made %d HTTP calls before configuration was complete", calls)
	}
}

func TestClientMapsUpstreamFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = io.WriteString(w, `{"error":{"message":"provider secret detail"}}`)
	}))
	defer server.Close()

	client, err := newClient(server.URL, "sk-test-secret", "gpt-4o-mini", server.Client())
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	_, err = client.complete(context.Background(), "system", "user")
	var upstream *UpstreamError
	if !errors.As(err, &upstream) {
		t.Fatalf("error = %v, want *UpstreamError", err)
	}
	if upstream.StatusCode != http.StatusInternalServerError {
		t.Fatalf("status = %d, want 500", upstream.StatusCode)
	}
	if upstream.StatusClass() != "5xx" {
		t.Fatalf("status class = %q, want 5xx", upstream.StatusClass())
	}
	if strings.Contains(err.Error(), "provider secret detail") {
		t.Fatalf("error leaked the upstream body: %v", err)
	}
}

func TestClientRejectsOversizedResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"`)
		_, _ = io.WriteString(w, strings.Repeat("a", maxResponseBytes+1))
		_, _ = io.WriteString(w, `"}}]}`)
	}))
	defer server.Close()

	client, err := newClient(server.URL, "sk-test-secret", "gpt-4o-mini", server.Client())
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	_, err = client.complete(context.Background(), "system", "user")
	var upstream *UpstreamError
	if !errors.As(err, &upstream) {
		t.Fatalf("error = %v, want *UpstreamError", err)
	}
	if !strings.Contains(err.Error(), "too large") {
		t.Fatalf("error = %v, want an oversized response error", err)
	}
}

func TestClientRejectsLongAnswer(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"`)
		_, _ = io.WriteString(w, strings.Repeat("a", maxAnswerRunes+1))
		_, _ = io.WriteString(w, `"}}]}`)
	}))
	defer server.Close()

	client, err := newClient(server.URL, "sk-test-secret", "gpt-4o-mini", server.Client())
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	_, err = client.complete(context.Background(), "system", "user")
	var upstream *UpstreamError
	if !errors.As(err, &upstream) {
		t.Fatalf("error = %v, want *UpstreamError", err)
	}
	if !strings.Contains(err.Error(), "too long") {
		t.Fatalf("error = %v, want an answer length error", err)
	}
}
