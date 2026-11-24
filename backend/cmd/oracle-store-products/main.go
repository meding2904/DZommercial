// Command oracle-store-products is the differential oracle for the Rust
// product store port. It reads one JSON command per line and writes one JSON
// result per line. Every run owns an `it_products_<nanos>` fixture namespace
// and deletes it on the cleanup command.
package main

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	"azahar/internal/db"
	"azahar/internal/store"
)

type wireCategory struct {
	ID          string `json:"id"`
	Slug        string `json:"slug"`
	Name        string `json:"name"`
	Description string `json:"description"`
	SortOrder   int    `json:"sort_order"`
	PluginID    string `json:"plugin_id"`
}

type wireProduct struct {
	ID             string              `json:"id"`
	Slug           string              `json:"slug"`
	Name           string              `json:"name"`
	Subtitle       string              `json:"subtitle"`
	Description    string              `json:"description"`
	PriceCents     int64               `json:"price_cents"`
	CompareAtCents *int64              `json:"compare_at_cents"`
	SalePriceCents *int64              `json:"sale_price_cents"`
	SaleStartsAt   string              `json:"sale_starts_at"`
	SaleEndsAt     string              `json:"sale_ends_at"`
	Currency       string              `json:"currency"`
	Size           string              `json:"size"`
	CategoryID     string              `json:"category_id"`
	Concerns       []string            `json:"concerns"`
	Ingredients    []string            `json:"ingredients"`
	HowTo          string              `json:"how_to"`
	Image          string              `json:"image"`
	Gallery        []string            `json:"gallery"`
	Stock          int                 `json:"stock"`
	Featured       bool                `json:"featured"`
	Active         bool                `json:"active"`
	PluginID       string              `json:"plugin_id"`
	Attributes     map[string][]string `json:"attributes"`
	CreatedAt      string              `json:"created_at"`
	UpdatedAt      string              `json:"updated_at"`
}

type wireFilter struct {
	Category        string `json:"category"`
	PluginID        string `json:"plugin_id"`
	Concern         string `json:"concern"`
	Query           string `json:"query"`
	Sort            string `json:"sort"`
	IncludeInactive bool   `json:"include_inactive"`
	Limit           int    `json:"limit"`
}

type slotFixtures struct {
	PluginID      string        `json:"plugin_id"`
	OtherPluginID string        `json:"other_plugin_id"`
	CategoryA     wireCategory  `json:"category_a"`
	CategoryB     wireCategory  `json:"category_b"`
	CategoryX     wireCategory  `json:"category_x"`
	Products      []wireProduct `json:"products"`
}

type fixtures struct {
	Prefix string                  `json:"prefix"`
	Slots  map[string]slotFixtures `json:"slots"`
}

type request struct {
	Op         string       `json:"op"`
	Slot       string       `json:"slot"`
	ID         string       `json:"id"`
	Slug       string       `json:"slug"`
	ActiveOnly bool         `json:"active_only"`
	Active     bool         `json:"active"`
	Quantity   int          `json:"quantity"`
	Limit      int          `json:"limit"`
	CategoryID string       `json:"category_id"`
	ExcludeID  string       `json:"exclude_id"`
	Now        string       `json:"now"`
	PluginID   string       `json:"plugin_id"`
	Category   wireCategory `json:"category"`
	Filter     wireFilter   `json:"filter"`
	Product    wireProduct  `json:"product"`
}

type oracle struct {
	db       *sql.DB
	repo     *store.Store
	prefix   string
	fixtures fixtures
}

func main() {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		dsn = "http://127.0.0.1:8880"
	}

	ctx := context.Background()
	handle, err := db.Open(ctx, dsn)
	if err != nil {
		fmt.Fprintln(os.Stderr, "open database:", err)
		os.Exit(1)
	}
	defer handle.Close()
	if err := db.Migrate(ctx, handle); err != nil {
		fmt.Fprintln(os.Stderr, "migrate database:", err)
		os.Exit(1)
	}

	o := &oracle{
		db:     handle,
		repo:   store.New(handle),
		prefix: fmt.Sprintf("it_products_%d", time.Now().UnixNano()),
	}

	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 64*1024), 8*1024*1024)
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()

	for in.Scan() {
		var req request
		var resp map[string]any
		if err := json.Unmarshal(in.Bytes(), &req); err != nil {
			resp = map[string]any{"ok": false, "error": "bad request: " + err.Error()}
		} else {
			resp = o.handle(ctx, req)
		}

		payload, err := json.Marshal(resp)
		if err != nil {
			payload = []byte(`{"ok":false,"error":"encode response"}`)
		}
		if _, err := out.Write(payload); err != nil {
			fmt.Fprintln(os.Stderr, "write:", err)
			os.Exit(1)
		}
		if err := out.WriteByte('\n'); err != nil {
			fmt.Fprintln(os.Stderr, "write:", err)
			os.Exit(1)
		}
		if err := out.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "flush:", err)
			os.Exit(1)
		}
	}
	if err := in.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "read:", err)
		os.Exit(1)
	}
}

