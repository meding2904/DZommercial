package store

import (
	"context"
	"database/sql"
	"fmt"
	"strings"
	"time"
)

type AdminOrderFilter struct {
	Status string
	Query  string
	Limit  int
}

func (s *Store) CountOrders(ctx context.Context) (int, error) {
	var count int
	err := s.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM orders`).Scan(&count)
	return count, err
}

func (s *Store) NextOrderNumber(ctx context.Context, q querier, year int) (string, error) {
	if _, err := q.ExecContext(ctx,
		`INSERT INTO order_sequences (year, last_value) VALUES (?, 1)
		 ON CONFLICT(year) DO UPDATE SET last_value = last_value + 1`, year,
	); err != nil {
		return "", fmt.Errorf("advance order sequence: %w", err)
	}
	var last int
	if err := q.QueryRowContext(ctx,
		`SELECT last_value FROM order_sequences WHERE year = ?`, year,
	).Scan(&last); err != nil {
		return "", fmt.Errorf("read order sequence: %w", err)
	}
	return fmt.Sprintf("AZ-%d-%04d", year, last), nil
}

func (s *Store) CreateOrder(ctx context.Context, q querier, order Order, items []OrderItem) error {
	_, err := q.ExecContext(ctx,
		`INSERT INTO orders (id, number, user_id, name, email, phone, address, city,
		 notes, subtotal_cents, shipping_cents, total_cents, currency, status,
		 token_hash, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		order.ID, order.Number, order.UserID, order.Name, order.Email, order.Phone,
		order.Address, order.City, order.Notes, order.SubtotalCents,
		order.ShippingCents, order.TotalCents, defaultCurrency(order.Currency),
		order.Status, order.TokenHash, FormatTime(order.CreatedAt), FormatTime(order.UpdatedAt),
	)
	if err != nil {
		return fmt.Errorf("create order: %w", err)
	}
	for _, item := range items {
		_, err := q.ExecContext(ctx,
			`INSERT INTO order_items (id, order_id, product_id, slug, name, image,
			 unit_price_cents, quantity, line_total_cents)
			 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			item.ID, order.ID, item.ProductID, item.Slug, item.Name, item.Image,
			item.UnitPriceCents, item.Quantity, item.LineTotalCents,
		)
		if err != nil {
			return fmt.Errorf("create order item: %w", err)
		}
	}
	return nil
}

func (s *Store) OrderByTokenHash(ctx context.Context, tokenHash string) (Order, error) {
	return scanOrder(s.db.QueryRowContext(ctx,
		`SELECT `+orderColumns+` FROM orders o WHERE o.token_hash = ?`, tokenHash,
	))
}

func (s *Store) OrderByID(ctx context.Context, id string) (Order, error) {
	return scanOrder(s.db.QueryRowContext(ctx,
		`SELECT `+orderColumns+` FROM orders o WHERE o.id = ?`, id,
	))
}

func (s *Store) OrderItems(ctx context.Context, orderID string) ([]OrderItem, error) {
	return s.orderItems(ctx, s.db, orderID)
}

func (s *Store) orderItems(ctx context.Context, q querier, orderID string) ([]OrderItem, error) {
	rows, err := q.QueryContext(ctx,
		`SELECT id, order_id, product_id, slug, name, image, unit_price_cents,
		 quantity, line_total_cents
		 FROM order_items WHERE order_id = ? ORDER BY rowid`, orderID,
	)
	if err != nil {
		return nil, fmt.Errorf("list order items: %w", err)
	}
	defer rows.Close()

	items := make([]OrderItem, 0)
	for rows.Next() {
		var item OrderItem
		if err := rows.Scan(&item.ID, &item.OrderID, &item.ProductID, &item.Slug,
			&item.Name, &item.Image, &item.UnitPriceCents, &item.Quantity,
			&item.LineTotalCents); err != nil {
			return nil, fmt.Errorf("scan order item: %w", err)
		}
		items = append(items, item)
	}
	return items, rows.Err()
}

func (s *Store) OrdersByUser(ctx context.Context, userID string) ([]Order, error) {
	rows, err := s.db.QueryContext(ctx,
		`SELECT `+orderColumns+` FROM orders o
		 WHERE o.user_id = ? ORDER BY o.created_at DESC`, userID,
	)
	if err != nil {
		return nil, fmt.Errorf("list customer orders: %w", err)
	}
	defer rows.Close()

	orders := make([]Order, 0)
	for rows.Next() {
		order, err := scanOrder(rows)
		if err != nil {
			return nil, fmt.Errorf("scan customer order: %w", err)
		}
		orders = append(orders, order)
	}
	return orders, rows.Err()
}

func (s *Store) AdminOrders(ctx context.Context, filter AdminOrderFilter) ([]Order, error) {
	var where []string
	var args []any
	if filter.Status != "" {
		where = append(where, "o.status = ?")
		args = append(args, filter.Status)
	}
	if filter.Query != "" {
		where = append(where,
			"(o.number LIKE ? OR o.name LIKE ? OR o.email LIKE ? OR o.phone LIKE ?)")
		like := "%" + filter.Query + "%"
		args = append(args, like, like, like, like)
	}
	query := `SELECT ` + orderColumns + ` FROM orders o`
	if len(where) > 0 {
		query += " WHERE " + strings.Join(where, " AND ")
	}
	query += " ORDER BY o.created_at DESC"
	limit := filter.Limit
	if limit <= 0 || limit > 500 {
		limit = 200
	}
	query += " LIMIT ?"
	args = append(args, limit)

	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("list admin orders: %w", err)
	}
	defer rows.Close()

	orders := make([]Order, 0)
	for rows.Next() {
		order, err := scanOrder(rows)
		if err != nil {
			return nil, fmt.Errorf("scan admin order: %w", err)
		}
		orders = append(orders, order)
	}
	return orders, rows.Err()
}

func (s *Store) UpdateOrderStatus(ctx context.Context, q querier, id, from, to string, now time.Time) error {
	result, err := q.ExecContext(ctx,
		`UPDATE orders SET status = ?, updated_at = ? WHERE id = ? AND status = ?`,
		to, FormatTime(now), id, from,
	)
	if err != nil {
		return fmt.Errorf("update order status: %w", err)
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
