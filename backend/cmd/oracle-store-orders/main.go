// Command oracle-store-orders answers newline-delimited JSON requests with the
// Go store implementation, so the Rust port can be diff-tested against it.
// Commands run against the libSQL database in AZAHAR_TEST_DATABASE_URL
// (default http://127.0.0.1:8880).
package main

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	"azahar/internal/db"
	"azahar/internal/store"
)

type request struct {
	Op string `json:"op"`

	Year      int          `json:"year"`
	Order     *orderInput  `json:"order"`
	Items     []itemInput  `json:"items"`
	TokenHash string       `json:"token_hash"`
	ID        string       `json:"id"`
	OrderID   string       `json:"order_id"`
	IDs       []string     `json:"ids"`
	UserID    string       `json:"user_id"`
	Filter    *filterInput `json:"filter"`

	From         string `json:"from"`
	To           string `json:"to"`
	PreviousFrom string `json:"previous_from"`
	PreviousTo   string `json:"previous_to"`

	FromStatus     string               `json:"from_status"`
	ToStatus       string               `json:"to_status"`
	CourierKey     string               `json:"courier_key"`
	TrackingNumber string               `json:"tracking_number"`
	Status         string               `json:"status"`
	Events         []store.CourierEvent `json:"events"`
	SyncedAt       string               `json:"synced_at"`
	Now            string               `json:"now"`

	Image *imageInput `json:"image"`
	Limit int         `json:"limit"`
}

type filterInput struct {
	Status string `json:"status"`
	Query  string `json:"query"`
	Limit  int    `json:"limit"`
}

type orderInput struct {
	ID             string               `json:"id"`
	Number         string               `json:"number"`
	UserID         *string              `json:"user_id"`
	Name           string               `json:"name"`
	Email          string               `json:"email"`
	Phone          string               `json:"phone"`
	Address        string               `json:"address"`
	City           string               `json:"city"`
	Notes          string               `json:"notes"`
	SubtotalCents  int64                `json:"subtotal_cents"`
	ShippingCents  int64                `json:"shipping_cents"`
	TotalCents     int64                `json:"total_cents"`
	Currency       string               `json:"currency"`
	Status         string               `json:"status"`
	TokenHash      string               `json:"token_hash"`
	CreatedAt      string               `json:"created_at"`
	UpdatedAt      string               `json:"updated_at"`
	WilayaCode     int                  `json:"wilaya_code"`
	WilayaName     string               `json:"wilaya_name"`
	CommuneName    string               `json:"commune_name"`
	DeliveryType   string               `json:"delivery_type"`
	StopDeskID     string               `json:"stop_desk_id"`
	StopDeskName   string               `json:"stop_desk_name"`
	CourierKey     string               `json:"courier_key"`
	TrackingNumber string               `json:"tracking_number"`
	CourierStatus  string               `json:"courier_status"`
	CourierEvents  []store.CourierEvent `json:"courier_events"`
	CourierSynced  string               `json:"courier_synced_at"`
}

