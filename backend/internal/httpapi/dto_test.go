package httpapi

import (
	"encoding/json"
	"testing"
	"time"

	"azahar/internal/store"
)

func TestProductJSONContract(t *testing.T) {
	product := store.Product{
		ID:          "prd_01",
		Slug:        "orange-blossom-cleansing-oil",
		Name:        "Orange blossom cleansing oil",
		PriceCents:  480000,
		Currency:    "DZD",
		Size:        "100 ml",
		Category:    store.Category{Slug: "face", Name: "Face"},
		Image:       "/images/products/cleansing-oil-1200.webp",
		Stock:       24,
		Featured:    true,
		Active:      true,
		CreatedAt:   time.Now().UTC(),
		UpdatedAt:   time.Now().UTC(),
		Concerns:    nil,
		Ingredients: nil,
		Gallery:     nil,
	}

	buf, err := json.Marshal(toProduct(product, true))
	if err != nil {
		t.Fatal(err)
	}
	var body map[string]any
	if err := json.Unmarshal(buf, &body); err != nil {
		t.Fatal(err)
	}

	required := []string{
		"id", "slug", "name", "subtitle", "description", "price_cents",
		"compare_at_cents", "currency", "size", "category", "concerns",
		"ingredients", "how_to", "image", "gallery", "stock", "featured",
		"active", "created_at", "updated_at",
	}
	for _, key := range required {
		if _, ok := body[key]; !ok {
			t.Errorf("missing key %s", key)
		}
	}

	category, ok := body["category"].(map[string]any)
	if !ok {
		t.Fatalf("category is %T, want an object", body["category"])
	}
	if category["slug"] != "face" || category["name"] != "Face" {
		t.Errorf("unexpected category: %#v", category)
	}
	for _, key := range []string{"concerns", "ingredients", "gallery"} {
		values, ok := body[key].([]any)
		if !ok {
			t.Errorf("%s is %T, want an array", key, body[key])
		}
		if values == nil {
			t.Errorf("%s is null, want an empty array", key)
		}
	}
	if body["image"] != "/images/products/cleansing-oil-1200.webp" {
		t.Errorf("unexpected image: %v", body["image"])
	}
}
