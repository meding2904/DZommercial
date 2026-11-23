package whatsapp

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

type statusDTO struct {
	Enabled               bool           `json:"enabled"`
	Configured            bool           `json:"configured"`
	SettingsError         string         `json:"settings_error,omitempty"`
	WebhookURL            string         `json:"webhook_url"`
	PhoneNumberID         string         `json:"phone_number_id"`
	GraphVersion          string         `json:"graph_version"`
	OrderTemplate         string         `json:"order_template"`
	OrderTemplateLanguage string         `json:"order_template_language"`
	StatusTemplate        string         `json:"status_template"`
	StatusLanguage        string         `json:"status_template_language"`
	Counts                map[string]int `json:"counts"`
	Contacts              int            `json:"contacts"`
	OptedOut              int            `json:"opted_out"`
	LastWebhookAt         string         `json:"last_webhook_at"`
	LastFailure           string         `json:"last_failure"`
	LastFailureAt         string         `json:"last_failure_at"`
	ReplyWindowHours      int            `json:"reply_window_hours"`
}

func (p *Plugin) handleStatus(c *plugins.Context) {
	env, ok := p.requestEnv(c)
	if !ok {
		return
	}
	ctx := c.R.Context()
	config, settingsErr := p.liveSettings(ctx)
	status := statusDTO{
		Enabled:               config.Enabled,
		Configured:            config.configured(),
		WebhookURL:            webhookURL(c.R),
		PhoneNumberID:         config.PhoneNumberID,
		GraphVersion:          config.Version,
		OrderTemplate:         config.OrderTemplate,
		OrderTemplateLanguage: config.OrderTemplateLanguage,
		StatusTemplate:        config.StatusTemplate,
		StatusLanguage:        config.StatusTemplateLanguage,
		Counts:                map[string]int{},
		LastWebhookAt:         getState(ctx, env.DB, "last_webhook_at"),
		LastFailure:           getState(ctx, env.DB, "last_failure"),
		LastFailureAt:         getState(ctx, env.DB, "last_failure_at"),
		ReplyWindowHours:      int(replyWindow / time.Hour),
	}
	if settingsErr != nil {
		status.SettingsError = "The saved settings cannot be read with the current PLUGIN_SECRET_KEY."
	}

	rows, err := env.DB.QueryContext(ctx,
		`SELECT status, COUNT(*) FROM whatsapp_messages GROUP BY status`)
	if err != nil {
		p.warn("whatsapp status counts failed", "error", err)
		c.Error(http.StatusInternalServerError, "internal_error", "The message state could not be read.")
		return
	}
	for rows.Next() {
		var name string
		var count int
		if err := rows.Scan(&name, &count); err != nil {
			rows.Close()
			c.Error(http.StatusInternalServerError, "internal_error", "The message state could not be read.")
			return
		}
		status.Counts[name] = count
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The message state could not be read.")
		return
	}

	_ = env.DB.QueryRowContext(ctx,
		`SELECT COUNT(*), COALESCE(SUM(CASE WHEN opted_in = 0 THEN 1 ELSE 0 END), 0)
		 FROM whatsapp_contacts`).Scan(&status.Contacts, &status.OptedOut)

	c.JSON(http.StatusOK, status)
}

type messageDTO struct {
	ID               string   `json:"id"`
	Direction        string   `json:"direction"`
	Kind             string   `json:"kind"`
	Phone            string   `json:"phone"`
	ContactName      string   `json:"contact_name"`
	Body             string   `json:"body"`
	TemplateName     string   `json:"template_name"`
	TemplateLanguage string   `json:"template_language"`
	TemplateParams   []string `json:"template_params"`
	Status           string   `json:"status"`
	Error            string   `json:"error"`
	MetaMessageID    string   `json:"meta_message_id"`
	OrderID          string   `json:"order_id"`
	OrderNumber      string   `json:"order_number"`
	ContactOptedIn   bool     `json:"contact_opted_in"`
	Attempts         int      `json:"attempts"`
	CreatedAt        string   `json:"created_at"`
	UpdatedAt        string   `json:"updated_at"`
}

