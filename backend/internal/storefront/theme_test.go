package storefront_test

import (
	"testing"

	"azahar/internal/storefront"
)

func TestDefaultThemeIsValid(t *testing.T) {
	fields := storefront.Validate(storefront.Default())
	if len(fields) != 0 {
		t.Fatalf("the default theme must pass validation: %#v", fields)
	}
}

func TestDefaultContrast(t *testing.T) {
	theme := storefront.Default()
	cases := []struct {
		name string
		got  float64
		min  float64
	}{
		{"light ink", storefront.ContrastRatio(theme.Light.Ink, theme.Light.Ground), 7},
		{"light muted", storefront.ContrastRatio(theme.Light.Muted, theme.Light.Ground), 4.5},
		{"light accent", storefront.ContrastRatio(theme.Light.Accent, theme.Light.Ground), 4.5},
		{"dark ink", storefront.ContrastRatio(theme.Dark.Ink, theme.Dark.Ground), 7},
		{"dark muted", storefront.ContrastRatio(theme.Dark.Muted, theme.Dark.Ground), 4.5},
	}
	for _, tc := range cases {
		if tc.got < tc.min {
			t.Errorf("%s is %.2f:1, want at least %.1f:1", tc.name, tc.got, tc.min)
		}
	}
}

func TestRejectsLowContrast(t *testing.T) {
	theme := storefront.Default()
	theme.Light.Muted = "#b8aeb0"
	fields := storefront.Validate(theme)
	if fields["light.muted"] == "" {
		t.Fatalf("low-contrast muted text must be rejected: %#v", fields)
	}
}

func TestRejectsUnknownFont(t *testing.T) {
	theme := storefront.Default()
	theme.FontDisplay = "comic-sans"
	fields := storefront.Validate(theme)
	if fields["font_display"] == "" {
		t.Fatalf("an unknown font must be rejected: %#v", fields)
	}
}

func TestRejectsCSSInjection(t *testing.T) {
	theme := storefront.Default()
	theme.Light.Ground = "red; background-image: url(https://example.test/x"
	fields := storefront.Validate(theme)
	if fields["light.ground"] == "" {
		t.Fatalf("a non-hex colour must be rejected: %#v", fields)
	}
}

func TestParseFillsDefaults(t *testing.T) {
	theme, err := storefront.Parse(`{"brand_name":"Nour","font_body":"inter"}`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if theme.BrandName != "Nour" || theme.FontBody != "inter" {
		t.Fatalf("stored values did not apply: %+v", theme)
	}
	if theme.Light.Ground != storefront.Default().Light.Ground {
		t.Fatalf("missing values must keep the default palette: %+v", theme.Light)
	}
	if !theme.ConcernsEnabled {
		t.Fatal("a missing concern toggle must default to visible")
	}

	hidden, err := storefront.Parse(`{"concerns_enabled":false}`)
	if err != nil {
		t.Fatalf("parse hidden concerns: %v", err)
	}
	if hidden.ConcernsEnabled {
		t.Fatal("the stored concern toggle must apply")
	}
}

func TestParseRejectsUnknownFields(t *testing.T) {
	if _, err := storefront.Parse(`{"brand_name":"Nour","css":"body{}"}`); err == nil {
		t.Fatal("unknown fields must be rejected")
	}
}

func TestEncodeRoundTrip(t *testing.T) {
	theme := storefront.Default()
	raw, err := storefront.Encode(theme)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	parsed, err := storefront.Parse(raw)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if parsed.BrandName != theme.BrandName || parsed.Light.Accent != theme.Light.Accent {
		t.Fatalf("round trip changed the theme: %+v", parsed)
	}
}

func TestContactEmailMustBePlain(t *testing.T) {
	theme := storefront.Default()
	theme.ContactEmail = "Shop <hello@example.test>"
	if storefront.Validate(theme)["contact_email"] == "" {
		t.Fatal("a display-name email must be rejected")
	}
	theme.ContactEmail = "hello@example.test"
	if storefront.Validate(theme)["contact_email"] != "" {
		t.Fatal("a plain email should pass")
	}
}

func TestThemeImagesAreBundledOnly(t *testing.T) {
	theme := storefront.Default()
	theme.HeroImage = "javascript:alert(1)"
	if storefront.Validate(theme)["hero_image"] == "" {
		t.Fatal("hero_image must reject a javascript URL")
	}
	theme.HeroImage = "https://images.example.test/hero.webp"
	if storefront.Validate(theme)["hero_image"] == "" {
		t.Fatal("hero_image must reject a remote image")
	}
	theme.HeroImage = "/images/hero-1600.webp"
	if storefront.Validate(theme)["hero_image"] != "" {
		t.Fatal("hero_image should accept a bundled image")
	}
}
