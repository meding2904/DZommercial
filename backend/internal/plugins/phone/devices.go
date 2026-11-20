package phone

import (
	"context"
	"database/sql"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

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

type rowScanner interface {
	Scan(dest ...any) error
}

const deviceSelect = `SELECT d.id, d.product_id, p.name, d.variant_key, d.config,
		d.color, d.imei, d.status, d.order_id, COALESCE(o.number, ''), d.created_at
	FROM phone_devices d
	JOIN products p ON p.id = d.product_id
	LEFT JOIN orders o ON o.id = d.order_id`

func (p *Plugin) handleListDevices(ctx *plugins.Context) {
	productID := strings.TrimSpace(ctx.R.URL.Query().Get("product_id"))
	suffix := ` ORDER BY d.created_at DESC, d.id DESC LIMIT 500`
	args := make([]any, 0, 1)
	if productID != "" {
		product, err := p.store.ProductByID(ctx.R.Context(), productID, false)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				phoneProductProblem(ctx)
				return
			}
			writeInternal(ctx)
			return
		}
		if product.PluginID != pluginID {
			phoneProductProblem(ctx)
			return
		}
		suffix = ` WHERE d.product_id = ?` + suffix
		args = append(args, productID)
	}

	devices, err := p.queryDevices(ctx.R.Context(), suffix, args...)
	if err != nil {
		writeInternal(ctx)
		return
	}
	counts, err := p.countDevices(ctx.R.Context(), productID)
	if err != nil {
		writeInternal(ctx)
		return
	}
	ctx.JSON(http.StatusOK, map[string]any{"devices": devices, "counts": counts})
}

func (p *Plugin) handleCreateDevice(ctx *plugins.Context) {
	payload, bad := plugins.DecodeBody[createDevicePayload](ctx)
	if bad != nil {
		ctx.Problem(bad.Status, bad.Code, bad.Message, bad.Fields)
		return
	}
	payload.ProductID = strings.TrimSpace(payload.ProductID)
	payload.Config = strings.TrimSpace(payload.Config)
	payload.Color = strings.TrimSpace(payload.Color)
	payload.IMEI = strings.TrimSpace(payload.IMEI)

	product, err := p.store.ProductByID(ctx.R.Context(), payload.ProductID, false)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			phoneProductProblem(ctx)
			return
		}
		writeInternal(ctx)
		return
	}
	if product.PluginID != pluginID {
		phoneProductProblem(ctx)
		return
	}

	fields := make(map[string]string)
	options := map[string]string{
		"config": payload.Config,
		"color":  payload.Color,
	}
	for _, key := range optionKeys {
		if !allowedOption(product.Attributes[key], options[key]) {
			fields[key] = "Choose an available " + strings.ToLower(optionLabels[key]) + " option."
		}
	}
	if !ValidIMEI(payload.IMEI) {
		fields["imei"] = "Enter a 15-digit IMEI that passes the checksum."
	}
	if len(fields) > 0 {
		ctx.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.", fields)
		return
	}

	now := store.FormatTime(time.Now().UTC())
	id := ids.New("phn")
	err = p.store.InTx(ctx.R.Context(), func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(ctx.R.Context(),
			`INSERT INTO phone_devices (id, product_id, variant_key, config, color,
			 imei, status, order_id, order_item_id, created_at, updated_at)
			 VALUES (?, ?, ?, ?, ?, ?, 'in_stock', '', '', ?, ?)`,
			id, product.ID, VariantKey(options), payload.Config, payload.Color,
			payload.IMEI, now, now,
		); err != nil {
			return err
		}
		return p.syncProductStock(ctx.R.Context(), tx, product.ID)
	})
	if err != nil {
		// The unique index is the authority. Read once more only to tell a
		// duplicate apart from another database error.
		if p.imeiExists(ctx.R.Context(), payload.IMEI) {
			ctx.Problem(http.StatusConflict, "conflict", "That IMEI is already registered.",
				map[string]string{"imei": "This IMEI is already registered."})
			return
		}
		writeInternal(ctx)
		return
	}

	ctx.JSON(http.StatusCreated, map[string]any{"device": device{
		ID:           id,
		ProductID:    product.ID,
		ProductName:  product.Name,
		VariantKey:   VariantKey(options),
		VariantLabel: VariantLabel(options),
		Config:       payload.Config,
		Color:        payload.Color,
		IMEI:         payload.IMEI,
		Status:       "in_stock",
		CreatedAt:    now,
	}})
}

