// Package whatsapp connects the store to the WhatsApp Business Cloud API.
//
// Outbound order notifications are written to an outbox inside the order
// transaction, then a background dispatcher sends them through Meta. Inbound
// messages and delivery receipts arrive on a signed webhook. The admin screen
// reads the message log, sends replies inside the 24-hour window and retries
// failed deliveries.
package whatsapp

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

const (
	pluginID = "whatsapp"

	defaultVersion           = "v23.0"
	defaultCountryCode       = "213"
	defaultOrderTemplate     = "order_confirmation"
	defaultStatusTemplate    = "order_update"
	defaultTemplateLanguage  = "en"
	maxSendAttempts          = 5
	dispatchInterval         = 5 * time.Second
	replyWindow              = 24 * time.Hour
	staleSendingAfter        = 5 * time.Minute
	defaultDispatchBatchSize = 20
)

var (
	versionPattern  = regexp.MustCompile(`^v[0-9]{1,2}\.[0-9]{1,2}$`)
	templatePattern = regexp.MustCompile(`^[a-z0-9_]{1,512}$`)
	languagePattern = regexp.MustCompile(`^[a-z]{2,3}(_[A-Z]{2})?$`)
	phoneIDPattern  = regexp.MustCompile(`^[0-9]{5,32}$`)
	countryPattern  = regexp.MustCompile(`^[0-9]{1,4}$`)
)

var (
	_ plugins.Plugin            = (*Plugin)(nil)
	_ plugins.Migrator          = (*Plugin)(nil)
	_ plugins.EnvAware          = (*Plugin)(nil)
	_ plugins.Closer            = (*Plugin)(nil)
	_ plugins.SettingsValidator = (*Plugin)(nil)
	_ plugins.CheckoutHook      = (*Plugin)(nil)
	_ plugins.OrderStatusHook   = (*Plugin)(nil)
)

type Plugin struct {
	mu          sync.RWMutex
	env         *plugins.Env
	config      settings
	configErr   error
	coreEnabled bool

	baseURL    string
	httpClient *http.Client

	wake      chan struct{}
	done      chan struct{}
	startOnce sync.Once
	closeOnce sync.Once
	stop      context.CancelFunc
	closed    bool
}

type settings struct {
	Enabled                bool
	PhoneNumberID          string
	AccessToken            string
	AppSecret              string
	VerifyToken            string
	Version                string
	CountryCode            string
	NotifyOrderCreated     bool
	NotifyShipped          bool
	NotifyDelivered        bool
	NotifyCancelled        bool
	OrderTemplate          string
	OrderTemplateLanguage  string
	StatusTemplate         string
	StatusTemplateLanguage string
}