func (p *Plugin) handleMessages(c *plugins.Context) {
	env, ok := p.requestEnv(c)
	if !ok {
		return
	}
	limit, fields := listLimit(c.R.URL.Query().Get("limit"))
	status := strings.TrimSpace(c.R.URL.Query().Get("status"))
	if status != "" {
		switch status {
		case "queued", "sending", "sent", "delivered", "read", "failed", "received", "cancelled":
		default:
			fields["status"] = "Choose a known message state."
		}
	}
	direction := strings.TrimSpace(c.R.URL.Query().Get("direction"))
	if direction != "" && direction != "in" && direction != "out" {
		fields["direction"] = "Choose in or out."
	}
	if len(fields) > 0 {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.", fields)
		return
	}

	rows, err := env.DB.QueryContext(c.R.Context(),
		`SELECT m.id, m.direction, m.kind, m.phone, m.contact_name, m.body,
		        m.template_name, m.template_language, m.template_params, m.status,
		        m.error, m.meta_message_id, m.order_id, COALESCE(o.number, ''),
		        COALESCE(c.opted_in, 1), m.attempts, m.created_at, m.updated_at
		 FROM whatsapp_messages m
		 LEFT JOIN orders o ON o.id = m.order_id
		 LEFT JOIN whatsapp_contacts c ON c.phone = m.phone
		 WHERE (? = '' OR m.status = ?) AND (? = '' OR m.direction = ?)
		 ORDER BY m.created_at DESC, m.id DESC LIMIT ?`,
		status, status, direction, direction, limit,
	)
	if err != nil {
		p.warn("whatsapp messages could not be read", "error", err)
		c.Error(http.StatusInternalServerError, "internal_error", "The messages could not be read.")
		return
	}
	defer rows.Close()

	messages := make([]messageDTO, 0, limit)
	for rows.Next() {
		message, err := scanMessage(rows)
		if err != nil {
			p.warn("whatsapp message row could not be read", "error", err)
			c.Error(http.StatusInternalServerError, "internal_error", "The messages could not be read.")
			return
		}
		messages = append(messages, message)
	}
	if err := rows.Err(); err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The messages could not be read.")
		return
	}
	c.JSON(http.StatusOK, map[string]any{"messages": messages})
}

type scanner interface {
	Scan(...any) error
}

func scanMessage(row scanner) (messageDTO, error) {
	var message messageDTO
	var params string
	var optedIn int
	err := row.Scan(&message.ID, &message.Direction, &message.Kind, &message.Phone,
		&message.ContactName, &message.Body, &message.TemplateName, &message.TemplateLanguage,
		&params, &message.Status, &message.Error, &message.MetaMessageID, &message.OrderID,
		&message.OrderNumber, &optedIn, &message.Attempts, &message.CreatedAt, &message.UpdatedAt)
	if err != nil {
		return messageDTO{}, err
	}
	_ = json.Unmarshal([]byte(params), &message.TemplateParams)
	if message.TemplateParams == nil {
		message.TemplateParams = []string{}
	}
	message.ContactOptedIn = optedIn != 0
	return message, nil
}

type contactDTO struct {
	Phone         string `json:"phone"`
	Name          string `json:"name"`
	OptedIn       bool   `json:"opted_in"`
	LastInboundAt string `json:"last_inbound_at"`
	LastMessageAt string `json:"last_message_at"`
	ReplyOpen     bool   `json:"reply_open"`
	CreatedAt     string `json:"created_at"`
	UpdatedAt     string `json:"updated_at"`
}

func (p *Plugin) handleContacts(c *plugins.Context) {
	env, ok := p.requestEnv(c)
	if !ok {
		return
	}
	limit, fields := listLimit(c.R.URL.Query().Get("limit"))
	if len(fields) > 0 {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.", fields)
		return
	}
	rows, err := env.DB.QueryContext(c.R.Context(),
		`SELECT phone, name, opted_in, last_inbound_at, last_message_at, created_at, updated_at
		 FROM whatsapp_contacts ORDER BY last_message_at DESC, phone LIMIT ?`, limit)
	if err != nil {
		p.warn("whatsapp contacts could not be read", "error", err)
		c.Error(http.StatusInternalServerError, "internal_error", "The contacts could not be read.")
		return
	}
	defer rows.Close()
	contacts := make([]contactDTO, 0, limit)
	for rows.Next() {
		var contact contactDTO
		var optedIn int
		if err := rows.Scan(&contact.Phone, &contact.Name, &optedIn, &contact.LastInboundAt,
			&contact.LastMessageAt, &contact.CreatedAt, &contact.UpdatedAt); err != nil {
			c.Error(http.StatusInternalServerError, "internal_error", "The contacts could not be read.")
			return
		}
		contact.OptedIn = optedIn != 0
		contact.ReplyOpen = replyOpen(contact.LastInboundAt)
		contacts = append(contacts, contact)
	}
	if err := rows.Err(); err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The contacts could not be read.")
		return
	}
	c.JSON(http.StatusOK, map[string]any{"contacts": contacts})
}

