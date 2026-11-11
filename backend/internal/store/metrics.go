package store

import (
	"context"
	"fmt"
	"time"
)

type RevenuePoint struct {
	Date         string
	RevenueCents int64
	Orders       int
}

type StatusCount struct {
	Status string
	Count  int
}

type TopProduct struct {
	ProductID    string
	Name         string
	Units        int
	RevenueCents int64
}

type RecentOrder struct {
	ID         string
	Number     string
	Name       string
	TotalCents int64
	Status     string
	CreatedAt  time.Time
}

type LowStockItem struct {
	ID    string
	Name  string
	Stock int
	Image string
}

type Metrics struct {
	Demo                 bool
	From                 time.Time
	To                   time.Time
	RevenueCents         int64
	PreviousRevenueCents int64
	OrdersCount          int
	PreviousOrdersCount  int
	UnitsSold            int
	NewCustomers         int
	LowStock             []LowStockItem
	RevenueSeries        []RevenuePoint
	StatusSplit          map[string]int
	TopProducts          []TopProduct
	RecentOrders         []RecentOrder
}

func (s *Store) Metrics(ctx context.Context, from, to, previousFrom, previousTo time.Time) (Metrics, error) {
	metrics := Metrics{
		From:          from,
		To:            to,
		LowStock:      make([]LowStockItem, 0),
		RevenueSeries: make([]RevenuePoint, 0),
		StatusSplit:   make(map[string]int),
		TopProducts:   make([]TopProduct, 0),
		RecentOrders:  make([]RecentOrder, 0),
	}

	start := FormatTime(from)
	end := FormatTime(to.AddDate(0, 0, 1))
	previousStart := FormatTime(previousFrom)
	previousEnd := FormatTime(previousTo.AddDate(0, 0, 1))

	if err := s.db.QueryRowContext(ctx,
		`SELECT COALESCE(SUM(total_cents), 0), COUNT(*) FROM orders
		 WHERE created_at >= ? AND created_at < ? AND status <> 'cancelled'`,
		start, end,
	).Scan(&metrics.RevenueCents, &metrics.OrdersCount); err != nil {
		return Metrics{}, fmt.Errorf("range revenue: %w", err)
	}
	if err := s.db.QueryRowContext(ctx,
		`SELECT COALESCE(SUM(total_cents), 0), COUNT(*) FROM orders
		 WHERE created_at >= ? AND created_at < ? AND status <> 'cancelled'`,
		previousStart, previousEnd,
	).Scan(&metrics.PreviousRevenueCents, &metrics.PreviousOrdersCount); err != nil {
		return Metrics{}, fmt.Errorf("previous revenue: %w", err)
	}
	if err := s.db.QueryRowContext(ctx,
		`SELECT COALESCE(SUM(oi.quantity), 0) FROM order_items oi
		 JOIN orders o ON o.id = oi.order_id
		 WHERE o.created_at >= ? AND o.created_at < ? AND o.status <> 'cancelled'`,
		start, end,
	).Scan(&metrics.UnitsSold); err != nil {
		return Metrics{}, fmt.Errorf("units sold: %w", err)
	}
	if err := s.db.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM users
		 WHERE role = 'customer' AND created_at >= ? AND created_at < ?`,
		start, end,
	).Scan(&metrics.NewCustomers); err != nil {
		return Metrics{}, fmt.Errorf("new customers: %w", err)
	}

	seriesRows, err := s.db.QueryContext(ctx,
		`SELECT substr(created_at, 1, 10) AS day,
		        COALESCE(SUM(total_cents), 0), COUNT(*)
		 FROM orders
		 WHERE created_at >= ? AND created_at < ? AND status <> 'cancelled'
		 GROUP BY day ORDER BY day`,
		start, end,
	)
	if err != nil {
		return Metrics{}, fmt.Errorf("revenue series: %w", err)
	}
	series := make(map[string]RevenuePoint)
	for seriesRows.Next() {
		var point RevenuePoint
		if err := seriesRows.Scan(&point.Date, &point.RevenueCents, &point.Orders); err != nil {
			seriesRows.Close()
			return Metrics{}, fmt.Errorf("scan revenue point: %w", err)
		}
		series[point.Date] = point
	}
	seriesRows.Close()
	if err := seriesRows.Err(); err != nil {
		return Metrics{}, err
	}
	for day := from; !day.After(to); day = day.AddDate(0, 0, 1) {
		key := day.Format("2006-01-02")
		point, ok := series[key]
		if !ok {
			point = RevenuePoint{Date: key}
		}
		metrics.RevenueSeries = append(metrics.RevenueSeries, point)
	}

	statusRows, err := s.db.QueryContext(ctx,
		`SELECT status, COUNT(*) FROM orders
		 WHERE created_at >= ? AND created_at < ? GROUP BY status`,
		start, end,
	)
	if err != nil {
		return Metrics{}, fmt.Errorf("status split: %w", err)
	}
	for statusRows.Next() {
		var status string
		var count int
		if err := statusRows.Scan(&status, &count); err != nil {
			statusRows.Close()
			return Metrics{}, fmt.Errorf("scan status split: %w", err)
		}
		metrics.StatusSplit[status] = count
	}
	statusRows.Close()
	if err := statusRows.Err(); err != nil {
		return Metrics{}, err
	}

	topRows, err := s.db.QueryContext(ctx,
		`SELECT oi.product_id, MAX(oi.name), SUM(oi.quantity), SUM(oi.line_total_cents)
		 FROM order_items oi JOIN orders o ON o.id = oi.order_id
		 WHERE o.created_at >= ? AND o.created_at < ? AND o.status <> 'cancelled'
		 GROUP BY oi.product_id
		 ORDER BY SUM(oi.quantity) DESC, SUM(oi.line_total_cents) DESC
		 LIMIT 5`,
		start, end,
	)
	if err != nil {
		return Metrics{}, fmt.Errorf("top products: %w", err)
	}
	for topRows.Next() {
		var product TopProduct
		if err := topRows.Scan(&product.ProductID, &product.Name, &product.Units,
			&product.RevenueCents); err != nil {
			topRows.Close()
			return Metrics{}, fmt.Errorf("scan top product: %w", err)
		}
		metrics.TopProducts = append(metrics.TopProducts, product)
	}
	topRows.Close()
	if err := topRows.Err(); err != nil {
		return Metrics{}, err
	}

	lowRows, err := s.db.QueryContext(ctx,
		`SELECT id, name, stock, image FROM products
		 WHERE active = 1 AND stock <= 5 ORDER BY stock ASC, name ASC LIMIT 10`)
	if err != nil {
		return Metrics{}, fmt.Errorf("low stock: %w", err)
	}
	for lowRows.Next() {
		var item LowStockItem
		if err := lowRows.Scan(&item.ID, &item.Name, &item.Stock, &item.Image); err != nil {
			lowRows.Close()
			return Metrics{}, fmt.Errorf("scan low stock: %w", err)
		}
		metrics.LowStock = append(metrics.LowStock, item)
	}
	lowRows.Close()
	if err := lowRows.Err(); err != nil {
		return Metrics{}, err
	}

	recentRows, err := s.db.QueryContext(ctx,
		`SELECT id, number, name, total_cents, status, created_at FROM orders
		 WHERE created_at >= ? AND created_at < ?
		 ORDER BY created_at DESC LIMIT 8`,
		start, end,
	)
	if err != nil {
		return Metrics{}, fmt.Errorf("recent orders: %w", err)
	}
	for recentRows.Next() {
		var order RecentOrder
		var createdAt string
		if err := recentRows.Scan(&order.ID, &order.Number, &order.Name,
			&order.TotalCents, &order.Status, &createdAt); err != nil {
			recentRows.Close()
			return Metrics{}, fmt.Errorf("scan recent order: %w", err)
		}
		order.CreatedAt = ParseTime(createdAt)
		metrics.RecentOrders = append(metrics.RecentOrders, order)
	}
	recentRows.Close()
	if err := recentRows.Err(); err != nil {
		return Metrics{}, err
	}

	var demo int
	if err := s.db.QueryRowContext(ctx,
		`SELECT CASE WHEN
		   EXISTS (SELECT 1 FROM users WHERE email = 'sample01@azahar.example')
		   OR EXISTS (SELECT 1 FROM orders WHERE email = 'sample01@azahar.example')
		 THEN 1 ELSE 0 END`,
	).Scan(&demo); err != nil {
		return Metrics{}, fmt.Errorf("demo flag: %w", err)
	}
	metrics.Demo = demo != 0

	return metrics, nil
}