func New() *Plugin {
	return &Plugin{
		baseURL: "https://graph.facebook.com",
		httpClient: &http.Client{
			Timeout: 20 * time.Second,
			CheckRedirect: func(*http.Request, []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
		wake:        make(chan struct{}, 1),
		done:        make(chan struct{}),
		coreEnabled: true,
	}
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:          pluginID,
		Name:        "WhatsApp Business",
		Version:     "1.0.0",
		Author:      "Azahar",
		Description: "Sends order confirmations and delivery updates through the WhatsApp Business Cloud API and records customer replies.",
		AdminPath:   "/admin/plugins/whatsapp",
		Settings: []plugins.SettingField{
			{
				Key:      "enabled",
				Label:    "Enable WhatsApp messages",
				Type:     "boolean",
				Default:  "true",
				Required: false,
				Help:     "Stop queueing and sending messages without removing the saved credentials.",
			},
			{
				Key:         "phone_number_id",
				Label:       "Phone number ID",
				Type:        "text",
				Required:    true,
				MaxLength:   32,
				Placeholder: "123456789012345",
				Help:        "The WhatsApp Business phone number ID from Meta, not the phone number itself.",
			},
			{
				Key:       "access_token",
				Label:     "Access token",
				Type:      "secret",
				Required:  true,
				MaxLength: 2048,
				Help:      "A permanent system-user token with whatsapp_business_messaging permission. Stored encrypted.",
			},
			{
				Key:       "app_secret",
				Label:     "App secret",
				Type:      "secret",
				Required:  true,
				MaxLength: 512,
				Help:      "Used to verify the X-Hub-Signature-256 header on every webhook call. Stored encrypted.",
			},
			{
				Key:       "verify_token",
				Label:     "Webhook verify token",
				Type:      "secret",
				Required:  true,
				MaxLength: 256,
				Help:      "The same string you enter in Meta's webhook setup. Stored encrypted.",
			},
			{
				Key:         "graph_version",
				Label:       "Graph API version",
				Type:        "text",
				Required:    false,
				Default:     defaultVersion,
				MaxLength:   16,
				Placeholder: defaultVersion,
				Help:        "For example v23.0. Move this when Meta retires the current version.",
			},
			{
				Key:         "default_country_code",
				Label:       "Default country code",
				Type:        "text",
				Required:    false,
				Default:     defaultCountryCode,
				MaxLength:   4,
				Placeholder: defaultCountryCode,
				Help:        "Used when an order phone number is written as a local number, for example 0550 12 34 56.",
			},
			{
				Key:      "notify_order_created",
				Label:    "Send order confirmation",
				Type:     "boolean",
				Default:  "true",
				Required: false,
			},
			{
				Key:      "notify_shipped",
				Label:    "Send when an order ships",
				Type:     "boolean",
				Default:  "true",
				Required: false,
			},
			{
				Key:      "notify_delivered",
				Label:    "Send when an order is delivered",
				Type:     "boolean",
				Default:  "true",
				Required: false,
			},
			{
				Key:      "notify_cancelled",
				Label:    "Send when an order is cancelled",
				Type:     "boolean",
				Default:  "true",
				Required: false,
			},
			{
				Key:         "order_template",
				Label:       "Order confirmation template",
				Type:        "text",
				Required:    false,
				Default:     defaultOrderTemplate,
				MaxLength:   512,
				Placeholder: defaultOrderTemplate,
				Help:        "An approved template with three body parameters: customer name, order number, total.",
			},
			{
				Key:         "order_template_language",
				Label:       "Order template language",
				Type:        "text",
				Required:    false,
				Default:     defaultTemplateLanguage,
				MaxLength:   16,
				Placeholder: defaultTemplateLanguage,
				Help:        "The language code on the approved template, for example en or fr.",
			},
			{
				Key:         "status_template",
				Label:       "Status update template",
				Type:        "text",
				Required:    false,
				Default:     defaultStatusTemplate,
				MaxLength:   512,
				Placeholder: defaultStatusTemplate,
				Help:        "An approved template with four body parameters: customer name, order number, status, total.",
			},
			{
				Key:         "status_template_language",
				Label:       "Status template language",
				Type:        "text",
				Required:    false,
				Default:     defaultTemplateLanguage,
				MaxLength:   16,
				Placeholder: defaultTemplateLanguage,
			},
		},
	}
}

func (p *Plugin) ValidateSettings(values map[string]string) map[string]string {
	fields := make(map[string]string)

	phoneID := strings.TrimSpace(values["phone_number_id"])
	if !phoneIDPattern.MatchString(phoneID) {
		fields["phone_number_id"] = "Use the 5 to 32 digit phone number ID from Meta."
	}
	if len(strings.TrimSpace(values["access_token"])) < 10 {
		fields["access_token"] = "Enter the access token from Meta."
	}
	if len(strings.TrimSpace(values["app_secret"])) < 16 {
		fields["app_secret"] = "Enter the app secret from Meta."
	}
	if len(strings.TrimSpace(values["verify_token"])) < 8 {
		fields["verify_token"] = "Use at least 8 characters for the verify token."
	}

	version := strings.TrimSpace(values["graph_version"])
	if version == "" {
		version = defaultVersion
	}
	if !versionPattern.MatchString(version) {
		fields["graph_version"] = "Use a version such as v23.0."
	}

	country := strings.TrimSpace(values["default_country_code"])
	if country == "" {
		country = defaultCountryCode
	}
	if !countryPattern.MatchString(country) {
		fields["default_country_code"] = "Use 1 to 4 digits, for example 213."
	}

	orderTemplate := strings.TrimSpace(values["order_template"])
	if orderTemplate == "" {
		orderTemplate = defaultOrderTemplate
	}
	if !templatePattern.MatchString(orderTemplate) {
		fields["order_template"] = "Use lowercase letters, numbers and underscores."
	}
	statusTemplate := strings.TrimSpace(values["status_template"])
	if statusTemplate == "" {
		statusTemplate = defaultStatusTemplate
	}
	if !templatePattern.MatchString(statusTemplate) {
		fields["status_template"] = "Use lowercase letters, numbers and underscores."
	}

	orderLanguage := strings.TrimSpace(values["order_template_language"])
	if orderLanguage == "" {
		orderLanguage = defaultTemplateLanguage
	}
	if !languagePattern.MatchString(orderLanguage) {
		fields["order_template_language"] = "Use a language code such as en or pt_BR."
	}
	statusLanguage := strings.TrimSpace(values["status_template_language"])
	if statusLanguage == "" {
		statusLanguage = defaultTemplateLanguage
	}
	if !languagePattern.MatchString(statusLanguage) {
		fields["status_template_language"] = "Use a language code such as en or pt_BR."
	}

	return fields
}

func (p *Plugin) Migrations() []plugins.Migration {
	return []plugins.Migration{{
		Version: "001_messages.sql",
		SQL: `CREATE TABLE IF NOT EXISTS whatsapp_messages (
  id TEXT PRIMARY KEY,
  direction TEXT NOT NULL,
  kind TEXT NOT NULL,
  phone TEXT NOT NULL,
  contact_name TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  template_name TEXT NOT NULL DEFAULT '',
  template_language TEXT NOT NULL DEFAULT '',
  template_params TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL,
  error TEXT NOT NULL DEFAULT '',
  meta_message_id TEXT NOT NULL DEFAULT '',
  order_id TEXT NOT NULL DEFAULT '',
  attempts INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_whatsapp_messages_queue ON whatsapp_messages(status, next_attempt_at);
CREATE INDEX IF NOT EXISTS idx_whatsapp_messages_created ON whatsapp_messages(created_at);
CREATE INDEX IF NOT EXISTS idx_whatsapp_messages_phone ON whatsapp_messages(phone, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_whatsapp_messages_meta ON whatsapp_messages(meta_message_id) WHERE meta_message_id <> '';
CREATE TABLE IF NOT EXISTS whatsapp_contacts (
  phone TEXT PRIMARY KEY,
  name TEXT NOT NULL DEFAULT '',
  opted_in INTEGER NOT NULL DEFAULT 1,
  last_inbound_at TEXT NOT NULL DEFAULT '',
  last_message_at TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS whatsapp_state (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL
);`,
	}}
}

func (p *Plugin) Routes() []plugins.Route {
	return []plugins.Route{
		{Method: http.MethodGet, Pattern: "/webhook", Webhook: true, Handler: p.handleWebhookVerify},
		{Method: http.MethodPost, Pattern: "/webhook", Webhook: true, Handler: p.handleWebhookEvent},
		{Method: http.MethodGet, Pattern: "/status", Admin: true, Handler: p.handleStatus},
		{Method: http.MethodGet, Pattern: "/messages", Admin: true, Handler: p.handleMessages},
		{Method: http.MethodPost, Pattern: "/messages", Admin: true, Handler: p.handleSendMessage},
		{Method: http.MethodPost, Pattern: "/messages/{id}/retry", Admin: true, Handler: p.handleRetryMessage},
		{Method: http.MethodGet, Pattern: "/contacts", Admin: true, Handler: p.handleContacts},
		{Method: http.MethodPatch, Pattern: "/contacts/{phone}", Admin: true, Handler: p.handleUpdateContact},
	}
}

func (p *Plugin) Init(env *plugins.Env) {
	if env == nil {
		return
	}
	p.mu.Lock()
	if p.closed {
		p.mu.Unlock()
		return
	}
	p.env = env
	p.mu.Unlock()
	if env.DB == nil {
		return
	}
	p.startOnce.Do(func() {
		p.mu.Lock()
		if p.closed {
			p.mu.Unlock()
			return
		}
		ctx, cancel := context.WithCancel(context.Background())
		p.stop = cancel
		p.mu.Unlock()
		p.refreshConfig(ctx)
		go p.run(ctx)
	})
}

func (p *Plugin) Close() {
	p.closeOnce.Do(func() {
		p.mu.Lock()
		p.closed = true
		stop := p.stop
		p.mu.Unlock()
		if stop != nil {
			stop()
			<-p.done
			return
		}
		close(p.done)
	})
}

func (p *Plugin) envOrNil() *plugins.Env {
	p.mu.RLock()
	defer p.mu.RUnlock()
	return p.env
}

func (p *Plugin) currentConfig() settings {
	p.mu.RLock()
	defer p.mu.RUnlock()
	return p.config
}

func (p *Plugin) setConfig(config settings, err error) {
	p.mu.Lock()
	p.config = config
	p.configErr = err
	p.mu.Unlock()
}

func (p *Plugin) refreshConfig(ctx context.Context) {
	env := p.envOrNil()
	if env == nil {
		return
	}
	raw, err := env.Settings(ctx, pluginID)
	if err != nil {
		p.setConfig(settings{}, err)
		return
	}
	config := parseSettings(raw)
	// The core enable switch stops every plugin surface. Hooks still run for
	// data correctness, so the cache carries the switch into the send decision.
	if env.Store != nil {
		if states, stateErr := env.Store.PluginStates(ctx); stateErr == nil {
			if enabled, ok := states[pluginID]; ok {
				p.mu.Lock()
				p.coreEnabled = enabled
				p.mu.Unlock()
			}
		}
	}
	p.mu.RLock()
	coreEnabled := p.coreEnabled
	p.mu.RUnlock()
	if !coreEnabled {
		config.Enabled = false
	}
	p.setConfig(config, nil)
}

func parseSettings(values map[string]string) settings {
	config := settings{
		Enabled:                boolValue(values["enabled"], true),
		PhoneNumberID:          strings.TrimSpace(values["phone_number_id"]),
		AccessToken:            strings.TrimSpace(values["access_token"]),
		AppSecret:              strings.TrimSpace(values["app_secret"]),
		VerifyToken:            strings.TrimSpace(values["verify_token"]),
		Version:                fallback(strings.TrimSpace(values["graph_version"]), defaultVersion),
		CountryCode:            fallback(strings.TrimSpace(values["default_country_code"]), defaultCountryCode),
		NotifyOrderCreated:     boolValue(values["notify_order_created"], true),
		NotifyShipped:          boolValue(values["notify_shipped"], true),
		NotifyDelivered:        boolValue(values["notify_delivered"], true),
		NotifyCancelled:        boolValue(values["notify_cancelled"], true),
		OrderTemplate:          fallback(strings.TrimSpace(values["order_template"]), defaultOrderTemplate),
		OrderTemplateLanguage:  fallback(strings.TrimSpace(values["order_template_language"]), defaultTemplateLanguage),
		StatusTemplate:         fallback(strings.TrimSpace(values["status_template"]), defaultStatusTemplate),
		StatusTemplateLanguage: fallback(strings.TrimSpace(values["status_template_language"]), defaultTemplateLanguage),
	}
	if !versionPattern.MatchString(config.Version) {
		config.Version = defaultVersion
	}
	if !countryPattern.MatchString(config.CountryCode) {
		config.CountryCode = defaultCountryCode
	}
	if !templatePattern.MatchString(config.OrderTemplate) {
		config.OrderTemplate = defaultOrderTemplate
	}
	if !templatePattern.MatchString(config.StatusTemplate) {
		config.StatusTemplate = defaultStatusTemplate
	}
	if !languagePattern.MatchString(config.OrderTemplateLanguage) {
		config.OrderTemplateLanguage = defaultTemplateLanguage
	}
	if !languagePattern.MatchString(config.StatusTemplateLanguage) {
		config.StatusTemplateLanguage = defaultTemplateLanguage
	}
	return config
}

func (s settings) configured() bool {
	return s.PhoneNumberID != "" && s.AccessToken != "" && s.AppSecret != "" && s.VerifyToken != ""
}

func (s settings) active() bool {
	return s.Enabled && s.configured()
}

func boolValue(raw string, fallbackValue bool) bool {
	switch strings.TrimSpace(raw) {
	case "true":
		return true
	case "false":
		return false
	default:
		return fallbackValue
	}
}

func fallback(value, other string) string {
	if value == "" {
		return other
	}
	return value
}

func (p *Plugin) ValidateCheckoutLine(context.Context, *sql.Tx, plugins.CheckoutLine) *plugins.Problem {
	return nil
}

func (p *Plugin) OnOrderCreated(ctx context.Context, tx *sql.Tx, order plugins.OrderRef, lines []plugins.CheckoutLine) error {
	config := p.currentConfig()
	if !config.active() || !config.NotifyOrderCreated {
		return nil
	}
	return p.notifyOrder(ctx, tx, order.ID, config.OrderTemplate, config.OrderTemplateLanguage, orderTemplateParams)
}

func (p *Plugin) OnOrderStatusChanged(ctx context.Context, tx *sql.Tx, orderID, from, to string) error {
	if from == to {
		return nil
	}
	config := p.currentConfig()
	if !config.active() {
		return nil
	}
	var enabled bool
	switch to {
	case "shipped":
		enabled = config.NotifyShipped
	case "delivered":
		enabled = config.NotifyDelivered
	case "cancelled":
		enabled = config.NotifyCancelled
	default:
		return nil
	}
	if !enabled {
		return nil
	}
	return p.notifyOrder(ctx, tx, orderID, config.StatusTemplate, config.StatusTemplateLanguage,
		func(order orderFacts) []string {
			return []string{firstName(order.Name), order.Number, statusLabel(to), order.Total}
		})
}

type orderFacts struct {
	Name    string
	Phone   string
	Number  string
	Total   string
	OrderID string
}

func orderTemplateParams(order orderFacts) []string {
	return []string{firstName(order.Name), order.Number, order.Total}
}

func (p *Plugin) notifyOrder(ctx context.Context, tx *sql.Tx, orderID, template, language string, params func(orderFacts) []string) error {
	config := p.currentConfig()
	facts, found, err := loadOrder(ctx, tx, orderID, config.CountryCode)
	if err != nil {
		p.warn("whatsapp order lookup failed", "error", err)
		return nil
	}
	if !found {
		return nil
	}
	optedOut, err := contactOptedOut(ctx, tx, facts.Phone)
	if err != nil {
		p.warn("whatsapp contact lookup failed", "error", err)
		return nil
	}
	if optedOut {
		return nil
	}
	values := params(facts)
	body := templatePreview(template, values)
	if err := enqueueMessage(ctx, tx, outboundMessage{
		Phone:            facts.Phone,
		ContactName:      facts.Name,
		Kind:             "template",
		Body:             body,
		TemplateName:     template,
		TemplateLanguage: language,
		TemplateParams:   values,
		OrderID:          facts.OrderID,
	}); err != nil {
		// A notification must never roll back a paid order. The message is
		// logged so an operator can see why it was not queued.
		p.warn("whatsapp message could not be queued", "error", err)
		return nil
	}
	return nil
}

func loadOrder(ctx context.Context, q interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}, orderID, countryCode string) (orderFacts, bool, error) {
	var name, phone, number, currency string
	var total int64
	err := q.QueryRowContext(ctx,
		`SELECT name, phone, number, total_cents, currency FROM orders WHERE id = ?`, orderID,
	).Scan(&name, &phone, &number, &total, &currency)
	if errors.Is(err, sql.ErrNoRows) {
		return orderFacts{}, false, nil
	}
	if err != nil {
		return orderFacts{}, false, err
	}
	normalized, ok := normalizePhone(phone, countryCode)
	if !ok {
		return orderFacts{}, false, nil
	}
	return orderFacts{
		Name:    name,
		Phone:   normalized,
		Number:  number,
		Total:   formatMoney(total, currency),
		OrderID: orderID,
	}, true, nil
}

