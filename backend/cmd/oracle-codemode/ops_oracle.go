//go:build oracle

package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"

	"azahar/internal/db"
	"azahar/internal/plugins"
	"azahar/internal/plugins/codemode"
	"azahar/internal/store"
	"azahar/internal/storefront"
)

type oraclePlugin struct {
	def pluginInput
}

func (p oraclePlugin) Manifest() plugins.Manifest {
	manifest := plugins.Manifest{ID: p.def.ID, Name: p.def.ID, Version: "1.0.0"}
	if p.def.Label != "" {
		fields := make([]plugins.OptionField, 0, len(p.def.Fields))
		for _, field := range p.def.Fields {
			fields = append(fields, plugins.OptionField{
				Key:      field.Key,
				Label:    field.Label,
				Type:     field.Type,
				Required: field.Required,
			})
		}
		manifest.ProductType = &plugins.ProductType{Label: p.def.Label, Fields: fields}
	}
	return manifest
}

func (oraclePlugin) Routes() []plugins.Route { return nil }

func buildRegistry(defs []pluginInput) (*plugins.Registry, error) {
	registry := plugins.NewRegistry()
	for _, def := range defs {
		if err := registry.Register(oraclePlugin{def: def}); err != nil {
			return nil, err
		}
	}
	return registry, nil
}

func themeFrom(raw json.RawMessage) (storefront.Theme, error) {
	if len(raw) == 0 {
		return storefront.Default(), nil
	}
	return storefront.Parse(string(raw))
}

func encodeTheme(theme storefront.Theme) (string, error) {
	return storefront.Encode(theme)
}

func errorResult(err error) map[string]any {
	return map[string]any{"ok": false, "error": err.Error()}
}

func handle(ctx context.Context, req request) map[string]any {
	switch req.Op {
	case "parse_plan":
		return opParsePlan(req)
	case "parse_action":
		return opParseAction(req)
	case "validate_shape":
		return opValidateShape(req)
	case "theme_patch":
		return opThemePatch(req)
	case "theme_object":
		return opThemeObject(req)
	case "product_patch":
		return opProductPatch(req)
	case "product_draft":
		return opProductDraft(req)
	case "normalize_attributes":
		return opNormalizeAttributes(req)
	case "product_attributes":
		return opProductAttributes(req)
	case "first_field":
		return opFirstField(req)
	case "fingerprint":
		return opFingerprint(req)
	case "baseline":
		return opBaseline(req)
	case "prompt":
		return opPrompt(req)
	case "prompt_plugins":
		return opPromptPlugins(req)
	case "validate_settings":
		return opValidateSettings(req)
	case "config":
		return opConfig(req)
	case "manifest":
		return opManifest()
	case "migrations":
		return opMigrations()
	case "apply":
		return opApply(ctx, req)
	default:
		return map[string]any{"error": fmt.Sprintf("unknown op %q", req.Op)}
	}
}

func opParsePlan(req request) map[string]any {
	plan, err := codemode.OracleParsePlan(req.Raw)
	if err != nil {
		return errorResult(err)
	}
	encoded, err := codemode.OracleEncodePlan(plan)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "plan": encoded}
}

func opParseAction(req request) map[string]any {
	action, err := codemode.OracleParseAction([]byte(req.Raw))
	if err != nil {
		return errorResult(err)
	}
	encoded, err := json.Marshal(action)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "action": string(encoded)}
}

func opValidateShape(req request) map[string]any {
	plan, err := codemode.OracleParsePlan(req.Plan)
	if err != nil {
		return errorResult(err)
	}
	theme, err := themeFrom(req.Theme)
	if err != nil {
		return errorResult(err)
	}
	registry, err := buildRegistry(req.Plugins)
	if err != nil {
		return errorResult(err)
	}
	plugin := codemode.New(registry)
	next, err := codemode.OracleValidatePlanShape(plugin, &plan, theme)
	encodedPlan, encodeErr := codemode.OracleEncodePlan(plan)
	if encodeErr != nil {
		return errorResult(encodeErr)
	}
	encodedTheme, themeErr := encodeTheme(next)
	if themeErr != nil {
		return errorResult(themeErr)
	}
	if err != nil {
		return map[string]any{"ok": false, "error": err.Error(), "plan": encodedPlan, "theme": encodedTheme}
	}
	return map[string]any{"ok": true, "plan": encodedPlan, "theme": encodedTheme}
}

