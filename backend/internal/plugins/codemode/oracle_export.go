//go:build oracle

// Oracle-only exports of the code-mode helpers for the Rust differential
// tests. The `oracle` build tag keeps these adapters out of the normal API
// binary.
package codemode

import (
	"context"
	"database/sql"
	"encoding/json"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/store"
	"azahar/internal/storefront"
)

// OraclePromptOrder aliases the unexported prompt order row.
type OraclePromptOrder = promptOrder

// OraclePromptPlugin aliases the unexported prompt plugin row.
type OraclePromptPlugin = promptPlugin

// OraclePromptField aliases the unexported prompt option field.
type OraclePromptField = promptField

// OracleManifest exposes the plugin manifest.
func OracleManifest(p *Plugin) plugins.Manifest { return p.Manifest() }

// OracleMigrations exposes the plugin migrations.
func OracleMigrations(p *Plugin) []plugins.Migration { return p.Migrations() }

// OracleValidateSettings exposes the settings validator.
func OracleValidateSettings(p *Plugin, settings map[string]string) map[string]string {
	return p.ValidateSettings(settings)
}

// OracleParsePlan exposes parsePlan.
func OracleParsePlan(raw string) (Plan, error) { return parsePlan(raw) }

// OracleEncodePlan exposes encodePlan.
func OracleEncodePlan(plan Plan) (string, error) { return encodePlan(plan) }

// OracleParseAction exposes parseAction.
func OracleParseAction(raw []byte) (Action, error) { return parseAction(raw) }

// OracleValidatePlanShape exposes validatePlanShape on a mutable plan.
func OracleValidatePlanShape(p *Plugin, plan *Plan, theme storefront.Theme) (storefront.Theme, error) {
	return p.validatePlanShape(*plan, theme)
}

// OracleApplyThemePatch exposes applyThemePatch.
func OracleApplyThemePatch(theme storefront.Theme, raw json.RawMessage) (storefront.Theme, error) {
	return applyThemePatch(theme, raw)
}

// OracleThemeObject exposes themeObject.
func OracleThemeObject(theme storefront.Theme) (map[string]json.RawMessage, error) {
	return themeObject(theme)
}

// OracleParseProductPatch returns the patch fields that are set.
func OracleParseProductPatch(raw json.RawMessage) (map[string]any, error) {
	patch, err := parseProductPatch(raw)
	if err != nil {
		return nil, err
	}
	out := make(map[string]any)
	if patch.Name != nil {
		out["name"] = *patch.Name
	}
	if patch.Subtitle != nil {
		out["subtitle"] = *patch.Subtitle
	}
	if patch.Description != nil {
		out["description"] = *patch.Description
	}
	if patch.PriceCents != nil {
		out["price_cents"] = *patch.PriceCents
	}
	if patch.Stock != nil {
		out["stock"] = *patch.Stock
	}
	if patch.Active != nil {
		out["active"] = *patch.Active
	}
	return out, nil
}

// OracleValidateProductDraft exposes validateProductDraft.
func OracleValidateProductDraft(p *Plugin, draft *ProductDraft) error {
	return p.validateProductDraft(draft)
}

// OracleNormalizeAttributes exposes normalizeAttributes.
func OracleNormalizeAttributes(attributes map[string][]string) (map[string][]string, error) {
	return normalizeAttributes(attributes)
}

// OracleValidateProductAttributes exposes validateProductAttributes.
func OracleValidateProductAttributes(productType *plugins.ProductType, attributes map[string][]string) error {
	return validateProductAttributes(productType, attributes)
}

// OracleFirstFieldMessage exposes firstFieldMessage.
func OracleFirstFieldMessage(fields map[string]string) string { return firstFieldMessage(fields) }

// OracleProductFingerprint exposes productFingerprint.
func OracleProductFingerprint(product store.Product) string { return productFingerprint(product) }

// OraclePlanBaseline exposes planBaseline.
func OraclePlanBaseline(p *Plugin, plan Plan, products []store.Product, orders []promptOrder, theme storefront.Theme) (string, error) {
	return p.planBaseline(plan, products, orders, theme)
}

// OracleValidatePlanReferences exposes validatePlanReferences.
func OracleValidatePlanReferences(ctx context.Context, st *store.Store, plan Plan) error {
	return (&Plugin{}).validatePlanReferences(ctx, st, plan)
}

// OraclePromptPlugins exposes promptPlugins.
func OraclePromptPlugins(p *Plugin) []promptPlugin { return p.promptPlugins() }

// OracleBuildPrompt exposes buildPrompt.
func OracleBuildPrompt(theme storefront.Theme, products []store.Product, categories []store.Category, orders []promptOrder, plugins []promptPlugin) string {
	return buildPrompt(theme, products, categories, orders, plugins)
}

// OracleLoadTheme exposes loadTheme; a nil store yields the default theme.
func OracleLoadTheme(ctx context.Context, st *store.Store) (storefront.Theme, error) {
	return (&Plugin{}).loadTheme(ctx, st)
}

// OracleLoadThemeTx exposes loadThemeTx.
func OracleLoadThemeTx(ctx context.Context, tx *sql.Tx, c *plugins.Context) (storefront.Theme, error) {
	return (&Plugin{}).loadThemeTx(ctx, tx, c)
}

// OracleConfigFromSettings exposes configFromSettings.
func OracleConfigFromSettings(settings map[string]string) (apiKey, model string, err error) {
	config, err := configFromSettings(settings)
	return config.apiKey, config.model, err
}

// OracleEnabledFromSettings exposes enabledFromSettings.
func OracleEnabledFromSettings(settings map[string]string) bool {
	return enabledFromSettings(settings)
}

// OracleHandleApply runs the real HTTP handler against c.
func OracleHandleApply(p *Plugin, c *plugins.Context) { p.handleApply(c) }

// OracleHandlePlans runs the real plans handler against c.
func OracleHandlePlans(p *Plugin, c *plugins.Context) { p.handlePlans(c) }

// OracleHandlePreview runs the real preview handler against c.
func OracleHandlePreview(p *Plugin, c *plugins.Context) { p.handlePreview(c) }

// OracleNewPlanID exposes newPlanID.
func OracleNewPlanID() string { return newPlanID() }

// OracleFormatTime exposes store.FormatTime for dump parity.
func OracleFormatTime(value time.Time) string { return store.FormatTime(value) }
