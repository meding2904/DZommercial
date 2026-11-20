package phone

import (
	"context"
	"reflect"
	"strings"
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

func TestValidIMEI(t *testing.T) {
	valid := []string{
		"490154203237518",
		"356938035643809",
		"352099001761481",
	}
	for _, imei := range valid {
		if !ValidIMEI(imei) {
			t.Errorf("ValidIMEI(%q) = false, want true", imei)
		}
	}

	invalid := []string{
		"",
		"49015420323751",
		"4901542032375181",
		"490154203237519",
		"123456789012345",
		"860000000000000",
		"49015420323751a",
		"49015 203237518",
	}
	for _, imei := range invalid {
		if ValidIMEI(imei) {
			t.Errorf("ValidIMEI(%q) = true, want false", imei)
		}
	}
}

func TestVariantKeyIsStableAcrossMapOrder(t *testing.T) {
	first := VariantKey(map[string]string{"config": "12 GB RAM + 256 GB", "color": "Midnight"})
	second := VariantKey(map[string]string{"color": "Midnight", "config": "12 GB RAM + 256 GB"})
	if first != second {
		t.Fatalf("VariantKey is order dependent: %q != %q", first, second)
	}
	want := "config=12+GB+RAM+%2B+256+GB&color=Midnight"
	if first != want {
		t.Errorf("VariantKey = %q, want %q", first, want)
	}

	if got := VariantKey(map[string]string{"config": "16 GB RAM + 1 TB"}); got != "config=16+GB+RAM+%2B+1+TB&color=" {
		t.Errorf("VariantKey with missing options = %q", got)
	}
	if got := VariantKey(nil); got != "" {
		t.Errorf("VariantKey(nil) = %q, want an empty string", got)
	}
}

func TestVariantLabel(t *testing.T) {
	got := VariantLabel(map[string]string{"config": "12 GB RAM + 256 GB", "color": "Midnight"})
	want := "Memory and storage: 12 GB RAM + 256 GB · Color: Midnight"
	if got != want {
		t.Errorf("VariantLabel = %q, want %q", got, want)
	}
	if got := VariantLabel(map[string]string{"color": "Blue"}); got != "Color: Blue" {
		t.Errorf("VariantLabel skips empty options, got %q", got)
	}
	if got := VariantLabel(nil); got != "" {
		t.Errorf("VariantLabel(nil) = %q, want an empty string", got)
	}
}

func TestManifest(t *testing.T) {
	manifest := New().Manifest()
	if manifest.ID != "phone" {
		t.Errorf("ID = %q", manifest.ID)
	}
	if manifest.Name != "Phone catalog" {
		t.Errorf("Name = %q", manifest.Name)
	}
	if manifest.Version != "1.0.0" {
		t.Errorf("Version = %q", manifest.Version)
	}
	if manifest.Author != "Azahar" {
		t.Errorf("Author = %q", manifest.Author)
	}
	if strings.TrimSpace(manifest.Description) == "" {
		t.Error("Description is empty")
	}
	if manifest.AdminPath != "/admin/plugins/phone" {
		t.Errorf("AdminPath = %q", manifest.AdminPath)
	}

	productType := manifest.ProductType
	if productType == nil {
		t.Fatal("ProductType is nil")
	}
	if productType.Label != "Phone" {
		t.Errorf("ProductType.Label = %q", productType.Label)
	}
	if productType.Summary != "Every handset is tracked by IMEI and sold as one memory and storage configuration." {
		t.Errorf("ProductType.Summary = %q", productType.Summary)
	}

	want := []struct {
		key         string
		label       string
		suggestions []string
	}{
		{"config", "Memory and storage", []string{"12 GB RAM + 256 GB", "16 GB RAM + 512 GB", "16 GB RAM + 1 TB"}},
		{"color", "Color", []string{"Midnight", "Graphite", "Silver", "Blue"}},
	}
	if len(productType.Fields) != len(want) {
		t.Fatalf("got %d fields, want %d", len(productType.Fields), len(want))
	}
	for i, expected := range want {
		field := productType.Fields[i]
		if field.Key != expected.key || field.Label != expected.label {
			t.Errorf("field %d = %q %q, want %q %q", i, field.Key, field.Label, expected.key, expected.label)
		}
		if field.Type != "select" || !field.Required {
			t.Errorf("field %q must be a required select", field.Key)
		}
		if strings.TrimSpace(field.Help) == "" {
			t.Errorf("field %q has no help text", field.Key)
		}
		if !reflect.DeepEqual(field.Suggestions, expected.suggestions) {
			t.Errorf("field %q suggestions = %v, want %v", field.Key, field.Suggestions, expected.suggestions)
		}
	}
}

func TestRegistration(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(New()); err != nil {
		t.Fatalf("Register: %v", err)
	}
	if _, ok := registry.Get("phone"); !ok {
		t.Fatal("phone is not registered")
	}
	if registry.ProductType("phone") == nil {
		t.Fatal("product type is missing")
	}
	if registry.CheckoutHook("phone") == nil {
		t.Fatal("checkout hook is missing")
	}
	if len(registry.OrderStatusHooks()) != 1 {
		t.Fatalf("got %d order status hooks, want 1", len(registry.OrderStatusHooks()))
	}
}

func TestRoutes(t *testing.T) {
	want := map[string]bool{
		"GET /devices":                  true,
		"POST /devices":                 true,
		"DELETE /devices/{id}":          true,
		"GET /orders/{orderID}/devices": true,
	}
	routes := New().Routes()
	for _, route := range routes {
		key := route.Method + " " + route.Pattern
		if !want[key] {
			t.Errorf("unexpected route %q", key)
			continue
		}
		delete(want, key)
		if !route.Admin {
			t.Errorf("route %q is not admin-only", key)
		}
		if route.Handler == nil {
			t.Errorf("route %q has no handler", key)
		}
		if strings.HasPrefix(route.Pattern, "/admin") {
			t.Errorf("admin route %q includes the /admin prefix", route.Pattern)
		}
	}
	for key := range want {
		t.Errorf("missing route %q", key)
	}
}

func TestValidateCheckoutLineIgnoresOtherPluginsWithoutStockLookup(t *testing.T) {
	plugin := New()
	for _, pluginID := range []string{"", "core", "coffee"} {
		problem := plugin.ValidateCheckoutLine(context.Background(), nil, plugins.CheckoutLine{
			Product:  store.Product{ID: "prd_1", Name: "Soap", PluginID: pluginID},
			Options:  map[string]string{"color": "Blue"},
			Quantity: 2,
		})
		if problem != nil {
			t.Errorf("plugin %q: got problem %v, want nil", pluginID, problem)
		}
	}
}

func TestValidateCheckoutLineChecksOptionsBeforeStock(t *testing.T) {
	plugin := New()
	product := store.Product{
		ID:       "prd_1",
		Name:     "Azahar One",
		PluginID: pluginID,
		Attributes: map[string][]string{
			"config": {"12 GB RAM + 256 GB"},
			"color":  {"Midnight"},
		},
	}
	cases := []map[string]string{
		nil,
		{"config": "12 GB RAM + 256 GB"},
		{"config": "12 GB RAM + 256 GB", "color": "Blue"},
	}
	for _, options := range cases {
		problem := plugin.ValidateCheckoutLine(context.Background(), nil, plugins.CheckoutLine{
			Product:  product,
			Options:  options,
			Quantity: 1,
		})
		if problem == nil {
			t.Fatalf("options %v: got nil problem, want validation error", options)
		}
		if problem.Status != 422 {
			t.Errorf("options %v: status = %d, want 422", options, problem.Status)
		}
	}
}
