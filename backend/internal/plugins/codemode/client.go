package codemode

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"
)

const (
	maxResponseBytes = 2 << 20
	maxPlanRunes     = 30000
)

var (
	errDisabled      = errors.New("codemode plugin is disabled")
	errNotConfigured = errors.New("codemode plugin is not configured")
)

// UpstreamError marks a failure of the configured endpoint. It carries no
// response body, so the handler can map it to a plain 502 without leaking
// provider output to the browser.
type UpstreamError struct {
	StatusCode int
	Reason     string
}

func (e *UpstreamError) Error() string {
	if e.StatusCode > 0 {
		return fmt.Sprintf("codemode upstream returned HTTP %d", e.StatusCode)
	}
	return "codemode upstream request failed: " + e.Reason
}

func (e *UpstreamError) StatusClass() string {
	if e.StatusCode > 0 {
		return fmt.Sprintf("%dxx", e.StatusCode/100)
	}
	return "network"
}

func (e *UpstreamError) Detail() string {
	return e.Reason
}

type settingsConfig struct {
	apiKey string
	model  string
}

func configFromSettings(settings map[string]string) (settingsConfig, error) {
	if !enabledFromSettings(settings) {
		return settingsConfig{}, errDisabled
	}
	config := settingsConfig{
		apiKey: strings.TrimSpace(settings["api_key"]),
		model:  strings.TrimSpace(settings["model"]),
	}
	if config.model == "" {
		config.model = defaultModel
	}
	if config.apiKey == "" {
		return settingsConfig{}, errNotConfigured
	}
	if !modelPattern.MatchString(config.model) {
		return settingsConfig{}, errNotConfigured
	}
	return config, nil
}

func enabledFromSettings(settings map[string]string) bool {
	switch strings.TrimSpace(settings["enabled"]) {
	case "", "true":
		return true
	default:
		return false
	}
}

type client struct {
	baseURL string
	apiKey  string
	model   string
	http    *http.Client
}

func newClient(baseURL, apiKey, model string, httpClient *http.Client) (*client, error) {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	apiKey = strings.TrimSpace(apiKey)
	model = strings.TrimSpace(model)
	if baseURL == "" || apiKey == "" || model == "" {
		return nil, errNotConfigured
	}
	parsed, err := url.Parse(baseURL)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" {
		return nil, errNotConfigured
	}
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 25 * time.Second}
	} else {
		copied := *httpClient
		httpClient = &copied
	}
	// A provider redirect must not turn the server into a request forwarder.
	httpClient.CheckRedirect = func(*http.Request, []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &client{baseURL: baseURL, apiKey: apiKey, model: model, http: httpClient}, nil
}

type chatMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type chatCompletionRequest struct {
	Model       string        `json:"model"`
	Messages    []chatMessage `json:"messages"`
	Temperature float64       `json:"temperature"`
	MaxTokens   int           `json:"max_tokens"`
}

type chatCompletionResponse struct {
	Choices []struct {
		Message struct {
			Content json.RawMessage `json:"content"`
		} `json:"message"`
	} `json:"choices"`
}

func (c *client) plan(ctx context.Context, system, user string) (string, error) {
	payload, err := json.Marshal(chatCompletionRequest{
		Model: c.model,
		Messages: []chatMessage{
			{Role: "system", Content: system},
			{Role: "user", Content: user},
		},
		Temperature: 0.2,
		MaxTokens:   2000,
	})
	if err != nil {
		return "", &UpstreamError{Reason: "the request could not be encoded"}
	}

	request, err := http.NewRequestWithContext(ctx, http.MethodPost,
		c.baseURL+"/chat/completions", bytes.NewReader(payload))
	if err != nil {
		return "", &UpstreamError{Reason: "the request could not be built"}
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+c.apiKey)

	response, err := c.http.Do(request)
	if err != nil {
		// The key travels in a header, so transport errors never contain it.
		return "", &UpstreamError{Reason: err.Error()}
	}
	defer response.Body.Close()

	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", &UpstreamError{StatusCode: response.StatusCode}
	}

	body, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil {
		return "", &UpstreamError{Reason: "the response could not be read"}
	}
	if len(body) > maxResponseBytes {
		return "", &UpstreamError{Reason: "the response was too large"}
	}

	var decoded chatCompletionResponse
	if err := json.Unmarshal(body, &decoded); err != nil {
		return "", &UpstreamError{Reason: "the response was not valid JSON"}
	}
	if len(decoded.Choices) == 0 {
		return "", &UpstreamError{Reason: "the response had no choices"}
	}

	raw := decoded.Choices[0].Message.Content
	if len(raw) == 0 || string(raw) == "null" {
		return "", &UpstreamError{Reason: "the response had no content"}
	}

	var content string
	if err := json.Unmarshal(raw, &content); err != nil {
		return "", &UpstreamError{Reason: "the provider returned structured content instead of text"}
	}
	content = strings.TrimSpace(content)
	if content == "" {
		return "", &UpstreamError{Reason: "the provider returned an empty plan"}
	}
	if utf8.RuneCountInString(content) > maxPlanRunes {
		return "", &UpstreamError{Reason: "the plan was too long"}
	}
	return content, nil
}