type itemInput struct {
	ID             string            `json:"id"`
	OrderID        string            `json:"order_id"`
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

type imageInput struct {
	ID        string `json:"id"`
	MIME      string `json:"mime"`
	BytesB64  string `json:"bytes_base64"`
	CreatedBy string `json:"created_by"`
	CreatedAt string `json:"created_at"`
}

type orderDTO struct {
	ID             string               `json:"id"`
	Number         string               `json:"number"`
	UserID         *string              `json:"user_id"`
	Name           string               `json:"name"`
	Email          string               `json:"email"`
	Phone          string               `json:"phone"`
	Address        string               `json:"address"`
	City           string               `json:"city"`
	Notes          string               `json:"notes"`
	SubtotalCents  int64                `json:"subtotal_cents"`
	ShippingCents  int64                `json:"shipping_cents"`
	TotalCents     int64                `json:"total_cents"`
	Currency       string               `json:"currency"`
	Status         string               `json:"status"`
	TokenHash      string               `json:"token_hash"`
	CreatedAt      string               `json:"created_at"`
	UpdatedAt      string               `json:"updated_at"`
	WilayaCode     int                  `json:"wilaya_code"`
	WilayaName     string               `json:"wilaya_name"`
	CommuneName    string               `json:"commune_name"`
	DeliveryType   string               `json:"delivery_type"`
	StopDeskID     string               `json:"stop_desk_id"`
	StopDeskName   string               `json:"stop_desk_name"`
	CourierKey     string               `json:"courier_key"`
	TrackingNumber string               `json:"tracking_number"`
	CourierStatus  string               `json:"courier_status"`
	CourierEvents  []store.CourierEvent `json:"courier_events"`
	CourierSynced  string               `json:"courier_synced_at"`
}

type itemDTO struct {
	ID             string            `json:"id"`
	OrderID        string            `json:"order_id"`
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

type lowStockDTO struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Stock int    `json:"stock"`
	Image string `json:"image"`
}

type revenuePointDTO struct {
	Date         string `json:"date"`
	RevenueCents int64  `json:"revenue_cents"`
	Orders       int    `json:"orders"`
}

type topProductDTO struct {
	ProductID    string `json:"product_id"`
	Name         string `json:"name"`
	Units        int    `json:"units"`
	RevenueCents int64  `json:"revenue_cents"`
}

type recentOrderDTO struct {
	ID         string `json:"id"`
	Number     string `json:"number"`
	Name       string `json:"name"`
	TotalCents int64  `json:"total_cents"`
	Status     string `json:"status"`
	CreatedAt  string `json:"created_at"`
}

type metricsDTO struct {
	Demo                 bool              `json:"demo"`
	From                 string            `json:"from"`
	To                   string            `json:"to"`
	RevenueCents         int64             `json:"revenue_cents"`
	PreviousRevenueCents int64             `json:"previous_revenue_cents"`
	OrdersCount          int               `json:"orders_count"`
	PreviousOrdersCount  int               `json:"previous_orders_count"`
	UnitsSold            int               `json:"units_sold"`
	NewCustomers         int               `json:"new_customers"`
	LowStock             []lowStockDTO     `json:"low_stock"`
	RevenueSeries        []revenuePointDTO `json:"revenue_series"`
	StatusSplit          map[string]int    `json:"status_split"`
	TopProducts          []topProductDTO   `json:"top_products"`
	RecentOrders         []recentOrderDTO  `json:"recent_orders"`
}

type imageDTO struct {
	ID        string `json:"id"`
	MIME      string `json:"mime"`
	BytesB64  string `json:"bytes_base64"`
	CreatedAt string `json:"created_at"`
}

func main() {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		dsn = "http://127.0.0.1:8880"
	}

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	handle, err := db.Open(ctx, dsn)
	if err != nil {
		fmt.Fprintf(os.Stderr, "connect to %s: %v\n", dsn, err)
		os.Exit(1)
	}
	defer handle.Close()
	if err := db.Migrate(ctx, handle); err != nil {
		fmt.Fprintf(os.Stderr, "migrate %s: %v\n", dsn, err)
		os.Exit(1)
	}
	repo := store.New(handle)

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 8*1024*1024)
	encoder := json.NewEncoder(os.Stdout)

	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var req request
		if err := json.Unmarshal(line, &req); err != nil {
			if err := encoder.Encode(map[string]any{"ok": false, "error": "invalid JSON: " + err.Error()}); err != nil {
				fmt.Fprintln(os.Stderr, err)
				os.Exit(1)
			}
			continue
		}
		if err := encoder.Encode(handleRequest(repo, req)); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func handleRequest(repo *store.Store, req request) map[string]any {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	switch req.Op {
	case "next_order_number":
		number, err := repo.NextOrderNumber(ctx, repo.DB(), req.Year)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "number": number}

	case "create_order":
		if req.Order == nil {
			return map[string]any{"ok": false, "error": "create_order needs an order"}
		}
		order, err := req.Order.toStore()
		if err != nil {
			return errorResponse(err)
		}
		items := make([]store.OrderItem, 0, len(req.Items))
		for _, input := range req.Items {
			items = append(items, input.toStore())
		}
		if err := repo.CreateOrder(ctx, repo.DB(), order, items); err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true}

	case "order_by_token":
		order, err := repo.OrderByTokenHash(ctx, req.TokenHash)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "order": toOrderDTO(order)}

	case "order_by_id":
		order, err := repo.OrderByID(ctx, req.ID)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "order": toOrderDTO(order)}

	case "order_items":
		items, err := repo.OrderItems(ctx, req.OrderID)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "items": toItemDTOs(items)}

	case "items_by_order_ids":
		grouped, err := repo.OrderItemsByOrderIDs(ctx, req.IDs)
		if err != nil {
			return errorResponse(err)
		}
		result := make(map[string][]itemDTO, len(grouped))
		for orderID, items := range grouped {
			result[orderID] = toItemDTOs(items)
		}
		return map[string]any{"ok": true, "items": result}

	case "orders_by_user":
		orders, err := repo.OrdersByUser(ctx, req.UserID)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "orders": toOrderDTOs(orders)}

	case "admin_orders":
		filter := store.AdminOrderFilter{}
		if req.Filter != nil {
			filter = store.AdminOrderFilter{
				Status: req.Filter.Status,
				Query:  req.Filter.Query,
				Limit:  req.Filter.Limit,
			}
		}
		orders, err := repo.AdminOrders(ctx, filter)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "orders": toOrderDTOs(orders)}

	case "update_status":
		now, err := parseTimestamp(req.Now)
		if err != nil {
			return errorResponse(err)
		}
		if err := repo.UpdateOrderStatus(ctx, repo.DB(), req.ID, req.FromStatus, req.ToStatus, now); err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true}

	case "update_shipment":
		syncedAt, err := parseTimestamp(req.SyncedAt)
		if err != nil {
			return errorResponse(err)
		}
		now, err := parseTimestamp(req.Now)
		if err != nil {
			return errorResponse(err)
		}
		if err := repo.UpdateShipment(ctx, repo.DB(), req.ID, req.CourierKey, req.TrackingNumber,
			req.Status, req.Events, syncedAt, now); err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true}

	case "update_tracking":
		syncedAt, err := parseTimestamp(req.SyncedAt)
		if err != nil {
			return errorResponse(err)
		}
		now, err := parseTimestamp(req.Now)
		if err != nil {
			return errorResponse(err)
		}
		if err := repo.UpdateTracking(ctx, repo.DB(), req.ID, req.Status, req.Events, syncedAt, now); err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true}

	case "metrics":
		from, err := parseTimestamp(req.From)
		if err != nil {
			return errorResponse(err)
		}
		to, err := parseTimestamp(req.To)
		if err != nil {
			return errorResponse(err)
		}
		previousFrom, err := parseTimestamp(req.PreviousFrom)
		if err != nil {
			return errorResponse(err)
		}
		previousTo, err := parseTimestamp(req.PreviousTo)
		if err != nil {
			return errorResponse(err)
		}
		metrics, err := repo.Metrics(ctx, from, to, previousFrom, previousTo)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "metrics": toMetricsDTO(metrics)}

	case "create_image":
		if req.Image == nil {
			return map[string]any{"ok": false, "error": "create_image needs an image"}
		}
		body, err := base64.StdEncoding.DecodeString(req.Image.BytesB64)
		if err != nil {
			return errorResponse(err)
		}
		createdAt, err := parseTimestamp(req.Image.CreatedAt)
		if err != nil {
			return errorResponse(err)
		}
		if err := repo.CreateProductImage(ctx, repo.DB(), store.ProductImage{
			ID:        req.Image.ID,
			MIME:      req.Image.MIME,
			Bytes:     body,
			CreatedBy: req.Image.CreatedBy,
			CreatedAt: createdAt,
		}); err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true}

	case "product_image":
		image, err := repo.ProductImage(ctx, req.ID)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "image": imageDTO{
			ID:        image.ID,
			MIME:      image.MIME,
			BytesB64:  base64.StdEncoding.EncodeToString(image.Bytes),
			CreatedAt: store.FormatOptionalTime(image.CreatedAt),
		}}

	case "flush_audit":
		flushed, err := repo.FlushAuditBatch(ctx, req.Limit)
		if err != nil {
			return errorResponse(err)
		}
		return map[string]any{"ok": true, "count": flushed}
	}

	return map[string]any{"ok": false, "error": fmt.Sprintf("unknown op %q", req.Op)}
}

