package orderintake_test

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"azahar/internal/db"
	"azahar/internal/ids"
	"azahar/internal/orderintake"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

type testEnv struct {
	repo     *store.Store
	handle   *sql.DB
	suffix   string
	category store.Category
}

func newTestEnv(t *testing.T) *testEnv {
	t.Helper()
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
	categorySlug := "orderintake-test-" + suffix
	if _, err := handle.ExecContext(ctx,
		`INSERT INTO categories (id, slug, name, description, sort_order) VALUES (?, ?, ?, ?, ?)`,
		ids.New("cat_test"), categorySlug, "Order intake test", "Temporary integration-test category.", 99,
	); err != nil {
		t.Fatalf("insert category: %v", err)
	}
	t.Cleanup(func() {
		if _, err := handle.Exec(`DELETE FROM categories WHERE slug = ?`, categorySlug); err != nil {
			t.Errorf("cleanup category: %v", err)
		}
	})
	category, err := repo.CategoryBySlug(ctx, categorySlug)
	if err != nil {
		t.Fatalf("load category: %v", err)
	}
	return &testEnv{repo: repo, handle: handle, suffix: suffix, category: category}
}

// product stores one active product and removes it after the test.
func (e *testEnv) product(t *testing.T, ctx context.Context, label string, price int64, stock int) store.Product {
	t.Helper()
	now := time.Now().UTC()
	product := store.Product{
		ID:          "prd_test_" + e.suffix + "_" + label,
		Slug:        "orderintake-product-" + e.suffix + "-" + label,
		Name:        "Order intake " + label,
		Description: "A temporary product used by the order intake integration test.",
		PriceCents:  price,
		Currency:    "DZD",
		Size:        "10 ml",
		Category:    e.category,
		Image:       "/images/products/serum-1200.webp",
		Stock:       stock,
		Active:      true,
		CreatedAt:   now,
		UpdatedAt:   now,
	}
	if err := e.repo.CreateProduct(ctx, e.handle, product); err != nil {
		t.Fatalf("create product: %v", err)
	}
	t.Cleanup(func() {
		if _, err := e.handle.Exec(`DELETE FROM products WHERE id = ?`, product.ID); err != nil {
			t.Errorf("cleanup product: %v", err)
		}
	})
	return product
}

// trackOrder removes an order, its lines and its audit rows after the test.
func (e *testEnv) trackOrder(t *testing.T, orderID string) {
	t.Helper()
	t.Cleanup(func() {
		handle := e.handle
		if _, err := handle.Exec(`DELETE FROM audit_outbox WHERE entity_type = 'order' AND entity_id = ?`, orderID); err != nil {
			t.Errorf("cleanup audit_outbox: %v", err)
		}
		if _, err := handle.Exec(`DELETE FROM audit_log WHERE entity_type = 'order' AND entity_id = ?`, orderID); err != nil {
			t.Errorf("cleanup audit_log: %v", err)
		}
		if _, err := handle.Exec(`DELETE FROM order_items WHERE order_id = ?`, orderID); err != nil {
			t.Errorf("cleanup order_items: %v", err)
		}
		if _, err := handle.Exec(`DELETE FROM orders WHERE id = ?`, orderID); err != nil {
			t.Errorf("cleanup order: %v", err)
		}
	})
}

func (e *testEnv) stock(t *testing.T, ctx context.Context, productID string) int {
	t.Helper()
	product, err := e.repo.ProductByID(ctx, productID, true)
	if err != nil {
		t.Fatalf("reload product: %v", err)
	}
	return product.Stock
}

