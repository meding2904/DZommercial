package httpapi

import (
	"context"
	"time"

	"azahar/internal/plugins"
	"azahar/internal/store"
)

type categoryDTO struct {
	Slug        string `json:"slug"`
	Name        string `json:"name"`
	Description string `json:"description"`
	PluginID    string `json:"plugin_id"`
}

type categoryRefDTO struct {
	Slug     string `json:"slug"`
	Name     string `json:"name"`
	PluginID string `json:"plugin_id"`
}

type productDTO struct {
	ID             string                `json:"id"`
	Slug           string                `json:"slug"`
	Name           string                `json:"name"`
	Subtitle       string                `json:"subtitle"`
	Description    string                `json:"description"`
	PriceCents     int64                 `json:"price_cents"`
	CompareAtCents *int64                `json:"compare_at_cents"`
	SalePriceCents *int64                `json:"sale_price_cents"`
	SaleStartsAt   string                `json:"sale_starts_at"`
	SaleEndsAt     string                `json:"sale_ends_at"`
	EffectivePrice int64                 `json:"effective_price_cents"`
	OnSale         bool                  `json:"on_sale"`
	Currency       string                `json:"currency"`
	Size           string                `json:"size"`
	Category       categoryRefDTO        `json:"category"`
	Concerns       []string              `json:"concerns"`
	Ingredients    []string              `json:"ingredients"`
	HowTo          string                `json:"how_to"`
	Image          string                `json:"image"`
	Gallery        []string              `json:"gallery"`
	Stock          int                   `json:"stock"`
	Featured       bool                  `json:"featured"`
	Active         bool                  `json:"active"`
	PluginID       string                `json:"plugin_id"`
	PluginName     string                `json:"plugin_name"`
	PluginSummary  string                `json:"plugin_summary"`
	OptionSchema   []plugins.OptionField `json:"option_schema"`
	Attributes     map[string][]string   `json:"attributes"`
	PluginData     map[string]any        `json:"plugin_data,omitempty"`
	CreatedAt      string                `json:"created_at,omitempty"`
	UpdatedAt      string                `json:"updated_at,omitempty"`
}

func productDTOFromStore(product store.Product, withTimestamps bool) productDTO {
	now := time.Now().UTC()
	dto := productDTO{
		ID:             product.ID,
		Slug:           product.Slug,
		Name:           product.Name,
		Subtitle:       product.Subtitle,
		Description:    product.Description,
		PriceCents:     product.PriceCents,
		CompareAtCents: product.CompareAtCents,
		SalePriceCents: product.SalePriceCents,
		SaleStartsAt:   store.FormatOptionalTime(product.SaleStartsAt),
		SaleEndsAt:     store.FormatOptionalTime(product.SaleEndsAt),
		EffectivePrice: product.EffectivePriceCents(now),
		OnSale:         product.SaleActive(now),
		Currency:       product.Currency,
		Size:           product.Size,
		Category:       categoryRefDTO{Slug: product.Category.Slug, Name: product.Category.Name, PluginID: product.Category.PluginID},
		Concerns:       nonNil(product.Concerns),
		Ingredients:    nonNil(product.Ingredients),
		HowTo:          product.HowTo,
		Image:          product.Image,
		Gallery:        nonNil(product.Gallery),
		Stock:          product.Stock,
		Featured:       product.Featured,
		Active:         product.Active,
		PluginID:       store.NormalizePluginID(product.PluginID),
		OptionSchema:   []plugins.OptionField{},
		Attributes:     nonNilStringListMap(product.Attributes),
	}
	if withTimestamps {
		dto.CreatedAt = store.FormatTime(product.CreatedAt)
		dto.UpdatedAt = store.FormatTime(product.UpdatedAt)
	}
	return dto
}

func (s *Server) toProduct(ctx context.Context, product store.Product, withTimestamps bool) productDTO {
	dto := s.decorateProduct(product, productDTOFromStore(product, withTimestamps))
	if dto.PluginID != store.CorePluginID {
		dto.PluginData = s.plugins.EnrichProduct(ctx, product)
	}
	return dto
}

