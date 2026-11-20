package pcbuilder

import (
	"net/http"
	"sort"
	"strings"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/plugins/pcparts"
	"azahar/internal/store"
)

// maxParts caps one catalog response. The storefront only has one slot per
// category, so a larger shelf would not fit the builder anyway.
const maxParts = 500

type partsResponse struct {
	Categories []partsCategory `json:"categories"`
}

type partsCategory struct {
	Slug     string         `json:"slug"`
	Name     string         `json:"name"`
	Products []partsProduct `json:"products"`
}

type partsProduct struct {
	ID                  string              `json:"id"`
	Slug                string              `json:"slug"`
	Name                string              `json:"name"`
	Subtitle            string              `json:"subtitle"`
	Description         string              `json:"description"`
	PriceCents          int64               `json:"price_cents"`
	SalePriceCents      *int64              `json:"sale_price_cents"`
	SaleStartsAt        string              `json:"sale_starts_at"`
	SaleEndsAt          string              `json:"sale_ends_at"`
	EffectivePriceCents int64               `json:"effective_price_cents"`
	OnSale              bool                `json:"on_sale"`
	Currency            string              `json:"currency"`
	Image               string              `json:"image"`
	Stock               int                 `json:"stock"`
	PluginID            string              `json:"plugin_id"`
	Attributes          map[string][]string `json:"attributes"`
	Category            partsCategoryRef    `json:"category"`
}

type partsCategoryRef struct {
	Slug     string `json:"slug"`
	Name     string `json:"name"`
	PluginID string `json:"plugin_id"`
}

func (p *Plugin) handleParts(c *plugins.Context) {
	if c == nil {
		return
	}
	if c.Env == nil || c.Env.Store == nil {
		c.Error(http.StatusServiceUnavailable, "store_unavailable",
			"The parts catalog is not available right now.")
		return
	}

	ctx := c.R.Context()
	categories, err := c.Env.Store.Categories(ctx)
	if err != nil {
		c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	products := make([]store.Product, 0)
	for _, category := range categories {
		if category.PluginID != pcparts.PluginID {
			continue
		}
		items, err := c.Env.Store.ListProducts(ctx, store.ProductFilter{
			Category: category.Slug,
			Limit:    maxParts,
		})
		if err != nil {
			c.Error(http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		products = append(products, items...)
	}

	c.JSON(http.StatusOK, buildPartsResponse(categories, products))
}

// buildPartsResponse is kept free of the database so the response shape and
// ordering can be checked without a store.
func buildPartsResponse(categories []store.Category, products []store.Product) partsResponse {
	owned := make([]store.Category, 0, len(categories))
	seen := make(map[string]bool, len(categories))
	for _, category := range categories {
		if category.PluginID != pcparts.PluginID || seen[category.Slug] {
			continue
		}
		seen[category.Slug] = true
		owned = append(owned, category)
	}
	sort.SliceStable(owned, func(i, j int) bool {
		if owned[i].SortOrder != owned[j].SortOrder {
			return owned[i].SortOrder < owned[j].SortOrder
		}
		if owned[i].Name != owned[j].Name {
			return owned[i].Name < owned[j].Name
		}
		return owned[i].Slug < owned[j].Slug
	})

	grouped := make(map[string][]store.Product, len(owned))
	for _, product := range products {
		if !product.Active || product.PluginID != pcparts.PluginID {
			continue
		}
		grouped[product.Category.Slug] = append(grouped[product.Category.Slug], product)
	}

	response := partsResponse{Categories: make([]partsCategory, 0, len(owned))}
	left := maxParts
	for _, category := range owned {
		items := grouped[category.Slug]
		sort.SliceStable(items, func(i, j int) bool {
			leftName := strings.ToLower(items[i].Name)
			rightName := strings.ToLower(items[j].Name)
			if leftName != rightName {
				return leftName < rightName
			}
			return items[i].Slug < items[j].Slug
		})

		listed := make([]partsProduct, 0, len(items))
		for _, product := range items {
			if left == 0 {
				break
			}
			listed = append(listed, toPartsProduct(product))
			left--
		}
		response.Categories = append(response.Categories, partsCategory{
			Slug:     category.Slug,
			Name:     category.Name,
			Products: listed,
		})
	}
	return response
}

func toPartsProduct(product store.Product) partsProduct {
	attributes := product.Attributes
	if attributes == nil {
		attributes = map[string][]string{}
	}
	currency := product.Currency
	if currency == "" {
		currency = "DZD"
	}
	now := time.Now().UTC()
	return partsProduct{
		ID:                  product.ID,
		Slug:                product.Slug,
		Name:                product.Name,
		Subtitle:            product.Subtitle,
		Description:         product.Description,
		PriceCents:          product.PriceCents,
		SalePriceCents:      product.SalePriceCents,
		SaleStartsAt:        store.FormatOptionalTime(product.SaleStartsAt),
		SaleEndsAt:          store.FormatOptionalTime(product.SaleEndsAt),
		EffectivePriceCents: product.EffectivePriceCents(now),
		OnSale:              product.SaleActive(now),
		Currency:            currency,
		Image:               product.Image,
		Stock:               product.Stock,
		PluginID:            pcparts.PluginID,
		Attributes:          attributes,
		Category: partsCategoryRef{
			Slug:     product.Category.Slug,
			Name:     product.Category.Name,
			PluginID: product.Category.PluginID,
		},
	}
}
