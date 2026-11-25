// Command oracle-phone exposes the phone variant helpers and the device/hook
// data layer for Rust differential tests.
//
//	go run ./cmd/oracle-phone
//
// It reads one JSON command per line and writes one JSON result per line,
// operating against AZAHAR_TEST_DATABASE_URL (default http://127.0.0.1:8880).
// The Rust test owns every `itph_*` fixture row and deletes it when done.
package main

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"

	"azahar/internal/db"
	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/plugins/phone"
	"azahar/internal/store"
)

const (
	defaultDSN = "http://127.0.0.1:8880"
	pluginID   = "phone"
)

type request struct {
	Op        string              `json:"op"`
	IMEIs     []string            `json:"imeis"`
	Variants  []map[string]string `json:"variants"`
	ProductID string              `json:"product_id"`
	Config    string              `json:"config"`
	Color     string              `json:"color"`
	IMEI      string              `json:"imei"`
	DeviceID  string              `json:"device_id"`
	OrderID   string              `json:"order_id"`
	ItemID    string              `json:"item_id"`
	Quantity  int                 `json:"quantity"`
	Options   map[string]string   `json:"options"`
	From      string              `json:"from"`
	To        string              `json:"to"`
}

type device struct {
	ID           string `json:"id"`
	ProductID    string `json:"product_id"`
	ProductName  string `json:"product_name"`
	VariantKey   string `json:"variant_key"`
	VariantLabel string `json:"variant_label"`
	Config       string `json:"config"`
	Color        string `json:"color"`
	IMEI         string `json:"imei"`
	Status       string `json:"status"`
	OrderID      string `json:"order_id"`
	OrderNumber  string `json:"order_number"`
	CreatedAt    string `json:"created_at"`
}

type deviceCounts struct {
	InStock  int `json:"in_stock"`
	Reserved int `json:"reserved"`
	Sold     int `json:"sold"`
}

type createDevicePayload struct {
	ProductID string `json:"product_id"`
	Config    string `json:"config"`
	Color     string `json:"color"`
	IMEI      string `json:"imei"`
}

const deviceSelect = `SELECT d.id, d.product_id, p.name, d.variant_key, d.config,
		d.color, d.imei, d.status, d.order_id, COALESCE(o.number, ''), d.created_at
	FROM phone_devices d
	JOIN products p ON p.id = d.product_id
	LEFT JOIN orders o ON o.id = d.order_id`

