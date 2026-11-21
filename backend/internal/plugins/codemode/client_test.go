package codemode

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

func TestClientPlansChat(t *testing.T) {
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
		if len(payload.Messages) != 2 || payload.Messages[0].Role != "system" ||
			payload.Messages[0].Content != "system prompt" || payload.Messages[1].Content != "user instruction" {
			t.Errorf("messages = %#v", payload.Messages)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"  {\"summary\":\"A change.\",\"actions\":[]}  "}}]}`)
	}))
	defer server.Close()

	client, err := newClient(server.URL, "sk-test-secret", "gpt-4o-mini", server.Client())
	if err != nil {
		t.Fatalf("new client: %v", err)
	}

	content, err := client.plan(context.Background(), "system prompt", "user instruction")
	if err != nil {
		t.Fatalf("plan: %v", err)
	}
	if content != `{"summary":"A change.","actions":[]}` {
		t.Fatalf("content = %q", content)
	}
	if gotAuth != "Bearer sk-test-secret" {
		t.Fatalf("authorization = %q", gotAuth)
	}
	if gotModel != "gpt-4o-mini" {
		t.Fatalf("model = %q", gotModel)
	}
	if gotTemperature != 0.2 || gotMaxTokens != 2000 {
		t.Fatalf("temperature = %v, max_tokens = %d", gotTemperature, gotMaxTokens)
	}
}

func TestClientRejectsRedirect(t *testing.T) {
	followed := 0
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		followed++
		_, _ = io.WriteString(w, `{"choices":[{"message":{"content":"{}"}}]}`)
	}))
	defer target.Close()

	redirect := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL+"/chat/completions", http.StatusFound)
	}))
	defer redirect.Close()

	client, err := newClient(redirect.URL, "sk-test-secret", "gpt-4o-mini", redirect.Client())
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	_, err = client.plan(context.Background(), "system", "user")
	var upstream *UpstreamError
	if !errors.As(err, &upstream) {
		t.Fatalf("error = %v, want *UpstreamError", err)
	}
	if followed != 0 {
		t.Fatalf("redirect was followed %d times", followed)
	}
	if upstream.StatusClass() != "3xx" {
		t.Fatalf("status class = %q", upstream.StatusClass())
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
	_, err = client.plan(context.Background(), "system", "user")
	var upstream *UpstreamError
	if !errors.As(err, &upstream) {
		t.Fatalf("error = %v, want *UpstreamError", err)
	}
	if !strings.Contains(err.Error(), "too large") {
		t.Fatalf("error = %v, want an oversized response error", err)
	}
}

func TestClientRejectsMissingConfigurationBeforeRequest(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	for _, test := range []struct {
		baseURL string
		apiKey  string
		model   string
	}{
		{server.URL, "", "gpt-4o-mini"},
		{server.URL, "sk-key", ""},
		{"", "sk-key", "gpt-4o-mini"},
		{"ftp://example.test", "sk-key", "gpt-4o-mini"},
	} {
		if _, err := newClient(test.baseURL, test.apiKey, test.model, server.Client()); !errors.Is(err, errNotConfigured) {
			t.Fatalf("configuration error = %v, want errNotConfigured", err)
		}
	}
	if calls != 0 {
		t.Fatalf("made %d HTTP calls before configuration was complete", calls)
	}
}
