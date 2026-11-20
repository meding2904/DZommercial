package codemode

import (
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/storefront"
)

type testProductPlugin struct{}

func (testProductPlugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:      "oils",
		Name:    "Oil shelf",
		Version: "1.0.0",
		ProductType: &plugins.ProductType{
			Label: "Oil",
			Fields: []plugins.OptionField{
				{Key: "size", Label: "Size", Type: "text", Required: true},
				{Key: "concerns", Label: "Concerns", Type: "list"},
			},
		},
	}
}

func (testProductPlugin) Routes() []plugins.Route { return nil }

func testRegistry(t *testing.T) *plugins.Registry {
	t.Helper()
	registry := plugins.NewRegistry()
	if err := registry.Register(testProductPlugin{}); err != nil {
		t.Fatalf("register test plugin: %v", err)
	}
	return registry
}

func TestPluginRegisters(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(New(registry)); err != nil {
		t.Fatalf("register plugin: %v", err)
	}
}

func TestManifest(t *testing.T) {
	manifest := New().Manifest()
	if manifest.ID != "codemode" {
		t.Fatalf("id = %q", manifest.ID)
	}
	if manifest.Name != "Code mode" {
		t.Fatalf("name = %q", manifest.Name)
	}
	if manifest.Version != "1.0.0" {
		t.Fatalf("version = %q", manifest.Version)
	}
	if manifest.AdminPath != "/admin/plugins/codemode" {
		t.Fatalf("admin path = %q", manifest.AdminPath)
	}
	if manifest.Description != "Plans and applies storefront, catalogue and order changes from an admin instruction." {
		t.Fatalf("description = %q", manifest.Description)
	}
	if len(manifest.Settings) != 3 {
		t.Fatalf("settings = %d, want 3", len(manifest.Settings))
	}
	byKey := make(map[string]plugins.SettingField)
	for _, field := range manifest.Settings {
		byKey[field.Key] = field
	}
	if field := byKey["api_key"]; field.Type != "secret" || !field.Required || field.MaxLength != 512 {
		t.Fatalf("api_key = %#v", field)
	}
	if field := byKey["model"]; field.Default != "gpt-4o-mini" || field.MaxLength != 64 || !field.Required {
		t.Fatalf("model = %#v", field)
	}
	if field := byKey["enabled"]; field.Type != "boolean" || field.Default != "true" || !field.Required {
		t.Fatalf("enabled = %#v", field)
	}
}

func TestMigrationsCreatePlanTables(t *testing.T) {
	migrations := New().Migrations()
	if len(migrations) != 2 {
		t.Fatalf("migrations = %d, want 2", len(migrations))
	}
	migration := migrations[0]
	if migration.Version != "001_plans.sql" {
		t.Fatalf("version = %q", migration.Version)
	}
	for _, column := range []string{
		"id TEXT PRIMARY KEY",
		"actor_user_id TEXT NOT NULL DEFAULT ''",
		"instruction TEXT NOT NULL",
		"plan TEXT NOT NULL",
		"status TEXT NOT NULL DEFAULT 'preview'",
		"result TEXT NOT NULL DEFAULT ''",
		"created_at TEXT NOT NULL",
	} {
		if !strings.Contains(migration.SQL, column) {
			t.Fatalf("migration is missing %q", column)
		}
	}
	if strings.Contains(migration.SQL, ";") {
		t.Fatal("migration should be one statement without semicolons")
	}

	baseline := migrations[1]
	if baseline.Version != "002_plan_baseline.sql" {
		t.Fatalf("version = %q", baseline.Version)
	}
	if !strings.Contains(baseline.SQL, "baseline TEXT NOT NULL DEFAULT ''") {
		t.Fatalf("baseline migration = %q", baseline.SQL)
	}
	if strings.Contains(baseline.SQL, ";") {
		t.Fatal("baseline migration should be one statement without semicolons")
	}
}

func TestValidateSettings(t *testing.T) {
	plugin := New(plugins.NewRegistry())
	if fields := plugin.ValidateSettings(map[string]string{
		"api_key": "sk-test",
		"model":   "gpt-4o-mini",
		"enabled": "true",
	}); len(fields) != 0 {
		t.Fatalf("valid settings rejected: %#v", fields)
	}

	fields := plugin.ValidateSettings(map[string]string{
		"api_key": "",
		"model":   "gpt 4o",
		"enabled": "yes",
	})
	for _, key := range []string{"api_key", "model", "enabled"} {
		if fields[key] == "" {
			t.Fatalf("missing %s error in %#v", key, fields)
		}
	}
}

