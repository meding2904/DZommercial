package seed

import (
	"context"
	"database/sql"
	"fmt"
	"log/slog"
	"math/rand"
	"time"

	"azahar/internal/auth"
	"azahar/internal/ids"
	"azahar/internal/store"
)

type Options struct {
	AdminEmail       string
	AdminPassword    string
	CustomerEmail    string
	CustomerPassword string
}

type Summary struct {
	Categories int
	Products   int
	Users      int
	Orders     int
}

func Run(ctx context.Context, db *sql.DB, logger *slog.Logger, opts Options) (Summary, error) {
	repo := store.New(db)
	var summary Summary

	catalog, err := seedCatalog(ctx, repo)
	if err != nil {
		return summary, fmt.Errorf("seed catalog: %w", err)
	}
	summary.Categories = catalog.categories
	summary.Products = catalog.products

	users, err := seedUsers(ctx, repo, opts)
	if err != nil {
		return summary, fmt.Errorf("seed users: %w", err)
	}
	summary.Users = users

	orders, err := seedOrders(ctx, db, repo)
	if err != nil {
		return summary, fmt.Errorf("seed orders: %w", err)
	}
	summary.Orders = orders

	if summary.Categories+summary.Products+summary.Users+summary.Orders > 0 {
		logger.Info("demo seed applied",
			"categories", summary.Categories,
			"products", summary.Products,
			"users", summary.Users,
			"orders", summary.Orders,
		)
	}
	return summary, nil
}

type seedCategory struct {
	Slug        string
	Name        string
	Description string
	SortOrder   int
}

type seedProduct struct {
	Slug        string
	Name        string
	Subtitle    string
	Description string
	PriceCents  int64
	Size        string
	Category    string
	Concerns    []string
	Ingredients []string
	HowTo       string
	Image       string
	Stock       int
	Featured    bool
}

var categories = []seedCategory{
	{Slug: "face", Name: "Face", Description: "Oils, serums and masks for the face.", SortOrder: 1},
	{Slug: "body", Name: "Body", Description: "Balms, oils and creams for the body.", SortOrder: 2},
	{Slug: "ritual", Name: "Ritual", Description: "Soaks for the bath.", SortOrder: 3},
}