type sendRequest struct {
	Phone            string   `json:"phone"`
	Kind             string   `json:"kind"`
	Text             string   `json:"text"`
	TemplateName     string   `json:"template_name"`
	TemplateLanguage string   `json:"template_language"`
	TemplateParams   []string `json:"template_params"`
	OrderID          string   `json:"order_id"`
}

func (p *Plugin) handleSendMessage(c *plugins.Context) {
	env, ok := p.requestEnv(c)
	if !ok {
		return
	}
	request, bad := plugins.DecodeBody[sendRequest](c)
	if bad != nil {
		c.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
		return
	}
	config, settingsErr := p.liveSettings(c.R.Context())
	if settingsErr != nil {
		c.Error(http.StatusConflict, "plugin_settings_unreadable",
			"These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.")
		return
	}
	if !config.Enabled {
		c.Error(http.StatusConflict, "plugin_disabled",
			"WhatsApp messages are paused. Enable them in the plugin settings before sending.")
		return
	}
	if !config.configured() {
		c.Error(http.StatusConflict, "plugin_not_configured",
			"Open the WhatsApp settings and add the phone number ID and credentials.")
		return
	}

	phone, valid := normalizePhone(request.Phone, config.CountryCode)
	kind := strings.TrimSpace(request.Kind)
	if kind == "" {
		kind = "text"
	}
	fields := make(map[string]string)
	if !valid {
		fields["phone"] = "Enter an Algerian or international phone number."
	}
	if kind != "text" && kind != "template" {
		fields["kind"] = "Choose text or template."
	}
	templateName := strings.TrimSpace(request.TemplateName)
	templateLanguage := fallback(strings.TrimSpace(request.TemplateLanguage), defaultTemplateLanguage)
	text := strings.TrimSpace(request.Text)
	if kind == "text" {
		if text == "" {
			fields["text"] = "Write the message."
		} else if len([]rune(text)) > 4096 {
			fields["text"] = "Keep the message under 4097 characters."
		}
	}
	if kind == "template" {
		if !templatePattern.MatchString(templateName) {
			fields["template_name"] = "Use the approved template name, for example order_update."
		}
		if !languagePattern.MatchString(templateLanguage) {
			fields["template_language"] = "Use a language code such as en or fr."
		}
		if len(request.TemplateParams) > 10 {
			fields["template_params"] = "A template can take at most 10 parameters."
		}
		for index, value := range request.TemplateParams {
			value = strings.TrimSpace(value)
			if value == "" || len([]rune(value)) > 1024 || strings.ContainsAny(value, "\r\n\t") {
				fields["template_params"] = "Parameter " + strconv.Itoa(index+1) +
					" must be 1 to 1024 characters on one line."
				break
			}
			request.TemplateParams[index] = value
		}
	}
	orderID := strings.TrimSpace(request.OrderID)
	if orderID != "" {
		var count int
		if err := env.DB.QueryRowContext(c.R.Context(),
			`SELECT COUNT(*) FROM orders WHERE id = ?`, orderID).Scan(&count); err != nil || count == 0 {
			fields["order_id"] = "That order does not exist."
		}
	}
	if len(fields) > 0 {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.", fields)
		return
	}

	optedOut, err := contactOptedOut(c.R.Context(), env.DB, phone)
	if err != nil {
		p.warn("whatsapp contact lookup failed", "error", err)
		c.Error(http.StatusInternalServerError, "internal_error", "The message could not be queued.")
		return
	}
	if optedOut {
		c.Error(http.StatusConflict, "contact_opted_out",
			"That contact asked to stop receiving WhatsApp messages. Update the contact before sending.")
		return
	}
	if kind == "text" && !replyWindowOpen(c.R.Context(), env.DB, phone) {
		c.Error(http.StatusConflict, "reply_window_closed",
			"The 24-hour reply window is closed. Send an approved template, or wait for the customer to message first.")
		return
	}

	body := text
	if kind == "template" {
		body = templatePreview(templateName, request.TemplateParams)
	}
	if err := enqueueMessage(c.R.Context(), env.DB, outboundMessage{
		Phone:            phone,
		Kind:             kind,
		Body:             body,
		TemplateName:     templateName,
		TemplateLanguage: templateLanguage,
		TemplateParams:   request.TemplateParams,
		OrderID:          orderID,
	}); err != nil {
		p.warn("whatsapp message could not be queued", "error", err)
		c.Error(http.StatusInternalServerError, "internal_error", "The message could not be queued.")
		return
	}
	p.triggerDispatch()

	message, err := p.messageByRecent(c.R.Context(), env.DB, phone)
	if err != nil {
		c.JSON(http.StatusCreated, map[string]any{"queued": true})
		return
	}
	c.JSON(http.StatusCreated, map[string]any{"message": message})
}

