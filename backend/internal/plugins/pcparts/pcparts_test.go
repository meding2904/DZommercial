package pcparts_test

import (
	"testing"

	"azahar/internal/plugins"
	"azahar/internal/plugins/pcparts"
)

func TestManifestDeclaresPartFields(t *testing.T) {
	manifest := pcparts.New().Manifest()
	if manifest.ID != "pcparts" {
		t.Fatalf("id = %q, want pcparts", manifest.ID)
	}
	if manifest.ProductType == nil {
		t.Fatal("the pcparts plugin must declare a product type")
	}
	if manifest.ProductType.Label != "PC part" {
		t.Fatalf("product type = %q, want PC part", manifest.ProductType.Label)
	}

	fields := make(map[string]string)
	for _, field := range manifest.ProductType.Fields {
		fields[field.Key] = field.Type
	}
	want := []string{
		"socket", "memory_type", "form_factor", "interface", "tdp_watts",
		"wattage_watts", "length_mm", "max_gpu_length_mm", "supported_form_factor",
	}
	for _, key := range want {
		if fields[key] != "text" {
			t.Fatalf("field %s type = %q, want text", key, fields[key])
		}
	}
}

func TestManifestDeclaresCategories(t *testing.T) {
	manifest := pcparts.New().Manifest()
	got := make(map[string]string, len(manifest.Categories))
	for _, category := range manifest.Categories {
		got[category.Slug] = category.Name
	}
	want := map[string]string{
		"cpus":         "CPU",
		"gpus":         "Graphics cards",
		"motherboards": "Motherboards",
		"memory":       "Memory",
		"storage":      "Storage",
		"psus":         "Power supplies",
		"cases":        "Cases",
		"cooling":      "Cooling",
	}
	for slug, name := range want {
		if got[slug] != name {
			t.Fatalf("category %s = %q, want %q", slug, got[slug], name)
		}
	}
}

func TestRegistration(t *testing.T) {
	if err := plugins.NewRegistry().Register(pcparts.New()); err != nil {
		t.Fatalf("register pcparts: %v", err)
	}
}
