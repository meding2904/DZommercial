package whatsapp

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"

	"azahar/internal/plugins"
)

var errNotReady = errors.New("whatsapp plugin has no environment")

type webhookPayload struct {
	Object string `json:"object"`
	Entry  []struct {
		ID      string `json:"id"`
		Changes []struct {
			Field string `json:"field"`
			Value struct {
				MessagingProduct string `json:"messaging_product"`
				Metadata         struct {
					DisplayPhoneNumber string `json:"display_phone_number"`
					PhoneNumberID      string `json:"phone_number_id"`
				} `json:"metadata"`
				Contacts []webhookContact `json:"contacts"`
				Messages []webhookMessage `json:"messages"`
				Statuses []webhookStatus  `json:"statuses"`
			} `json:"value"`
		} `json:"changes"`
	} `json:"entry"`
}

type webhookContact struct {
	Profile struct {
		Name string `json:"name"`
	} `json:"profile"`
	WaID string `json:"wa_id"`
}

type webhookMessage struct {
	From      string `json:"from"`
	ID        string `json:"id"`
	Timestamp string `json:"timestamp"`
	Type      string `json:"type"`
	Text      *struct {
		Body string `json:"body"`
	} `json:"text"`
	Button *struct {
		Text string `json:"text"`
	} `json:"button"`
	Interactive *struct {
		ButtonReply *struct {
			Title string `json:"title"`
		} `json:"button_reply"`
		ListReply *struct {
			Title string `json:"title"`
		} `json:"list_reply"`
	} `json:"interactive"`
	Image *struct {
		Caption string `json:"caption"`
	} `json:"image"`
	Document *struct {
		Caption string `json:"caption"`
	} `json:"document"`
	Video *struct {
		Caption string `json:"caption"`
	} `json:"video"`
}

type webhookStatus struct {
	ID          string         `json:"id"`
	Status      string         `json:"status"`
	Timestamp   string         `json:"timestamp"`
	RecipientID string         `json:"recipient_id"`
	Errors      []webhookError `json:"errors"`
}

type webhookError struct {
	Code      int    `json:"code"`
	Title     string `json:"title"`
	Message   string `json:"message"`
	ErrorData struct {
		Details string `json:"details"`
	} `json:"error_data"`
}

func (p *Plugin) liveSettings(ctx context.Context) (settings, error) {
	env := p.envOrNil()
	if env == nil {
		return settings{}, errNotReady
	}
	raw, err := env.Settings(ctx, pluginID)
	if err != nil {
		return settings{}, err
	}
	return parseSettings(raw), nil
}

func (p *Plugin) handleWebhookVerify(c *plugins.Context) {
	config, err := p.liveSettings(c.R.Context())
	if err != nil || !config.active() {
		c.Error(http.StatusForbidden, "forbidden", "The request could not be verified.")
		return
	}
	query := c.R.URL.Query()
	if query.Get("hub.mode") != "subscribe" {
		c.Error(http.StatusForbidden, "forbidden", "The request could not be verified.")
		return
	}
	token := query.Get("hub.verify_token")
	if token == "" || subtle.ConstantTimeCompare([]byte(token), []byte(config.VerifyToken)) != 1 {
		c.Error(http.StatusForbidden, "forbidden", "The request could not be verified.")
		return
	}
	challenge := query.Get("hub.challenge")
	if challenge == "" {
		c.Error(http.StatusBadRequest, "invalid_request", "The challenge is missing.")
		return
	}
	c.W.Header().Set("Content-Type", "text/plain; charset=utf-8")
	c.W.WriteHeader(http.StatusOK)
	_, _ = io.WriteString(c.W, challenge)
}