var products = []seedProduct{
	{
		Slug: "orange-blossom-cleansing-oil", Name: "Orange blossom cleansing oil",
		Subtitle:    "The day, lifted",
		Description: "A light oil that lifts makeup and the day's dust. Massage it over dry skin, add water, and rinse to a clean, soft finish.",
		PriceCents:  480000, Size: "100 ml", Category: "face",
		Concerns:    []string{"dry", "sensitive"},
		Ingredients: []string{"Olive squalane", "Orange blossom", "Sweet almond"},
		HowTo:       "Warm two pumps between dry hands, massage over the face, add water, then rinse.",
		Image:       "/images/products/cleansing-oil-1200.webp", Stock: 24, Featured: true,
	},
	{
		Slug: "rosehip-and-olive-face-serum", Name: "Rosehip and olive face serum",
		Subtitle:    "A quiet layer of oil",
		Description: "Rosehip and olive oils sink in without a heavy film. Use it at night after cleansing, or in the morning under a cream.",
		PriceCents:  690000, Size: "30 ml", Category: "face",
		Concerns:    []string{"dry", "dull", "texture"},
		Ingredients: []string{"Rosehip seed oil", "Olive oil", "Jojoba"},
		HowTo:       "Press three drops into clean skin, morning or night.",
		Image:       "/images/products/serum-1200.webp", Stock: 18, Featured: true,
	},
	{
		Slug: "rose-clay-mask", Name: "Rose clay mask",
		Subtitle:    "Ten minutes in the grove",
		Description: "A soft clay mask with rose and chamomile. It lifts oil from the surface and leaves the skin comfortable, not tight.",
		PriceCents:  360000, Size: "75 g", Category: "face",
		Concerns:    []string{"oily", "congested"},
		Ingredients: []string{"Rose clay", "Kaolin", "Chamomile"},
		HowTo:       "Spread a thin layer over clean skin, leave for ten minutes, then rinse with warm water.",
		Image:       "/images/products/clay-mask-1200.webp", Stock: 4, Featured: false,
	},
	{
		Slug: "fig-and-sesame-body-balm", Name: "Fig and sesame body balm",
		Subtitle:    "For the space after a bath",
		Description: "A firm balm that melts with body heat. Fig and sesame soften rough patches on elbows, knees and hands.",
		PriceCents:  420000, Size: "120 ml", Category: "body",
		Concerns:    []string{"dry", "rough"},
		Ingredients: []string{"Fig extract", "Sesame oil", "Shea butter"},
		HowTo:       "Warm a small amount between the palms and press it into damp skin.",
		Image:       "/images/products/body-balm-1200.webp", Stock: 12, Featured: false,
	},
	{
		Slug: "prickly-pear-face-oil", Name: "Prickly pear face oil",
		Subtitle:    "The desert, pressed",
		Description: "Prickly pear and argan oils in a light blend. It leaves a soft sheen and works well on cold mornings.",
		PriceCents:  840000, Size: "30 ml", Category: "face",
		Concerns:    []string{"dry", "dull"},
		Ingredients: []string{"Prickly pear seed oil", "Argan oil", "Vitamin E"},
		HowTo:       "Warm two drops and press over the face and neck.",
		Image:       "/images/products/prickly-pear-1200.webp", Stock: 2, Featured: true,
	},
	{
		Slug: "cedar-and-myrrh-hand-cream", Name: "Cedar and myrrh hand cream",
		Subtitle:    "Wood, resin, warm hands",
		Description: "A rich cream that stays through a day of washing. Cedar and myrrh leave a dry, woody scent.",
		PriceCents:  260000, Size: "60 ml", Category: "body",
		Concerns:    []string{"dry", "rough"},
		Ingredients: []string{"Cedarwood", "Myrrh", "Shea butter"},
		HowTo:       "Massage into hands and cuticles, then work the rest into the wrists.",
		Image:       "/images/products/hand-cream-1200.webp", Stock: 30, Featured: false,
	},
	{
		Slug: "green-fig-body-oil", Name: "Green fig body oil",
		Subtitle:    "Oil for bare arms",
		Description: "Green fig and sesame in a quick body oil. It absorbs after a minute and leaves the skin soft, not wet.",
		PriceCents:  540000, Size: "100 ml", Category: "body",
		Concerns:    []string{"dry", "dull"},
		Ingredients: []string{"Green fig", "Sesame oil", "Grape seed oil"},
		HowTo:       "Smooth over damp skin after a shower and let it settle.",
		Image:       "/images/products/fig-oil-1200.webp", Stock: 15, Featured: true,
	},
	{
		Slug: "rose-petal-bath-soak", Name: "Rose petal bath soak",
		Subtitle:    "The bath, slowed",
		Description: "Rose petals, sea salt and oats for a warm bath. The water turns soft and the room smells of the garden.",
		PriceCents:  320000, Size: "200 g", Category: "ritual",
		Concerns:    []string{"dry", "sensitive"},
		Ingredients: []string{"Rose petals", "Sea salt", "Oat"},
		HowTo:       "Tip a handful into warm running water and steep for ten minutes.",
		Image:       "/images/products/rose-soak-1200.webp", Stock: 9, Featured: true,
	},
}

type catalogResult struct {
	categories int
	products   int
}

func seedCatalog(ctx context.Context, repo *store.Store) (catalogResult, error) {
	var result catalogResult
	count, err := repo.CountProducts(ctx)
	if err != nil {
		return result, err
	}
	if count > 0 {
		return result, nil
	}

	now := time.Now().UTC()
	err = repo.InTx(ctx, func(tx *sql.Tx) error {
		for _, category := range categories {
			_, err := tx.ExecContext(ctx,
				`INSERT INTO categories (id, slug, name, description, sort_order)
				 VALUES (?, ?, ?, ?, ?) ON CONFLICT(slug) DO NOTHING`,
				ids.New("cat"), category.Slug, category.Name, category.Description, category.SortOrder,
			)
			if err != nil {
				return err
			}
			result.categories++
		}

		for index, item := range products {
			category, err := repo.CategoryBySlugTx(ctx, tx, item.Category)
			if err != nil {
				return fmt.Errorf("category %s: %w", item.Category, err)
			}
			createdAt := now.AddDate(0, 0, -(len(products) - index))
			product := store.Product{
				ID:          fmt.Sprintf("prd_%02d", index+1),
				Slug:        item.Slug,
				Name:        item.Name,
				Subtitle:    item.Subtitle,
				Description: item.Description,
				PriceCents:  item.PriceCents,
				Currency:    "DZD",
				Size:        item.Size,
				Category:    category,
				Concerns:    item.Concerns,
				Ingredients: item.Ingredients,
				HowTo:       item.HowTo,
				Image:       item.Image,
				Gallery:     []string{item.Image},
				Stock:       item.Stock,
				Featured:    item.Featured,
				Active:      true,
				CreatedAt:   createdAt,
				UpdatedAt:   createdAt,
			}
			if err := repo.CreateProduct(ctx, tx, product); err != nil {
				return err
			}
			result.products++
		}
		return nil
	})
	if err != nil {
		return catalogResult{}, err
	}
	return result, nil
}