func opThemePatch(req request) map[string]any {
	theme, err := themeFrom(req.Theme)
	if err != nil {
		return errorResult(err)
	}
	next, err := codemode.OracleApplyThemePatch(theme, req.Patch)
	if err != nil {
		return errorResult(err)
	}
	encoded, err := encodeTheme(next)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "theme": encoded}
}

func opThemeObject(req request) map[string]any {
	theme, err := themeFrom(req.Theme)
	if err != nil {
		return errorResult(err)
	}
	object, err := codemode.OracleThemeObject(theme)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "object": object}
}

func opProductPatch(req request) map[string]any {
	patch, err := codemode.OracleParseProductPatch(req.Patch)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "patch": patch}
}

func opProductDraft(req request) map[string]any {
	var draft codemode.ProductDraft
	if err := json.Unmarshal(req.Patch, &draft); err != nil {
		return errorResult(err)
	}
	registry, err := buildRegistry(req.Plugins)
	if err != nil {
		return errorResult(err)
	}
	plugin := codemode.New(registry)
	if err := codemode.OracleValidateProductDraft(plugin, &draft); err != nil {
		return errorResult(err)
	}
	encoded, err := json.Marshal(draft)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "draft": string(encoded)}
}

func opNormalizeAttributes(req request) map[string]any {
	normalized, err := codemode.OracleNormalizeAttributes(req.Attributes)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "attributes": normalized}
}

func opProductAttributes(req request) map[string]any {
	productType := &plugins.ProductType{}
	if req.ProductType != nil {
		fields := make([]plugins.OptionField, 0, len(req.ProductType.Fields))
		for _, field := range req.ProductType.Fields {
			fields = append(fields, plugins.OptionField{
				Key:      field.Key,
				Label:    field.Label,
				Type:     field.Type,
				Required: field.Required,
			})
		}
		productType = &plugins.ProductType{Label: req.ProductType.Label, Fields: fields}
	}
	if err := codemode.OracleValidateProductAttributes(productType, req.Attributes); err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true}
}

func opFirstField(req request) map[string]any {
	return map[string]any{"message": codemode.OracleFirstFieldMessage(req.Fields)}
}

func opFingerprint(req request) map[string]any {
	if len(req.Products) == 0 {
		return map[string]any{"error": "one product is required"}
	}
	return map[string]any{"fingerprint": codemode.OracleProductFingerprint(productFromWire(req.Products[0]))}
}

func opBaseline(req request) map[string]any {
	plan, err := codemode.OracleParsePlan(req.Plan)
	if err != nil {
		return errorResult(err)
	}
	theme, err := themeFrom(req.Theme)
	if err != nil {
		return errorResult(err)
	}
	products := make([]store.Product, 0, len(req.Products))
	for _, product := range req.Products {
		products = append(products, productFromWire(product))
	}
	orders := make([]codemode.OraclePromptOrder, 0, len(req.Orders))
	for _, order := range req.Orders {
		orders = append(orders, codemode.OraclePromptOrder{ID: order.ID, Number: order.Number, Status: order.Status})
	}
	registry, err := buildRegistry(req.Plugins)
	if err != nil {
		return errorResult(err)
	}
	baseline, err := codemode.OraclePlanBaseline(codemode.New(registry), plan, products, orders, theme)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"ok": true, "baseline": baseline}
}