func (o *oracle) handle(ctx context.Context, req request) map[string]any {
	switch req.Op {
	case "seed":
		if err := o.seed(ctx); err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": o.fixtures}
	case "cleanup":
		if err := o.cleanup(ctx); err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": true}
	case "list_products":
		products, err := o.repo.ListProducts(ctx, store.ProductFilter{
			Category:        req.Filter.Category,
			PluginID:        req.Filter.PluginID,
			Concern:         req.Filter.Concern,
			Query:           req.Filter.Query,
			Sort:            req.Filter.Sort,
			IncludeInactive: req.Filter.IncludeInactive,
			Limit:           req.Filter.Limit,
		})
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productsJSON(products)}
	case "product_by_slug":
		product, err := o.repo.ProductBySlug(ctx, req.Slug, req.ActiveOnly)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productJSON(product)}
	case "product_by_id":
		product, err := o.repo.ProductByID(ctx, req.ID, req.ActiveOnly)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productJSON(product)}
	case "categories":
		categories, err := o.repo.Categories(ctx)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": categoriesJSON(categories)}
	case "public_categories":
		categories, err := o.repo.PublicCategories(ctx)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": categoriesJSON(categories)}
	case "category_by_slug":
		category, err := o.repo.CategoryBySlug(ctx, req.Slug)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": categoryJSON(category)}
	case "count_products":
		count, err := o.repo.CountProducts(ctx)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": count}
	case "create_product":
		if err := o.repo.CreateProduct(ctx, o.db, req.Product.toStore()); err != nil {
			return failure(err)
		}
		product, err := o.repo.ProductByID(ctx, req.Product.ID, false)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productJSON(product)}
	case "update_product":
		if err := o.repo.UpdateProduct(ctx, o.db, req.Product.toStore()); err != nil {
			return failure(err)
		}
		product, err := o.repo.ProductByID(ctx, req.Product.ID, false)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productJSON(product)}
	case "set_active":
		now := store.ParseTime(req.Now)
		if err := o.repo.SetProductActive(ctx, o.db, req.ID, req.Active, now); err != nil {
			return failure(err)
		}
		product, err := o.repo.ProductByID(ctx, req.ID, false)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productJSON(product)}
	case "decrement_stock":
		sufficient, err := o.repo.DecrementStock(ctx, o.db, req.ID, req.Quantity)
		if err != nil {
			return failure(err)
		}
		product, err := o.repo.ProductByID(ctx, req.ID, false)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": map[string]any{
			"sufficient": sufficient,
			"stock":      product.Stock,
		}}
	case "increment_stock":
		if err := o.repo.IncrementStock(ctx, o.db, req.ID, req.Quantity); err != nil {
			return failure(err)
		}
		product, err := o.repo.ProductByID(ctx, req.ID, false)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": map[string]any{
			"sufficient": true,
			"stock":      product.Stock,
		}}
	case "related_products":
		products, err := o.repo.RelatedProducts(ctx, req.CategoryID, req.ExcludeID, req.Limit)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productsJSON(products)}
	case "upsert_category":
		if err := o.repo.UpsertCategory(ctx, req.PluginID, req.Category.toStore()); err != nil {
			return failure(err)
		}
		category, err := o.repo.CategoryBySlug(ctx, req.Category.Slug)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": categoryJSON(category)}
	case "reconcile":
		if err := o.repo.ReconcileProductCategories(ctx); err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": true}
	case "seed_mismatch":
		product, err := o.seedMismatch(ctx, req.Slot)
		if err != nil {
			return failure(err)
		}
		return map[string]any{"ok": true, "value": productJSON(product)}
	default:
		return map[string]any{"ok": false, "error": "unknown op"}
	}
}

func (o *oracle) seed(ctx context.Context) error {
	if o.fixtures.Slots != nil {
		return nil
	}
	base := time.Now().UTC().Truncate(time.Second)
	slots := make(map[string]slotFixtures, 2)
	for _, slot := range []string{"g", "r"} {
		fixture, err := o.seedSlot(ctx, slot, base)
		if err != nil {
			return err
		}
		slots[slot] = fixture
	}
	o.fixtures = fixtures{Prefix: o.prefix, Slots: slots}
	return nil
}