func (e *testEnv) orderCount(t *testing.T, ctx context.Context, tokenHash string) int {
	t.Helper()
	var count int
	if err := e.handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM orders WHERE token_hash = ?`, tokenHash,
	).Scan(&count); err != nil {
		t.Fatalf("count orders: %v", err)
	}
	return count
}

func checkoutRequest(lines []orderintake.Line, tokenHash string) orderintake.Request {
	return orderintake.Request{
		Lines:   lines,
		Channel: orderintake.CheckoutChannel("2026-11-14"),
		Customer: orderintake.Customer{
			Name:    "Test Buyer",
			Email:   "buyer@example.com",
			Phone:   "0550123456",
			Address: "1 Rue des Tests",
		},
		Destination: orderintake.Destination{
			WilayaCode:   16,
			WilayaName:   "Alger",
			CommuneName:  "Alger Centre",
			DeliveryType: "home",
		},
		Status:    "pending",
		TokenHash: tokenHash,
	}
}

func counterRequest(lines []orderintake.Line, tokenHash string) orderintake.Request {
	return orderintake.Request{
		Lines:       lines,
		Channel:     orderintake.CounterChannel(),
		Customer:    orderintake.Customer{Name: "Counter sale"},
		Destination: orderintake.Destination{DeliveryType: "pickup"},
		Status:      "pending",
		TokenHash:   tokenHash,
		ActorUserID: "usr_test",
	}
}

func requireProblem(t *testing.T, err error, status int, code string) *orderintake.Problem {
	t.Helper()
	var refusal *orderintake.Problem
	if !errors.As(err, &refusal) {
		t.Fatalf("error = %v, want an order intake problem", err)
	}
	if refusal.Status != status || refusal.Code != code {
		t.Fatalf("problem = %d %q, want %d %q", refusal.Status, refusal.Code, status, code)
	}
	return refusal
}

func auditDetail(t *testing.T, handle *sql.DB, orderID, action string) map[string]any {
	t.Helper()
	var raw string
	if err := handle.QueryRow(
		`SELECT detail FROM audit_outbox WHERE entity_type = 'order' AND entity_id = ? AND action = ?`,
		orderID, action,
	).Scan(&raw); err != nil {
		t.Fatalf("load %s audit row: %v", action, err)
	}
	var detail map[string]any
	if err := json.Unmarshal([]byte(raw), &detail); err != nil {
		t.Fatalf("decode %s audit detail: %v", action, err)
	}
	return detail
}

func TestPriceChangeRefusesTheOrderAgainstLibSQL(t *testing.T) {
	env := newTestEnv(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	intake := orderintake.New(env.repo, plugins.NewRegistry())
	product := env.product(t, ctx, "price", 123400, 5)

	wrong := int64(100000)
	request := checkoutRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 1, ExpectedPriceCents: &wrong},
	}, "hash-price-"+env.suffix)
	_, err := intake.Place(ctx, request)
	refusal := requireProblem(t, err, 409, "price_changed")
	if !strings.Contains(refusal.Message, "A price changed") {
		t.Fatalf("message = %q, want the price-changed wording", refusal.Message)
	}
	if got := refusal.Fields["items"]; got != product.Name+" now costs a different amount." {
		t.Fatalf("items field = %q, want the product's price note", got)
	}
	if got := env.stock(t, ctx, product.ID); got != 5 {
		t.Fatalf("stock = %d, want 5", got)
	}
	if got := env.orderCount(t, ctx, request.TokenHash); got != 0 {
		t.Fatalf("orders = %d, want none", got)
	}
}

func TestStockRefusalLeavesTheShopUntouchedAgainstLibSQL(t *testing.T) {
	env := newTestEnv(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	intake := orderintake.New(env.repo, plugins.NewRegistry())
	product := env.product(t, ctx, "stock", 123400, 2)

	request := checkoutRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 3},
	}, "hash-stock-"+env.suffix)
	_, err := intake.Place(ctx, request)
	refusal := requireProblem(t, err, 409, "conflict")
	if got := refusal.Fields["items"]; got != "Reduce the quantity for "+product.Name+"." {
		t.Fatalf("items field = %q, want the stock note", got)
	}
	if got := env.stock(t, ctx, product.ID); got != 2 {
		t.Fatalf("stock = %d, want 2", got)
	}
	if got := env.orderCount(t, ctx, request.TokenHash); got != 0 {
		t.Fatalf("orders = %d, want none", got)
	}
}

func TestCancelledOrderRestocksAgainstLibSQL(t *testing.T) {
	env := newTestEnv(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	intake := orderintake.New(env.repo, plugins.NewRegistry())
	product := env.product(t, ctx, "cancel", 123400, 5)

	placed, err := intake.Place(ctx, checkoutRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 2},
	}, "hash-cancel-"+env.suffix))
	if err != nil {
		t.Fatalf("place order: %v", err)
	}
	env.trackOrder(t, placed.Order.ID)
	if got := env.stock(t, ctx, product.ID); got != 3 {
		t.Fatalf("stock after the order = %d, want 3", got)
	}

	changed, err := intake.ChangeStatus(ctx, orderintake.Change{
		OrderID:     placed.Order.ID,
		To:          "cancelled",
		ActorUserID: "usr_admin",
		ActorRole:   "admin",
	})
	if err != nil {
		t.Fatalf("cancel order: %v", err)
	}
	if changed.Order.Status != "cancelled" {
		t.Fatalf("status = %q, want cancelled", changed.Order.Status)
	}
	if got := env.stock(t, ctx, product.ID); got != 5 {
		t.Fatalf("stock after the cancel = %d, want 5", got)
	}
	detail := auditDetail(t, env.handle, placed.Order.ID, "order.status")
	if detail["from"] != "pending" || detail["to"] != "cancelled" {
		t.Fatalf("audit detail = %#v, want the pending to cancelled move", detail)
	}
}

func TestEmployeeCannotCancelAgainstLibSQL(t *testing.T) {
	env := newTestEnv(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	intake := orderintake.New(env.repo, plugins.NewRegistry())
	product := env.product(t, ctx, "cancel-role", 123400, 5)

	placed, err := intake.Place(ctx, checkoutRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 1},
	}, "hash-cancel-role-"+env.suffix))
	if err != nil {
		t.Fatalf("place order: %v", err)
	}
	env.trackOrder(t, placed.Order.ID)

	_, err = intake.ChangeStatus(ctx, orderintake.Change{
		OrderID:     placed.Order.ID,
		To:          "cancelled",
		ActorUserID: "usr_employee",
		ActorRole:   "employee",
	})
	requireProblem(t, err, 403, "forbidden")
	if got := env.stock(t, ctx, product.ID); got != 4 {
		t.Fatalf("stock = %d, want 4", got)
	}
	order, err := env.repo.OrderByID(ctx, placed.Order.ID)
	if err != nil {
		t.Fatalf("reload order: %v", err)
	}
	if order.Status != "pending" {
		t.Fatalf("status = %q, want pending", order.Status)
	}
}

func TestCounterSaleIsDeliveredInOneTransactionAgainstLibSQL(t *testing.T) {
	env := newTestEnv(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	intake := orderintake.New(env.repo, plugins.NewRegistry())
	product := env.product(t, ctx, "counter", 123400, 5)

	placed, err := intake.Place(ctx, counterRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 2},
	}, "hash-counter-"+env.suffix))
	if err != nil {
		t.Fatalf("counter sale: %v", err)
	}
	env.trackOrder(t, placed.Order.ID)

	if placed.Order.Status != "delivered" {
		t.Fatalf("status = %q, want delivered", placed.Order.Status)
	}
	if placed.Order.ShippingCents != 0 {
		t.Fatalf("shipping = %d, want 0", placed.Order.ShippingCents)
	}
	if placed.Order.SubtotalCents != 246800 || placed.Order.TotalCents != 246800 {
		t.Fatalf("subtotal/total = %d/%d, want 246800/246800", placed.Order.SubtotalCents, placed.Order.TotalCents)
	}

	stored, err := env.repo.OrderByID(ctx, placed.Order.ID)
	if err != nil {
		t.Fatalf("reload order: %v", err)
	}
	if stored.Status != "delivered" {
		t.Fatalf("stored status = %q, want delivered", stored.Status)
	}
	if got := env.stock(t, ctx, product.ID); got != 3 {
		t.Fatalf("stock = %d, want 3", got)
	}

	detail := auditDetail(t, env.handle, placed.Order.ID, "order.counter_sale")
	if detail["channel"] != "counter" {
		t.Fatalf("audit detail = %#v, want the counter channel", detail)
	}
	if detail["items"] != float64(1) || detail["total_cents"] != float64(246800) {
		t.Fatalf("audit detail = %#v, want one item worth 246800", detail)
	}
}

func TestShippingRuleAgainstLibSQL(t *testing.T) {
	env := newTestEnv(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	intake := orderintake.New(env.repo, plugins.NewRegistry())
	product := env.product(t, ctx, "shipping", 400000, 5)

	atTheLine, err := intake.Place(ctx, checkoutRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 2},
	}, "hash-shipping-line-"+env.suffix))
	if err != nil {
		t.Fatalf("place order at the free-shipping line: %v", err)
	}
	env.trackOrder(t, atTheLine.Order.ID)
	if atTheLine.Order.SubtotalCents != 800000 {
		t.Fatalf("subtotal = %d, want 800000", atTheLine.Order.SubtotalCents)
	}
	if atTheLine.Order.ShippingCents != 60000 || atTheLine.Order.TotalCents != 860000 {
		t.Fatalf("shipping/total = %d/%d, want 60000/860000",
			atTheLine.Order.ShippingCents, atTheLine.Order.TotalCents)
	}

	overTheLine, err := intake.Place(ctx, checkoutRequest([]orderintake.Line{
		{ProductID: product.ID, Quantity: 3},
	}, "hash-shipping-over-"+env.suffix))
	if err != nil {
		t.Fatalf("place order over the free-shipping line: %v", err)
	}
	env.trackOrder(t, overTheLine.Order.ID)
	if overTheLine.Order.SubtotalCents != 1200000 {
		t.Fatalf("subtotal = %d, want 1200000", overTheLine.Order.SubtotalCents)
	}
	if overTheLine.Order.ShippingCents != 0 || overTheLine.Order.TotalCents != 1200000 {
		t.Fatalf("shipping/total = %d/%d, want 0/1200000",
			overTheLine.Order.ShippingCents, overTheLine.Order.TotalCents)
	}
}