func opPrompt(req request) map[string]any {
	theme, err := themeFrom(req.Theme)
	if err != nil {
		return errorResult(err)
	}
	products := make([]store.Product, 0, len(req.Products))
	for _, product := range req.Products {
		products = append(products, productFromWire(product))
	}
	categories := make([]store.Category, 0, len(req.Categories))
	for _, category := range req.Categories {
		categories = append(categories, store.Category{Slug: category.Slug, PluginID: category.PluginID})
	}
	orders := make([]codemode.OraclePromptOrder, 0, len(req.Orders))
	for _, order := range req.Orders {
		orders = append(orders, codemode.OraclePromptOrder{ID: order.ID, Number: order.Number, Status: order.Status})
	}
	registry, err := buildRegistry(req.Plugins)
	if err != nil {
		return errorResult(err)
	}
	plugin := codemode.New(registry)
	prompt := codemode.OracleBuildPrompt(theme, products, categories, orders, codemode.OraclePromptPlugins(plugin))
	return map[string]any{"prompt": prompt}
}

func opPromptPlugins(req request) map[string]any {
	registry, err := buildRegistry(req.Plugins)
	if err != nil {
		return errorResult(err)
	}
	return map[string]any{"plugins": codemode.OraclePromptPlugins(codemode.New(registry))}
}

func opValidateSettings(req request) map[string]any {
	return map[string]any{"fields": codemode.OracleValidateSettings(codemode.New(), req.Settings)}
}

func opConfig(req request) map[string]any {
	apiKey, model, err := codemode.OracleConfigFromSettings(req.Settings)
	if err != nil {
		return map[string]any{
			"ok":      false,
			"error":   err.Error(),
			"enabled": codemode.OracleEnabledFromSettings(req.Settings),
		}
	}
	return map[string]any{
		"ok":      true,
		"enabled": codemode.OracleEnabledFromSettings(req.Settings),
		"api_key": apiKey,
		"model":   model,
	}
}

func opManifest() map[string]any {
	return map[string]any{"manifest": codemode.OracleManifest(codemode.New())}
}

func opMigrations() map[string]any {
	return map[string]any{"migrations": codemode.OracleMigrations(codemode.New())}
}

func productFromWire(product wireProduct) store.Product {
	return store.Product{
		ID:          product.ID,
		Slug:        product.Slug,
		Name:        product.Name,
		Subtitle:    product.Subtitle,
		Description: product.Description,
		PriceCents:  product.PriceCents,
		Stock:       product.Stock,
		Active:      product.Active,
		PluginID:    product.PluginID,
		Category:    store.Category{Slug: product.CategorySlug},
	}
}

func opApply(ctx context.Context, req request) map[string]any {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		dsn = defaultDSN
	}
	handleDB, err := db.Open(ctx, dsn)
	if err != nil {
		return map[string]any{"error": fmt.Sprintf("open database: %v", err)}
	}
	defer handleDB.Close()

	registry, err := buildRegistry(req.Plugins)
	if err != nil {
		return errorResult(err)
	}
	plugin := codemode.New(registry)
	environment := &plugins.Env{
		DB:        handleDB,
		Store:     store.New(handleDB),
		SecretKey: req.SecretKey,
	}
	recorder := httptest.NewRecorder()
	body := fmt.Sprintf(`{"plan_id":%q}`, req.PlanID)
	httpRequest := httptest.NewRequest(http.MethodPost, "/apply", strings.NewReader(body)).WithContext(ctx)
	httpRequest.Header.Set("Content-Type", "application/json")
	pluginContext := &plugins.Context{
		W:        recorder,
		R:        httpRequest,
		Env:      environment,
		PluginID: "codemode",
		UserID:   req.Actor,
		UserRole: "admin",
	}
	codemode.OracleHandleApply(plugin, pluginContext)

	state, err := dumpState(ctx, handleDB, req.PlanID)
	if err != nil {
		return map[string]any{"error": err.Error()}
	}
	return map[string]any{
		"status": recorder.Code,
		"body":   recorder.Body.String(),
		"state":  state,
	}
}