func (o *oracle) seedSlot(ctx context.Context, slot string, base time.Time) (slotFixtures, error) {
	pluginID := o.prefix + "_plug_" + slot
	otherPluginID := o.prefix + "_other_" + slot
	categoryA, err := o.insertCategory(ctx, wireCategory{
		ID:          o.prefix + "_cat_a_" + slot,
		Slug:        o.prefix + "-cat-a-" + slot,
		Name:        "Alpha Shelf",
		Description: "First shelf.",
		SortOrder:   10,
		PluginID:    pluginID,
	})
	if err != nil {
		return slotFixtures{}, err
	}
	categoryB, err := o.insertCategory(ctx, wireCategory{
		ID:          o.prefix + "_cat_b_" + slot,
		Slug:        o.prefix + "-cat-b-" + slot,
		Name:        "Beta Shelf",
		Description: "Second shelf.",
		SortOrder:   20,
		PluginID:    pluginID,
	})
	if err != nil {
		return slotFixtures{}, err
	}
	categoryX, err := o.insertCategory(ctx, wireCategory{
		ID:          o.prefix + "_cat_x_" + slot,
		Slug:        o.prefix + "-cat-x-" + slot,
		Name:        "Foreign Shelf",
		Description: "Another plugin's shelf.",
		SortOrder:   30,
		PluginID:    otherPluginID,
	})
	if err != nil {
		return slotFixtures{}, err
	}

	created := func(offset int) string {
		return store.FormatTime(time.Date(2026, 1, 1, 0, 0, offset, 0, time.UTC))
	}
	activeStart := store.FormatTime(base.Add(-time.Hour))
	activeEnd := store.FormatTime(base.Add(time.Hour))
	expiredStart := store.FormatTime(base.Add(-48 * time.Hour))
	expiredEnd := store.FormatTime(base.Add(-24 * time.Hour))
	futureStart := store.FormatTime(base.Add(24 * time.Hour))
	futureEnd := store.FormatTime(base.Add(48 * time.Hour))

	price := func(value int64) *int64 { return &value }

	products := []wireProduct{
		{
			ID: o.prefix + "_prd_1_" + slot, Slug: o.prefix + "-prd-1-" + slot,
			Name: "Argan Day Oil", Subtitle: "Bright", Description: "Light everyday oil.",
			PriceCents: 100000, CompareAtCents: price(120000), Currency: "DZD", Size: "30 ml",
			CategoryID: categoryA.ID, Concerns: []string{"dry", "mature"},
			Ingredients: []string{"Argan"}, HowTo: "Massage.", Image: "/images/a.webp",
			Gallery: []string{"/images/a.webp", "/images/a2.webp"},
			Stock:   5, Featured: true, Active: true, PluginID: pluginID,
			Attributes: map[string][]string{"size": {"30 ml"}, "concerns": {"dry"}},
			CreatedAt:  created(1), UpdatedAt: created(1),
		},
		{
			ID: o.prefix + "_prd_2_" + slot, Slug: o.prefix + "-prd-2-" + slot,
			Name: "Rose Night Oil", Subtitle: "Calm", Description: "Night repair oil.",
			PriceCents: 200000, SalePriceCents: price(150000),
			SaleStartsAt: activeStart, SaleEndsAt: activeEnd,
			Currency: "DZD", Size: "20 ml", CategoryID: categoryA.ID,
			Concerns: []string{"sensitive"}, Gallery: []string{},
			Stock: 2, Active: true, PluginID: pluginID,
			Attributes: map[string][]string{}, CreatedAt: created(2), UpdatedAt: created(2),
		},
		{
			ID: o.prefix + "_prd_3_" + slot, Slug: o.prefix + "-prd-3-" + slot,
			Name: "Clay Mask", Subtitle: "Purifying clay mask", Description: "Weekly clay ritual.",
			PriceCents: 50000, SalePriceCents: price(40000),
			SaleStartsAt: expiredStart, SaleEndsAt: expiredEnd,
			Currency: "DZD", Size: "100 g", CategoryID: categoryA.ID,
			Gallery: []string{}, Stock: 0, Active: true, PluginID: pluginID,
			Attributes: map[string][]string{}, CreatedAt: created(3), UpdatedAt: created(3),
		},
		{
			ID: o.prefix + "_prd_4_" + slot, Slug: o.prefix + "-prd-4-" + slot,
			Name: "Future Serum", Subtitle: "Soon", Description: "Not on sale yet.",
			PriceCents: 300000, SalePriceCents: price(250000),
			SaleStartsAt: futureStart, SaleEndsAt: futureEnd,
			Currency: "DZD", Size: "15 ml", CategoryID: categoryA.ID,
			Gallery: []string{}, Stock: 7, Featured: true, Active: false, PluginID: pluginID,
			Attributes: map[string][]string{}, CreatedAt: created(4), UpdatedAt: created(4),
		},
		{
			ID: o.prefix + "_prd_5_" + slot, Slug: o.prefix + "-prd-5-" + slot,
			Name: "Hidden Balm", Subtitle: "Store room", Description: "Kept out of the storefront.",
			PriceCents: 120000, Currency: "DZD", Size: "50 ml", CategoryID: categoryB.ID,
			Gallery: []string{}, Stock: 3, Active: false, PluginID: pluginID,
			Attributes: map[string][]string{}, CreatedAt: created(5), UpdatedAt: created(5),
		},
		{
			ID: o.prefix + "_prd_6_" + slot, Slug: o.prefix + "-prd-6-" + slot,
			Name: "Citrus Mist", Subtitle: "Fresh", Description: "A verbena finishing mist.",
			PriceCents: 90000, Currency: "DZD", Size: "60 ml", CategoryID: categoryA.ID,
			Gallery: []string{}, Stock: 10, Active: true, PluginID: pluginID,
			Attributes: map[string][]string{"concerns": {"dry"}},
			CreatedAt:  created(6), UpdatedAt: created(6),
		},
	}
	seeded := make([]wireProduct, 0, len(products))
	for _, product := range products {
		if err := o.repo.CreateProduct(ctx, o.db, product.toStore()); err != nil {
			return slotFixtures{}, fmt.Errorf("seed product %s: %w", product.ID, err)
		}
		loaded, err := o.repo.ProductByID(ctx, product.ID, false)
		if err != nil {
			return slotFixtures{}, fmt.Errorf("reload product %s: %w", product.ID, err)
		}
		seeded = append(seeded, productWire(loaded))
	}

	return slotFixtures{
		PluginID:      pluginID,
		OtherPluginID: otherPluginID,
		CategoryA:     categoryA,
		CategoryB:     categoryB,
		CategoryX:     categoryX,
		Products:      seeded,
	}, nil
}

