package plugins_test

import (
	"testing"

	"azahar/internal/plugins"
)

type testPlugin struct {
	manifest plugins.Manifest
	routes   []plugins.Route
}

func (p testPlugin) Manifest() plugins.Manifest { return p.manifest }
func (p testPlugin) Routes() []plugins.Route    { return p.routes }

func validPlugin() testPlugin {
	return testPlugin{
		manifest: plugins.Manifest{
			ID:      "sample",
			Name:    "Sample",
			Version: "1.0.0",
			ProductType: &plugins.ProductType{
				Label: "Sample product",
				Fields: []plugins.OptionField{
					{Key: "size", Label: "Size", Type: "select", Required: true},
				},
			},
			Settings: []plugins.SettingField{
				{Key: "api_key", Label: "API key", Type: "secret", Required: true},
			},
		},
		routes: []plugins.Route{
			{Method: "GET", Pattern: "/items", Handler: func(*plugins.Context) {}},
			{Method: "POST", Pattern: "/items", Admin: true, Handler: func(*plugins.Context) {}},
		},
	}
}

func TestRegisterAndLookup(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(validPlugin()); err != nil {
		t.Fatalf("register: %v", err)
	}
	if _, ok := registry.Get("sample"); !ok {
		t.Fatal("registered plugin was not found")
	}
	if registry.ProductType("sample") == nil {
		t.Fatal("product type was not exposed")
	}
	if len(registry.Manifests()) != 1 {
		t.Fatalf("expected one manifest, got %d", len(registry.Manifests()))
	}
}

func TestRejectsDuplicateID(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(validPlugin()); err != nil {
		t.Fatalf("register: %v", err)
	}
	if err := registry.Register(validPlugin()); err == nil {
		t.Fatal("duplicate plugin id must be rejected")
	}
}

func TestRejectsAdminPrefixInRoute(t *testing.T) {
	for _, admin := range []bool{true, false} {
		plugin := validPlugin()
		plugin.routes = []plugins.Route{
			{Method: "GET", Pattern: "/admin/items", Admin: admin, Handler: func(*plugins.Context) {}},
		}
		if err := plugins.NewRegistry().Register(plugin); err == nil {
			t.Fatalf("a route with an /admin prefix must be rejected (admin=%v)", admin)
		}
	}
}

func TestRejectsUnsafeStorefrontPagePath(t *testing.T) {
	for _, path := range []string{`/\evil.com`, "/images/%2e%2e/x", "/build?x=1", "/build#x"} {
		plugin := validPlugin()
		plugin.manifest.StorefrontPages = []plugins.StorefrontPage{
			{Path: path, Label: "Unsafe"},
		}
		if err := plugins.NewRegistry().Register(plugin); err == nil {
			t.Fatalf("storefront page path %q must be rejected", path)
		}
	}
}

func TestRejectsInvalidOptionKey(t *testing.T) {
	plugin := validPlugin()
	plugin.manifest.ProductType.Fields[0].Key = "Storage Size"
	if err := plugins.NewRegistry().Register(plugin); err == nil {
		t.Fatal("option keys must be lowercase identifiers")
	}
}

func TestRejectsDuplicateRoute(t *testing.T) {
	plugin := validPlugin()
	plugin.routes = append(plugin.routes, plugin.routes[0])
	if err := plugins.NewRegistry().Register(plugin); err == nil {
		t.Fatal("duplicate routes must be rejected")
	}
}

func TestWebhookRouteRegistrationAndLookup(t *testing.T) {
	plugin := validPlugin()
	plugin.manifest.ID = "hooked"
	plugin.routes = []plugins.Route{
		{Method: "POST", Pattern: "/hook", Webhook: true, Handler: func(*plugins.Context) {}},
	}
	registry := plugins.NewRegistry()
	if err := registry.Register(plugin); err != nil {
		t.Fatalf("register webhook route: %v", err)
	}
	if !registry.IsWebhook("POST", "/api/plugins/hooked/hook") {
		t.Fatal("the registered webhook path was not recognised")
	}
	if registry.IsWebhook("GET", "/api/plugins/hooked/hook") {
		t.Fatal("the wrong method was recognised as a webhook")
	}
	if registry.IsWebhook("POST", "/api/plugins/hooked/other") {
		t.Fatal("an unregistered path was recognised as a webhook")
	}
}

