// Package codemode plans storefront, catalogue and order changes from a plain
// admin instruction and applies only the strict actions the plan validator
// accepts. The model never receives arbitrary code or tool access.
package codemode

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/store"
	"azahar/internal/storefront"
)

const (
	pluginID     = "codemode"
	defaultModel = "gpt-4o-mini"
)

var (
	modelPattern                           = regexp.MustCompile(`^[A-Za-z0-9._:/-]{1,64}$`)
	_            plugins.Plugin            = (*Plugin)(nil)
	_            plugins.Migrator          = (*Plugin)(nil)
	_            plugins.SettingsValidator = (*Plugin)(nil)
)

type Plugin struct {
	registry *plugins.Registry
}

// New accepts the plugin registry so product.create can check the installed
// product types. Passing it is optional only for tests and registration checks;
// product creation is rejected when the registry is missing.
func New(registries ...*plugins.Registry) *Plugin {
	plugin := &Plugin{}
	if len(registries) > 0 {
		plugin.registry = registries[0]
	}
	return plugin
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:          pluginID,
		Name:        "Code mode",
		Version:     "1.0.0",
		Author:      "Azahar",
		Description: "Plans and applies storefront, catalogue and order changes from an admin instruction.",
		AdminPath:   "/admin/plugins/codemode",
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
				Key:      "enabled",
				Label:    "Enable planning",
				Type:     "boolean",
				Required: true,
				Default:  "true",
			},
		},
	}
}

func (p *Plugin) ValidateSettings(settings map[string]string) map[string]string {
	fields := make(map[string]string)

	if strings.TrimSpace(settings["api_key"]) == "" {
		fields["api_key"] = "Add the API key for the AI provider."
	}
	model := strings.TrimSpace(settings["model"])
	if !modelPattern.MatchString(model) {
		fields["model"] = "Use 1 to 64 letters, numbers, dots, dashes, colons or slashes."
	}
	switch strings.TrimSpace(settings["enabled"]) {
	case "true", "false":
	default:
		fields["enabled"] = "Choose true or false."
	}
	return fields
}

func (p *Plugin) Migrations() []plugins.Migration {
	return []plugins.Migration{
		{
			Version: "001_plans.sql",
			SQL: `CREATE TABLE IF NOT EXISTS codemode_plans (
  id TEXT PRIMARY KEY,
  actor_user_id TEXT NOT NULL DEFAULT '',
  instruction TEXT NOT NULL,
  plan TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'preview',
  result TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
)`,
		},
		{
			Version: "002_plan_baseline.sql",
			SQL:     `ALTER TABLE codemode_plans ADD COLUMN baseline TEXT NOT NULL DEFAULT ''`,
		},
	}
}

func (p *Plugin) Routes() []plugins.Route {
	return []plugins.Route{
		{Method: http.MethodGet, Pattern: "/plans", Admin: true, Handler: p.handlePlans},
		{Method: http.MethodPost, Pattern: "/preview", Admin: true, Handler: p.handlePreview},
		{Method: http.MethodPost, Pattern: "/apply", Admin: true, Handler: p.handleApply},
	}
}

type planRecord struct {
	ID          string
	ActorUserID string
	Instruction string
	Plan        string
	Status      string
	Result      string
	Baseline    string
	CreatedAt   string
}