func TestParsePlan(t *testing.T) {
	raw := "```json\n" + `{
		"summary":"Raise the oil price and update the hero.",
		"actions":[
			{"type":"theme.update","patch":{"hero_heading":"A shorter shelf."}},
			{"type":"product.update","product_id":"prd_1","patch":{"price_cents":4200,"stock":9}},
			{"type":"product.archive","product_id":"prd_2"},
			{"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":4,"image":"/images/rose.webp","description":"A balm.","attributes":{"size":["50 ml"]}}},
			{"type":"order.status","order_id":"ord_1","status":"shipped"}
		]
	}` + "\n```"
	plan, err := parsePlan(raw)
	if err != nil {
		t.Fatalf("parse plan: %v", err)
	}
	if len(plan.Actions) != 5 {
		t.Fatalf("actions = %d", len(plan.Actions))
	}
	if plan.Summary != "Raise the oil price and update the hero." {
		t.Fatalf("summary = %q", plan.Summary)
	}
	if plan.Actions[0].Type != "theme.update" || len(plan.Actions[0].Patch) == 0 {
		t.Fatalf("theme action = %#v", plan.Actions[0])
	}
	if plan.Actions[1].ProductID != "prd_1" {
		t.Fatalf("product update id = %q", plan.Actions[1].ProductID)
	}
	if plan.Actions[3].Product == nil || plan.Actions[3].Product.Slug != "rose-balm" {
		t.Fatalf("product create = %#v", plan.Actions[3].Product)
	}
	if plan.Actions[4].OrderID != "ord_1" || plan.Actions[4].Status != "shipped" {
		t.Fatalf("order status = %#v", plan.Actions[4])
	}
}

func TestOrderStatusShape(t *testing.T) {
	for _, raw := range []string{
		`{"summary":"A.","actions":[{"type":"order.status","order_id":"ord_1"}]}`,
		`{"summary":"A.","actions":[{"type":"order.status","status":"shipped"}]}`,
		`{"summary":"A.","actions":[{"type":"order.status","order_id":"ord_1","status":"lost"}]}`,
		`{"summary":"A.","actions":[{"type":"order.status","order_id":"ord_1","status":"shipped","price_cents":1}]}`,
	} {
		if _, err := parsePlan(raw); err == nil {
			t.Errorf("order plan %s was accepted", raw)
		}
	}

	for _, pair := range [][2]string{
		{"pending", "confirmed"},
		{"confirmed", "packed"},
		{"packed", "shipped"},
		{"packed", "cancelled"},
		{"shipped", "delivered"},
	} {
		if !canOrderTransition(pair[0], pair[1]) {
			t.Errorf("expected %s -> %s to be allowed", pair[0], pair[1])
		}
	}
	for _, pair := range [][2]string{
		{"delivered", "cancelled"}, {"cancelled", "confirmed"}, {"pending", "shipped"},
	} {
		if canOrderTransition(pair[0], pair[1]) {
			t.Errorf("%s -> %s must be rejected", pair[0], pair[1])
		}
	}
}

func TestParsePlanRejectsUnknownShape(t *testing.T) {
	cases := map[string]string{
		"unknown type":       `{"summary":"A.","actions":[{"type":"order.cancel","order_id":"o1"}]}`,
		"extra action field": `{"summary":"A.","actions":[{"type":"theme.update","patch":{},"product_id":"prd_1"}]}`,
		"missing patch":      `{"summary":"A.","actions":[{"type":"product.update","product_id":"prd_1"}]}`,
		"unknown top field":  `{"summary":"A.","actions":[{"type":"product.archive","product_id":"prd_1"}],"code":"rm -rf"}`,
		"not an object":      `[]`,
	}
	for name, raw := range cases {
		if _, err := parsePlan(raw); err == nil {
			t.Errorf("%s was accepted", name)
		}
	}
}

func TestValidatePlanShape(t *testing.T) {
	plugin := New(testRegistry(t))
	theme := storefront.Default()

	if _, err := plugin.validatePlanShape(Plan{
		Summary: "Change the heading.",
		Actions: []Action{{Type: "theme.update", Patch: json.RawMessage(`{"hero_heading":"A new shelf."}`)}},
	}, theme); err != nil {
		t.Fatalf("valid theme plan rejected: %v", err)
	}

	if _, err := plugin.validatePlanShape(Plan{
		Summary: strings.Repeat("a", 301),
		Actions: []Action{{Type: "product.archive", ProductID: "prd_1"}},
	}, theme); err == nil {
		t.Fatal("summary over 300 characters was accepted")
	}

	actions := make([]Action, 0, 11)
	for index := 0; index < 11; index++ {
		actions = append(actions, Action{Type: "theme.update", Patch: json.RawMessage(`{"radius":4}`)})
	}
	if _, err := plugin.validatePlanShape(Plan{Summary: "Too many.", Actions: actions}, theme); err == nil {
		t.Fatal("eleven actions were accepted")
	}

	if _, err := plugin.validatePlanShape(Plan{Summary: "None."}, theme); err == nil {
		t.Fatal("an empty plan was accepted")
	}
}

