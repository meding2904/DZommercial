// Package ai adds an admin-only analyst that explains store and product
// performance using recorded sales facts and an OpenAI-compatible endpoint.
package ai

import (
	"database/sql"
	"errors"
	"fmt"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

const (
	pluginID        = "ai"
	defaultModel    = "gpt-4o-mini"
	defaultLanguage = "en"
	defaultDays     = 30
)

var (
	modelPattern       = regexp.MustCompile(`^[A-Za-z0-9._:/-]{1,64}$`)
	supportedLanguages = map[string]string{
		"en": "English",
		"fr": "French",
		"ar": "Arabic",
	}
)

var (
	_ plugins.Plugin            = (*Plugin)(nil)
	_ plugins.Migrator          = (*Plugin)(nil)
	_ plugins.SettingsValidator = (*Plugin)(nil)
)

type Plugin struct{}

func New() *Plugin {
	return &Plugin{}
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:          pluginID,
		Name:        "AI analyst",
		Version:     "1.0.0",
		Author:      "Azahar",
		Description: "Explains store and product performance in plain language using only recorded sales facts.",
		AdminPath:   "/admin/plugins/ai",
		Settings: []plugins.SettingField{
			{
				Key:       "api_key",
				Label:     "API key",
				Type:      "secret",
				Required:  true,
				Help:      "Stored encrypted on the server. It is never sent back to the browser.",
				MaxLength: 512,
			},
			{
				Key:       "model",
				Label:     "Model",
				Type:      "text",
				Required:  true,
				Default:   defaultModel,
				MaxLength: 64,
				Help:      "The model name accepted by your OpenAI-compatible endpoint.",
			},
			{
				Key:      "language",
				Label:    "Answer language",
				Type:     "select",
				Required: true,
				Default:  defaultLanguage,
				Options: []plugins.SettingOption{
					{Value: "en", Label: "English"},
					{Value: "fr", Label: "Français"},
					{Value: "ar", Label: "العربية"},
				},
			},
			{
				Key:      "enabled",
				Label:    "Enable analysis",
				Type:     "boolean",
				Required: true,
				Default:  "true",
			},
		},
	}
}

func (p *Plugin) ValidateSettings(settings map[string]string) map[string]string {
	fields := make(map[string]string)

	model := strings.TrimSpace(settings["model"])
	if !modelPattern.MatchString(model) {
		fields["model"] = "Use 1 to 64 letters, numbers, dots, dashes, colons or slashes."
	}

	language := strings.TrimSpace(settings["language"])
	if _, ok := supportedLanguages[language]; !ok {
		fields["language"] = "Choose English, Français or العربية."
	}

	return fields
}

func (p *Plugin) Migrations() []plugins.Migration {
	return []plugins.Migration{{
		Version: "001_runs.sql",
		SQL: `CREATE TABLE IF NOT EXISTS plugin_ai_runs (
  id TEXT PRIMARY KEY,
  actor_user_id TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL,
  subject TEXT NOT NULL DEFAULT '',
  model TEXT NOT NULL,
  analysis TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_plugin_ai_runs_created ON plugin_ai_runs(created_at);`,
	}}
}

func (p *Plugin) Routes() []plugins.Route {
	return []plugins.Route{
		{Method: http.MethodPost, Pattern: "/analyze", Admin: true, Handler: p.handleAnalyze},
		{Method: http.MethodGet, Pattern: "/runs", Admin: true, Handler: p.handleRuns},
	}
}

type analyzeRequest struct {
	Kind      string `json:"kind"`
	ProductID string `json:"product_id"`
	Days      int    `json:"days"`
}

type runDTO struct {
	ID        string `json:"id"`
	Kind      string `json:"kind"`
	Subject   string `json:"subject"`
	Model     string `json:"model"`
	Analysis  string `json:"analysis"`
	CreatedAt string `json:"created_at"`
}