func (input orderInput) toStore() (store.Order, error) {
	createdAt, err := parseTimestamp(input.CreatedAt)
	if err != nil {
		return store.Order{}, err
	}
	updatedAt, err := parseTimestamp(input.UpdatedAt)
	if err != nil {
		return store.Order{}, err
	}
	syncedAt, err := parseTimestamp(input.CourierSynced)
	if err != nil {
		return store.Order{}, err
	}
	return store.Order{
		ID:              input.ID,
		Number:          input.Number,
		UserID:          input.UserID,
		Name:            input.Name,
		Email:           input.Email,
		Phone:           input.Phone,
		Address:         input.Address,
		City:            input.City,
		Notes:           input.Notes,
		SubtotalCents:   input.SubtotalCents,
		ShippingCents:   input.ShippingCents,
		TotalCents:      input.TotalCents,
		Currency:        input.Currency,
		Status:          input.Status,
		TokenHash:       input.TokenHash,
		CreatedAt:       createdAt,
		UpdatedAt:       updatedAt,
		WilayaCode:      input.WilayaCode,
		WilayaName:      input.WilayaName,
		CommuneName:     input.CommuneName,
		DeliveryType:    input.DeliveryType,
		StopDeskID:      input.StopDeskID,
		StopDeskName:    input.StopDeskName,
		CourierKey:      input.CourierKey,
		TrackingNumber:  input.TrackingNumber,
		CourierStatus:   input.CourierStatus,
		CourierEvents:   input.CourierEvents,
		CourierSyncedAt: syncedAt,
	}, nil
}