func (p *Plugin) handleDeleteDevice(ctx *plugins.Context) {
	id := strings.TrimSpace(chi.URLParam(ctx.R, "id"))
	var status, productID string
	err := p.db.QueryRowContext(ctx.R.Context(),
		`SELECT status, product_id FROM phone_devices WHERE id = ?`, id,
	).Scan(&status, &productID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			ctx.Error(http.StatusNotFound, "not_found", "Handset not found.")
			return
		}
		writeInternal(ctx)
		return
	}
	if status != "in_stock" {
		ctx.Error(http.StatusConflict, "conflict", "Only in-stock handsets can be deleted.")
		return
	}

	err = p.store.InTx(ctx.R.Context(), func(tx *sql.Tx) error {
		result, err := tx.ExecContext(ctx.R.Context(),
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
		return p.syncProductStock(ctx.R.Context(), tx, productID)
	})
	if errors.Is(err, sql.ErrNoRows) {
		ctx.Error(http.StatusConflict, "conflict", "Only in-stock handsets can be deleted.")
		return
	}
	if err != nil {
		writeInternal(ctx)
		return
	}
	ctx.W.WriteHeader(http.StatusNoContent)
}

func (p *Plugin) handleOrderDevices(ctx *plugins.Context) {
	orderID := strings.TrimSpace(chi.URLParam(ctx.R, "orderID"))
	devices, err := p.queryDevices(ctx.R.Context(),
		` WHERE d.order_id = ? AND d.status IN ('reserved', 'sold')
		 ORDER BY d.config, d.color, d.created_at, d.id`, orderID,
	)
	if err != nil {
		writeInternal(ctx)
		return
	}
	ctx.JSON(http.StatusOK, map[string]any{"devices": devices})
}

func (p *Plugin) queryDevices(ctx context.Context, suffix string, args ...any) ([]device, error) {
	rows, err := p.db.QueryContext(ctx, deviceSelect+suffix, args...)
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

func (p *Plugin) countDevices(ctx context.Context, productID string) (deviceCounts, error) {
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
	err := p.db.QueryRowContext(ctx, query, args...).Scan(&counts.InStock, &counts.Reserved, &counts.Sold)
	return counts, err
}

func scanDevice(row rowScanner) (device, error) {
	var item device
	err := row.Scan(&item.ID, &item.ProductID, &item.ProductName, &item.VariantKey,
		&item.Config, &item.Color, &item.IMEI, &item.Status,
		&item.OrderID, &item.OrderNumber, &item.CreatedAt)
	if err != nil {
		return device{}, err
	}
	item.VariantLabel = VariantLabel(map[string]string{
		"config": item.Config,
		"color":  item.Color,
	})
	return item, nil
}

// syncProductStock makes the product counter match the shelf count. The IMEI
// rows are authoritative for a handset, so add and delete both recompute it
// instead of trusting a previous value.
func (p *Plugin) syncProductStock(ctx context.Context, tx *sql.Tx, productID string) error {
	_, err := tx.ExecContext(ctx,
		`UPDATE products SET stock = (
		   SELECT COUNT(*) FROM phone_devices
		   WHERE product_id = ? AND status = 'in_stock'
		 ), updated_at = ? WHERE id = ?`,
		productID, store.FormatTime(time.Now().UTC()), productID)
	return err
}

func (p *Plugin) imeiExists(ctx context.Context, imei string) bool {
	var count int
	if err := p.db.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM phone_devices WHERE imei = ?`, imei,
	).Scan(&count); err != nil {
		return false
	}
	return count > 0
}

func phoneProductProblem(ctx *plugins.Context) {
	ctx.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.",
		map[string]string{"product_id": "Choose a phone product."})
}

func writeInternal(ctx *plugins.Context) {
	ctx.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
}