func (p *Plugin) handlePlans(c *plugins.Context) {
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
		`SELECT id, instruction, plan, status, created_at
		 FROM codemode_plans WHERE actor_user_id = ?
		 ORDER BY created_at DESC, id DESC LIMIT ?`, c.UserID, limit)
	if err != nil {
		logWarn(c.Env, "codemode plans lookup failed")
		c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	defer rows.Close()

	plans := make([]planHistoryDTO, 0, limit)
	for rows.Next() {
		var item planHistoryDTO
		var planJSON string
		if err := rows.Scan(&item.ID, &item.Instruction, &planJSON, &item.Status,
			&item.CreatedAt); err != nil {
			logWarn(c.Env, "codemode plan could not be read")
			c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		if plan, err := parsePlan(planJSON); err == nil {
			item.Summary = plan.Summary
		}
		plans = append(plans, item)
	}
	if err := rows.Err(); err != nil {
		logWarn(c.Env, "codemode plans could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	c.JSON(http.StatusOK, map[string]any{"plans": plans})
}

type previewRequest struct {
	Instruction string `json:"instruction"`
}

func recentOrders(ctx context.Context, c *plugins.Context) ([]promptOrder, error) {
	rows, err := c.Env.DB.QueryContext(ctx,
		`SELECT id, number, status, updated_at FROM orders ORDER BY created_at DESC LIMIT 20`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	orders := make([]promptOrder, 0, 20)
	for rows.Next() {
		var order promptOrder
		if err := rows.Scan(&order.ID, &order.Number, &order.Status, &order.updatedAt); err != nil {
			return nil, err
		}
		orders = append(orders, order)
	}
	return orders, rows.Err()
}

type applyRequest struct {
	PlanID string `json:"plan_id"`
}

func (p *Plugin) handlePreview(c *plugins.Context) {
	request, bad := plugins.DecodeBody[previewRequest](c)
	if bad != nil {
		c.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
		return
	}
	instruction := strings.TrimSpace(request.Instruction)
	if length := len([]rune(instruction)); length < 1 || length > 2000 || !plainText(instruction) {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.",
			map[string]string{"instruction": "Describe the change in 1 to 2000 plain characters."})
		return
	}

	ctx := c.R.Context()
	config, ok := p.configFor(c)
	if !ok {
		return
	}

	// The baseline is taken before the model sees the store. If a product, order
	// or the theme changes while the plan is being written, apply rejects it.
	baselineAt := time.Now().UTC()

	theme, err := p.loadTheme(ctx, c.Env.Store)
	if err != nil {
		logWarn(c.Env, "codemode theme could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The current theme could not be read.")
		return
	}
	products, err := c.Env.Store.ListProducts(ctx, store.ProductFilter{
		IncludeInactive: true,
		Limit:           500,
	})
	if err != nil {
		logWarn(c.Env, "codemode products could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The product list could not be read.")
		return
	}
	categories, err := c.Env.Store.Categories(ctx)
	if err != nil {
		logWarn(c.Env, "codemode categories could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The category list could not be read.")
		return
	}
	orders, err := recentOrders(ctx, c)
	if err != nil {
		logWarn(c.Env, "codemode orders could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The order list could not be read.")
		return
	}

	system := buildPrompt(theme, products, categories, orders, p.promptPlugins())
	user := instruction

	client, err := newClient(c.Env.AIBaseURL, config.apiKey, config.model,
		&http.Client{Timeout: c.Env.AITimeout})
	if err != nil {
		c.Error(http.StatusConflict, "plugin_not_configured",
			"Open the Code mode settings and add the API key and model.")
		return
	}

	content, err := client.plan(ctx, system, user)
	if err != nil {
		logUpstreamFailure(c.Env, err)
		c.Error(http.StatusBadGateway, "ai_upstream_error",
			"The AI provider did not return a usable plan. Try again shortly.")
		return
	}

	plan, err := parsePlan(content)
	if err != nil {
		c.Problem(http.StatusUnprocessableEntity, "invalid_plan",
			"The AI provider returned a plan this server cannot apply. Try rephrasing the instruction.", nil)
		return
	}

	if _, err := p.validatePlanShape(plan, theme); err != nil {
		c.Problem(http.StatusUnprocessableEntity, "invalid_plan", err.Error(), nil)
		return
	}
	if err := p.validatePlanReferences(ctx, c.Env.Store, plan); err != nil {
		var invalid *invalidPlanError
		if errors.As(err, &invalid) {
			c.Problem(http.StatusUnprocessableEntity, "invalid_plan", invalid.Error(), nil)
			return
		}
		logWarn(c.Env, "codemode plan references could not be checked")
		c.Error(http.StatusInternalServerError, "internal_error", "The plan could not be checked.")
		return
	}

	encoded, err := encodePlan(plan)
	if err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "The plan could not be saved.")
		return
	}
	baseline, err := p.planBaseline(plan, products, orders, theme)
	if err != nil {
		var invalid *invalidPlanError
		if errors.As(err, &invalid) {
			c.Problem(http.StatusUnprocessableEntity, "invalid_plan", invalid.Error(), nil)
			return
		}
		logWarn(c.Env, "codemode plan baseline could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The plan could not be saved.")
		return
	}

	record := planRecord{
		ID:          newPlanID(),
		ActorUserID: c.UserID,
		Instruction: instruction,
		Plan:        encoded,
		Status:      "preview",
		Baseline:    baseline,
		CreatedAt:   store.FormatTime(baselineAt),
	}
	if _, err := c.Env.DB.ExecContext(ctx,
		`INSERT INTO codemode_plans (id, actor_user_id, instruction, plan, status, result, baseline, created_at)
		 VALUES (?, ?, ?, ?, ?, '', ?, ?)`,
		record.ID, record.ActorUserID, record.Instruction, record.Plan, record.Status,
		record.Baseline, record.CreatedAt,
	); err != nil {
		logWarn(c.Env, "codemode plan could not be saved")
		c.Error(http.StatusInternalServerError, "internal_error", "The plan could not be saved.")
		return
	}

	c.JSON(http.StatusOK, map[string]any{"plan": planDTO(record.ID, plan, record.Status, record.CreatedAt)})
}

func (p *Plugin) handleApply(c *plugins.Context) {
	request, bad := plugins.DecodeBody[applyRequest](c)
	if bad != nil {
		c.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
		return
	}
	planID := strings.TrimSpace(request.PlanID)
	if planID == "" || len(planID) > 64 {
		c.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.",
			map[string]string{"plan_id": "Choose a saved preview plan."})
		return
	}

	ctx := c.R.Context()
	record, err := p.loadPlanRecord(ctx, c, planID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			c.Error(http.StatusNotFound, "not_found", "That plan no longer exists.")
			return
		}
		logWarn(c.Env, "codemode plan could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The plan could not be read.")
		return
	}
	if record.Status != "preview" {
		c.Error(http.StatusConflict, "plan_already_applied", "This plan has already been applied.")
		return
	}
	if !p.ensureEnabled(c) {
		return
	}

	plan, err := parsePlan(record.Plan)
	if err != nil {
		p.failPlan(ctx, c, planID, []actionResult{})
		c.Problem(http.StatusUnprocessableEntity, "invalid_plan",
			"The saved plan is no longer valid. Preview the change again.", nil)
		return
	}

	theme, err := p.loadTheme(ctx, c.Env.Store)
	if err != nil {
		logWarn(c.Env, "codemode theme could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The current theme could not be read.")
		return
	}
	if _, err := p.validatePlanShape(plan, theme); err != nil {
		p.failPlan(ctx, c, planID, []actionResult{})
		c.Problem(http.StatusUnprocessableEntity, "invalid_plan", err.Error(), nil)
		return
	}
	currentTheme := theme
	if err := p.validatePlanReferences(ctx, c.Env.Store, plan); err != nil {
		var invalid *invalidPlanError
		if errors.As(err, &invalid) {
			p.failPlan(ctx, c, planID, []actionResult{})
			c.Problem(http.StatusUnprocessableEntity, "invalid_plan", invalid.Error(), nil)
			return
		}
		logWarn(c.Env, "codemode plan references could not be checked")
		c.Error(http.StatusInternalServerError, "internal_error", "The plan could not be checked.")
		return
	}

	baseline := make(map[string]string)
	if strings.TrimSpace(record.Baseline) == "" || json.Unmarshal([]byte(record.Baseline), &baseline) != nil {
		p.failPlan(ctx, c, planID, []actionResult{})
		c.Problem(http.StatusConflict, "plan_stale",
			"This preview has no readable baseline. Preview the change again.", nil)
		return
	}

	results := make([]actionResult, 0, len(plan.Actions))
	now := time.Now().UTC()
	applyErr := c.Env.Store.InTx(ctx, func(tx *sql.Tx) error {
		claimed, err := claimPlan(ctx, tx, planID, c.UserID)
		if err != nil {
			return err
		}
		if !claimed {
			return errAlreadyApplied
		}

		// Theme changes are checked once inside the write transaction and every
		// theme action in the plan is applied to that same current theme.
		for _, action := range plan.Actions {
			if action.Type != "theme.update" {
				continue
			}
			current, err := p.loadThemeTx(ctx, tx, c)
			if err != nil {
				return err
			}
			encoded, err := storefront.Encode(current)
			if err != nil {
				return err
			}
			if encoded != baseline["theme"] {
				return errStalePlan
			}
			currentTheme = current
			break
		}

		for _, action := range plan.Actions {
			message, nextTheme, err := p.applyAction(ctx, tx, c, planID, action, currentTheme, baseline, now)
			if err != nil {
				results = append(results, actionResult{
					Type:    action.Type,
					OK:      false,
					Message: err.Error(),
				})
				return err
			}
			currentTheme = nextTheme
			results = append(results, actionResult{
				Type:    action.Type,
				OK:      true,
				Message: message,
			})
		}

		status := "applied"
		encodedResults, _ := encodeResults(results)
		_, err = tx.ExecContext(ctx,
			`UPDATE codemode_plans SET status = ?, result = ? WHERE id = ?`,
			status, encodedResults, planID)
		return err
	})

	if errors.Is(applyErr, errAlreadyApplied) {
		c.Error(http.StatusConflict, "plan_already_applied", "This plan has already been applied.")
		return
	}
	if errors.Is(applyErr, errStalePlan) {
		p.failPlan(ctx, c, planID, results)
		c.Problem(http.StatusConflict, "plan_stale",
			"The store changed after this preview. Preview the change again.", nil)
		return
	}
	if applyErr != nil {
		for index := range results {
			if results[index].OK {
				results[index].OK = false
				results[index].Message = "Rolled back because a later action failed. " + results[index].Message
			}
		}
		p.failPlan(ctx, c, planID, results)
		updated, loadErr := p.loadPlanRecord(ctx, c, planID)
		if loadErr != nil {
			c.Error(http.StatusInternalServerError, "internal_error", "The plan failed and its result could not be saved.")
			return
		}
		c.JSON(http.StatusOK, map[string]any{
			"plan":    planDTO(updated.ID, plan, updated.Status, updated.CreatedAt),
			"results": results,
		})
		return
	}

	updated, err := p.loadPlanRecord(ctx, c, planID)
	if err != nil {
		logWarn(c.Env, "codemode applied plan could not be read")
		c.Error(http.StatusInternalServerError, "internal_error", "The plan was applied but could not be read back.")
		return
	}
	c.JSON(http.StatusOK, map[string]any{
		"plan":    planDTO(updated.ID, plan, updated.Status, updated.CreatedAt),
		"results": results,
	})
}

