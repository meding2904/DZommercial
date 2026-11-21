package store

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"time"
)

type ProductFilter struct {
	Category        string
	PluginID        string
	Concern         string
	Query           string
	Sort            string
	IncludeInactive bool
	Limit           int
}

func (s *Store) Categories(ctx context.Context) ([]Category, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT id, slug, name, description, sort_order, plugin_id FROM categories
		 ORDER BY plugin_id, sort_order, name`)
	if err != nil {
		return nil, fmt.Errorf("list categories: %w", err)
	}
	defer rows.Close()

	return scanCategories(rows)
}

// PublicCategories omits categories with no active product, so an installed
// plugin does not leave an empty shelf in the storefront navigation.
func (s *Store) PublicCategories(ctx context.Context) ([]Category, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT c.id, c.slug, c.name, c.description, c.sort_order, c.plugin_id
		 FROM categories c
		 WHERE EXISTS (SELECT 1 FROM products p WHERE p.category_id = c.id AND p.active = 1)
		 ORDER BY c.plugin_id, c.sort_order, c.name`)
	if err != nil {
		return nil, fmt.Errorf("list public categories: %w", err)
	}
	defer rows.Close()

	return scanCategories(rows)
}

func scanCategories(rows *sql.Rows) ([]Category, error) {
	categories := make([]Category, 0)
	for rows.Next() {
		var category Category
		if err := rows.Scan(&category.ID, &category.Slug, &category.Name,
			&category.Description, &category.SortOrder, &category.PluginID); err != nil {
			return nil, fmt.Errorf("scan category: %w", err)
		}
		categories = append(categories, category)
	}
	return categories, rows.Err()
}

func (s *Store) CategoryBySlug(ctx context.Context, slug string) (Category, error) {
	return s.CategoryBySlugTx(ctx, s.db, slug)
}

// UpsertCategory keeps the categories declared by a plugin in sync. The plugin
// owns the name, description and order; the category remains a core table so a
// product keeps one category foreign key.
func (s *Store) UpsertCategory(ctx context.Context, pluginID string, category Category) error {
	result, err := s.db.ExecContext(ctx,
		`INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
		 VALUES (?, ?, ?, ?, ?, ?)
		 ON CONFLICT(slug) DO UPDATE SET
		   name = excluded.name,
		   description = excluded.description,
		   sort_order = excluded.sort_order
		 WHERE categories.plugin_id = excluded.plugin_id`,
		"cat_"+pluginID+"_"+strings.ReplaceAll(category.Slug, "-", "_"),
		category.Slug, category.Name, category.Description, category.SortOrder, pluginID,
	)
	if err != nil {
		return fmt.Errorf("upsert category: %w", err)
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("upsert category: %w", err)
	}
	if affected == 0 {
		return fmt.Errorf("category %q belongs to another plugin", category.Slug)
	}
	return nil
}

// ReconcileProductCategories moves a product to its plugin's first category
// when an older category belongs to another plugin.
func (s *Store) ReconcileProductCategories(ctx context.Context) error {
	_, err := s.db.ExecContext(ctx,
		`UPDATE products SET category_id = (
		   SELECT c.id FROM categories c
		   WHERE c.plugin_id = products.plugin_id
		   ORDER BY c.sort_order, c.slug LIMIT 1
		 )
		 WHERE EXISTS (
		   SELECT 1 FROM categories c WHERE c.plugin_id = products.plugin_id
		 )
		 AND category_id NOT IN (
		   SELECT id FROM categories WHERE plugin_id = products.plugin_id
		 )`)
	if err != nil {
		return fmt.Errorf("reconcile product categories: %w", err)
	}
	return nil
}

func (s *Store) CategoryBySlugTx(ctx context.Context, q querier, slug string) (Category, error) {
	var category Category
	err := q.QueryRowContext(ctx,
		`SELECT id, slug, name, description, sort_order, plugin_id FROM categories WHERE slug = ?`, slug,
	).Scan(&category.ID, &category.Slug, &category.Name, &category.Description, &category.SortOrder,
		&category.PluginID)
	if err != nil {
		return Category{}, err
	}
	return category, nil
}

