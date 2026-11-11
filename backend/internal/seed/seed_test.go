package seed

import "testing"

func TestSeedProductContract(t *testing.T) {
	expected := []struct {
		slug  string
		name  string
		size  string
		price int64
		image string
	}{
		{"orange-blossom-cleansing-oil", "Orange blossom cleansing oil", "100 ml", 480000, "/images/products/cleansing-oil-1200.webp"},
		{"rosehip-and-olive-face-serum", "Rosehip and olive face serum", "30 ml", 690000, "/images/products/serum-1200.webp"},
		{"rose-clay-mask", "Rose clay mask", "75 g", 360000, "/images/products/clay-mask-1200.webp"},
		{"fig-and-sesame-body-balm", "Fig and sesame body balm", "120 ml", 420000, "/images/products/body-balm-1200.webp"},
		{"prickly-pear-face-oil", "Prickly pear face oil", "30 ml", 840000, "/images/products/prickly-pear-1200.webp"},
		{"cedar-and-myrrh-hand-cream", "Cedar and myrrh hand cream", "60 ml", 260000, "/images/products/hand-cream-1200.webp"},
		{"green-fig-body-oil", "Green fig body oil", "100 ml", 540000, "/images/products/fig-oil-1200.webp"},
		{"rose-petal-bath-soak", "Rose petal bath soak", "200 g", 320000, "/images/products/rose-soak-1200.webp"},
	}

	if len(products) != len(expected) {
		t.Fatalf("seeded product count %d, want %d", len(products), len(expected))
	}

	featured := 0
	for index, want := range expected {
		product := products[index]
		if product.Slug != want.slug || product.Name != want.name ||
			product.Size != want.size || product.PriceCents != want.price ||
			product.Image != want.image {
			t.Errorf("product %d = %+v, want slug %s name %s size %s price %d image %s",
				index, product, want.slug, want.name, want.size, want.price, want.image)
		}
		if len(product.Concerns) < 2 || len(product.Concerns) > 4 {
			t.Errorf("%s has %d concerns, want 2 to 4", product.Slug, len(product.Concerns))
		}
		if len(product.Ingredients) < 3 || len(product.Ingredients) > 4 {
			t.Errorf("%s has %d ingredients, want 3 to 4", product.Slug, len(product.Ingredients))
		}
		if product.Stock < 0 || product.Stock > 30 {
			t.Errorf("%s stock %d is outside 0 to 30", product.Slug, product.Stock)
		}
		if product.Featured {
			featured++
		}
	}
	if featured < 4 || featured > 6 {
		t.Errorf("%d featured products, want 4 to 6", featured)
	}

	categorySlugs := make(map[string]bool)
	for _, category := range categories {
		categorySlugs[category.Slug] = true
	}
	for _, product := range products {
		if !categorySlugs[product.Category] {
			t.Errorf("%s has unknown category %s", product.Slug, product.Category)
		}
	}
}