func TestRejectsUnsafeWebhookRoutes(t *testing.T) {
	tests := []plugins.Route{
		{Method: "POST", Pattern: "/hook", Admin: true, Webhook: true, Handler: func(*plugins.Context) {}},
		{Method: "POST", Pattern: "/hook/{id}", Webhook: true, Handler: func(*plugins.Context) {}},
		{Method: "PUT", Pattern: "/hook", Webhook: true, Handler: func(*plugins.Context) {}},
	}
	for _, route := range tests {
		plugin := validPlugin()
		plugin.routes = []plugins.Route{route}
		if err := plugins.NewRegistry().Register(plugin); err == nil {
			t.Fatalf("unsafe webhook route %s %q must be rejected", route.Method, route.Pattern)
		}
	}
}

func pluginWith(id string, dependencies ...string) testPlugin {
	plugin := validPlugin()
	plugin.manifest.ID = id
	plugin.manifest.Name = id
	plugin.manifest.Dependencies = dependencies
	return plugin
}

func TestDependencyValidation(t *testing.T) {
	registry := plugins.NewRegistry()
	for _, plugin := range []plugins.Plugin{
		pluginWith("base"),
		pluginWith("middle", "base"),
		pluginWith("top", "middle"),
	} {
		if err := registry.Register(plugin); err != nil {
			t.Fatalf("register: %v", err)
		}
	}
	if err := registry.ValidateDependencies(); err != nil {
		t.Fatalf("valid dependency graph rejected: %v", err)
	}

	unknown := plugins.NewRegistry()
	if err := unknown.Register(pluginWith("top", "missing")); err != nil {
		t.Fatalf("register: %v", err)
	}
	if err := unknown.ValidateDependencies(); err == nil {
		t.Fatal("unknown dependency must be rejected")
	}

	cycle := plugins.NewRegistry()
	if err := cycle.Register(pluginWith("alpha", "beta")); err != nil {
		t.Fatalf("register alpha: %v", err)
	}
	if err := cycle.Register(pluginWith("beta", "alpha")); err != nil {
		t.Fatalf("register beta: %v", err)
	}
	if err := cycle.ValidateDependencies(); err == nil {
		t.Fatal("dependency cycle must be rejected")
	}
}

func TestDependencyEnableState(t *testing.T) {
	registry := plugins.NewRegistry()
	for _, plugin := range []plugins.Plugin{
		pluginWith("base"),
		pluginWith("middle", "base"),
		pluginWith("top", "middle"),
	} {
		if err := registry.Register(plugin); err != nil {
			t.Fatalf("register: %v", err)
		}
	}
	registry.SetStates(map[string]bool{"base": true, "middle": true, "top": true})

	if got := registry.Dependents("base"); len(got) != 2 || got[0] != "middle" || got[1] != "top" {
		t.Fatalf("dependents = %#v, want [middle top]", got)
	}
	if !registry.EffectiveEnabled("top") {
		t.Fatal("top should be effective when every dependency is enabled")
	}

	registry.SetEnabled("base", false)
	if registry.Enabled("middle") != true {
		t.Fatal("middle should keep its desired enabled state")
	}
	if registry.EffectiveEnabled("middle") || registry.EffectiveEnabled("top") {
		t.Fatal("disabling base must block middle and top")
	}
	if got := registry.EnabledDependents("base"); len(got) != 2 {
		t.Fatalf("enabled dependents = %#v, want middle and top", got)
	}
	if got := registry.BlockedBy("top"); len(got) != 1 || got[0] != "base" {
		t.Fatalf("blocked_by = %#v, want [base]", got)
	}

	registry.SetEnabled("top", false)
	if got := registry.EnabledDependents("base"); len(got) != 1 || got[0] != "middle" {
		t.Fatalf("disabled dependent must leave the warning set: %#v", got)
	}
}

func TestSetStatesDefaultsMissingPluginsToEnabled(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(pluginWith("base")); err != nil {
		t.Fatalf("register: %v", err)
	}
	registry.SetStates(map[string]bool{})
	if !registry.Enabled("base") || !registry.EffectiveEnabled("base") {
		t.Fatal("a plugin without stored state must stay enabled")
	}
}