func (input itemInput) toStore() store.OrderItem {
	return store.OrderItem{
		ID:             input.ID,
		OrderID:        input.OrderID,
		ProductID:      input.ProductID,
		Slug:           input.Slug,
		Name:           input.Name,
		Image:          input.Image,
		UnitPriceCents: input.UnitPriceCents,
		Quantity:       input.Quantity,
		LineTotalCents: input.LineTotalCents,
		Options:        input.Options,
		OptionsLabel:   input.OptionsLabel,
		PluginID:       input.PluginID,
	}
}

func toOrderDTO(order store.Order) orderDTO {
	events := order.CourierEvents
	if events == nil {
		events = []store.CourierEvent{}
	}
	return orderDTO{
		ID:             order.ID,
		Number:         order.Number,
		UserID:         order.UserID,
		Name:           order.Name,
		Email:          order.Email,
		Phone:          order.Phone,
		Address:        order.Address,
		City:           order.City,
		Notes:          order.Notes,
		SubtotalCents:  order.SubtotalCents,
		ShippingCents:  order.ShippingCents,
		TotalCents:     order.TotalCents,
		Currency:       order.Currency,
		Status:         order.Status,
		TokenHash:      order.TokenHash,
		CreatedAt:      store.FormatOptionalTime(order.CreatedAt),
		UpdatedAt:      store.FormatOptionalTime(order.UpdatedAt),
		WilayaCode:     order.WilayaCode,
		WilayaName:     order.WilayaName,
		CommuneName:    order.CommuneName,
		DeliveryType:   order.DeliveryType,
		StopDeskID:     order.StopDeskID,
		StopDeskName:   order.StopDeskName,
		CourierKey:     order.CourierKey,
		TrackingNumber: order.TrackingNumber,
		CourierStatus:  order.CourierStatus,
		CourierEvents:  events,
		CourierSynced:  store.FormatOptionalTime(order.CourierSyncedAt),
	}
}

