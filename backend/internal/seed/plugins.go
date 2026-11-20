package seed

import (
	"context"
	"database/sql"
	"fmt"
	"time"

	"azahar/internal/ids"
	"azahar/internal/store"
)

type demoPart struct {
	Slug       string
	Name       string
	Category   string
	PriceCents int64
	Stock      int
	Attributes map[string][]string
}

var demoParts = []demoPart{
	{
		Slug: "demo-ryzen-7", Name: "Ryzen 7 9700X", Category: "cpus",
		PriceCents: 5490000, Stock: 6,
		Attributes: map[string][]string{"socket": {"AM5"}, "tdp_watts": {"120"}},
	},
	{
		Slug: "demo-b650m", Name: "B650M Pro", Category: "motherboards",
		PriceCents: 2890000, Stock: 5,
		Attributes: map[string][]string{
			"socket": {"AM5"}, "memory_type": {"DDR5"}, "form_factor": {"micro-ATX"},
		},
	},
	{
		Slug: "demo-ddr5-32", Name: "DDR5 32 GB kit", Category: "memory",
		PriceCents: 1590000, Stock: 9,
		Attributes: map[string][]string{"memory_type": {"DDR5"}},
	},
	{
		Slug: "demo-nvme-1tb", Name: "1 TB NVMe drive", Category: "storage",
		PriceCents: 1190000, Stock: 12,
		Attributes: map[string][]string{"interface": {"NVMe"}},
	},
	{
		Slug: "demo-psu-750", Name: "750 W Gold supply", Category: "psus",
		PriceCents: 1290000, Stock: 7,
		Attributes: map[string][]string{"wattage_watts": {"750"}},
	},
	{
		Slug: "demo-mesh-atx", Name: "Mesh ATX case", Category: "cases",
		PriceCents: 990000, Stock: 4,
		Attributes: map[string][]string{
			"supported_form_factor": {"ATX, micro-ATX"}, "max_gpu_length_mm": {"360"},
		},
	},
	{
		Slug: "demo-rx-9070", Name: "RX 9070", Category: "gpus",
		PriceCents: 8990000, Stock: 3,
		Attributes: map[string][]string{"tdp_watts": {"260"}, "length_mm": {"320"}},
	},
	{
		Slug: "demo-am5-tower", Name: "AM5 tower cooler", Category: "cooling",
		PriceCents: 690000, Stock: 8,
		Attributes: map[string][]string{"socket": {"AM5, AM4"}},
	},
}

func seedPluginCatalog(ctx context.Context, db *sql.DB, repo *store.Store) (int, error) {
	created := 0
	now := time.Now().UTC()

	phoneCount := 0
	if err := db.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM products WHERE plugin_id = 'phone'`).Scan(&phoneCount); err != nil {
		return 0, err
	}
	if phoneCount == 0 {
		category, err := repo.CategoryBySlug(ctx, "phones")
		if err != nil {
			return created, fmt.Errorf("phone category: %w", err)
		}
		product := store.Product{
			ID:          "prd_demo_phone",
			Slug:        "aster-5",
			Name:        "Aster 5",
			Subtitle:    "A quiet handset, recorded by IMEI",
			Description: "A 6.4 inch handset with a clean display and two days of battery. Each unit is recorded with its IMEI before it leaves the shelf.",
			PriceCents:  6290000,
			Currency:    "DZD",
			Category:    category,
			Image:       "/images/demo/phone.svg",
			Gallery:     []string{"/images/demo/phone.svg"},
			Stock:       6,
			Featured:    true,
			Active:      true,
			PluginID:    "phone",
			Attributes: map[string][]string{
				"config": {"12 GB RAM + 256 GB", "16 GB RAM + 512 GB"},
				"color":  {"Midnight", "Silver"},
			},
			CreatedAt: now,
			UpdatedAt: now,
		}
		err = repo.InTx(ctx, func(tx *sql.Tx) error {
			if err := repo.CreateProduct(ctx, tx, product); err != nil {
				return err
			}
			devices := []struct {
				imei   string
				config string
				color  string
			}{
				{"356938035640003", "12 GB RAM + 256 GB", "Silver"},
				{"356938035640011", "12 GB RAM + 256 GB", "Silver"},
				{"356938035640029", "16 GB RAM + 512 GB", "Midnight"},
				{"356938035640037", "16 GB RAM + 512 GB", "Midnight"},
				{"356938035640045", "12 GB RAM + 256 GB", "Midnight"},
				{"356938035640052", "16 GB RAM + 512 GB", "Silver"},
			}
			for _, device := range devices {
				if _, err := tx.ExecContext(ctx,
					`INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei,
					 status, order_id, order_item_id, created_at, updated_at)
					 VALUES (?, ?, '', ?, ?, ?, 'in_stock', '', '', ?, ?)`,
					ids.New("phn"), product.ID, device.config, device.color, device.imei,
					store.FormatTime(now), store.FormatTime(now),
				); err != nil {
					return err
				}
			}
			return nil
		})
		if err != nil {
			return created, fmt.Errorf("seed phone demo: %w", err)
		}
		created++
	}

	partCount := 0
	if err := db.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM products WHERE plugin_id = 'pcparts'`).Scan(&partCount); err != nil {
		return created, err
	}
	if partCount == 0 {
		err := repo.InTx(ctx, func(tx *sql.Tx) error {
			for _, part := range demoParts {
				category, err := repo.CategoryBySlugTx(ctx, tx, part.Category)
				if err != nil {
					return fmt.Errorf("part category %s: %w", part.Category, err)
				}
				product := store.Product{
					ID:          "prd_demo_" + part.Category,
					Slug:        part.Slug,
					Name:        part.Name,
					Subtitle:    "Builder sample part",
					Description: "A sample PC part used by the Build your PC demo.",
					PriceCents:  part.PriceCents,
					Currency:    "DZD",
					Category:    category,
					Image:       "/images/demo/" + part.Category + ".svg",
					Gallery:     []string{"/images/demo/" + part.Category + ".svg"},
					Stock:       part.Stock,
					Featured:    false,
					Active:      true,
					PluginID:    "pcparts",
					Attributes:  part.Attributes,
					CreatedAt:   now,
					UpdatedAt:   now,
				}
				if err := repo.CreateProduct(ctx, tx, product); err != nil {
					return err
				}
				created++
			}
			return nil
		})
		if err != nil {
			return created, fmt.Errorf("seed pc parts demo: %w", err)
		}
	}

	return created, nil
}
