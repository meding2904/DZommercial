package oils_test

import (
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/plugins/oils"
)

func TestManifestDeclaresOilFields(t *testing.T) {
	manifest := oils.New().Manifest()
	if manifest.ID != "oils" {
		t.Fatalf("id = %q, want oils", manifest.ID)
	}
	if manifest.ProductType == nil {
		t.Fatal("the oil plugin must declare a product type")
	}
	types := make(map[string]string)
	for _, field := range manifest.ProductType.Fields {
		types[field.Key] = field.Type
	}
	want := map[string]string{
		"size": "text", "concerns": "list", "ingredients": "list", "how_to": "textarea",
	}
	for key, kind := range want {
		if types[key] != kind {
			t.Fatalf("field %s type = %q, want %q", key, types[key], kind)
		}
	}
}

func TestRegistration(t *testing.T) {
	if err := plugins.NewRegistry().Register(oils.New()); err != nil {
		t.Fatalf("register oils: %v", err)
	}
}
