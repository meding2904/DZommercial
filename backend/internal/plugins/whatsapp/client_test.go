package whatsapp

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestCloudClientSendsText(t *testing.T) {
	var gotAuth, gotPath, gotTo, gotBody string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		gotPath = r.URL.Path
		var payload struct {
			MessagingProduct string `json:"messaging_product"`
			RecipientType    string `json:"recipient_type"`
			To               string `json:"to"`
			Type             string `json:"type"`
			Text             struct {
				PreviewURL bool   `json:"preview_url"`
				Body       string `json:"body"`
			} `json:"text"`
		}
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Errorf("decode payload: %v", err)
		}
		gotTo = payload.To
		gotBody = payload.Text.Body
		if payload.MessagingProduct != "whatsapp" || payload.RecipientType != "individual" || payload.Type != "text" {
			t.Errorf("unexpected payload: %#v", payload)
		}
		if payload.Text.PreviewURL {
			t.Error("preview_url must be false")
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[{"id":"wamid.text"}]}`))
	}))
	defer server.Close()

	client := &cloudClient{
		baseURL:       server.URL,
		version:       "v23.0",
		phoneNumberID: "123456789",
		accessToken:   "EAAG-secret-token",
		http:          server.Client(),
	}
	id, err := client.send(context.Background(), queuedMessage{
		Phone: "+213550778899", Kind: "text", Body: "Your order is confirmed.",
	})
	if err != nil {
		t.Fatalf("send: %v", err)
	}
	if id != "wamid.text" {
		t.Fatalf("message id = %q", id)
	}
	if gotAuth != "Bearer EAAG-secret-token" {
		t.Fatalf("authorization = %q", gotAuth)
	}
	if gotPath != "/v23.0/123456789/messages" {
		t.Fatalf("path = %q", gotPath)
	}
	if gotTo != "213550778899" {
		t.Fatalf("to = %q, want digits without plus", gotTo)
	}
	if gotBody != "Your order is confirmed." {
		t.Fatalf("body = %q", gotBody)
	}
}

func TestCloudClientSendsTemplateParameters(t *testing.T) {
	var payload struct {
		Type     string `json:"type"`
		Template struct {
			Name     string `json:"name"`
			Language struct {
				Code string `json:"code"`
			} `json:"language"`
			Components []struct {
				Type       string `json:"type"`
				Parameters []struct {
					Type string `json:"type"`
					Text string `json:"text"`
				} `json:"parameters"`
			} `json:"components"`
		} `json:"template"`
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
			t.Errorf("decode payload: %v", err)
		}
		_, _ = w.Write([]byte(`{"messages":[{"id":"wamid.template"}]}`))
	}))
	defer server.Close()

	client := &cloudClient{
		baseURL: server.URL, version: "v23.0", phoneNumberID: "1",
		accessToken: "token", http: server.Client(),
	}
	_, err := client.send(context.Background(), queuedMessage{
		Phone: "+213550778899", Kind: "template",
		TemplateName: "order_confirmation", TemplateLanguage: "en",
		TemplateParams: []string{"Amina", "AZ-2026-0001", "5,400 DZD"},
	})
	if err != nil {
		t.Fatalf("send: %v", err)
	}
	if payload.Type != "template" || payload.Template.Name != "order_confirmation" ||
		payload.Template.Language.Code != "en" {
		t.Fatalf("template payload = %#v", payload)
	}
	if len(payload.Template.Components) != 1 || payload.Template.Components[0].Type != "body" {
		t.Fatalf("components = %#v", payload.Template.Components)
	}
	if len(payload.Template.Components[0].Parameters) != 3 ||
		payload.Template.Components[0].Parameters[0].Text != "Amina" {
		t.Fatalf("parameters = %#v", payload.Template.Components[0].Parameters)
	}
}

func TestCloudClientMapsErrors(t *testing.T) {
	tests := []struct {
		status    int
		body      string
		permanent bool
		contains  string
	}{
		{status: 400, body: `{"error":{"message":"Invalid parameter","code":100}}`, permanent: true, contains: "(100) Invalid parameter"},
		{status: 401, body: `{"error":{"message":"Invalid OAuth access token"}}`, permanent: true, contains: "Invalid OAuth"},
		{status: 429, body: `{"error":{"message":"Rate limit hit"}}`, permanent: false, contains: "HTTP 429"},
		{status: 500, body: `upstream exploded`, permanent: false, contains: "HTTP 500"},
	}
	for _, test := range tests {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(test.status)
			_, _ = w.Write([]byte(test.body))
		}))
		client := &cloudClient{
			baseURL: server.URL, version: "v23.0", phoneNumberID: "1",
			accessToken: "token", http: server.Client(),
		}
		_, err := client.send(context.Background(), queuedMessage{Phone: "+213550778899", Kind: "text", Body: "hi"})
		var apiError *sendError
		if err == nil || !errors.As(err, &apiError) {
			t.Fatalf("status %d: error = %v", test.status, err)
		}
		if apiError.Permanent != test.permanent {
			t.Errorf("status %d permanent = %v, want %v", test.status, apiError.Permanent, test.permanent)
		}
		if !strings.Contains(err.Error(), test.contains) {
			t.Errorf("status %d error = %q, want %q", test.status, err.Error(), test.contains)
		}
		server.Close()
	}
}

func TestValidSignature(t *testing.T) {
	secret := "0123456789abcdef0123456789abcdef"
	body := []byte(`{"object":"whatsapp_business_account"}`)
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write(body)
	header := "sha256=" + hex.EncodeToString(mac.Sum(nil))

	if !validSignature(secret, body, header) {
		t.Fatal("a correct signature was refused")
	}
	if validSignature(secret, body, "sha256=deadbeef") {
		t.Fatal("a wrong signature was accepted")
	}
	if validSignature(secret, []byte(`{"tampered":true}`), header) {
		t.Fatal("a signature for another body was accepted")
	}
	if validSignature("", body, header) || validSignature(secret, body, "") {
		t.Fatal("an empty secret or header was accepted")
	}
}

func TestMessageBodyAndStatusError(t *testing.T) {
	text := webhookMessage{Type: "text", Text: &struct {
		Body string `json:"body"`
	}{Body: " Hello "}}
	if got := messageBody(text); got != "Hello" {
		t.Fatalf("text body = %q", got)
	}
	image := webhookMessage{Type: "image", Image: &struct {
		Caption string `json:"caption"`
	}{Caption: "the parcel"}}
	if got := messageBody(image); got != "the parcel" {
		t.Fatalf("image body = %q", got)
	}
	unknown := webhookMessage{Type: "sticker"}
	if got := messageBody(unknown); got != "[sticker]" {
		t.Fatalf("unknown body = %q", got)
	}

	status := webhookStatus{Status: "failed"}
	status.Errors = []webhookError{{Code: 131026, Title: "Message undeliverable"}}
	if got := statusError(status); got != "(131026) Message undeliverable" {
		t.Fatalf("status error = %q", got)
	}
}