func TestThemePatchAllowlist(t *testing.T) {
	theme := storefront.Default()

	next, err := applyThemePatch(theme, json.RawMessage(`{"hero_heading":"A shorter shelf.","light":{"accent":"#123456"}}`))
	if err != nil {
		t.Fatalf("valid patch rejected: %v", err)
	}
	if next.HeroHeading != "A shorter shelf." || next.Light.Accent != "#123456" {
		t.Fatalf("patch was not applied: %#v", next)
	}

	for _, raw := range []string{
		`{"css":"body { display: none }"}`,
		`{"updated_at":"2026-01-01T00:00:00Z"}`,
		`{"light":{"background":"#ffffff"}}`,
		`{"hero_heading":null}`,
		`{"hero_heading":""}`,
		`{"font_scale":4}`,
		`{"contact_email":"not-an-email"}`,
	} {
		if _, err := applyThemePatch(storefront.Default(), json.RawMessage(raw)); err == nil {
			t.Errorf("patch %s was accepted", raw)
		}
	}
}

func TestProductPatchValidation(t *testing.T) {
	patch, err := parseProductPatch(json.RawMessage(
		`{"price_cents":4200,"stock":9,"active":false,"name":"New name","subtitle":"Short","description":"Long enough."}`))
	if err != nil {
		t.Fatalf("valid patch rejected: %v", err)
	}
	if patch.PriceCents == nil || *patch.PriceCents != 4200 || patch.Active == nil || *patch.Active {
		t.Fatalf("patch = %#v", patch)
	}

	for _, raw := range []string{
		`{}`,
		`{"price_cents":0}`,
		`{"price_cents":100000000}`,
		`{"stock":-1}`,
		`{"stock":100001}`,
		`{"name":"x"}`,
		`{"slug":"new-slug"}`,
		`{"price_cents":1.5}`,
		`{"active":null}`,
	} {
		if _, err := parseProductPatch(json.RawMessage(raw)); err == nil {
			t.Errorf("product patch %s was accepted", raw)
		}
	}
}

func TestProductDraftValidation(t *testing.T) {
	plugin := New(testRegistry(t))

	valid := &ProductDraft{
		Name:         "Rose balm",
		Slug:         "rose-balm",
		PriceCents:   3200,
		CategorySlug: "body",
		PluginID:     "oils",
		Stock:        4,
		Image:        "/images/rose.webp",
		Description:  "A balm.",
		Attributes:   map[string][]string{"size": {"50 ml"}},
	}
	if err := plugin.validateProductDraft(valid); err != nil {
		t.Fatalf("valid draft rejected: %v", err)
	}

	missingSize := &ProductDraft{
		Name:         "Rose balm",
		Slug:         "rose-balm",
		PriceCents:   3200,
		CategorySlug: "body",
		PluginID:     "oils",
		Stock:        4,
		Image:        "/images/rose.webp",
	}
	if err := plugin.validateProductDraft(missingSize); err == nil {
		t.Fatal("draft without a required option was accepted")
	}

	unknownPlugin := &ProductDraft{
		Name:         "Rose balm",
		Slug:         "rose-balm",
		PriceCents:   3200,
		CategorySlug: "body",
		PluginID:     "missing",
		Stock:        4,
		Image:        "/images/rose.webp",
	}
	if err := plugin.validateProductDraft(unknownPlugin); err == nil {
		t.Fatal("draft with an unknown plugin was accepted")
	}

	coreWithOptions := &ProductDraft{
		Name:         "Rose balm",
		Slug:         "rose-balm",
		PriceCents:   3200,
		CategorySlug: "body",
		PluginID:     "core",
		Stock:        4,
		Image:        "/images/rose.webp",
		Attributes:   map[string][]string{"size": {"50 ml"}},
	}
	if err := plugin.validateProductDraft(coreWithOptions); err == nil {
		t.Fatal("core draft with plugin options was accepted")
	}
}

func TestConfigFromSettings(t *testing.T) {
	if _, err := configFromSettings(map[string]string{
		"enabled": "false", "api_key": "sk-key", "model": "gpt-4o-mini",
	}); !errors.Is(err, errDisabled) {
		t.Fatalf("disabled error = %v", err)
	}
	if _, err := configFromSettings(map[string]string{
		"enabled": "true", "model": "gpt-4o-mini",
	}); !errors.Is(err, errNotConfigured) {
		t.Fatalf("missing key error = %v", err)
	}
	config, err := configFromSettings(map[string]string{"api_key": "sk-key"})
	if err != nil {
		t.Fatalf("default model: %v", err)
	}
	if config.model != defaultModel {
		t.Fatalf("model = %q", config.model)
	}
}
