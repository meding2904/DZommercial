// Package phone sells handsets whose stock is tracked per IMEI.
package phone

import (
	"context"
	"database/sql"
	"net/http"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

const pluginID = "phone"

const devicesMigrationSQL = `CREATE TABLE IF NOT EXISTS phone_devices (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL REFERENCES products(id),
  variant_key TEXT NOT NULL,
  storage TEXT NOT NULL DEFAULT '',
  ram TEXT NOT NULL DEFAULT '',
  color TEXT NOT NULL DEFAULT '',
  imei TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL DEFAULT 'in_stock',
  order_id TEXT NOT NULL DEFAULT '',
  order_item_id TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_phone_devices_variant ON phone_devices(product_id, variant_key, status);
CREATE INDEX IF NOT EXISTS idx_phone_devices_order ON phone_devices(order_id);`

const configMigrationSQL = `ALTER TABLE phone_devices ADD COLUMN config TEXT NOT NULL DEFAULT '';
UPDATE phone_devices SET config = ram || ' RAM + ' || storage WHERE config = '';
CREATE INDEX IF NOT EXISTS idx_phone_devices_config ON phone_devices(product_id, config, color, status);`

var (
	_ plugins.Plugin          = (*Plugin)(nil)
	_ plugins.Migrator        = (*Plugin)(nil)
	_ plugins.CheckoutHook    = (*Plugin)(nil)
	_ plugins.OrderStatusHook = (*Plugin)(nil)
	_ plugins.ProductEnricher = (*Plugin)(nil)
)

type Plugin struct {
	db    *sql.DB
	store *store.Store
}

func New() *Plugin {
	return &Plugin{}
}

func (p *Plugin) Init(env *plugins.Env) {
	if env == nil {
		return
	}
	p.db = env.DB
	p.store = env.Store
	if p.db == nil {
		return
	}
	// The IMEI rows are the real stock for a handset. Reconcile the product
	// counter at startup so an import, a manual edit or an older deployment
	// cannot leave the storefront showing a different quantity than the shelf.
	if _, err := p.db.ExecContext(context.Background(),
		`UPDATE products SET stock = (
		   SELECT COUNT(*) FROM phone_devices d
		   WHERE d.product_id = products.id AND d.status = 'in_stock'
		 ), updated_at = ? WHERE plugin_id = ?`,
		store.FormatTime(time.Now().UTC()), pluginID); err != nil && env.Logger != nil {
		env.Logger.Warn("phone stock reconciliation failed", "error", err)
	}
}

func (p *Plugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:          pluginID,
		Name:        "Phone catalog",
		Version:     "1.0.0",
		Description: "Sell handsets while tracking every unit by IMEI.",
		Author:      "Azahar",
		AdminPath:   "/admin/plugins/phone",
		Categories: []plugins.Category{
			{Slug: "phones", Name: "Phones", Description: "Handsets with registered IMEI stock.", SortOrder: 1},
		},
		ProductType: &plugins.ProductType{
			Label:   "Phone",
			Summary: "Every handset is tracked by IMEI and sold as one memory and storage configuration.",
			Fields: []plugins.OptionField{
				{
					Key:         "config",
					Label:       "Memory and storage",
					Type:        "select",
					Required:    true,
					Help:        "One paired memory and storage configuration, for example 12 GB RAM + 256 GB.",
					Suggestions: []string{"12 GB RAM + 256 GB", "16 GB RAM + 512 GB", "16 GB RAM + 1 TB"},
				},
				{
					Key:         "color",
					Label:       "Color",
					Type:        "select",
					Required:    true,
					Help:        "The finish shown on the product and order.",
					Suggestions: []string{"Midnight", "Graphite", "Silver", "Blue"},
				},
			},
		},
	}
}

func (p *Plugin) Routes() []plugins.Route {
	return []plugins.Route{
		{Method: http.MethodGet, Pattern: "/devices", Admin: true, Handler: p.handleListDevices},
		{Method: http.MethodPost, Pattern: "/devices", Admin: true, Handler: p.handleCreateDevice},
		{Method: http.MethodDelete, Pattern: "/devices/{id}", Admin: true, Handler: p.handleDeleteDevice},
		{Method: http.MethodGet, Pattern: "/orders/{orderID}/devices", Admin: true, Handler: p.handleOrderDevices},
	}
}

func (p *Plugin) Migrations() []plugins.Migration {
	return []plugins.Migration{
		{Version: "001_devices.sql", SQL: devicesMigrationSQL},
		{Version: "002_config.sql", SQL: configMigrationSQL},
	}
}