func dumpState(ctx context.Context, handleDB *sql.DB, planID string) (map[string]any, error) {
	state := map[string]any{
		"plan_status": "",
		"plan_result": "",
		"theme":       "",
		"products":    []map[string]any{},
		"orders":      []map[string]any{},
		"audits":      []map[string]any{},
	}

	var planStatus, planResult sql.NullString
	if err := handleDB.QueryRowContext(ctx,
		"SELECT status, result FROM codemode_plans WHERE id = ?", planID,
	).Scan(&planStatus, &planResult); err != nil && err != sql.ErrNoRows {
		return nil, fmt.Errorf("read plan: %w", err)
	}
	if planStatus.Valid {
		state["plan_status"] = planStatus.String
	}
	if planResult.Valid {
		state["plan_result"] = planResult.String
	}

	var theme sql.NullString
	if err := handleDB.QueryRowContext(ctx,
		"SELECT data FROM storefront_settings WHERE id = 'default'",
	).Scan(&theme); err != nil && err != sql.ErrNoRows {
		return nil, fmt.Errorf("read theme: %w", err)
	}
	if theme.Valid {
		state["theme"] = theme.String
	}

	productRows, err := handleDB.QueryContext(ctx,
		`SELECT p.id, p.slug, p.name, p.subtitle, p.description, p.price_cents, p.stock,
		        p.active, p.plugin_id, p.image, p.attributes, c.slug
		 FROM products p JOIN categories c ON c.id = p.category_id
		 WHERE p.slug LIKE 'itcm-%' ORDER BY p.slug`)
	if err != nil {
		return nil, fmt.Errorf("read products: %w", err)
	}
	defer productRows.Close()
	products := make([]map[string]any, 0)
	for productRows.Next() {
		var id, slug, name, subtitle, description, pluginID, image, attributes, categorySlug string
		var priceCents int64
		var stock int
		var active int
		if err := productRows.Scan(&id, &slug, &name, &subtitle, &description, &priceCents,
			&stock, &active, &pluginID, &image, &attributes, &categorySlug); err != nil {
			return nil, fmt.Errorf("scan product: %w", err)
		}
		var decodedAttributes any
		if err := json.Unmarshal([]byte(attributes), &decodedAttributes); err != nil {
			decodedAttributes = attributes
		}
		products = append(products, map[string]any{
			"id":            id,
			"slug":          slug,
			"name":          name,
			"subtitle":      subtitle,
			"description":   description,
			"price_cents":   priceCents,
			"stock":         stock,
			"active":        active != 0,
			"plugin_id":     pluginID,
			"image":         image,
			"attributes":    decodedAttributes,
			"category_slug": categorySlug,
		})
	}
	if err := productRows.Err(); err != nil {
		return nil, fmt.Errorf("read products: %w", err)
	}
	state["products"] = products

	orderRows, err := handleDB.QueryContext(ctx,
		"SELECT id, number, status FROM orders WHERE id LIKE 'itcm_%' ORDER BY id")
	if err != nil {
		return nil, fmt.Errorf("read orders: %w", err)
	}
	defer orderRows.Close()
	orders := make([]map[string]any, 0)
	for orderRows.Next() {
		var id, number, status string
		if err := orderRows.Scan(&id, &number, &status); err != nil {
			return nil, fmt.Errorf("scan order: %w", err)
		}
		orders = append(orders, map[string]any{"id": id, "number": number, "status": status})
	}
	if err := orderRows.Err(); err != nil {
		return nil, fmt.Errorf("read orders: %w", err)
	}
	state["orders"] = orders

	auditRows, err := handleDB.QueryContext(ctx,
		`SELECT action, entity_type, entity_id, detail FROM audit_outbox
		 WHERE detail LIKE '%cmp_itcm_%' ORDER BY action, entity_id`)
	if err != nil {
		return nil, fmt.Errorf("read audits: %w", err)
	}
	defer auditRows.Close()
	audits := make([]map[string]any, 0)
	for auditRows.Next() {
		var action, entityType, entityID, detail string
		if err := auditRows.Scan(&action, &entityType, &entityID, &detail); err != nil {
			return nil, fmt.Errorf("scan audit: %w", err)
		}
		audits = append(audits, map[string]any{
			"action":      action,
			"entity_type": entityType,
			"entity_id":   entityID,
			"detail":      detail,
		})
	}
	if err := auditRows.Err(); err != nil {
		return nil, fmt.Errorf("read audits: %w", err)
	}
	state["audits"] = audits

	return state, nil
}