func (p *Plugin) ensureEnabled(c *plugins.Context) bool {
	settings, err := c.Settings(c.R.Context())
	if err != nil {
		logWarn(c.Env, "codemode settings could not be read")
		c.Error(http.StatusConflict, "plugin_settings_unreadable",
			"These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.")
		return false
	}
	if !enabledFromSettings(settings) {
		c.Error(http.StatusConflict, "plugin_disabled",
			"Enable the plugin in its settings before applying a plan.")
		return false
	}
	return true
}

func (p *Plugin) configFor(c *plugins.Context) (settingsConfig, bool) {
	settings, err := c.Settings(c.R.Context())
	if err != nil {
		logWarn(c.Env, "codemode settings could not be read")
		c.Error(http.StatusConflict, "plugin_settings_unreadable",
			"These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.")
		return settingsConfig{}, false
	}
	config, err := configFromSettings(settings)
	if errors.Is(err, errDisabled) {
		c.Error(http.StatusConflict, "plugin_disabled",
			"Enable the plugin in its settings before planning a change.")
		return settingsConfig{}, false
	}
	if errors.Is(err, errNotConfigured) {
		c.Error(http.StatusConflict, "plugin_not_configured",
			"Open the Code mode settings and add the API key and model.")
		return settingsConfig{}, false
	}
	return config, true
}

