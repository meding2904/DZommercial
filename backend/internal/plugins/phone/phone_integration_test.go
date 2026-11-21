package phone_test

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"testing"
	"time"

	"azahar/internal/db"
	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/plugins/phone"
	"azahar/internal/store"
)

func TestPhoneReservationLifecycleAgainstLibSQL(t *testing.T) {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("set AZAHAR_TEST_DATABASE_URL to run the phone integration test")
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
	instance := phone.New()
	migrations := make([]db.PluginMigration, 0)
	for _, migration := range instance.Migrations() {
		migrations = append(migrations, db.PluginMigration{
			PluginID: "phone", Version: migration.Version, SQL: migration.SQL,
		})
	}
	if err := db.MigratePlugins(ctx, handle, migrations); err != nil {
		t.Fatalf("plugin migrate: %v", err)
	}

	repo := store.New(handle)
	env := &plugins.Env{DB: handle, Store: repo}
	instance.Init(env)

	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	categoryID := ids.New("cat_test")
	if _, err := handle.ExecContext(ctx,
		`INSERT INTO categories (id, slug, name, description, sort_order) VALUES (?, ?, ?, ?, ?)`,
		categoryID, "phone-test-"+suffix, "Phone test", "Temporary", 99,
	); err != nil {
		t.Fatalf("insert category: %v", err)
	}

	now := time.Now().UTC()
	product := store.Product{
		ID:         ids.New("prd"),
		Slug:       "phone-test-" + suffix,
		Name:       "Test handset",
		PriceCents: 100000,
		Currency:   "DZD",
		Category:   store.Category{ID: categoryID, Slug: "phone-test-" + suffix, Name: "Phone test"},
		Image:      "/images/atelier-1200.webp",
		Stock:      4,
		Active:     true,
		PluginID:   "phone",
		Attributes: map[string][]string{
			"config": {"12 GB RAM + 256 GB"},
			"color":  {"Midnight"},
		},
		CreatedAt: now,
		UpdatedAt: now,
	}
	if err := repo.CreateProduct(ctx, handle, product); err != nil {
		t.Fatalf("create product: %v", err)
	}

	t.Cleanup(func() {
		handle.Exec(`DELETE FROM phone_devices WHERE product_id = ?`, product.ID)
		handle.Exec(`DELETE FROM products WHERE id = ?`, product.ID)
		handle.Exec(`DELETE FROM categories WHERE id = ?`, categoryID)
	})

	variant := phone.VariantKey(map[string]string{
		"config": "12 GB RAM + 256 GB", "color": "Midnight",
	})
	imeis := make([]string, 4)
	base := time.Now().UnixNano() % 100000000000000
	for index := range imeis {
		imeis[index] = testIMEI(base + int64(index))
	}
	for index, imei := range imeis {
		if _, err := handle.ExecContext(ctx,
			`INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei,
			 status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'in_stock', ?, ?)`,
			ids.New("phn"), product.ID, variant, "12 GB RAM + 256 GB", "Midnight",
			imei, store.FormatTime(now.Add(time.Duration(index)*time.Second)),
			store.FormatTime(now),
		); err != nil {
			t.Fatalf("insert device: %v", err)
		}
	}

	enriched, err := instance.EnrichProduct(ctx, product)
	if err != nil {
		t.Fatalf("enrich product: %v", err)
	}
	variants, ok := enriched["variants"].([]map[string]any)
	if !ok || len(variants) != 1 || variants[0]["in_stock"] != 4 {
		t.Fatalf("variant counts did not reach the product: %#v", enriched)
	}
	if variants[0]["config"] != "12 GB RAM + 256 GB" || variants[0]["color"] != "Midnight" {
		t.Fatalf("variant options did not reach the product: %#v", variants[0])
	}

	line := plugins.CheckoutLine{
		Product:  product,
		Options:  map[string]string{"config": "12 GB RAM + 256 GB", "color": "Midnight"},
		Quantity: 2,
		OrderID:  ids.New("ord"),
		ItemID:   ids.New("itm"),
	}

	tx, err := handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin: %v", err)
	}
	if problem := instance.ValidateCheckoutLine(ctx, tx, line); problem != nil {
		tx.Rollback()
		t.Fatalf("validate: %+v", problem)
	}
	if err := instance.OnOrderCreated(ctx, tx, plugins.OrderRef{ID: line.OrderID, Number: "AZ-TEST"}, []plugins.CheckoutLine{line}); err != nil {
		tx.Rollback()
		t.Fatalf("reserve: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit: %v", err)
	}

	if got := countDevices(t, handle, product.ID, "reserved"); got != 2 {
		t.Fatalf("reserved = %d, want 2", got)
	}

	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin cancel: %v", err)
	}
	if err := instance.OnOrderStatusChanged(ctx, tx, line.OrderID, "pending", "cancelled"); err != nil {
		tx.Rollback()
		t.Fatalf("cancel: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit cancel: %v", err)
	}
	if got := countDevices(t, handle, product.ID, "in_stock"); got != 4 {
		t.Fatalf("after cancel in_stock = %d, want 4", got)
	}

	second := line
	second.OrderID = ids.New("ord")
	second.ItemID = ids.New("itm")
	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin second: %v", err)
	}
	if err := instance.OnOrderCreated(ctx, tx, plugins.OrderRef{ID: second.OrderID}, []plugins.CheckoutLine{second}); err != nil {
		tx.Rollback()
		t.Fatalf("reserve second: %v", err)
	}
	if err := instance.OnOrderStatusChanged(ctx, tx, second.OrderID, "shipped", "delivered"); err != nil {
		tx.Rollback()
		t.Fatalf("deliver: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit second: %v", err)
	}
	if got := countDevices(t, handle, product.ID, "sold"); got != 2 {
		t.Fatalf("sold = %d, want 2", got)
	}

	short := line
	short.Quantity = 99
	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin short: %v", err)
	}
	defer tx.Rollback()
	if problem := instance.ValidateCheckoutLine(ctx, tx, short); problem == nil {
		t.Fatal("a short reservation must be rejected")
	}
}

func testIMEI(seed int64) string {
	first14 := fmt.Sprintf("%014d", seed%100000000000000)
	sum := 0
	for index, char := range first14 {
		digit := int(char - '0')
		if index%2 == 1 {
			digit *= 2
			if digit > 9 {
				digit -= 9
			}
		}
		sum += digit
	}
	return first14 + fmt.Sprintf("%d", (10-sum%10)%10)
}

func countDevices(t *testing.T, handle *sql.DB, productID, status string) int {
	t.Helper()
	var count int
	if err := handle.QueryRow(
		`SELECT COUNT(*) FROM phone_devices WHERE product_id = ? AND status = ?`,
		productID, status,
	).Scan(&count); err != nil {
		t.Fatalf("count devices: %v", err)
	}
	return count
}