func (p *Plugin) handleWebhookEvent(c *plugins.Context) {
	config, err := p.liveSettings(c.R.Context())
	if err != nil || !config.active() {
		c.Error(http.StatusForbidden, "forbidden", "The request could not be verified.")
		return
	}

	body, err := io.ReadAll(io.LimitReader(c.R.Body, 1<<20+1))
	if err != nil {
		c.Error(http.StatusBadRequest, "invalid_request", "The request body could not be read.")
		return
	}
	if len(body) > 1<<20 {
		c.Error(http.StatusRequestEntityTooLarge, "payload_too_large", "The request body is too large.")
		return
	}
	if !validSignature(config.AppSecret, body, c.R.Header.Get("X-Hub-Signature-256")) {
		c.Error(http.StatusForbidden, "forbidden", "The request could not be verified.")
		return
	}

	var payload webhookPayload
	if err := json.Unmarshal(body, &payload); err != nil {
		c.Error(http.StatusBadRequest, "invalid_json", "The webhook body is not valid JSON.")
		return
	}

	env := p.envOrNil()
	if env == nil || env.DB == nil {
		c.Error(http.StatusServiceUnavailable, "unavailable", "The webhook is not ready.")
		return
	}
	ctx := c.R.Context()
	setState(ctx, env.DB, "last_webhook_at", timestamp(now()))

	for _, entry := range payload.Entry {
		for _, change := range entry.Changes {
			if change.Field != "" && change.Field != "messages" {
				continue
			}
			value := change.Value
			names := make(map[string]string, len(value.Contacts))
			for _, contact := range value.Contacts {
				names[contact.WaID] = contact.Profile.Name
			}
			for _, status := range value.Statuses {
				if status.ID == "" || !knownReceipt(status.Status) {
					continue
				}
				if err := updateStatus(ctx, env.DB, status.ID, status.Status,
					statusError(status), timestamp(inboundTimestamp(status.Timestamp))); err != nil {
					p.warn("whatsapp delivery receipt could not be saved", "error", err)
					c.Error(http.StatusInternalServerError, "internal_error", "The webhook could not be saved.")
					return
				}
			}
			for _, message := range value.Messages {
				phone, ok := normalizePhone(message.From, "")
				if !ok || message.ID == "" {
					continue
				}
				if err := recordInbound(ctx, env.DB, inboundMessage{
					Phone:       phone,
					ContactName: names[message.From],
					Kind:        messageType(message.Type),
					Body:        messageBody(message),
					MetaID:      message.ID,
					At:          timestamp(inboundTimestamp(message.Timestamp)),
				}); err != nil {
					p.warn("whatsapp inbound message could not be saved", "error", err)
					c.Error(http.StatusInternalServerError, "internal_error", "The webhook could not be saved.")
					return
				}
			}
		}
	}

	c.JSON(http.StatusOK, map[string]any{"received": true})
}

func knownReceipt(status string) bool {
	switch status {
	case "sent", "delivered", "read", "failed":
		return true
	default:
		return false
	}
}

func statusError(status webhookStatus) string {
	if len(status.Errors) == 0 {
		return ""
	}
	first := status.Errors[0]
	parts := make([]string, 0, 3)
	if first.Code != 0 {
		parts = append(parts, "("+strconv.Itoa(first.Code)+")")
	}
	if first.Title != "" {
		parts = append(parts, first.Title)
	}
	if first.ErrorData.Details != "" {
		parts = append(parts, first.ErrorData.Details)
	} else if first.Message != "" {
		parts = append(parts, first.Message)
	}
	return strings.Join(parts, " ")
}

func messageType(kind string) string {
	kind = strings.TrimSpace(kind)
	if kind == "" {
		return "unknown"
	}
	return kind
}

func messageBody(message webhookMessage) string {
	if message.Text != nil {
		return strings.TrimSpace(message.Text.Body)
	}
	if message.Button != nil {
		return strings.TrimSpace(message.Button.Text)
	}
	if message.Interactive != nil {
		if message.Interactive.ButtonReply != nil {
			return strings.TrimSpace(message.Interactive.ButtonReply.Title)
		}
		if message.Interactive.ListReply != nil {
			return strings.TrimSpace(message.Interactive.ListReply.Title)
		}
	}
	caption := ""
	switch {
	case message.Image != nil:
		caption = message.Image.Caption
		if caption == "" {
			caption = "[image]"
		}
	case message.Document != nil:
		caption = message.Document.Caption
		if caption == "" {
			caption = "[document]"
		}
	case message.Video != nil:
		caption = message.Video.Caption
		if caption == "" {
			caption = "[video]"
		}
	default:
		caption = "[" + messageType(message.Type) + "]"
	}
	return strings.TrimSpace(caption)
}