func (p *Plugin) loadTheme(ctx context.Context, st *store.Store) (storefront.Theme, error) {
	if st == nil {
		return storefront.Default(), nil
	}
	raw, _, err := st.StorefrontSettings(ctx)
	if errors.Is(err, sql.ErrNoRows) {
		return storefront.Default(), nil
	}
	if err != nil {
		return storefront.Theme{}, err
	}
	theme, err := storefront.Parse(raw)
	if err != nil {
		return storefront.Theme{}, err
	}
	theme.UpdatedAt = ""
	return theme, nil
}

func (p *Plugin) loadThemeTx(ctx context.Context, tx *sql.Tx, c *plugins.Context) (storefront.Theme, error) {
	raw, _, err := c.Env.Store.StorefrontSettingsTx(ctx, tx)
	if errors.Is(err, sql.ErrNoRows) {
		return storefront.Default(), nil
	}
	if err != nil {
		return storefront.Theme{}, err
	}
	theme, err := storefront.Parse(raw)
	if err != nil {
		return storefront.Theme{}, err
	}
	theme.UpdatedAt = ""
	return theme, nil
}

func (p *Plugin) loadPlanRecord(ctx context.Context, c *plugins.Context, planID string) (planRecord, error) {
	var record planRecord
	err := c.Env.DB.QueryRowContext(ctx,
		`SELECT id, actor_user_id, instruction, plan, status, result, baseline, created_at
		 FROM codemode_plans WHERE id = ? AND actor_user_id = ?`, planID, c.UserID,
	).Scan(&record.ID, &record.ActorUserID, &record.Instruction, &record.Plan,
		&record.Status, &record.Result, &record.Baseline, &record.CreatedAt)
	return record, err
}

func (p *Plugin) failPlan(ctx context.Context, c *plugins.Context, planID string, results []actionResult) {
	encoded, _ := encodeResults(results)
	if _, err := c.Env.DB.ExecContext(ctx,
		`UPDATE codemode_plans SET status = 'failed', result = ?
		 WHERE id = ? AND actor_user_id = ? AND status IN ('preview', 'applying')`,
		encoded, planID, c.UserID); err != nil {
		logWarn(c.Env, "codemode failed plan could not be saved")
	}
}

func claimPlan(ctx context.Context, tx *sql.Tx, planID, actorID string) (bool, error) {
	result, err := tx.ExecContext(ctx,
		`UPDATE codemode_plans SET status = 'applying'
		 WHERE id = ? AND actor_user_id = ? AND status = 'preview'`, planID, actorID)
	if err != nil {
		return false, err
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return false, err
	}
	return affected == 1, nil
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
		logWarn(env, "codemode upstream request failed",
			"plugin", pluginID, "status_class", upstream.StatusClass(), "detail", upstream.Detail())
		return
	}
	logWarn(env, "codemode planning failed", "plugin", pluginID, "detail", "unknown error")
}
