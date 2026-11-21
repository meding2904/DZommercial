// Package storefront defines the admin-owned storefront theme: brand copy,
// typography, palette and hero image. The API validates every value so a theme
// can be applied as CSS variables without injecting arbitrary CSS.
package storefront

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"strconv"
	"strings"
)

type Palette struct {
	Ground    string `json:"ground"`
	Surface   string `json:"surface"`
	Ink       string `json:"ink"`
	Muted     string `json:"muted"`
	Accent    string `json:"accent"`
	AccentInk string `json:"accent_ink"`
}

type Theme struct {
	BrandName       string  `json:"brand_name"`
	Tagline         string  `json:"tagline"`
	Description     string  `json:"description"`
	HeroHeading     string  `json:"hero_heading"`
	HeroBody        string  `json:"hero_body"`
	HeroAlt         string  `json:"hero_alt"`
	HeroCTA         string  `json:"hero_cta"`
	HeroImage       string  `json:"hero_image"`
	Statement       string  `json:"statement"`
	ShelfHeading    string  `json:"shelf_heading"`
	ShopTitle       string  `json:"shop_title"`
	HouseLabel      string  `json:"house_label"`
	AboutHeading    string  `json:"about_heading"`
	AboutBody       string  `json:"about_body"`
	AboutImage      string  `json:"about_image"`
	AboutAlt        string  `json:"about_alt"`
	ClosingHeading  string  `json:"closing_heading"`
	FooterNote      string  `json:"footer_note"`
	FooterCredit    string  `json:"footer_credit"`
	ContactEmail    string  `json:"contact_email"`
	Announcement    string  `json:"announcement"`
	ConcernsEnabled bool    `json:"concerns_enabled"`
	FontDisplay     string  `json:"font_display"`
	FontBody        string  `json:"font_body"`
	FontScale       float64 `json:"font_scale"`
	Radius          int     `json:"radius"`
	Light           Palette `json:"light"`
	Dark            Palette `json:"dark"`
	UpdatedAt       string  `json:"updated_at,omitempty"`
}

var (
	displayFonts = []string{"schibsted", "source_serif", "fraunces", "system"}
	bodyFonts    = []string{"schibsted", "source_serif", "inter", "system"}
	hexColor     = regexp.MustCompile(`^#[0-9a-f]{6}$`)
	plainEmail   = regexp.MustCompile(`^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$`)
)

func Default() Theme {
	return Theme{
		BrandName:       "Azahar",
		Tagline:         "Botanical skincare from the Mediterranean edge.",
		Description:     "Oils, clays and blossoms, blended in small batches in Algiers.",
		HeroHeading:     "Botanical skincare from the Mediterranean edge.",
		HeroBody:        "Oils, clays and blossoms, blended in small batches in Algiers. Eight formulas for the face, the body and the bath.",
		HeroAlt:         "Amber glass bottles and a linen cloth on a plaster shelf",
		HeroCTA:         "Shop the shelf",
		HeroImage:       "/images/hero-1600.webp",
		Statement:       "We make fewer things, with more of the plant in them. The shelf is short on purpose.",
		ShelfHeading:    "Eight formulas for face, body and the bath.",
		ShopTitle:       "The shelf",
		HouseLabel:      "The house",
		AboutHeading:    "A small shelf in Algiers.",
		AboutBody:       "Azahar began with the orange blossom that scents the city in spring. We press olives and figs, mix clays with rose, and pour everything into amber glass. The shelf is short on purpose.",
		AboutImage:      "/images/atelier-1200.webp",
		AboutAlt:        "Amber bottles, ceramic jars, linen and dried botanicals on a work table",
		ClosingHeading:  "Take the whole shelf, or one jar.",
		FooterNote:      "Botanical skincare from the Mediterranean edge. Oils, clays and blossoms, blended in small batches in Algiers.",
		FooterCredit:    "Photography from Pexels. Products, prices and story are sample data.",
		ContactEmail:    "hello@azahar.example",
		Announcement:    "Cash on delivery across Algeria.",
		ConcernsEnabled: true,
		FontDisplay:     "schibsted",
		FontBody:        "schibsted",
		FontScale:       1,
		Radius:          4,
		Light: Palette{
			Ground:    "#f6f0eb",
			Surface:   "#ebe0d9",
			Ink:       "#2c2325",
			Muted:     "#6a5d5e",
			Accent:    "#9a3c4c",
			AccentInk: "#ffffff",
		},
		Dark: Palette{
			Ground:    "#171311",
			Surface:   "#221b18",
			Ink:       "#f4ece5",
			Muted:     "#b9aca4",
			Accent:    "#e0a0a8",
			AccentInk: "#241316",
		},
	}
}