func (p *Plugin) handleAnalyze(c *plugins.Context) {
	request, bad := plugins.DecodeBody[analyzeRequest](c)
	if bad != nil {
		c.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
		return
	}

	kind := strings.TrimSpace(request.Kind)
	productID := strings.TrimSpace(request.ProductID)
	days := request.Days
	if days == 0 {
		days = defaultDays
	}

	fields := make(map[string]string)
	if kind != "store" && kind != "product" {
		fields["kind"] = "Choose store or product."
	}
	if days != 7 && days != 30 && days != 90 {
		fields["days"] = "Choose 7, 30 or 90."
	}
	if kind == "product" && productID == "" {
		fields["product_id"] = "Choose a product."
	}
	if len(fields) > 0 {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.", fields)
		return
	}

	ctx := c.R.Context()

	var product store.Product
	if kind == "product" {
		found, err := c.Env.Store.ProductByID(ctx, productID, false)
		if errors.Is(err, sql.ErrNoRows) {
			c.Error(http.StatusNotFound, "not_found", "That product no longer exists.")
			return
		}
		if err != nil {
			logWarn(c.Env, "ai product lookup failed", "plugin", pluginID)
			c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		product = found
	}

	settings, err := c.Settings(ctx)
	if err != nil {
		logWarn(c.Env, "ai settings could not be read", "plugin", pluginID)
		c.Error(http.StatusConflict, "plugin_settings_unreadable",
			"These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.")
		return
	}

	config, err := configFromSettings(settings)
	if errors.Is(err, errDisabled) {
		c.Error(http.StatusConflict, "plugin_disabled",
			"Enable the plugin in its settings before asking for an analysis.")
		return
	}
	if errors.Is(err, errNotConfigured) {
		c.Error(http.StatusConflict, "plugin_not_configured",
			"Open the AI plugin settings and add the API key and model.")
		return
	}

	var system, user, subject string
	switch kind {
	case "store":
		now := time.Now().UTC()
		to := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
		from := to.AddDate(0, 0, -(days - 1))
		previousTo := from.AddDate(0, 0, -1)
		previousFrom := previousTo.AddDate(0, 0, -(days - 1))

		metrics, metricsErr := c.Env.Store.Metrics(ctx, from, to, previousFrom, previousTo)
		if metricsErr != nil {
			logWarn(c.Env, "ai metrics lookup failed", "plugin", pluginID)
			c.Error(http.StatusInternalServerError, "internal_error", "The analysis could not be prepared.")
			return
		}
		system, user = buildStorePrompt(config.language, days, metrics)
		subject = fmt.Sprintf("store:%dd", days)
	case "product":
		sales := productSales{Days: 30}
		now := time.Now().UTC()
		from := now.AddDate(0, 0, -sales.Days)
		if err := c.Env.DB.QueryRowContext(ctx,
			`SELECT COALESCE(SUM(oi.quantity), 0), COALESCE(SUM(oi.line_total_cents), 0),
			        COUNT(DISTINCT oi.order_id)
			 FROM order_items oi
			 JOIN orders o ON o.id = oi.order_id
			 WHERE oi.product_id = ? AND o.status <> 'cancelled' AND o.created_at >= ?`,
			product.ID, store.FormatTime(from),
		).Scan(&sales.Units, &sales.RevenueCents, &sales.Orders); err != nil {
			logWarn(c.Env, "ai product sales lookup failed", "plugin", pluginID)
			c.Error(http.StatusInternalServerError, "internal_error", "The analysis could not be prepared.")
			return
		}
		system, user = buildProductPrompt(config.language, product, sales)
		subject = product.Name
	}

	client, err := newClient(c.Env.AIBaseURL, config.apiKey, config.model,
		&http.Client{Timeout: c.Env.AITimeout})
	if err != nil {
		c.Error(http.StatusConflict, "plugin_not_configured",
			"Open the AI plugin settings and add the API key and model.")
		return
	}

	analysis, err := client.complete(ctx, system, user)
	if err != nil {
		logUpstreamFailure(c.Env, err)
		c.Error(http.StatusBadGateway, "ai_upstream_error",
			"The AI provider did not return a usable answer. Try again shortly.")
		return
	}

	run := runDTO{
		ID:        ids.New("air"),
		Kind:      kind,
		Subject:   subject,
		Model:     config.model,
		Analysis:  analysis,
		CreatedAt: store.FormatTime(time.Now().UTC()),
	}
	if _, err := c.Env.DB.ExecContext(ctx,
		`INSERT INTO plugin_ai_runs (id, actor_user_id, kind, subject, model, analysis, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?)`,
		run.ID, c.UserID, run.Kind, run.Subject, run.Model, run.Analysis, run.CreatedAt,
	); err != nil {
		logWarn(c.Env, "ai run could not be saved", "plugin", pluginID)
		c.Error(http.StatusInternalServerError, "internal_error", "The analysis could not be saved.")
		return
	}

	c.JSON(http.StatusOK, map[string]any{"run": run})
}

func (p *Plugin) handleRuns(c *plugins.Context) {
	limit := 20
	if raw := strings.TrimSpace(c.R.URL.Query().Get("limit")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 || parsed > 50 {
			c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.",
				map[string]string{"limit": "Choose a limit from 1 to 50."})
			return
		}
		limit = parsed
	}

	rows, err := c.Env.DB.QueryContext(c.R.Context(),
		`SELECT id, kind, subject, model, analysis, created_at
		 FROM plugin_ai_runs ORDER BY created_at DESC, id DESC LIMIT ?`, limit)
	if err != nil {
		logWarn(c.Env, "ai runs lookup failed", "plugin", pluginID)
		c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	defer rows.Close()

	runs := make([]runDTO, 0, limit)
	for rows.Next() {
		var run runDTO
		if err := rows.Scan(&run.ID, &run.Kind, &run.Subject, &run.Model,
			&run.Analysis, &run.CreatedAt); err != nil {
			logWarn(c.Env, "ai run could not be read", "plugin", pluginID)
			c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		runs = append(runs, run)
	}
	if err := rows.Err(); err != nil {
		logWarn(c.Env, "ai runs could not be read", "plugin", pluginID)
		c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	c.JSON(http.StatusOK, map[string]any{"runs": runs})
}

func logWarn(env *plugins.Env, message string, attrs ...any) {
	if env == nil || env.Logger == nil {
		return
	}
	env.Logger.Warn(message, attrs...)
}

func logUpstreamFailure(env *plugins.Env, err error) {
	var upstream *UpstreamError
	if errors.As(err, &upstream) {
		logWarn(env, "ai upstream request failed",
			"plugin", pluginID, "status_class", upstream.StatusClass(), "detail", upstream.Detail())
		return
	}
	logWarn(env, "ai analysis failed", "plugin", pluginID, "detail", "unknown error")
}
