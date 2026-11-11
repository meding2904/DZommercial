package httpapi

import (
	"testing"

	"azahar/internal/store"
)

func TestValidEmail(t *testing.T) {
	valid := []string{"amina@example.com", "a.b+tag@sub.example.dz", "x@y.co"}
	for _, email := range valid {
		if !validEmail(email) {
			t.Errorf("%q should be valid", email)
		}
	}
	invalid := []string{"", "amina", "amina@", "@example.com", "Amina <amina@example.com>",
		"amina example@example.com", "amina@example.com "}
	for _, email := range invalid {
		if validEmail(email) {
			t.Errorf("%q should be invalid", email)
		}
	}
}

func TestValidSlug(t *testing.T) {
	valid := []string{"face", "orange-blossom-cleansing-oil", "a1-b2"}
	for _, slug := range valid {
		if !validSlug(slug) {
			t.Errorf("%q should be valid", slug)
		}
	}
	invalid := []string{"", "Face", "two words", "-leading", "trailing-", "double--hyphen", "under_score"}
	for _, slug := range invalid {
		if validSlug(slug) {
			t.Errorf("%q should be invalid", slug)
		}
	}
}

func TestValidImagePath(t *testing.T) {
	valid := []string{
		"/images/products/cleansing-oil-1200.webp",
		"https://images.example.com/product.webp",
	}
	for _, image := range valid {
		if !validImagePath(image) {
			t.Errorf("%q should be valid", image)
		}
	}
	invalid := []string{
		"", "images/product.webp", "/static/product.webp",
		"/images/../secret", "/images/products/../../etc/passwd",
		"http://images.example.com/product.webp",
		"javascript:alert(1)",
		"https://",
	}
	for _, image := range invalid {
		if validImagePath(image) {
			t.Errorf("%q should be invalid", image)
		}
	}
}

func TestValidPhone(t *testing.T) {
	valid := []string{"0550 00 00 00", "+213 550-00-00", "(0550) 000000"}
	for _, phone := range valid {
		if !validPhone(phone) {
			t.Errorf("%q should be valid", phone)
		}
	}
	invalid := []string{"", "12345", "call me", "0550 00 00 00<script>", "1234567890123456789012345678901"}
	for _, phone := range invalid {
		if validPhone(phone) {
			t.Errorf("%q should be invalid", phone)
		}
	}
}

func validTestProduct() store.Product {
	return store.Product{
		Name:        "Test oil",
		Slug:        "test-oil",
		Subtitle:    "A test",
		Description: "A short description.",
		PriceCents:  480000,
		Currency:    "DZD",
		Size:        "100 ml",
		Concerns:    []string{"dry"},
		Ingredients: []string{"Olive oil"},
		HowTo:       "Apply.",
		Image:       "/images/products/cleansing-oil-1200.webp",
		Gallery:     []string{"/images/products/cleansing-oil-1200.webp"},
		Stock:       10,
	}
}

func TestValidateProductAcceptsValidInput(t *testing.T) {
	if fields := validateProduct(validTestProduct()); len(fields) != 0 {
		t.Fatalf("unexpected fields: %v", fields)
	}
}

func TestValidateProductRejectsInvalidInput(t *testing.T) {
	product := validTestProduct()
	product.Name = "x"
	product.Slug = "Not A Slug"
	product.PriceCents = 0
	product.Stock = -1
	product.Image = "javascript:alert(1)"
	product.Concerns = make([]string, 13)
	product.Ingredients = make([]string, 13)
	product.HowTo = string(make([]rune, 1001))

	fields := validateProduct(product)
	for _, key := range []string{"name", "slug", "price_cents", "stock", "image", "concerns", "ingredients", "how_to"} {
		if _, ok := fields[key]; !ok {
			t.Errorf("expected a %s error, got %v", key, fields)
		}
	}
}

func TestValidateProductBoundaries(t *testing.T) {
	product := validTestProduct()
	product.PriceCents = 99999999
	product.Stock = 0
	product.CompareAtCents = nil
	if fields := validateProduct(product); len(fields) != 0 {
		t.Fatalf("boundary values should pass: %v", fields)
	}

	product.PriceCents = 100000000
	product.Stock = 100001
	if fields := validateProduct(product); fields["price_cents"] == "" || fields["stock"] == "" {
		t.Fatalf("out of range values should fail: %v", fields)
	}
}

func TestTrimAll(t *testing.T) {
	got := trimAll([]string{" dry ", "", "  ", "sensitive"})
	if len(got) != 2 || got[0] != "dry" || got[1] != "sensitive" {
		t.Fatalf("unexpected result: %#v", got)
	}
}