func (s *Server) toProducts(ctx context.Context, products []store.Product, withTimestamps bool) []productDTO {
	result := make([]productDTO, 0, len(products))
	for _, product := range products {
		// List views do not need per-product plugin counts; the detail view does.
		result = append(result, s.decorateProduct(product, productDTOFromStore(product, withTimestamps)))
	}
	return result
}

func (s *Server) decorateProduct(product store.Product, dto productDTO) productDTO {
	dto.OptionSchema = s.optionSchema(product)
	dto.PluginName = "Core"
	if plugin, ok := s.plugins.Get(dto.PluginID); ok {
		manifest := plugin.Manifest()
		dto.PluginName = manifest.Name
		if manifest.ProductType != nil {
			dto.PluginSummary = manifest.ProductType.Summary
		}
	}
	return dto
}

func nonNilStringListMap(values map[string][]string) map[string][]string {
	if values == nil {
		return map[string][]string{}
	}
	return values
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
	ID             string            `json:"id"`
	ProductID      string            `json:"product_id"`
	Slug           string            `json:"slug"`
	Name           string            `json:"name"`
	Image          string            `json:"image"`
	UnitPriceCents int64             `json:"unit_price_cents"`
	Quantity       int               `json:"quantity"`
	LineTotalCents int64             `json:"line_total_cents"`
	Options        map[string]string `json:"options"`
	OptionsLabel   string            `json:"options_label"`
	PluginID       string            `json:"plugin_id"`
}

type orderCustomerDTO struct {
	ID    string `json:"id,omitempty"`
	Name  string `json:"name"`
	Email string `json:"email"`
	Phone string `json:"phone"`
}

type orderDTO struct {
	ID            string               `json:"id"`
	Number        string               `json:"number"`
	Token         string               `json:"token,omitempty"`
	Status        string               `json:"status"`
	Name          string               `json:"name"`
	Email         string               `json:"email"`
	Phone         string               `json:"phone"`
	Address       string               `json:"address,omitempty"`
	City          string               `json:"city"`
	Notes         string               `json:"notes"`
	WilayaCode    int                  `json:"wilaya_code,omitempty"`
	WilayaName    string               `json:"wilaya_name,omitempty"`
	CommuneName   string               `json:"commune_name,omitempty"`
	DeliveryType  string               `json:"delivery_type"`
	StopDeskID    string               `json:"stop_desk_id,omitempty"`
	StopDeskName  string               `json:"stop_desk_name,omitempty"`
	CourierKey    string               `json:"courier_key,omitempty"`
	Tracking      string               `json:"tracking_number,omitempty"`
	CourierStatus string               `json:"courier_status,omitempty"`
	CourierEvents []store.CourierEvent `json:"courier_events,omitempty"`
	CourierSynced string               `json:"courier_synced_at,omitempty"`
	SubtotalCents int64                `json:"subtotal_cents"`
	ShippingCents int64                `json:"shipping_cents"`
	TotalCents    int64                `json:"total_cents"`
	Currency      string               `json:"currency"`
	Items         []orderItemDTO       `json:"items"`
	Customer      orderCustomerDTO     `json:"customer"`
	CreatedAt     string               `json:"created_at"`
	UpdatedAt     string               `json:"updated_at"`
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
		WilayaCode:    order.WilayaCode,
		WilayaName:    order.WilayaName,
		CommuneName:   order.CommuneName,
		DeliveryType:  order.DeliveryType,
		StopDeskID:    order.StopDeskID,
		StopDeskName:  order.StopDeskName,
		CourierKey:    order.CourierKey,
		Tracking:      order.TrackingNumber,
		CourierStatus: order.CourierStatus,
		CourierEvents: order.CourierEvents,
		CourierSynced: store.FormatOptionalTime(order.CourierSyncedAt),
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
			Options:        nonNilStringMap(item.Options),
			OptionsLabel:   item.OptionsLabel,
			PluginID:       store.NormalizePluginID(item.PluginID),
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

func nonNilStringMap(values map[string]string) map[string]string {
	if values == nil {
		return map[string]string{}
	}
	return values
}

func fmtTime(t time.Time) string {
	return t.UTC().Format(time.RFC3339)
}
