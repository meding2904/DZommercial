package whatsapp

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
)

const maxAPIResponseBytes = 1 << 20

type cloudClient struct {
	baseURL       string
	version       string
	phoneNumberID string
	accessToken   string
	http          *http.Client
}

type queuedMessage struct {
	ID               string
	Phone            string
	Kind             string
	Body             string
	TemplateName     string
	TemplateLanguage string
	TemplateParams   []string
}

type sendError struct {
	Status    int
	Detail    string
	Permanent bool
}

func (e *sendError) Error() string {
	if e.Status > 0 {
		return fmt.Sprintf("whatsapp API returned HTTP %d: %s", e.Status, e.Detail)
	}
	return "whatsapp API request failed: " + e.Detail
}

type messagePayload struct {
	MessagingProduct string           `json:"messaging_product"`
	RecipientType    string           `json:"recipient_type"`
	To               string           `json:"to"`
	Type             string           `json:"type"`
	Text             *textPayload     `json:"text,omitempty"`
	Template         *templatePayload `json:"template,omitempty"`
}

type textPayload struct {
	PreviewURL bool   `json:"preview_url"`
	Body       string `json:"body"`
}

type templatePayload struct {
	Name       string              `json:"name"`
	Language   templateLanguage    `json:"language"`
	Components []templateComponent `json:"components,omitempty"`
}

type templateLanguage struct {
	Code string `json:"code"`
}

type templateComponent struct {
	Type       string              `json:"type"`
	Parameters []templateParameter `json:"parameters"`
}

type templateParameter struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

type sendResponse struct {
	Messages []struct {
		ID string `json:"id"`
	} `json:"messages"`
}

type apiErrorBody struct {
	Error struct {
		Message string `json:"message"`
		Type    string `json:"type"`
		Code    int    `json:"code"`
	} `json:"error"`
}

func (p *Plugin) send(ctx context.Context, config settings, message queuedMessage) (string, error) {
	client := &cloudClient{
		baseURL:       strings.TrimRight(p.baseURL, "/"),
		version:       config.Version,
		phoneNumberID: config.PhoneNumberID,
		accessToken:   config.AccessToken,
		http:          p.httpClient,
	}
	return client.send(ctx, message)
}

func (c *cloudClient) send(ctx context.Context, message queuedMessage) (string, error) {
	payload := messagePayload{
		MessagingProduct: "whatsapp",
		RecipientType:    "individual",
		To:               strings.TrimPrefix(message.Phone, "+"),
		Type:             message.Kind,
	}
	switch message.Kind {
	case "template":
		template := &templatePayload{
			Name:     message.TemplateName,
			Language: templateLanguage{Code: message.TemplateLanguage},
		}
		if len(message.TemplateParams) > 0 {
			parameters := make([]templateParameter, 0, len(message.TemplateParams))
			for _, value := range message.TemplateParams {
				parameters = append(parameters, templateParameter{Type: "text", Text: value})
			}
			template.Components = []templateComponent{{Type: "body", Parameters: parameters}}
		}
		payload.Template = template
	case "text":
		payload.Text = &textPayload{PreviewURL: false, Body: message.Body}
	default:
		return "", &sendError{Detail: "unsupported message kind", Permanent: true}
	}

	encoded, err := json.Marshal(payload)
	if err != nil {
		return "", &sendError{Detail: "the request could not be encoded", Permanent: true}
	}
	url := fmt.Sprintf("%s/%s/%s/messages", c.baseURL, c.version, c.phoneNumberID)
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(encoded))
	if err != nil {
		return "", &sendError{Detail: "the request could not be built"}
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+c.accessToken)

	response, err := c.http.Do(request)
	if err != nil {
		// The token travels in a header, so a transport error never contains it.
		return "", &sendError{Detail: transportDetail(err)}
	}
	defer response.Body.Close()

	body, err := io.ReadAll(io.LimitReader(response.Body, maxAPIResponseBytes+1))
	if err != nil {
		return "", &sendError{Status: response.StatusCode, Detail: "the response could not be read"}
	}
	if len(body) > maxAPIResponseBytes {
		return "", &sendError{Status: response.StatusCode, Detail: "the response was too large"}
	}

	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", &sendError{
			Status:    response.StatusCode,
			Detail:    apiErrorDetail(body),
			Permanent: response.StatusCode >= 400 && response.StatusCode < 500 && response.StatusCode != http.StatusTooManyRequests,
		}
	}

	var decoded sendResponse
	if err := json.Unmarshal(body, &decoded); err != nil {
		return "", &sendError{Status: response.StatusCode, Detail: "the response was not JSON"}
	}
	if len(decoded.Messages) == 0 {
		return "", nil
	}
	return decoded.Messages[0].ID, nil
}

func apiErrorDetail(body []byte) string {
	var decoded apiErrorBody
	if err := json.Unmarshal(body, &decoded); err == nil && strings.TrimSpace(decoded.Error.Message) != "" {
		detail := strings.Join(strings.Fields(decoded.Error.Message), " ")
		if decoded.Error.Code != 0 {
			detail = fmt.Sprintf("(%d) %s", decoded.Error.Code, detail)
		}
		if len([]rune(detail)) > 400 {
			detail = string([]rune(detail)[:400])
		}
		return detail
	}
	return "the request was rejected"
}

func transportDetail(err error) string {
	var urlError interface{ Timeout() bool }
	detail := err.Error()
	if errors.As(err, &urlError) && urlError.Timeout() {
		return "the request timed out"
	}
	if len([]rune(detail)) > 200 {
		detail = string([]rune(detail)[:200])
	}
	return detail
}

func validSignature(appSecret string, body []byte, header string) bool {
	if appSecret == "" || !strings.HasPrefix(header, "sha256=") {
		return false
	}
	mac := hmac.New(sha256.New, []byte(appSecret))
	mac.Write(body)
	expected := "sha256=" + hex.EncodeToString(mac.Sum(nil))
	return hmac.Equal([]byte(expected), []byte(header))
}
