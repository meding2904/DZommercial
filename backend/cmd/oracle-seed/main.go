// Command oracle-seed runs the Go demo seed against an empty scratch libSQL
// server and prints one JSON report.
//
// It is an additive oracle for the Rust port in crates/azahar-seed. The caller
// must supply the DSN of a disposable server; the command refuses to run when
// schema_migrations already exists, so pointing it at the shared demo database
// fails instead of changing it.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"sort"
	"strings"
	"time"

	"azahar/internal/auth"
	"azahar/internal/db"
	"azahar/internal/plugins"
	"azahar/internal/plugins/oils"
	"azahar/internal/plugins/pcparts"
	"azahar/internal/plugins/phone"
	"azahar/internal/seed"
	"azahar/internal/store"
)

type report struct {
	First        summary       `json:"first"`
	Second       summary       `json:"second"`
	Counts       counts        `json:"counts"`
	Users        []userRow     `json:"users"`
	Products     []productRow  `json:"products"`
	Categories   []categoryRow `json:"categories"`
	PhoneDevices []deviceRow   `json:"phone_devices"`
	Orders       orderReport   `json:"orders"`
}

type summary struct {
	Categories int `json:"categories"`
	Products   int `json:"products"`
	Users      int `json:"users"`
	Orders     int `json:"orders"`
}

func summarize(value seed.Summary) summary {
	return summary{
		Categories: value.Categories,
		Products:   value.Products,
		Users:      value.Users,
		Orders:     value.Orders,
	}
}

type counts struct {
	Users         int `json:"users"`
	Products      int `json:"products"`
	Orders        int `json:"orders"`
	OrderItems    int `json:"order_items"`
	Categories    int `json:"categories"`
	PhoneDevices  int `json:"phone_devices"`
	AuditLog      int `json:"audit_log"`
	SequenceTotal int `json:"sequence_total"`
}

type userRow struct {
	ID              string `json:"id"`
	Name            string `json:"name"`
	Email           string `json:"email"`
	Role            string `json:"role"`
	ThemePreset     string `json:"theme_preset"`
	CreatedDay      int    `json:"created_day_offset"`
	HashAlgorithm   string `json:"hash_algorithm"`
	PasswordOK      bool   `json:"password_ok"`
	WrongPasswordOK bool   `json:"wrong_password_ok"`
}

type productRow struct {
	ID                  string              `json:"id"`
	Slug                string              `json:"slug"`
	Name                string              `json:"name"`
	Subtitle            string              `json:"subtitle"`
	Description         string              `json:"description"`
	PriceCents          int64               `json:"price_cents"`
	SalePriceCents      *int64              `json:"sale_price_cents"`
	HasSaleWindow       bool                `json:"has_sale_window"`
	SaleWindowHours     int                 `json:"sale_window_hours"`
	Currency            string              `json:"currency"`
	Size                string              `json:"size"`
	CategoryID          string              `json:"category_id"`
	CategorySlug        string              `json:"category_slug"`
	CategoryPluginID    string              `json:"category_plugin_id"`
	Concerns            []string            `json:"concerns"`
	Ingredients         []string            `json:"ingredients"`
	HowTo               string              `json:"how_to"`
	Image               string              `json:"image"`
	Gallery             []string            `json:"gallery"`
	Stock               int                 `json:"stock"`
	Featured            bool                `json:"featured"`
	Active              bool                `json:"active"`
	PluginID            string              `json:"plugin_id"`
	Attributes          map[string][]string `json:"attributes"`
	CreatedDay          int                 `json:"created_day_offset"`
	UpdatedEqualsCreate bool                `json:"updated_equals_created"`
}

type categoryRow struct {
	ID        string `json:"id"`
	Slug      string `json:"slug"`
	Name      string `json:"name"`
	PluginID  string `json:"plugin_id"`
	SortOrder int    `json:"sort_order"`
}

