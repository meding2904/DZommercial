package httpapi

import (
	"time"

	"azahar/internal/store"
)

type categoryDTO struct {
	Slug        string `json:"slug"`
	Name        string `json:"name"`
	Description string `json:"description"`
}

type categoryRefDTO struct {
	Slug string `json:"slug"`
	Name string `json:"name"`
}

type productDTO struct {
	ID             string         `json:"id"`
	Slug           string         `json:"slug"`
	Name           string         `json:"name"`
	Subtitle       string         `json:"subtitle"`
	Description    string         `json:"description"`
	PriceCents     int64          `json:"price_cents"`
	CompareAtCents *int64         `json:"compare_at_cents"`
	Currency       string         `json:"currency"`
	Size           string         `json:"size"`
	Category       categoryRefDTO `json:"category"`
	Concerns       []string       `json:"concerns"`
	Ingredients    []string       `json:"ingredients"`
	HowTo          string         `json:"how_to"`
	Image          string         `json:"image"`
	Gallery        []string       `json:"gallery"`
	Stock          int            `json:"stock"`
	Featured       bool           `json:"featured"`
	Active         bool           `json:"active"`
	CreatedAt      string         `json:"created_at,omitempty"`
	UpdatedAt      string         `json:"updated_at,omitempty"`
}

func toProduct(product store.Product, withTimestamps bool) productDTO {
	dto := productDTO{
		ID:             product.ID,
		Slug:           product.Slug,
		Name:           product.Name,
		Subtitle:       product.Subtitle,
		Description:    product.Description,
		PriceCents:     product.PriceCents,
		CompareAtCents: product.CompareAtCents,
		Currency:       product.Currency,
		Size:           product.Size,
		Category:       categoryRefDTO{Slug: product.Category.Slug, Name: product.Category.Name},
		Concerns:       nonNil(product.Concerns),
		Ingredients:    nonNil(product.Ingredients),
		HowTo:          product.HowTo,
		Image:          product.Image,
		Gallery:        nonNil(product.Gallery),
		Stock:          product.Stock,
		Featured:       product.Featured,
		Active:         product.Active,
	}
	if withTimestamps {
		dto.CreatedAt = store.FormatTime(product.CreatedAt)
		dto.UpdatedAt = store.FormatTime(product.UpdatedAt)
	}
	return dto
}

func toProducts(products []store.Product, withTimestamps bool) []productDTO {
	result := make([]productDTO, 0, len(products))
	for _, product := range products {
		result = append(result, toProduct(product, withTimestamps))
	}
	return result
}

type userDTO struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Email string `json:"email"`
	Role  string `json:"role"`
}

func toUser(user store.User) userDTO {
	return userDTO{ID: user.ID, Name: user.Name, Email: user.Email, Role: user.Role}
}

type orderItemDTO struct {
	ID             string `json:"id"`
	ProductID      string `json:"product_id"`
	Slug           string `json:"slug"`
	Name           string `json:"name"`
	Image          string `json:"image"`
	UnitPriceCents int64  `json:"unit_price_cents"`
	Quantity       int    `json:"quantity"`
	LineTotalCents int64  `json:"line_total_cents"`
}

type orderCustomerDTO struct {
	ID    string `json:"id,omitempty"`
	Name  string `json:"name"`
	Email string `json:"email"`
	Phone string `json:"phone"`
}

type orderDTO struct {
	ID            string           `json:"id"`
	Number        string           `json:"number"`
	Token         string           `json:"token,omitempty"`
	Status        string           `json:"status"`
	Name          string           `json:"name"`
	Email         string           `json:"email"`
	Phone         string           `json:"phone"`
	Address       string           `json:"address,omitempty"`
	City          string           `json:"city"`
	Notes         string           `json:"notes"`
	SubtotalCents int64            `json:"subtotal_cents"`
	ShippingCents int64            `json:"shipping_cents"`
	TotalCents    int64            `json:"total_cents"`
	Currency      string           `json:"currency"`
	Items         []orderItemDTO   `json:"items"`
	Customer      orderCustomerDTO `json:"customer"`
	CreatedAt     string           `json:"created_at"`
	UpdatedAt     string           `json:"updated_at"`
}

func toOrder(order store.Order, items []store.OrderItem, token string, includeAddress bool) orderDTO {
	dto := orderDTO{
		ID:            order.ID,
		Number:        order.Number,
		Token:         token,
		Status:        order.Status,
		Name:          order.Name,
		Email:         order.Email,
		Phone:         order.Phone,
		City:          order.City,
		Notes:         order.Notes,
		SubtotalCents: order.SubtotalCents,
		ShippingCents: order.ShippingCents,
		TotalCents:    order.TotalCents,
		Currency:      order.Currency,
		Items:         toOrderItems(items),
		Customer: orderCustomerDTO{
			Name:  order.Name,
			Email: order.Email,
			Phone: order.Phone,
		},
		CreatedAt: store.FormatTime(order.CreatedAt),
		UpdatedAt: store.FormatTime(order.UpdatedAt),
	}
	if includeAddress {
		dto.Address = order.Address
	}
	if order.UserID != nil {
		dto.Customer.ID = *order.UserID
	}
	return dto
}

func toOrderItems(items []store.OrderItem) []orderItemDTO {
	result := make([]orderItemDTO, 0, len(items))
	for _, item := range items {
		result = append(result, orderItemDTO{
			ID:             item.ID,
			ProductID:      item.ProductID,
			Slug:           item.Slug,
			Name:           item.Name,
			Image:          item.Image,
			UnitPriceCents: item.UnitPriceCents,
			Quantity:       item.Quantity,
			LineTotalCents: item.LineTotalCents,
		})
	}
	return result
}

func nonNil(values []string) []string {
	if values == nil {
		return []string{}
	}
	return values
}

func fmtTime(t time.Time) string {
	return t.UTC().Format(time.RFC3339)
}