func Parse(raw string) (Theme, error) {
	theme := Default()
	if strings.TrimSpace(raw) == "" {
		return theme, nil
	}
	decoder := json.NewDecoder(bytes.NewReader([]byte(raw)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&theme); err != nil {
		return Theme{}, fmt.Errorf("read storefront theme: %w", err)
	}
	return Normalize(theme), nil
}

func Normalize(theme Theme) Theme {
	trim := func(value string) string { return strings.TrimSpace(value) }
	theme.BrandName = trim(theme.BrandName)
	theme.Tagline = trim(theme.Tagline)
	theme.Description = trim(theme.Description)
	theme.HeroHeading = trim(theme.HeroHeading)
	theme.HeroBody = trim(theme.HeroBody)
	theme.HeroAlt = trim(theme.HeroAlt)
	theme.HeroCTA = trim(theme.HeroCTA)
	theme.HeroImage = trim(theme.HeroImage)
	theme.Statement = trim(theme.Statement)
	theme.ShelfHeading = trim(theme.ShelfHeading)
	theme.ShopTitle = trim(theme.ShopTitle)
	theme.HouseLabel = trim(theme.HouseLabel)
	theme.AboutHeading = trim(theme.AboutHeading)
	theme.AboutBody = trim(theme.AboutBody)
	theme.AboutImage = trim(theme.AboutImage)
	theme.AboutAlt = trim(theme.AboutAlt)
	theme.ClosingHeading = trim(theme.ClosingHeading)
	theme.FooterNote = trim(theme.FooterNote)
	theme.FooterCredit = trim(theme.FooterCredit)
	theme.ContactEmail = strings.ToLower(trim(theme.ContactEmail))
	theme.Announcement = trim(theme.Announcement)
	theme.FontDisplay = strings.ToLower(trim(theme.FontDisplay))
	theme.FontBody = strings.ToLower(trim(theme.FontBody))
	theme.Light = normalizePalette(theme.Light)
	theme.Dark = normalizePalette(theme.Dark)
	if theme.FontScale == 0 {
		theme.FontScale = 1
	}
	return theme
}

func normalizePalette(p Palette) Palette {
	p.Ground = strings.ToLower(strings.TrimSpace(p.Ground))
	p.Surface = strings.ToLower(strings.TrimSpace(p.Surface))
	p.Ink = strings.ToLower(strings.TrimSpace(p.Ink))
	p.Muted = strings.ToLower(strings.TrimSpace(p.Muted))
	p.Accent = strings.ToLower(strings.TrimSpace(p.Accent))
	p.AccentInk = strings.ToLower(strings.TrimSpace(p.AccentInk))
	return p
}

func Encode(theme Theme) (string, error) {
	raw, err := json.Marshal(theme)
	if err != nil {
		return "", err
	}
	return string(raw), nil
}

// Validate returns field-keyed messages, ready for the API error envelope.
func Validate(theme Theme) map[string]string {
	fields := make(map[string]string)

	text := func(key, value string, min, max int) {
		if len([]rune(value)) < min {
			fields[key] = fmt.Sprintf("Use at least %d characters.", min)
			return
		}
		if len([]rune(value)) > max {
			fields[key] = fmt.Sprintf("Keep this under %d characters.", max+1)
		}
		if hasControl(value) {
			fields[key] = "Use plain text without control characters."
		}
	}
	optional := func(key, value string, max int) {
		if len([]rune(value)) > max {
			fields[key] = fmt.Sprintf("Keep this under %d characters.", max+1)
		}
		if hasControl(value) {
			fields[key] = "Use plain text without control characters."
		}
	}

	text("brand_name", theme.BrandName, 1, 40)
	text("tagline", theme.Tagline, 1, 120)
	optional("description", theme.Description, 400)
	text("hero_heading", theme.HeroHeading, 1, 120)
	optional("hero_body", theme.HeroBody, 500)
	optional("hero_alt", theme.HeroAlt, 160)
	text("hero_cta", theme.HeroCTA, 1, 60)
	text("statement", theme.Statement, 1, 220)
	text("shelf_heading", theme.ShelfHeading, 1, 120)
	text("shop_title", theme.ShopTitle, 1, 80)
	text("house_label", theme.HouseLabel, 1, 40)
	optional("about_heading", theme.AboutHeading, 120)
	optional("about_body", theme.AboutBody, 2000)
	optional("about_alt", theme.AboutAlt, 160)
	text("closing_heading", theme.ClosingHeading, 1, 120)
	optional("footer_note", theme.FooterNote, 300)
	optional("footer_credit", theme.FooterCredit, 300)
	optional("announcement", theme.Announcement, 140)

	if !validImagePath(theme.HeroImage) {
		fields["hero_image"] = "Use an /images/ path."
	}
	if !validImagePath(theme.AboutImage) {
		fields["about_image"] = "Use an /images/ path."
	}
	if !plainEmail.MatchString(theme.ContactEmail) || len(theme.ContactEmail) > 254 {
		fields["contact_email"] = "Enter a valid email address."
	}

	if !contains(displayFonts, theme.FontDisplay) {
		fields["font_display"] = "Choose one of the available display fonts."
	}
	if !contains(bodyFonts, theme.FontBody) {
		fields["font_body"] = "Choose one of the available text fonts."
	}
	if theme.FontScale < 0.85 || theme.FontScale > 1.15 {
		fields["font_scale"] = "Use a body text scale between 0.85 and 1.15."
	}
	if theme.Radius < 0 || theme.Radius > 12 {
		fields["radius"] = "Use a corner radius between 0 and 12."
	}

	validatePalette("light", theme.Light, fields)
	validatePalette("dark", theme.Dark, fields)
	return fields
}

func validatePalette(name string, p Palette, fields map[string]string) {
	colors := map[string]string{
		"ground":     p.Ground,
		"surface":    p.Surface,
		"ink":        p.Ink,
		"muted":      p.Muted,
		"accent":     p.Accent,
		"accent_ink": p.AccentInk,
	}
	valid := true
	for key, value := range colors {
		if !hexColor.MatchString(value) {
			fields[name+"."+key] = "Use a six-digit hex colour, for example #2c2325."
			valid = false
		}
	}
	if !valid {
		return
	}

	check := func(field, foreground, background string, minimum float64) {
		ratio := ContrastRatio(foreground, background)
		if ratio < minimum {
			fields[name+"."+field] = fmt.Sprintf(
				"This pair measures %.2f:1. Use at least %.1f:1.", ratio, minimum)
		}
	}
	check("ink", p.Ink, p.Ground, 7)
	check("muted", p.Muted, p.Ground, 4.5)
	check("surface_ink", p.Ink, p.Surface, 4.5)
	check("accent", p.Accent, p.Ground, 4.5)
	check("accent_ink", p.AccentInk, p.Accent, 4.5)
}

// ContrastRatio returns the WCAG 2 contrast ratio of two #rrggbb colours.
func ContrastRatio(foreground, background string) float64 {
	fg, ok := luminance(foreground)
	if !ok {
		return 0
	}
	bg, ok := luminance(background)
	if !ok {
		return 0
	}
	light, dark := math.Max(fg, bg), math.Min(fg, bg)
	return (light + 0.05) / (dark + 0.05)
}

func luminance(value string) (float64, bool) {
	if !hexColor.MatchString(strings.ToLower(value)) {
		return 0, false
	}
	number, err := strconv.ParseUint(strings.TrimPrefix(strings.ToLower(value), "#"), 16, 32)
	if err != nil {
		return 0, false
	}
	channel := func(shift uint) float64 {
		value := float64((number>>shift)&0xff) / 255
		if value <= 0.04045 {
			return value / 12.92
		}
		return math.Pow((value+0.055)/1.055, 2.4)
	}
	return 0.2126*channel(16) + 0.7152*channel(8) + 0.0722*channel(0), true
}

func validImagePath(value string) bool {
	if !strings.HasPrefix(value, "/images/") || len(value) > 300 {
		return false
	}
	return !strings.Contains(value, "..") && !strings.ContainsAny(value, "\\%?#")
}

func hasControl(value string) bool {
	for _, r := range value {
		if r == '\n' || r == '\t' {
			continue
		}
		if r < 0x20 || r == 0x7f {
			return true
		}
	}
	return false
}

func contains(values []string, value string) bool {
	for _, item := range values {
		if item == value {
			return true
		}
	}
	return false
}

func ParseLenient(raw string) Theme {
	theme, err := Parse(raw)
	if err != nil {
		return Default()
	}
	return theme
}