func toOrderDTOs(orders []store.Order) []orderDTO {
	result := make([]orderDTO, 0, len(orders))
	for _, order := range orders {
		result = append(result, toOrderDTO(order))
	}
	return result
}

func toItemDTOs(items []store.OrderItem) []itemDTO {
	result := make([]itemDTO, 0, len(items))
	for _, item := range items {
		result = append(result, itemDTO{
			ID:             item.ID,
			OrderID:        item.OrderID,
			ProductID:      item.ProductID,
			Slug:           item.Slug,
			Name:           item.Name,
			Image:          item.Image,
			UnitPriceCents: item.UnitPriceCents,
			Quantity:       item.Quantity,
			LineTotalCents: item.LineTotalCents,
			Options:        item.Options,
			OptionsLabel:   item.OptionsLabel,
			PluginID:       item.PluginID,
		})
	}
	return result
}

func toMetricsDTO(metrics store.Metrics) metricsDTO {
	result := metricsDTO{
		Demo:                 metrics.Demo,
		From:                 store.FormatOptionalTime(metrics.From),
		To:                   store.FormatOptionalTime(metrics.To),
		RevenueCents:         metrics.RevenueCents,
		PreviousRevenueCents: metrics.PreviousRevenueCents,
		OrdersCount:          metrics.OrdersCount,
		PreviousOrdersCount:  metrics.PreviousOrdersCount,
		UnitsSold:            metrics.UnitsSold,
		NewCustomers:         metrics.NewCustomers,
		LowStock:             make([]lowStockDTO, 0, len(metrics.LowStock)),
		RevenueSeries:        make([]revenuePointDTO, 0, len(metrics.RevenueSeries)),
		StatusSplit:          make(map[string]int, len(metrics.StatusSplit)),
		TopProducts:          make([]topProductDTO, 0, len(metrics.TopProducts)),
		RecentOrders:         make([]recentOrderDTO, 0, len(metrics.RecentOrders)),
	}
	for _, item := range metrics.LowStock {
		result.LowStock = append(result.LowStock, lowStockDTO{
			ID: item.ID, Name: item.Name, Stock: item.Stock, Image: item.Image,
		})
	}
	for _, point := range metrics.RevenueSeries {
		result.RevenueSeries = append(result.RevenueSeries, revenuePointDTO{
			Date: point.Date, RevenueCents: point.RevenueCents, Orders: point.Orders,
		})
	}
	for status, count := range metrics.StatusSplit {
		result.StatusSplit[status] = count
	}
	for _, product := range metrics.TopProducts {
		result.TopProducts = append(result.TopProducts, topProductDTO{
			ProductID:    product.ProductID,
			Name:         product.Name,
			Units:        product.Units,
			RevenueCents: product.RevenueCents,
		})
	}
	for _, order := range metrics.RecentOrders {
		result.RecentOrders = append(result.RecentOrders, recentOrderDTO{
			ID:         order.ID,
			Number:     order.Number,
			Name:       order.Name,
			TotalCents: order.TotalCents,
			Status:     order.Status,
			CreatedAt:  store.FormatOptionalTime(order.CreatedAt),
		})
	}
	return result
}

func parseTimestamp(value string) (time.Time, error) {
	if value == "" {
		return time.Time{}, nil
	}
	parsed, err := time.Parse(time.RFC3339Nano, value)
	if err != nil {
		return time.Time{}, fmt.Errorf("parse timestamp %q: %w", value, err)
	}
	return parsed.UTC(), nil
}

func errorResponse(err error) map[string]any {
	if errors.Is(err, sql.ErrNoRows) {
		return map[string]any{"ok": false, "not_found": true}
	}
	return map[string]any{"ok": false, "error": err.Error()}
}
