package httpapi

import (
	"reflect"
	"testing"

	"azahar/internal/storefront"
)

func TestChangedThemeFieldsNamesEverySection(t *testing.T) {
	before := storefront.Default()
	after := storefront.Default()
	after.BrandName = "Nour"
	after.Light.Accent = "#7a5c2e"
	after.FontBody = "inter"
	after.AboutBody = "A different story."

	got := changedThemeFields(before, after)
	want := []string{"about_body", "brand_name", "font_body", "light.accent"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("changed fields = %#v, want %#v", got, want)
	}
}

func TestChangedThemeFieldsIgnoresTimestamp(t *testing.T) {
	before := storefront.Default()
	after := storefront.Default()
	after.UpdatedAt = "2026-11-20T10:00:00Z"
	if got := changedThemeFields(before, after); len(got) != 0 {
		t.Fatalf("updated_at must not be audited as a change: %#v", got)
	}
}