type deviceRow struct {
	ProductID  string `json:"product_id"`
	VariantKey string `json:"variant_key"`
	Config     string `json:"config"`
	Color      string `json:"color"`
	IMEI       string `json:"imei"`
	Status     string `json:"status"`
	OrderID    string `json:"order_id"`
	CreatedDay int    `json:"created_day_offset"`
}

type orderReport struct {
	Count            int              `json:"count"`
	StatusCounts     map[string]int   `json:"status_counts"`
	StatusDayOffsets map[string][]int `json:"status_day_offsets"`
	GuestCount       int              `json:"guest_count"`
	LinkedCount      int              `json:"linked_count"`
	MinItems         int              `json:"min_items"`
	MaxItems         int              `json:"max_items"`
	MinQuantity      int              `json:"min_quantity"`
	MaxQuantity      int              `json:"max_quantity"`
	DistinctProducts int              `json:"distinct_products"`
	InvariantsOK     bool             `json:"invariants_ok"`
	NumbersUnique    bool             `json:"numbers_unique"`
	TokensUnique     bool             `json:"tokens_unique"`
}

func main() {
	dsn := flag.String("dsn", "", "DSN of an empty, disposable libSQL server")
	flag.Parse()
	if *dsn == "" {
		fail("missing -dsn")
	}

	ctx := context.Background()
	handle, err := db.Open(ctx, *dsn)
	if err != nil {
		fail(err.Error())
	}
	defer handle.Close()

	var existing int
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM sqlite_master
		 WHERE type = 'table' AND name = 'schema_migrations'`).Scan(&existing); err != nil {
		fail(err.Error())
	}
	if existing != 0 {
		fail("refusing to seed a database that already has schema_migrations")
	}

	if err := db.Migrate(ctx, handle); err != nil {
		fail(err.Error())
	}
	phonePlugin := phone.New()
	phoneMigrations := make([]db.PluginMigration, 0, len(phonePlugin.Migrations()))
	for _, migration := range phonePlugin.Migrations() {
		phoneMigrations = append(phoneMigrations, db.PluginMigration{
			PluginID: "phone",
			Version:  migration.Version,
			SQL:      migration.SQL,
		})
	}
	if err := db.MigratePlugins(ctx, handle, phoneMigrations); err != nil {
		fail(err.Error())
	}

	repo := store.New(handle)
	for _, plugin := range []plugins.Plugin{oils.New(), pcparts.New(), phonePlugin} {
		manifest := plugin.Manifest()
		for _, category := range manifest.Categories {
			if err := repo.UpsertCategory(ctx, manifest.ID, store.Category{
				Slug:        category.Slug,
				Name:        category.Name,
				Description: category.Description,
				SortOrder:   category.SortOrder,
			}); err != nil {
				fail(err.Error())
			}
		}
	}

	logger := slog.New(slog.NewJSONHandler(io.Discard, nil))
	opts := seed.Options{
		AdminEmail:       "admin@azahar.example",
		AdminPassword:    "azahar-dev-admin-2026",
		CustomerEmail:    "client@azahar.example",
		CustomerPassword: "azahar-dev-client-2026",
		EmployeeEmail:    "staff@azahar.example",
		EmployeePassword: "azahar-dev-staff-2026",
	}
	first, err := seed.Run(ctx, handle, logger, opts)
	if err != nil {
		fail(err.Error())
	}
	second, err := seed.Run(ctx, handle, logger, opts)
	if err != nil {
		fail(err.Error())
	}

	result, err := collect(ctx, repo, opts, first, second)
	if err != nil {
		fail(err.Error())
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		fail(err.Error())
	}
	fmt.Println(string(encoded))
}

func collect(
	ctx context.Context,
	repo *store.Store,
	opts seed.Options,
	first, second seed.Summary,
) (report, error) {
	result := report{First: summarize(first), Second: summarize(second)}
	today := time.Now().UTC()
	today = time.Date(today.Year(), today.Month(), today.Day(), 0, 0, 0, 0, time.UTC)
	count := func(table string) (int, error) {
		var value int
		err := repo.DB().QueryRowContext(ctx, "SELECT COUNT(*) FROM "+table).Scan(&value)
		return value, err
	}

	var err error
	if result.Counts.Users, err = count("users"); err != nil {
		return result, err
	}
	if result.Counts.Products, err = count("products"); err != nil {
		return result, err
	}
	if result.Counts.Orders, err = count("orders"); err != nil {
		return result, err
	}
	if result.Counts.OrderItems, err = count("order_items"); err != nil {
		return result, err
	}
	if result.Counts.Categories, err = count("categories"); err != nil {
		return result, err
	}
	if result.Counts.PhoneDevices, err = count("phone_devices"); err != nil {
		return result, err
	}
	if result.Counts.AuditLog, err = count("audit_log"); err != nil {
		return result, err
	}
	if err := repo.DB().QueryRowContext(ctx,
		"SELECT COALESCE(SUM(last_value), 0) FROM order_sequences").Scan(&result.Counts.SequenceTotal); err != nil {
		return result, err
	}

	users, err := repo.ListUsers(ctx)
	if err != nil {
		return result, err
	}
	result.Users = make([]userRow, 0, len(users))
	for _, user := range users {
		row := userRow{
			ID:            user.ID,
			Name:          user.Name,
			Email:         user.Email,
			Role:          user.Role,
			ThemePreset:   user.ThemePreset,
			CreatedDay:    daysAgo(today, user.CreatedAt),
			HashAlgorithm: hashAlgorithm(user.PasswordHash),
		}
		password := "azahar-sample-customer-2026"
		switch {
		case user.Role == "admin":
			password = opts.AdminPassword
		case user.Role == "employee":
			password = opts.EmployeePassword
		case user.Email == opts.CustomerEmail:
			password = opts.CustomerPassword
		}
		row.PasswordOK = auth.VerifyPassword(password, user.PasswordHash)
		row.WrongPasswordOK = auth.VerifyPassword("definitely-wrong", user.PasswordHash)
		result.Users = append(result.Users, row)
	}
	sort.Slice(result.Users, func(i, j int) bool { return result.Users[i].ID < result.Users[j].ID })

	products, err := repo.ListProducts(ctx, store.ProductFilter{IncludeInactive: true, Limit: 500})
	if err != nil {
		return result, err
	}
	result.Products = make([]productRow, 0, len(products))
	for _, product := range products {
		row := productRow{
			ID:                  product.ID,
			Slug:                product.Slug,
			Name:                product.Name,
			Subtitle:            product.Subtitle,
			Description:         product.Description,
			PriceCents:          product.PriceCents,
			SalePriceCents:      product.SalePriceCents,
			Currency:            product.Currency,
			Size:                product.Size,
			CategoryID:          product.Category.ID,
			CategorySlug:        product.Category.Slug,
			CategoryPluginID:    product.Category.PluginID,
			Concerns:            product.Concerns,
			Ingredients:         product.Ingredients,
			HowTo:               product.HowTo,
			Image:               product.Image,
			Gallery:             product.Gallery,
			Stock:               product.Stock,
			Featured:            product.Featured,
			Active:              product.Active,
			PluginID:            product.PluginID,
			Attributes:          product.Attributes,
			CreatedDay:          daysAgo(today, product.CreatedAt),
			UpdatedEqualsCreate: product.UpdatedAt.Equal(product.CreatedAt),
		}
		row.HasSaleWindow = !product.SaleStartsAt.IsZero() || !product.SaleEndsAt.IsZero()
		if row.HasSaleWindow {
			row.SaleWindowHours = int(product.SaleEndsAt.Sub(product.SaleStartsAt).Hours() + 0.5)
		}
		result.Products = append(result.Products, row)
	}
	sort.Slice(result.Products, func(i, j int) bool { return result.Products[i].ID < result.Products[j].ID })

	categories, err := repo.Categories(ctx)
	if err != nil {
		return result, err
	}
	result.Categories = make([]categoryRow, 0, len(categories))
	for _, category := range categories {
		result.Categories = append(result.Categories, categoryRow{
			ID:        category.ID,
			Slug:      category.Slug,
			Name:      category.Name,
			PluginID:  category.PluginID,
			SortOrder: category.SortOrder,
		})
	}
	sort.Slice(result.Categories, func(i, j int) bool {
		return result.Categories[i].Slug < result.Categories[j].Slug
	})

	deviceRows, err := repo.DB().QueryContext(ctx,
		`SELECT product_id, variant_key, config, color, imei, status, order_id, created_at
		 FROM phone_devices ORDER BY imei`)
	if err != nil {
		return result, err
	}
	for deviceRows.Next() {
		var row deviceRow
		var created string
		if err := deviceRows.Scan(&row.ProductID, &row.VariantKey, &row.Config, &row.Color,
			&row.IMEI, &row.Status, &row.OrderID, &created); err != nil {
			deviceRows.Close()
			return result, err
		}
		row.CreatedDay = daysAgo(today, store.ParseTime(created))
		result.PhoneDevices = append(result.PhoneDevices, row)
	}
	deviceRows.Close()
	if err := deviceRows.Err(); err != nil {
		return result, err
	}
	sort.Slice(result.PhoneDevices, func(i, j int) bool {
		return result.PhoneDevices[i].IMEI < result.PhoneDevices[j].IMEI
	})

	orders, err := repo.AdminOrders(ctx, store.AdminOrderFilter{Limit: 500})
	if err != nil {
		return result, err
	}
	result.Orders = orderReport{
		Count:            len(orders),
		StatusCounts:     map[string]int{},
		StatusDayOffsets: map[string][]int{},
		MinItems:         -1,
		MinQuantity:      -1,
		InvariantsOK:     true,
		NumbersUnique:    true,
		TokensUnique:     true,
	}
	numbers := map[string]bool{}
	tokens := map[string]bool{}
	productsByID := map[string]productRow{}
	for _, product := range result.Products {
		productsByID[product.ID] = product
	}
	usedProducts := map[string]bool{}
	for _, order := range orders {
		result.Orders.StatusCounts[order.Status]++
		result.Orders.StatusDayOffsets[order.Status] = append(
			result.Orders.StatusDayOffsets[order.Status], daysAgo(today, order.CreatedAt))
		if order.UserID == nil {
			result.Orders.GuestCount++
		} else {
			result.Orders.LinkedCount++
		}
		result.Orders.NumbersUnique = result.Orders.NumbersUnique && !numbers[order.Number]
		numbers[order.Number] = true
		result.Orders.TokensUnique = result.Orders.TokensUnique && !tokens[order.TokenHash]
		tokens[order.TokenHash] = true

		items, err := repo.OrderItems(ctx, order.ID)
		if err != nil {
			return result, err
		}
		if result.Orders.MinItems < 0 || len(items) < result.Orders.MinItems {
			result.Orders.MinItems = len(items)
		}
		if len(items) > result.Orders.MaxItems {
			result.Orders.MaxItems = len(items)
		}
		productIDs := map[string]bool{}
		var subtotal int64
		for _, item := range items {
			productIDs[item.ProductID] = true
			usedProducts[item.ProductID] = true
			if result.Orders.MinQuantity < 0 || item.Quantity < result.Orders.MinQuantity {
				result.Orders.MinQuantity = item.Quantity
			}
			if item.Quantity > result.Orders.MaxQuantity {
				result.Orders.MaxQuantity = item.Quantity
			}
			product, ok := productsByID[item.ProductID]
			check := ok &&
				item.Slug == product.Slug &&
				item.Name == product.Name &&
				item.Image == product.Image &&
				item.UnitPriceCents == product.PriceCents &&
				item.Quantity >= 1 && item.Quantity <= 3 &&
				item.LineTotalCents == item.UnitPriceCents*int64(item.Quantity) &&
				item.PluginID == "oils"
			result.Orders.InvariantsOK = result.Orders.InvariantsOK && check
			subtotal += item.LineTotalCents
		}
		hour := order.CreatedAt.UTC().Hour()
		result.Orders.InvariantsOK = result.Orders.InvariantsOK &&
			len(items) >= 1 && len(items) <= 4 &&
			len(productIDs) == len(items) &&
			order.SubtotalCents == subtotal &&
			order.ShippingCents == shippingCents(subtotal) &&
			order.TotalCents == subtotal+order.ShippingCents &&
			order.Currency == "DZD" &&
			order.DeliveryType == "home" &&
			order.WilayaCode == destination(order.City).code &&
			order.WilayaName == destination(order.City).wilaya &&
			order.CommuneName == order.City &&
			hour >= 9 && hour <= 18 &&
			strings.HasPrefix(order.Phone, "0550 ") && len(order.Phone) == 13 &&
			strings.HasSuffix(order.Address, " rue des Oliviers") &&
			(order.Notes == "" || order.Notes == "Leave with the concierge") &&
			validOrderNumber(order.Number, order.CreatedAt.Year()) &&
			validTokenHash(order.TokenHash)
	}
	result.Orders.DistinctProducts = len(usedProducts)
	for status := range result.Orders.StatusDayOffsets {
		sort.Ints(result.Orders.StatusDayOffsets[status])
	}
	return result, nil
}

type destinationInfo struct {
	code    int
	wilaya  string
	commune string
}

func destination(commune string) destinationInfo {
	switch commune {
	case "Alger Centre":
		return destinationInfo{16, "Alger", "Alger Centre"}
	case "Oran":
		return destinationInfo{31, "Oran", "Oran"}
	case "Constantine":
		return destinationInfo{25, "Constantine", "Constantine"}
	case "Tlemcen":
		return destinationInfo{13, "Tlemcen", "Tlemcen"}
	case "Annaba":
		return destinationInfo{23, "Annaba", "Annaba"}
	case "Blida":
		return destinationInfo{9, "Blida", "Blida"}
	default:
		return destinationInfo{-1, "", ""}
	}
}

func shippingCents(subtotal int64) int64 {
	if subtotal > 800000 {
		return 0
	}
	return 60000
}

func validOrderNumber(number string, year int) bool {
	parts := strings.Split(number, "-")
	if len(parts) != 3 || parts[0] != "AZ" {
		return false
	}
	if parts[1] != fmt.Sprintf("%d", year) || len(parts[2]) < 4 {
		return false
	}
	var value int
	if _, err := fmt.Sscanf(parts[2], "%d", &value); err != nil || value < 1 {
		return false
	}
	return true
}

func validTokenHash(hash string) bool {
	if len(hash) != 64 {
		return false
	}
	for _, char := range hash {
		if !(char >= '0' && char <= '9') && !(char >= 'a' && char <= 'f') {
			return false
		}
	}
	return true
}

func hashAlgorithm(hash string) string {
	parts := strings.Split(hash, "$")
	if len(parts) == 6 && parts[1] == "argon2id" && parts[2] == "v=19" {
		return "argon2id-v19"
	}
	return "other"
}

func daysAgo(today, value time.Time) int {
	if value.IsZero() {
		return 1 << 30
	}
	day := time.Date(value.UTC().Year(), value.UTC().Month(), value.UTC().Day(), 0, 0, 0, 0, time.UTC)
	return int(today.Sub(day).Hours() / 24)
}

func fail(message string) {
	fmt.Fprintln(os.Stderr, "oracle-seed:", message)
	os.Exit(1)
}
