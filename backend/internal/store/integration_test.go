package store_test

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"azahar/internal/db"
	"azahar/internal/ids"
	"azahar/internal/store"
)

func TestProductLifecycleAgainstLibSQL(t *testing.T) {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("set AZAHAR_TEST_DATABASE_URL to run the libSQL integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	handle, err := db.Open(ctx, dsn)
	if err != nil {
		t.Fatalf("open database: %v", err)
	}
	t.Cleanup(func() { handle.Close() })
	if err := db.Migrate(ctx, handle); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	repo := store.New(handle)
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	categoryID := ids.New("cat_test")
	categorySlug := "test-category-" + suffix

	if _, err := handle.ExecContext(ctx,
		`INSERT INTO categories (id, slug, name, description, sort_order) VALUES (?, ?, ?, ?, ?)`,
		categoryID, categorySlug, "Test category", "Temporary integration-test category.", 99,
	); err != nil {
		t.Fatalf("insert category: %v", err)
	}
	t.Cleanup(func() {
		if _, err := handle.Exec(`DELETE FROM products WHERE id = ?`, "prd_test_"+suffix); err != nil {
			t.Errorf("cleanup product: %v", err)
		}
		if _, err := handle.Exec(`DELETE FROM categories WHERE id = ?`, categoryID); err != nil {
			t.Errorf("cleanup category: %v", err)
		}
	})

	category, err := repo.CategoryBySlug(ctx, categorySlug)
	if err != nil {
		t.Fatalf("load category: %v", err)
	}
	now := time.Now().UTC()
	product := store.Product{
		ID:          "prd_test_" + suffix,
		Slug:        "test-product-" + suffix,
		Name:        "Integration test oil",
		Subtitle:    "Temporary",
		Description: "A temporary product used by the integration test.",
		PriceCents:  123400,
		Currency:    "DZD",
		Size:        "10 ml",
		Category:    category,
		Concerns:    []string{"dry"},
		Ingredients: []string{"Olive oil"},
		HowTo:       "Apply.",
		Image:       "/images/products/serum-1200.webp",
		Gallery:     []string{"/images/products/serum-1200.webp"},
		Stock:       3,
		Featured:    false,
		Active:      true,
		CreatedAt:   now,
		UpdatedAt:   now,
	}
	if err := repo.CreateProduct(ctx, handle, product); err != nil {
		t.Fatalf("create product: %v", err)
	}

	loaded, err := repo.ProductBySlug(ctx, product.Slug, true)
	if err != nil {
		t.Fatalf("load product: %v", err)
	}
	if loaded.ID != product.ID || loaded.PriceCents != product.PriceCents {
		t.Fatalf("unexpected product: %+v", loaded)
	}
	if len(loaded.Concerns) != 1 || loaded.Concerns[0] != "dry" {
		t.Fatalf("concerns did not round trip: %#v", loaded.Concerns)
	}

	ok, err := repo.DecrementStock(ctx, handle, product.ID, 2)
	if err != nil || !ok {
		t.Fatalf("first decrement: ok=%v err=%v", ok, err)
	}
	ok, err = repo.DecrementStock(ctx, handle, product.ID, 2)
	if err != nil {
		t.Fatalf("second decrement: %v", err)
	}
	if ok {
		t.Fatal("stock should not go below zero")
	}

	loaded.Featured = true
	loaded.Stock = 1
	loaded.UpdatedAt = now.Add(time.Second)
	if err := repo.UpdateProduct(ctx, handle, loaded); err != nil {
		t.Fatalf("update product: %v", err)
	}
	updated, err := repo.ProductByID(ctx, product.ID, true)
	if err != nil {
		t.Fatalf("reload product: %v", err)
	}
	if !updated.Featured || updated.Stock != 1 {
		t.Fatalf("update did not apply: %+v", updated)
	}

	if err := repo.SetProductActive(ctx, handle, product.ID, false, now.Add(2*time.Second)); err != nil {
		t.Fatalf("archive product: %v", err)
	}
	if _, err := repo.ProductBySlug(ctx, product.Slug, true); err == nil {
		t.Fatal("archived product should not be public")
	}
	if _, err := repo.ProductBySlug(ctx, product.Slug, false); err != nil {
		t.Fatalf("archived product should still exist for admin: %v", err)
	}
}