type seedUser struct {
	ID        string
	Name      string
	Email     string
	Role      string
	CreatedAt time.Time
}

func seedUsers(ctx context.Context, repo *store.Store, opts Options) (int, error) {
	count, err := repo.CountUsers(ctx)
	if err != nil {
		return 0, err
	}
	if count > 0 {
		return 0, nil
	}

	now := time.Now().UTC()
	created := 0
	adminHash, err := auth.HashPassword(opts.AdminPassword)
	if err != nil {
		return 0, err
	}
	customerHash, err := auth.HashPassword(opts.CustomerPassword)
	if err != nil {
		return 0, err
	}
	sampleHash, err := auth.HashPassword("azahar-sample-customer-2026")
	if err != nil {
		return 0, err
	}

	users := []seedUser{
		{ID: "usr_admin", Name: "Azahar admin", Email: opts.AdminEmail, Role: "admin", CreatedAt: now.AddDate(0, 0, -60)},
		{ID: "usr_client", Name: "Demo client", Email: opts.CustomerEmail, Role: "customer", CreatedAt: now.AddDate(0, 0, -50)},
	}
	for index := 1; index <= 12; index++ {
		users = append(users, seedUser{
			ID:        fmt.Sprintf("usr_sample_%02d", index),
			Name:      fmt.Sprintf("Sample customer %02d", index),
			Email:     fmt.Sprintf("sample%02d@azahar.example", index),
			Role:      "customer",
			CreatedAt: now.AddDate(0, 0, -(index * 3)),
		})
	}

	err = repo.InTx(ctx, func(tx *sql.Tx) error {
		for _, user := range users {
			hash := sampleHash
			if user.Role == "admin" {
				hash = adminHash
			} else if user.Email == opts.CustomerEmail {
				hash = customerHash
			}
			err := repo.CreateUser(ctx, tx, store.User{
				ID:           user.ID,
				Name:         user.Name,
				Email:        user.Email,
				PasswordHash: hash,
				Role:         user.Role,
				ThemePreset:  "cellar",
				CreatedAt:    user.CreatedAt,
				UpdatedAt:    user.CreatedAt,
			})
			if err != nil {
				return err
			}
			created++
		}
		return nil
	})
	if err != nil {
		return 0, err
	}
	return created, nil
}

type seedOrderProduct struct {
	ID         string
	Slug       string
	Name       string
	Image      string
	PriceCents int64
}

type seedOrderUser struct {
	ID    string
	Name  string
	Email string
}

type statusPlan struct {
	status string
	count  int
	minDay int
	maxDay int
}