func (p *Plugin) handleRetryMessage(c *plugins.Context) {
	env, ok := p.requestEnv(c)
	if !ok {
		return
	}
	id := strings.TrimSpace(chi.URLParam(c.R, "id"))
	var status, direction, phone string
	err := env.DB.QueryRowContext(c.R.Context(),
		`SELECT status, direction, phone FROM whatsapp_messages WHERE id = ?`, id,
	).Scan(&status, &direction, &phone)
	if errors.Is(err, sql.ErrNoRows) {
		c.Error(http.StatusNotFound, "not_found", "That message no longer exists.")
		return
	}
	if err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The message could not be read.")
		return
	}
	if direction != "out" || status != "failed" {
		c.Error(http.StatusConflict, "not_retryable", "Only a failed outgoing message can be retried.")
		return
	}
	optedOut, err := contactOptedOut(c.R.Context(), env.DB, phone)
	if err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The message could not be retried.")
		return
	}
	if optedOut {
		c.Error(http.StatusConflict, "contact_opted_out",
			"That contact asked to stop receiving WhatsApp messages.")
		return
	}
	at := timestamp(now())
	if _, err := env.DB.ExecContext(c.R.Context(),
		`UPDATE whatsapp_messages SET status = 'queued', attempts = 0, next_attempt_at = ?,
		   error = '', updated_at = ? WHERE id = ?`,
		at, at, id,
	); err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The message could not be retried.")
		return
	}
	p.triggerDispatch()
	message, err := p.messageByID(c.R.Context(), env.DB, id)
	if err != nil {
		c.JSON(http.StatusOK, map[string]any{"queued": true})
		return
	}
	c.JSON(http.StatusOK, map[string]any{"message": message})
}

type contactRequest struct {
	OptedIn *bool `json:"opted_in"`
}

func (p *Plugin) handleUpdateContact(c *plugins.Context) {
	env, ok := p.requestEnv(c)
	if !ok {
		return
	}
	request, bad := plugins.DecodeBody[contactRequest](c)
	if bad != nil {
		c.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
		return
	}
	if request.OptedIn == nil {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.",
			map[string]string{"opted_in": "Choose whether the contact can receive messages."})
		return
	}
	config, _ := p.liveSettings(c.R.Context())
	phone, valid := normalizePhone(chi.URLParam(c.R, "phone"), config.CountryCode)
	if !valid {
		c.Error(http.StatusNotFound, "not_found", "That contact no longer exists.")
		return
	}
	value := 0
	if *request.OptedIn {
		value = 1
	}
	result, err := env.DB.ExecContext(c.R.Context(),
		`UPDATE whatsapp_contacts SET opted_in = ?, updated_at = ? WHERE phone = ?`,
		value, timestamp(now()), phone)
	if err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The contact could not be saved.")
		return
	}
	affected, _ := result.RowsAffected()
	if affected == 0 {
		c.Error(http.StatusNotFound, "not_found", "That contact no longer exists.")
		return
	}
	contact, err := p.contactByPhone(c.R.Context(), env.DB, phone)
	if err != nil {
		c.JSON(http.StatusOK, map[string]any{"phone": phone, "opted_in": *request.OptedIn})
		return
	}
	c.JSON(http.StatusOK, map[string]any{"contact": contact})
}

