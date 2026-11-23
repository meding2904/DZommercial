package productrules

import (
	"strings"
	"testing"
)

func TestValidNameBounds(t *testing.T) {
	cases := map[string]bool{
		"":                                  false,
		"x":                                 false,
		"olive oil":                         true,
		"  olive  ":                         true,
		"é":                                 false,
		"éé":                                true,
		strings.Repeat("a", MaxNameRunes):   true,
		strings.Repeat("a", MaxNameRunes+1): false,
	}
	for name, want := range cases {
		if got := ValidName(name); got != want {
			t.Errorf("ValidName(%q) = %v, want %v", name, got, want)
		}
	}
}

func TestValidPriceBounds(t *testing.T) {
	cases := map[int64]bool{-1: false, 0: false, 1: true, MaxPriceCents: true, MaxPriceCents + 1: false}
	for cents, want := range cases {
		if got := ValidPrice(cents); got != want {
			t.Errorf("ValidPrice(%d) = %v, want %v", cents, got, want)
		}
	}
}

func TestValidStockBounds(t *testing.T) {
	cases := map[int]bool{-1: false, 0: true, MaxStock: true, MaxStock + 1: false}
	for stock, want := range cases {
		if got := ValidStock(stock); got != want {
			t.Errorf("ValidStock(%d) = %v, want %v", stock, got, want)
		}
	}
}

func TestValidImagePath(t *testing.T) {
	valid := []string{
		"/images/products/cleansing-oil-1200.webp",
		"/api/images/img_0123456789abcdef01234567",
	}
	for _, image := range valid {
		if !ValidImagePath(image) {
			t.Errorf("%q should be valid", image)
		}
	}
	invalid := []string{
		"", "images/product.webp", "/static/product.webp",
		"/images/../secret", "/images/products/../../etc/passwd",
		`/images/\evil.webp`, "/images/%2e%2e/secret.webp",
		"/images/product.webp?x=1", "/images/product.webp#x",
		"http://images.example.com/product.webp",
		"javascript:alert(1)",
		"https://",
		"https://images.example.com/product.webp",
		"/api/images/img_",
		"/api/images/../secret",
		"/api/images/IMG_0123456789abcdef01234567",
		"/api/images/../../images/product.webp",
	}
	for _, image := range invalid {
		if ValidImagePath(image) {
			t.Errorf("%q should be invalid", image)
		}
	}
}

func TestValidImageID(t *testing.T) {
	if !ValidImageID("img_0123456789abcdef01234567") {
		t.Error("an uploaded image id should be valid")
	}
	for _, id := range []string{"", "img_", "img_0123456789ABCDEF01234567", "prd_0123456789abcdef01234567", " img_0123456789abcdef01234567"} {
		if ValidImageID(id) {
			t.Errorf("%q should be invalid", id)
		}
	}
}

func TestPlainText(t *testing.T) {
	valid := []string{"Olive oil", "Two\nlines", "Tab\there", ""}
	for _, value := range valid {
		if !PlainText(value) {
			t.Errorf("%q should be plain text", value)
		}
	}
	invalid := []string{"bell\x07", "null\x00", "del\x7f", "esc\x1b[0m"}
	for _, value := range invalid {
		if PlainText(value) {
			t.Errorf("%q should not be plain text", value)
		}
	}
}