func (s *Store) CountProducts(ctx context.Context) (int, error) {
	var count int
	err := s.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM products`).Scan(&count)
	return count, err
}

func (s *Store) ListProducts(ctx context.Context, filter ProductFilter) ([]Product, error) {
	var where []string
	var args []any
	if !filter.IncludeInactive {
		where = append(where, "p.active = 1")
	}
	if filter.Category != "" {
		where = append(where, "c.slug = ?")
		args = append(args, filter.Category)
	}
	if filter.PluginID != "" {
		where = append(where, "p.plugin_id = ?")
		args = append(args, filter.PluginID)
	}
	if filter.Concern != "" {
		where = append(where,
			`(EXISTS (SELECT 1 FROM json_each(p.concerns) AS j WHERE j.value = ?)
			  OR EXISTS (SELECT 1 FROM json_each(json_extract(p.attributes, '$.concerns')) AS j WHERE j.value = ?))`)
		args = append(args, filter.Concern, filter.Concern)
	}
	if filter.Query != "" {
		where = append(where,
			"(p.name LIKE ? OR p.subtitle LIKE ? OR p.description LIKE ?)")
		like := "%" + filter.Query + "%"
		args = append(args, like, like, like)
	}

	order := "p.featured DESC, p.created_at DESC"
	now := FormatTime(time.Now().UTC())
	switch filter.Sort {
	case "price_asc":
		order = effectivePriceOrder + " ASC, p.name ASC"
		args = append(args, now, now)
	case "price_desc":
		order = effectivePriceOrder + " DESC, p.name ASC"
		args = append(args, now, now)
	case "new":
		order = "p.created_at DESC, p.name ASC"
	}

	query := `SELECT ` + productColumns + `
		FROM products p JOIN categories c ON c.id = p.category_id`
	if len(where) > 0 {
		query += " WHERE " + strings.Join(where, " AND ")
	}
	query += " ORDER BY " + order
	limit := filter.Limit
	if limit <= 0 || limit > 500 {
		limit = 200
	}
	query += " LIMIT ?"
	args = append(args, limit)

	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("list products: %w", err)
	}
	defer rows.Close()

	products := make([]Product, 0)
	for rows.Next() {
		product, err := scanProduct(rows)
		if err != nil {
			return nil, fmt.Errorf("scan product: %w", err)
		}
		products = append(products, product)
	}
	return products, rows.Err()
}

func (s *Store) ProductBySlug(ctx context.Context, slug string, activeOnly bool) (Product, error) {
	query := `SELECT ` + productColumns + `
		FROM products p JOIN categories c ON c.id = p.category_id WHERE p.slug = ?`
	if activeOnly {
		query += " AND p.active = 1"
	}
	product, err := scanProduct(s.db.QueryRowContext(ctx, query, slug))
	if err != nil {
		return Product{}, err
	}
	return product, nil
}

func (s *Store) ProductByID(ctx context.Context, id string, activeOnly bool) (Product, error) {
	return s.ProductByIDTx(ctx, s.db, id, activeOnly)
}

func (s *Store) ProductByIDTx(ctx context.Context, q querier, id string, activeOnly bool) (Product, error) {
	query := `SELECT ` + productColumns + `
		FROM products p JOIN categories c ON c.id = p.category_id WHERE p.id = ?`
	if activeOnly {
		query += " AND p.active = 1"
	}
	product, err := scanProduct(q.QueryRowContext(ctx, query, id))
	if err != nil {
		return Product{}, err
	}
	return product, nil
}

func (s *Store) DecrementStock(ctx context.Context, q querier, productID string, quantity int) (bool, error) {
	result, err := q.ExecContext(ctx,
		`UPDATE products SET stock = stock - ?, updated_at = ? WHERE id = ? AND stock >= ?`,
		quantity, FormatTime(time.Now().UTC()), productID, quantity,
	)
	if err != nil {
		return false, fmt.Errorf("decrement stock: %w", err)
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return false, err
	}
	return affected == 1, nil
}

func (s *Store) IncrementStock(ctx context.Context, q querier, productID string, quantity int) error {
	if quantity <= 0 {
		return nil
	}
	_, err := q.ExecContext(ctx,
		`UPDATE products SET stock = stock + ?, updated_at = ? WHERE id = ?`,
		quantity, FormatTime(time.Now().UTC()), productID,
	)
	if err != nil {
		return fmt.Errorf("increment stock: %w", err)
	}
	return nil
}

func (s *Store) RelatedProducts(ctx context.Context, categoryID, excludeID string, limit int) ([]Product, error) {
	if limit <= 0 {
		limit = 4
	}
	rows, err := s.db.QueryContext(ctx,
		`SELECT `+productColumns+`
		 FROM products p JOIN categories c ON c.id = p.category_id
		 WHERE p.active = 1 AND p.category_id = ? AND p.id <> ?
		 ORDER BY p.featured DESC, p.created_at DESC LIMIT ?`,
		categoryID, excludeID, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("related products: %w", err)
	}
	defer rows.Close()

	products := make([]Product, 0)
	for rows.Next() {
		product, err := scanProduct(rows)
		if err != nil {
			return nil, fmt.Errorf("scan related product: %w", err)
		}
		products = append(products, product)
	}
	return products, rows.Err()
}

func (s *Store) CreateProduct(ctx context.Context, q querier, product Product) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO products (id, slug, name, subtitle, description, price_cents,
		 compare_at_cents, sale_price_cents, sale_starts_at, sale_ends_at, currency, size,
		 category_id, concerns, ingredients,
		 how_to, image, gallery, stock, featured, active, plugin_id, attributes,
		 created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		product.ID, product.Slug, product.Name, product.Subtitle, product.Description,
		product.PriceCents, product.CompareAtCents, product.SalePriceCents,
		FormatOptionalTime(product.SaleStartsAt), FormatOptionalTime(product.SaleEndsAt),
		defaultCurrency(product.Currency),
		product.Size, product.Category.ID, encodeJSON(product.Concerns),
		encodeJSON(product.Ingredients), product.HowTo, product.Image,
		encodeJSON(product.Gallery), product.Stock, boolInt(product.Featured),
		boolInt(product.Active), NormalizePluginID(product.PluginID), encodeStringListMap(product.Attributes),
		FormatTime(product.CreatedAt), FormatTime(product.UpdatedAt),
	)
	if err != nil {
		return fmt.Errorf("create product: %w", err)
	}
	return nil
}

func (s *Store) UpdateProduct(ctx context.Context, q querier, product Product) error {
	result, err := q.ExecContext(ctx,
		`UPDATE products SET slug = ?, name = ?, subtitle = ?, description = ?,
		 price_cents = ?, compare_at_cents = ?, sale_price_cents = ?, sale_starts_at = ?,
		 sale_ends_at = ?, currency = ?, size = ?, category_id = ?,
		 concerns = ?, ingredients = ?, how_to = ?, image = ?, gallery = ?, stock = ?,
		 featured = ?, active = ?, plugin_id = ?, attributes = ?, updated_at = ?
		 WHERE id = ?`,
		product.Slug, product.Name, product.Subtitle, product.Description,
		product.PriceCents, product.CompareAtCents,
		product.SalePriceCents, FormatOptionalTime(product.SaleStartsAt),
		FormatOptionalTime(product.SaleEndsAt), defaultCurrency(product.Currency),
		product.Size, product.Category.ID, encodeJSON(product.Concerns),
		encodeJSON(product.Ingredients), product.HowTo, product.Image,
		encodeJSON(product.Gallery), product.Stock, boolInt(product.Featured),
		boolInt(product.Active), NormalizePluginID(product.PluginID),
		encodeStringListMap(product.Attributes), FormatTime(product.UpdatedAt), product.ID,
	)
	if err != nil {
		return fmt.Errorf("update product: %w", err)
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if affected == 0 {
		return sql.ErrNoRows
	}
	return nil
}

func (s *Store) SetProductActive(ctx context.Context, q querier, id string, active bool, now time.Time) error {
	result, err := q.ExecContext(ctx,
		`UPDATE products SET active = ?, updated_at = ? WHERE id = ?`,
		boolInt(active), FormatTime(now), id,
	)
	if err != nil {
		return fmt.Errorf("archive product: %w", err)
	}
	affected, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if affected == 0 {
		return sql.ErrNoRows
	}
	return nil
}

func boolInt(value bool) int {
	if value {
		return 1
	}
	return 0
}

const (
	// CorePluginID marks a product that uses no plugin product type.
	CorePluginID = "core"
)

func NormalizePluginID(pluginID string) string {
	if pluginID == "" {
		return CorePluginID
	}
	return pluginID
}

func defaultCurrency(currency string) string {
	if currency == "" {
		return "DZD"
	}
	return currency
}