func statusLabel(status string) string {
	switch status {
	case "pending":
		return "waiting for confirmation"
	case "confirmed":
		return "confirmed"
	case "packed":
		return "packed"
	case "shipped":
		return "on its way"
	case "delivered":
		return "delivered"
	case "cancelled":
		return "cancelled"
	default:
		return status
	}
}

func firstName(name string) string {
	fields := strings.Fields(strings.TrimSpace(name))
	if len(fields) == 0 {
		return "there"
	}
	first := []rune(fields[0])
	if len(first) > 60 {
		first = first[:60]
	}
	return string(first)
}

func templatePreview(template string, params []string) string {
	if len(params) == 0 {
		return template
	}
	return fmt.Sprintf("%s: %s", template, strings.Join(params, " · "))
}

func formatMoney(cents int64, currency string) string {
	if currency == "" {
		currency = "DZD"
	}
	sign := ""
	if cents < 0 {
		sign = "-"
		cents = -cents
	}
	whole := cents / 100
	remainder := cents % 100
	digits := strconv.FormatInt(whole, 10)
	grouped := make([]byte, 0, len(digits)+len(digits)/3)
	for index, digit := range []byte(digits) {
		if index > 0 && (len(digits)-index)%3 == 0 {
			grouped = append(grouped, ',')
		}
		grouped = append(grouped, digit)
	}
	if remainder == 0 {
		return fmt.Sprintf("%s%s %s", sign, grouped, currency)
	}
	return fmt.Sprintf("%s%s.%02d %s", sign, grouped, remainder, currency)
}

func (p *Plugin) warn(message string, attrs ...any) {
	env := p.envOrNil()
	if env == nil || env.Logger == nil {
		return
	}
	env.Logger.Warn(message, append([]any{"plugin", pluginID}, attrs...)...)
}

// newMessageID makes the message id explicit for tests and log correlation.
func newMessageID() string {
	return ids.New("wam")
}

var now = time.Now

func timestamp(value time.Time) string {
	return store.FormatTime(value)
}