func seedOrders(ctx context.Context, db *sql.DB, repo *store.Store) (int, error) {
	count, err := repo.CountOrders(ctx)
	if err != nil {
		return 0, err
	}
	if count > 0 {
		return 0, nil
	}

	productRows, err := db.QueryContext(ctx,
		`SELECT id, slug, name, image, price_cents FROM products WHERE active = 1 ORDER BY id`)
	if err != nil {
		return 0, err
	}
	var catalog []seedOrderProduct
	for productRows.Next() {
		var product seedOrderProduct
		if err := productRows.Scan(&product.ID, &product.Slug, &product.Name,
			&product.Image, &product.PriceCents); err != nil {
			productRows.Close()
			return 0, err
		}
		catalog = append(catalog, product)
	}
	productRows.Close()
	if err := productRows.Err(); err != nil {
		return 0, err
	}
	if len(catalog) == 0 {
		return 0, fmt.Errorf("no active products to seed orders with")
	}

	userRows, err := db.QueryContext(ctx,
		`SELECT id, name, email FROM users WHERE role = 'customer' ORDER BY email`)
	if err != nil {
		return 0, err
	}
	var customers []seedOrderUser
	for userRows.Next() {
		var user seedOrderUser
		if err := userRows.Scan(&user.ID, &user.Name, &user.Email); err != nil {
			userRows.Close()
			return 0, err
		}
		customers = append(customers, user)
	}
	userRows.Close()
	if err := userRows.Err(); err != nil {
		return 0, err
	}
	if len(customers) == 0 {
		return 0, fmt.Errorf("no customers to seed orders with")
	}

	rng := rand.New(rand.NewSource(20261111))
	plans := []statusPlan{
		{status: "pending", count: 6, minDay: 0, maxDay: 4},
		{status: "confirmed", count: 6, minDay: 1, maxDay: 7},
		{status: "packed", count: 5, minDay: 2, maxDay: 10},
		{status: "shipped", count: 8, minDay: 4, maxDay: 20},
		{status: "delivered", count: 20, minDay: 8, maxDay: 44},
		{status: "cancelled", count: 3, minDay: 5, maxDay: 30},
	}

	now := time.Now().UTC()
	today := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
	cities := []string{"Algiers", "Oran", "Constantine", "Tlemcen", "Annaba", "Blida"}
	type destination struct {
		code    int
		wilaya  string
		commune string
	}
	destinations := map[string]destination{
		"Algiers":     {code: 16, wilaya: "Alger", commune: "Alger Centre"},
		"Oran":        {code: 31, wilaya: "Oran", commune: "Oran"},
		"Constantine": {code: 25, wilaya: "Constantine", commune: "Constantine"},
		"Tlemcen":     {code: 13, wilaya: "Tlemcen", commune: "Tlemcen"},
		"Annaba":      {code: 23, wilaya: "Annaba", commune: "Annaba"},
		"Blida":       {code: 9, wilaya: "Blida", commune: "Blida"},
	}
	guestCount := 0
	created := 0

	type pendingOrder struct {
		status string
		offset int
	}
	var queue []pendingOrder
	for _, plan := range plans {
		for index := 0; index < plan.count; index++ {
			offset := plan.minDay
			if plan.maxDay > plan.minDay {
				offset += index * (plan.maxDay - plan.minDay) / (plan.count - 1)
			}
			queue = append(queue, pendingOrder{status: plan.status, offset: offset})
		}
	}
	rng.Shuffle(len(queue), func(i, j int) {
		queue[i], queue[j] = queue[j], queue[i]
	})

	err = repo.InTx(ctx, func(tx *sql.Tx) error {
		for index, plan := range queue {
			createdAt := today.AddDate(0, 0, -plan.offset).
				Add(time.Duration(9+rng.Intn(10))*time.Hour + time.Duration(rng.Intn(60))*time.Minute)

			var userID *string
			var name, email string
			if index < 6 {
				guestCount++
				name = fmt.Sprintf("Sample guest %02d", guestCount)
				email = fmt.Sprintf("guest%02d@azahar.example", guestCount)
			} else {
				customer := customers[rng.Intn(len(customers))]
				userID = &customer.ID
				name = customer.Name
				email = customer.Email
			}

			lineCount := 1 + rng.Intn(4)
			permutation := rng.Perm(len(catalog))
			items := make([]store.OrderItem, 0, lineCount)
			var subtotal int64
			for line := 0; line < lineCount; line++ {
				product := catalog[permutation[line]]
				quantity := 1 + rng.Intn(3)
				lineTotal := product.PriceCents * int64(quantity)
				subtotal += lineTotal
				items = append(items, store.OrderItem{
					ID:             ids.New("itm"),
					ProductID:      product.ID,
					Slug:           product.Slug,
					Name:           product.Name,
					Image:          product.Image,
					UnitPriceCents: product.PriceCents,
					Quantity:       quantity,
					LineTotalCents: lineTotal,
				})
			}

			shipping := int64(60000)
			if subtotal > 800000 {
				shipping = 0
			}
			token, err := auth.NewToken()
			if err != nil {
				return err
			}
			orderID := ids.New("ord")
			number, err := repo.NextOrderNumber(ctx, tx, createdAt.Year())
			if err != nil {
				return err
			}
			notes := ""
			if rng.Intn(4) == 0 {
				notes = "Leave with the concierge"
			}
			city := cities[rng.Intn(len(cities))]
			destination := destinations[city]
			order := store.Order{
				ID:            orderID,
				Number:        number,
				UserID:        userID,
				Name:          name,
				Email:         email,
				Phone:         fmt.Sprintf("0550 %02d %02d %02d", 10+rng.Intn(80), 10+rng.Intn(80), 10+rng.Intn(80)),
				Address:       fmt.Sprintf("%d rue des Oliviers", 4+rng.Intn(80)),
				City:          destination.commune,
				Notes:         notes,
				SubtotalCents: subtotal,
				ShippingCents: shipping,
				TotalCents:    subtotal + shipping,
				Currency:      "DZD",
				Status:        plan.status,
				TokenHash:     auth.HashToken(token),
				CreatedAt:     createdAt,
				UpdatedAt:     createdAt,
				WilayaCode:    destination.code,
				WilayaName:    destination.wilaya,
				CommuneName:   destination.commune,
				DeliveryType:  "home",
			}
			if err := repo.CreateOrder(ctx, tx, order, items); err != nil {
				return err
			}
			created++
		}
		return nil
	})
	if err != nil {
		return 0, err
	}
	return created, nil
}
