package phone

import (
	"context"
	"database/sql"
	"fmt"
	"net/http"
	"strings"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

func (p *Plugin) ValidateCheckoutLine(ctx context.Context, tx *sql.Tx, line plugins.CheckoutLine) *plugins.Problem {
	if line.Product.PluginID != pluginID {
		return nil
	}

	options := make(map[string]string, len(optionKeys))
	for _, key := range optionKeys {
		value := strings.TrimSpace(line.Options[key])
		if !allowedOption(line.Product.Attributes[key], value) {
			return plugins.NewProblem(http.StatusUnprocessableEntity, "validation_error",
				"Check the marked fields.",
				map[string]string{"items": "Choose an available " + strings.ToLower(optionLabels[key]) +
					" option for " + line.Product.Name + "."})
		}
		options[key] = value
	}

	var available int
	err := tx.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM phone_devices
		 WHERE product_id = ? AND config = ? AND color = ? AND status = 'in_stock'`,
		line.Product.ID, options["config"], options["color"],
	).Scan(&available)
	if err != nil {
		return plugins.NewProblem(http.StatusInternalServerError, "internal_error",
			"Something went wrong.", nil)
	}
	if available < line.Quantity {
		return plugins.NewProblem(http.StatusConflict, "conflict",
			"Some products do not have enough stock.",
			map[string]string{"items": "Not enough IMEI-registered handsets for " +
				line.Product.Name + " in that configuration."})
	}
	return nil
}

func (p *Plugin) OnOrderCreated(ctx context.Context, tx *sql.Tx, order plugins.OrderRef, lines []plugins.CheckoutLine) error {
	now := store.FormatTime(time.Now().UTC())
	for _, line := range lines {
		if line.Product.PluginID != pluginID {
			continue
		}
		options := make(map[string]string, len(optionKeys))
		for _, key := range optionKeys {
			options[key] = strings.TrimSpace(line.Options[key])
		}
		for i := 0; i < line.Quantity; i++ {
			var deviceID string
			err := tx.QueryRowContext(ctx,
				`SELECT id FROM phone_devices
				 WHERE product_id = ? AND config = ? AND color = ? AND status = 'in_stock'
				 ORDER BY created_at, id LIMIT 1`,
				line.Product.ID, options["config"], options["color"],
			).Scan(&deviceID)
			if err != nil {
				return fmt.Errorf("claim handset for %s: %w", line.Product.Name, err)
			}

			// The status guard on the UPDATE decides the claim, so a racing
			// checkout cannot reserve the same IMEI.
			result, err := tx.ExecContext(ctx,
				`UPDATE phone_devices
				 SET status = 'reserved', order_id = ?, order_item_id = ?, updated_at = ?
				 WHERE id = ? AND status = 'in_stock'`,
				order.ID, line.ItemID, now, deviceID,
			)
			if err != nil {
				return fmt.Errorf("claim handset %s: %w", deviceID, err)
			}
			affected, err := result.RowsAffected()
			if err != nil {
				return fmt.Errorf("claim handset %s: %w", deviceID, err)
			}
			if affected != 1 {
				return fmt.Errorf("claim handset %s: device was already taken", deviceID)
			}
		}
	}
	return nil
}

func (p *Plugin) OnOrderStatusChanged(ctx context.Context, tx *sql.Tx, orderID, from, to string) error {
	now := store.FormatTime(time.Now().UTC())
	switch to {
	case "cancelled":
		if _, err := tx.ExecContext(ctx,
			`UPDATE phone_devices
			 SET status = 'in_stock', order_id = '', order_item_id = '', updated_at = ?
			 WHERE order_id = ? AND status = 'reserved'`,
			now, orderID,
		); err != nil {
			return fmt.Errorf("release handsets for order %s: %w", orderID, err)
		}
	case "delivered":
		if _, err := tx.ExecContext(ctx,
			`UPDATE phone_devices SET status = 'sold', updated_at = ?
			 WHERE order_id = ? AND status = 'reserved'`,
			now, orderID,
		); err != nil {
			return fmt.Errorf("mark handsets sold for order %s: %w", orderID, err)
		}
	}
	return nil
}

func (p *Plugin) EnrichProduct(ctx context.Context, product store.Product) (map[string]any, error) {
	if product.PluginID != pluginID || p.db == nil {
		return nil, nil
	}
	var inStock, reserved int
	err := p.db.QueryRowContext(ctx,
		`SELECT
		 COUNT(CASE WHEN status = 'in_stock' THEN 1 END),
		 COUNT(CASE WHEN status = 'reserved' THEN 1 END)
		 FROM phone_devices WHERE product_id = ?`,
		product.ID,
	).Scan(&inStock, &reserved)
	if err != nil {
		return nil, err
	}

	rows, err := p.db.QueryContext(ctx,
		`SELECT config, color, COUNT(*)
		 FROM phone_devices
		 WHERE product_id = ? AND status = 'in_stock'
		 GROUP BY config, color
		 ORDER BY config, color`,
		product.ID,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	variants := make([]map[string]any, 0)
	for rows.Next() {
		var config, color string
		var count int
		if err := rows.Scan(&config, &color, &count); err != nil {
			return nil, err
		}
		variants = append(variants, map[string]any{
			"config":   config,
			"color":    color,
			"in_stock": count,
		})
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	return map[string]any{
		"devices_in_stock": inStock,
		"variants":         variants,
	}, nil
}