func main() {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		dsn = defaultDSN
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	handle, err := db.Open(ctx, dsn)
	if err != nil {
		fmt.Fprintf(os.Stderr, "oracle-phone: cannot open AZAHAR_TEST_DATABASE_URL=%s: %v\nrun `make dev-db` or set AZAHAR_TEST_DATABASE_URL\n", dsn, err)
		os.Exit(1)
	}
	defer handle.Close()
	if err := db.Migrate(ctx, handle); err != nil {
		fmt.Fprintf(os.Stderr, "oracle-phone: migrate %s: %v\n", dsn, err)
		os.Exit(1)
	}

	instance := phone.New()
	migrations := make([]db.PluginMigration, 0, len(instance.Migrations()))
	for _, migration := range instance.Migrations() {
		migrations = append(migrations, db.PluginMigration{
			PluginID: pluginID, Version: migration.Version, SQL: migration.SQL,
		})
	}
	if err := db.MigratePlugins(ctx, handle, migrations); err != nil {
		fmt.Fprintf(os.Stderr, "oracle-phone: plugin migrate %s: %v\n", dsn, err)
		os.Exit(1)
	}

	repo := store.New(handle)
	env := &plugins.Env{DB: handle, Store: repo}
	instance.Init(env)

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 8<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var req request
		if err := json.Unmarshal(scanner.Bytes(), &req); err != nil {
			_ = encoder.Encode(failure(err))
			continue
		}
		switch req.Op {
		case "imei":
			values := make([]bool, len(req.IMEIs))
			for index, imei := range req.IMEIs {
				values[index] = phone.ValidIMEI(imei)
			}
			_ = encoder.Encode(success(values))
		case "variant":
			values := make([]map[string]string, 0, len(req.Variants))
			for _, options := range req.Variants {
				values = append(values, map[string]string{
					"key":   phone.VariantKey(options),
					"label": phone.VariantLabel(options),
				})
			}
			_ = encoder.Encode(success(values))
		case "manifest":
			_ = encoder.Encode(success(instance.Manifest()))
		case "create":
			_ = encoder.Encode(createDevice(ctx, handle, repo, req))
		case "list":
			_ = encoder.Encode(listDevices(ctx, handle, repo, req))
		case "delete":
			_ = encoder.Encode(deleteDevice(ctx, handle, repo, req))
		case "counts":
			_ = encoder.Encode(countResult(ctx, handle, req))
		case "order_devices":
			_ = encoder.Encode(orderDevices(ctx, handle, req))
		case "validate":
			_ = encoder.Encode(validateLine(ctx, handle, repo, instance, req))
		case "order_created":
			_ = encoder.Encode(orderCreated(ctx, handle, repo, instance, req))
		case "status":
			_ = encoder.Encode(changeStatus(ctx, handle, instance, req))
		case "enrich":
			_ = encoder.Encode(enrichProduct(ctx, repo, instance, req))
		case "imei_exists":
			_ = encoder.Encode(success(map[string]any{"exists": imeiExists(ctx, handle, req.IMEI)}))
		case "reconcile":
			instance.Init(env)
			_ = encoder.Encode(stockResult(ctx, handle, req))
		case "repair":
			instance.Init(env)
			_ = encoder.Encode(repairResult(ctx, handle, req))
		case "stock":
			_ = encoder.Encode(stockResult(ctx, handle, req))
		default:
			_ = encoder.Encode(failure(fmt.Errorf("unknown op %q", req.Op)))
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func success(value any) map[string]any {
	return map[string]any{"ok": true, "value": value}
}

func failure(err error) map[string]any {
	return map[string]any{"ok": false, "error": err.Error()}
}

func problemValue(problem *plugins.Problem) any {
	if problem == nil {
		return nil
	}
	return problem
}

func phoneProductProblem() *plugins.Problem {
	return plugins.NewProblem(422, "validation_error", "Check the marked fields.",
		map[string]string{"product_id": "Choose a phone product."})
}

func allowedOption(values []string, value string) bool {
	for _, candidate := range values {
		if candidate == value {
			return true
		}
	}
	return false
}

func createDevice(ctx context.Context, handle *sql.DB, repo *store.Store, req request) map[string]any {
	payload := createDevicePayload{
		ProductID: strings.TrimSpace(req.ProductID),
		Config:    strings.TrimSpace(req.Config),
		Color:     strings.TrimSpace(req.Color),
		IMEI:      strings.TrimSpace(req.IMEI),
	}
	product, err := repo.ProductByID(ctx, payload.ProductID, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return success(map[string]any{"device": nil, "problem": phoneProductProblem()})
		}
		return failure(err)
	}
	if product.PluginID != pluginID {
		return success(map[string]any{"device": nil, "problem": phoneProductProblem()})
	}

	fields := make(map[string]string)
	options := map[string]string{"config": payload.Config, "color": payload.Color}
	if !allowedOption(product.Attributes["config"], options["config"]) {
		fields["config"] = "Choose an available memory and storage option."
	}
	if !allowedOption(product.Attributes["color"], options["color"]) {
		fields["color"] = "Choose an available color option."
	}
	if !phone.ValidIMEI(payload.IMEI) {
		fields["imei"] = "Enter a 15-digit IMEI that passes the checksum."
	}
	if len(fields) > 0 {
		return success(map[string]any{
			"device":  nil,
			"problem": plugins.NewProblem(422, "validation_error", "Check the marked fields.", fields),
		})
	}

	now := store.FormatTime(time.Now().UTC())
	id := ids.New("phn")
	err = repo.InTx(ctx, func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO phone_devices (id, product_id, variant_key, config, color,
			 imei, status, order_id, order_item_id, created_at, updated_at)
			 VALUES (?, ?, ?, ?, ?, ?, 'in_stock', '', '', ?, ?)`,
			id, product.ID, phone.VariantKey(options), payload.Config, payload.Color,
			payload.IMEI, now, now,
		); err != nil {
			return err
		}
		return syncProductStock(ctx, tx, product.ID)
	})
	if err != nil {
		if imeiExists(ctx, handle, payload.IMEI) {
			return success(map[string]any{
				"device": nil,
				"problem": plugins.NewProblem(409, "conflict", "That IMEI is already registered.",
					map[string]string{"imei": "This IMEI is already registered."}),
			})
		}
		return failure(err)
	}
	return success(map[string]any{
		"device": device{
			ID:           id,
			ProductID:    product.ID,
			ProductName:  product.Name,
			VariantKey:   phone.VariantKey(options),
			VariantLabel: phone.VariantLabel(options),
			Config:       payload.Config,
			Color:        payload.Color,
			IMEI:         payload.IMEI,
			Status:       "in_stock",
			CreatedAt:    now,
		},
		"problem": nil,
	})
}

func listDevices(ctx context.Context, handle *sql.DB, repo *store.Store, req request) map[string]any {
	productID := strings.TrimSpace(req.ProductID)
	suffix := ` ORDER BY d.created_at DESC, d.id DESC LIMIT 500`
	args := make([]any, 0, 1)
	if productID != "" {
		product, err := repo.ProductByID(ctx, productID, false)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return success(map[string]any{"devices": nil, "counts": nil, "problem": phoneProductProblem()})
			}
			return failure(err)
		}
		if product.PluginID != pluginID {
			return success(map[string]any{"devices": nil, "counts": nil, "problem": phoneProductProblem()})
		}
		suffix = ` WHERE d.product_id = ?` + suffix
		args = append(args, productID)
	}
	devices, err := queryDevices(ctx, handle, suffix, args...)
	if err != nil {
		return failure(err)
	}
	counts, err := countDevices(ctx, handle, productID)
	if err != nil {
		return failure(err)
	}
	return success(map[string]any{"devices": devices, "counts": counts, "problem": nil})
}

func deleteDevice(ctx context.Context, handle *sql.DB, repo *store.Store, req request) map[string]any {
	id := strings.TrimSpace(req.DeviceID)
	var status, productID string
	err := handle.QueryRowContext(ctx,
		`SELECT status, product_id FROM phone_devices WHERE id = ?`, id,
	).Scan(&status, &productID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return success(map[string]any{
				"deleted": false,
				"problem": plugins.NewProblem(404, "not_found", "Handset not found.", nil),
			})
		}
		return failure(err)
	}
	if status != "in_stock" {
		return success(map[string]any{
			"deleted": false,
			"problem": plugins.NewProblem(409, "conflict", "Only in-stock handsets can be deleted.", nil),
		})
	}

	err = repo.InTx(ctx, func(tx *sql.Tx) error {
		result, err := tx.ExecContext(ctx,
			`DELETE FROM phone_devices WHERE id = ? AND status = 'in_stock'`, id)
		if err != nil {
			return err
		}
		affected, err := result.RowsAffected()
		if err != nil {
			return err
		}
		if affected != 1 {
			return sql.ErrNoRows
		}
		return syncProductStock(ctx, tx, productID)
	})
	if errors.Is(err, sql.ErrNoRows) {
		return success(map[string]any{
			"deleted": false,
			"problem": plugins.NewProblem(409, "conflict", "Only in-stock handsets can be deleted.", nil),
		})
	}
	if err != nil {
		return failure(err)
	}
	return success(map[string]any{"deleted": true, "problem": nil})
}

func countResult(ctx context.Context, handle *sql.DB, req request) map[string]any {
	counts, err := countDevices(ctx, handle, strings.TrimSpace(req.ProductID))
	if err != nil {
		return failure(err)
	}
	return success(map[string]any{"counts": counts})
}

func orderDevices(ctx context.Context, handle *sql.DB, req request) map[string]any {
	orderID := strings.TrimSpace(req.OrderID)
	devices, err := queryDevices(ctx, handle,
		` WHERE d.order_id = ? AND d.status IN ('reserved', 'sold')
		 ORDER BY d.config, d.color, d.created_at, d.id`, orderID)
	if err != nil {
		return failure(err)
	}
	return success(map[string]any{"devices": devices})
}

func queryDevices(ctx context.Context, handle *sql.DB, suffix string, args ...any) ([]device, error) {
	rows, err := handle.QueryContext(ctx, deviceSelect+suffix, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	devices := make([]device, 0)
	for rows.Next() {
		item, err := scanDevice(rows)
		if err != nil {
			return nil, err
		}
		devices = append(devices, item)
	}
	return devices, rows.Err()
}

func scanDevice(row *sql.Rows) (device, error) {
	var item device
	err := row.Scan(&item.ID, &item.ProductID, &item.ProductName, &item.VariantKey,
		&item.Config, &item.Color, &item.IMEI, &item.Status,
		&item.OrderID, &item.OrderNumber, &item.CreatedAt)
	if err != nil {
		return device{}, err
	}
	item.VariantLabel = phone.VariantLabel(map[string]string{
		"config": item.Config,
		"color":  item.Color,
	})
	return item, nil
}

func countDevices(ctx context.Context, handle *sql.DB, productID string) (deviceCounts, error) {
	query := `SELECT
		COUNT(CASE WHEN status = 'in_stock' THEN 1 END),
		COUNT(CASE WHEN status = 'reserved' THEN 1 END),
		COUNT(CASE WHEN status = 'sold' THEN 1 END)
		FROM phone_devices`
	var args []any
	if productID != "" {
		query += ` WHERE product_id = ?`
		args = append(args, productID)
	}
	var counts deviceCounts
	err := handle.QueryRowContext(ctx, query, args...).Scan(&counts.InStock, &counts.Reserved, &counts.Sold)
	return counts, err
}

func syncProductStock(ctx context.Context, tx *sql.Tx, productID string) error {
	_, err := tx.ExecContext(ctx,
		`UPDATE products SET stock = (
		   SELECT COUNT(*) FROM phone_devices
		   WHERE product_id = ? AND status = 'in_stock'
		 ), updated_at = ? WHERE id = ?`,
		productID, store.FormatTime(time.Now().UTC()), productID)
	return err
}

func imeiExists(ctx context.Context, handle *sql.DB, imei string) bool {
	var count int
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM phone_devices WHERE imei = ?`, imei,
	).Scan(&count); err != nil {
		return false
	}
	return count > 0
}

func lineFrom(ctx context.Context, repo *store.Store, req request) (store.Product, error) {
	return repo.ProductByID(ctx, strings.TrimSpace(req.ProductID), false)
}

func validateLine(ctx context.Context, handle *sql.DB, repo *store.Store, instance *phone.Plugin, req request) map[string]any {
	product, err := lineFrom(ctx, repo, req)
	if err != nil {
		return failure(err)
	}
	tx, err := handle.BeginTx(ctx, nil)
	if err != nil {
		return failure(err)
	}
	defer tx.Rollback()
	problem := instance.ValidateCheckoutLine(ctx, tx, plugins.CheckoutLine{
		Product:  product,
		Options:  req.Options,
		Quantity: req.Quantity,
		OrderID:  req.OrderID,
		ItemID:   req.ItemID,
	})
	return success(map[string]any{"problem": problemValue(problem)})
}

func orderCreated(ctx context.Context, handle *sql.DB, repo *store.Store, instance *phone.Plugin, req request) map[string]any {
	product, err := lineFrom(ctx, repo, req)
	if err != nil {
		return failure(err)
	}
	tx, err := handle.BeginTx(ctx, nil)
	if err != nil {
		return failure(err)
	}
	err = instance.OnOrderCreated(ctx, tx, plugins.OrderRef{
		ID: req.OrderID, Number: "AZ-ORACLE", Status: "pending",
	}, []plugins.CheckoutLine{{
		Product:  product,
		Options:  req.Options,
		Quantity: req.Quantity,
		OrderID:  req.OrderID,
		ItemID:   req.ItemID,
	}})
	if err != nil {
		tx.Rollback()
		return success(map[string]any{"error": err.Error()})
	}
	if err := tx.Commit(); err != nil {
		return failure(err)
	}
	return success(map[string]any{"error": ""})
}

func changeStatus(ctx context.Context, handle *sql.DB, instance *phone.Plugin, req request) map[string]any {
	tx, err := handle.BeginTx(ctx, nil)
	if err != nil {
		return failure(err)
	}
	err = instance.OnOrderStatusChanged(ctx, tx, req.OrderID, req.From, req.To)
	if err != nil {
		tx.Rollback()
		return success(map[string]any{"error": err.Error()})
	}
	if err := tx.Commit(); err != nil {
		return failure(err)
	}
	return success(map[string]any{"error": ""})
}

func enrichProduct(ctx context.Context, repo *store.Store, instance *phone.Plugin, req request) map[string]any {
	product, err := lineFrom(ctx, repo, req)
	if err != nil {
		return failure(err)
	}
	enrichment, err := instance.EnrichProduct(ctx, product)
	if err != nil {
		return failure(err)
	}
	return success(map[string]any{"enrichment": enrichment})
}

func stockResult(ctx context.Context, handle *sql.DB, req request) map[string]any {
	var stock int
	if err := handle.QueryRowContext(ctx,
		`SELECT stock FROM products WHERE id = ?`, strings.TrimSpace(req.ProductID),
	).Scan(&stock); err != nil {
		return failure(err)
	}
	return success(map[string]any{"stock": stock})
}

func repairResult(ctx context.Context, handle *sql.DB, req request) map[string]any {
	var raw string
	if err := handle.QueryRowContext(ctx,
		`SELECT attributes FROM products WHERE id = ?`, strings.TrimSpace(req.ProductID),
	).Scan(&raw); err != nil {
		return failure(err)
	}
	var attributes any
	if err := json.Unmarshal([]byte(raw), &attributes); err != nil {
		return failure(err)
	}
	return success(map[string]any{"attributes": attributes})
}
