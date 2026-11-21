package httpapi

import (
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/plugins/oils"
)

func TestOilAttributesAcceptPluginFields(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(oils.New()); err != nil {
		t.Fatalf("register oils: %v", err)
	}
	server := &Server{plugins: registry}
	attributes := map[string][]string{
		"size":        {"100 ml"},
		"concerns":    {"dry", "sensitive"},
		"ingredients": {"Olive squalane", "Orange blossom"},
		"how_to":      {"Warm two pumps and massage over dry skin."},
	}
	if fields := server.validateProductAttributes("oils", attributes); len(fields) != 0 {
		t.Fatalf("oil attributes were rejected: %#v", fields)
	}
}

func TestOilSizeTakesOneValue(t *testing.T) {
	registry := plugins.NewRegistry()
	if err := registry.Register(oils.New()); err != nil {
		t.Fatalf("register oils: %v", err)
	}
	server := &Server{plugins: registry}
	fields := server.validateProductAttributes("oils", map[string][]string{
		"size": {"100 ml", "50 ml"},
	})
	if fields["attributes.size"] == "" {
		t.Fatalf("a text field must reject two values: %#v", fields)
	}
}