func (o *oracle) insertCategory(ctx context.Context, category wireCategory) (wireCategory, error) {
	_, err := o.db.ExecContext(ctx,
		`INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		category.ID, category.Slug, category.Name, category.Description,
		category.SortOrder, category.PluginID,
	)
	if err != nil {
		return wireCategory{}, fmt.Errorf("seed category %s: %w", category.Slug, err)
	}
	return category, nil
}

func (o *oracle) seedMismatch(ctx context.Context, slot string) (store.Product, error) {
	fixture, ok := o.fixtures.Slots[slot]
	if !ok {
		return store.Product{}, errors.New("seed first")
	}
	now := store.FormatTime(time.Date(2026, 1, 2, 0, 0, 0, 0, time.UTC))
	product := wireProduct{
		ID: o.prefix + "_rec_" + slot, Slug: o.prefix + "-rec-" + slot,
		Name: "Reconcile Product", Description: "Sits on another plugin's shelf.",
		PriceCents: 70000, Currency: "DZD", CategoryID: fixture.CategoryX.ID,
		Gallery: []string{}, Stock: 1, Active: true, PluginID: fixture.PluginID,
		Attributes: map[string][]string{}, CreatedAt: now, UpdatedAt: now,
	}
	if err := o.repo.CreateProduct(ctx, o.db, product.toStore()); err != nil {
		return store.Product{}, err
	}
	return o.repo.ProductByID(ctx, product.ID, false)
}

func (o *oracle) cleanup(ctx context.Context) error {
	like := o.prefix + "%"
	if _, err := o.db.ExecContext(ctx, `DELETE FROM products WHERE id LIKE ?`, like); err != nil {
		return fmt.Errorf("cleanup products: %w", err)
	}
	if _, err := o.db.ExecContext(ctx,
		`DELETE FROM categories WHERE id LIKE ? OR id LIKE ? OR slug LIKE ?`,
		like, "cat_"+like, like,
	); err != nil {
		return fmt.Errorf("cleanup categories: %w", err)
	}
	return nil
}

func productWire(product store.Product) wireProduct {
	return wireProduct{
		ID: product.ID, Slug: product.Slug, Name: product.Name, Subtitle: product.Subtitle,
		Description: product.Description, PriceCents: product.PriceCents,
		CompareAtCents: product.CompareAtCents, SalePriceCents: product.SalePriceCents,
		SaleStartsAt: store.FormatOptionalTime(product.SaleStartsAt),
		SaleEndsAt:   store.FormatOptionalTime(product.SaleEndsAt),
		Currency:     product.Currency, Size: product.Size, CategoryID: product.Category.ID,
		Concerns: product.Concerns, Ingredients: product.Ingredients, HowTo: product.HowTo,
		Image: product.Image, Gallery: product.Gallery, Stock: product.Stock,
		Featured: product.Featured, Active: product.Active, PluginID: product.PluginID,
		Attributes: product.Attributes,
		CreatedAt:  store.FormatOptionalTime(product.CreatedAt),
		UpdatedAt:  store.FormatOptionalTime(product.UpdatedAt),
	}
}

func (w wireProduct) toStore() store.Product {
	return store.Product{
		ID: w.ID, Slug: w.Slug, Name: w.Name, Subtitle: w.Subtitle, Description: w.Description,
		PriceCents: w.PriceCents, CompareAtCents: w.CompareAtCents,
		SalePriceCents: w.SalePriceCents,
		SaleStartsAt:   store.ParseTime(w.SaleStartsAt),
		SaleEndsAt:     store.ParseTime(w.SaleEndsAt),
		Currency:       w.Currency, Size: w.Size,
		Category: store.Category{ID: w.CategoryID},
		Concerns: w.Concerns, Ingredients: w.Ingredients, HowTo: w.HowTo, Image: w.Image,
		Gallery: w.Gallery, Stock: w.Stock, Featured: w.Featured, Active: w.Active,
		PluginID: w.PluginID, Attributes: w.Attributes,
		CreatedAt: store.ParseTime(w.CreatedAt), UpdatedAt: store.ParseTime(w.UpdatedAt),
	}
}

func (w wireCategory) toStore() store.Category {
	return store.Category{
		ID: w.ID, Slug: w.Slug, Name: w.Name, Description: w.Description,
		SortOrder: w.SortOrder, PluginID: w.PluginID,
	}
}

func failure(err error) map[string]any {
	if errors.Is(err, sql.ErrNoRows) {
		return map[string]any{"ok": false, "error": "not_found"}
	}
	return map[string]any{"ok": false, "error": err.Error()}
}

func productsJSON(products []store.Product) []map[string]any {
	result := make([]map[string]any, 0, len(products))
	for _, product := range products {
		result = append(result, productJSON(product))
	}
	return result
}

func categoriesJSON(categories []store.Category) []map[string]any {
	result := make([]map[string]any, 0, len(categories))
	for _, category := range categories {
		result = append(result, categoryJSON(category))
	}
	return result
}

func categoryJSON(category store.Category) map[string]any {
	return map[string]any{
		"id":          category.ID,
		"slug":        category.Slug,
		"name":        category.Name,
		"description": category.Description,
		"sort_order":  category.SortOrder,
		"plugin_id":   category.PluginID,
	}
}

func productJSON(product store.Product) map[string]any {
	var compareAt, salePrice any
	if product.CompareAtCents != nil {
		compareAt = *product.CompareAtCents
	}
	if product.SalePriceCents != nil {
		salePrice = *product.SalePriceCents
	}
	return map[string]any{
		"id":               product.ID,
		"slug":             product.Slug,
		"name":             product.Name,
		"subtitle":         product.Subtitle,
		"description":      product.Description,
		"price_cents":      product.PriceCents,
		"compare_at_cents": compareAt,
		"sale_price_cents": salePrice,
		"sale_starts_at":   timeJSON(product.SaleStartsAt),
		"sale_ends_at":     timeJSON(product.SaleEndsAt),
		"currency":         product.Currency,
		"size":             product.Size,
		"category":         categoryJSON(product.Category),
		"concerns":         product.Concerns,
		"ingredients":      product.Ingredients,
		"how_to":           product.HowTo,
		"image":            product.Image,
		"gallery":          product.Gallery,
		"stock":            product.Stock,
		"featured":         product.Featured,
		"active":           product.Active,
		"plugin_id":        product.PluginID,
		"attributes":       product.Attributes,
		"created_at":       timeJSON(product.CreatedAt),
		"updated_at":       timeJSON(product.UpdatedAt),
	}
}

func timeJSON(value time.Time) any {
	if value.IsZero() {
		return nil
	}
	return store.FormatTime(value)
}