func (p *Plugin) requestEnv(c *plugins.Context) (*plugins.Env, bool) {
	env := p.envOrNil()
	if env == nil || env.DB == nil {
		c.Error(http.StatusServiceUnavailable, "unavailable", "The plugin is not ready.")
		return nil, false
	}
	return env, true
}

func listLimit(raw string) (int, map[string]string) {
	limit := 50
	fields := make(map[string]string)
	if strings.TrimSpace(raw) == "" {
		return limit, fields
	}
	parsed, err := strconv.Atoi(strings.TrimSpace(raw))
	if err != nil || parsed < 1 || parsed > 100 {
		fields["limit"] = "Choose a limit from 1 to 100."
		return limit, fields
	}
	return parsed, fields
}

func webhookURL(r *http.Request) string {
	scheme := "http"
	if r.TLS != nil {
		scheme = "https"
	}
	if forwarded := strings.ToLower(strings.TrimSpace(r.Header.Get("X-Forwarded-Proto"))); forwarded == "https" || forwarded == "http" {
		scheme = forwarded
	}
	return scheme + "://" + r.Host + "/api/plugins/" + pluginID + "/webhook"
}

func replyWindowOpen(ctx context.Context, q interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}, phone string) bool {
	return replyOpen(lastInbound(ctx, q, phone))
}

func lastInbound(ctx context.Context, q interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}, phone string) string {
	var value string
	if err := q.QueryRowContext(ctx,
		`SELECT last_inbound_at FROM whatsapp_contacts WHERE phone = ?`, phone).Scan(&value); err != nil {
		return ""
	}
	return value
}

func replyOpen(lastInboundAt string) bool {
	if lastInboundAt == "" {
		return false
	}
	at := store.ParseTime(lastInboundAt)
	if at.IsZero() {
		return false
	}
	return time.Since(at) <= replyWindow
}

func (p *Plugin) messageByID(ctx context.Context, q *sql.DB, id string) (messageDTO, error) {
	return p.messageQuery(ctx, q,
		`SELECT m.id, m.direction, m.kind, m.phone, m.contact_name, m.body,
		        m.template_name, m.template_language, m.template_params, m.status,
		        m.error, m.meta_message_id, m.order_id, COALESCE(o.number, ''),
		        COALESCE(c.opted_in, 1), m.attempts, m.created_at, m.updated_at
		 FROM whatsapp_messages m
		 LEFT JOIN orders o ON o.id = m.order_id
		 LEFT JOIN whatsapp_contacts c ON c.phone = m.phone
		 WHERE m.id = ?`, id)
}

func (p *Plugin) messageByRecent(ctx context.Context, q *sql.DB, phone string) (messageDTO, error) {
	return p.messageQuery(ctx, q,
		`SELECT m.id, m.direction, m.kind, m.phone, m.contact_name, m.body,
		        m.template_name, m.template_language, m.template_params, m.status,
		        m.error, m.meta_message_id, m.order_id, COALESCE(o.number, ''),
		        COALESCE(c.opted_in, 1), m.attempts, m.created_at, m.updated_at
		 FROM whatsapp_messages m
		 LEFT JOIN orders o ON o.id = m.order_id
		 LEFT JOIN whatsapp_contacts c ON c.phone = m.phone
		 WHERE m.phone = ? AND m.direction = 'out'
		 ORDER BY m.created_at DESC, m.id DESC LIMIT 1`, phone)
}

func (p *Plugin) messageQuery(ctx context.Context, q *sql.DB, query string, args ...any) (messageDTO, error) {
	return scanMessage(q.QueryRowContext(ctx, query, args...))
}

func (p *Plugin) contactByPhone(ctx context.Context, q *sql.DB, phone string) (contactDTO, error) {
	var contact contactDTO
	var optedIn int
	err := q.QueryRowContext(ctx,
		`SELECT phone, name, opted_in, last_inbound_at, last_message_at, created_at, updated_at
		 FROM whatsapp_contacts WHERE phone = ?`, phone,
	).Scan(&contact.Phone, &contact.Name, &optedIn, &contact.LastInboundAt,
		&contact.LastMessageAt, &contact.CreatedAt, &contact.UpdatedAt)
	if err != nil {
		return contactDTO{}, err
	}
	contact.OptedIn = optedIn != 0
	contact.ReplyOpen = replyOpen(contact.LastInboundAt)
	return contact, nil
}
