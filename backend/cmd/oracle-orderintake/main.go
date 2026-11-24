// Command oracle-orderintake is the differential oracle for the Rust order
// intake port.
//
// It reads one JSON command per line and writes one JSON result per line,
// operating against AZAHAR_TEST_DATABASE_URL (default http://127.0.0.1:8880).
// The test owns every `ito_*` fixture row and deletes it when done.
//
// The fixture registry installs a product type only. It implements no checkout
// or order-status hooks, so both sides run the core path with an empty provider
// (NoopHooks on the Rust side). Hooks have their own unit tests in the Rust
// crate; the real Go application would have phone/AI hooks here.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	"azahar/internal/db"
	"azahar/internal/orderintake"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

const defaultDSN = "http://127.0.0.1:8880"

type wireLine struct {
	ProductID          string            `json:"product_id"`
	Quantity           int               `json:"quantity"`
	Options            map[string]string `json:"options"`
	ExpectedPriceCents *int64            `json:"expected_price_cents"`
}

type wireCustomer struct {
	UserID  string `json:"user_id"`
	Name    string `json:"name"`
	Email   string `json:"email"`
	Phone   string `json:"phone"`
	Address string `json:"address"`
	Notes   string `json:"notes"`
}

type wireDestination struct {
	WilayaCode   int    `json:"wilaya_code"`
	WilayaName   string `json:"wilaya_name"`
	CommuneName  string `json:"commune_name"`
	DeliveryType string `json:"delivery_type"`
	StopDeskID   string `json:"stop_desk_id"`
	StopDeskName string `json:"stop_desk_name"`
}

type request struct {
	Op            string          `json:"op"`
	Channel       string          `json:"channel"`
	TermsVersion  string          `json:"terms_version"`
	Lines         []wireLine      `json:"lines"`
	Customer      wireCustomer    `json:"customer"`
	Destination   wireDestination `json:"destination"`
	Status        string          `json:"status"`
	TokenHash     string          `json:"token_hash"`
	ActorUserID   string          `json:"actor_user_id"`
	OrderID       string          `json:"order_id"`
	To            string          `json:"to"`
	ActorRole     string          `json:"actor_role"`
	PluginID      string          `json:"plugin_id"`
	PluginEnabled *bool           `json:"plugin_enabled"`
	// Options exercises the pure option helpers without a stored product.
	ProductPluginID string              `json:"product_plugin_id"`
	Attributes      map[string][]string `json:"attributes"`
	Options         map[string]string   `json:"options"`
}

type wireOrder struct {
	ID            string  `json:"id"`
	Number        string  `json:"number"`
	UserID        *string `json:"user_id"`
	Name          string  `json:"name"`
	Email         string  `json:"email"`
	Phone         string  `json:"phone"`
	Address       string  `json:"address"`
	City          string  `json:"city"`
	Notes         string  `json:"notes"`
	SubtotalCents int64   `json:"subtotal_cents"`
	ShippingCents int64   `json:"shipping_cents"`
	TotalCents    int64   `json:"total_cents"`
	Currency      string  `json:"currency"`
	Status        string  `json:"status"`
	TokenHash     string  `json:"token_hash"`
	CreatedAt     string  `json:"created_at"`
	UpdatedAt     string  `json:"updated_at"`
	WilayaCode    int     `json:"wilaya_code"`
	WilayaName    string  `json:"wilaya_name"`
	CommuneName   string  `json:"commune_name"`
	DeliveryType  string  `json:"delivery_type"`
	StopDeskID    string  `json:"stop_desk_id"`
	StopDeskName  string  `json:"stop_desk_name"`
}

type wireItem struct {
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

func wireOrderFrom(order store.Order) wireOrder {
	return wireOrder{
		ID:            order.ID,
		Number:        order.Number,
		UserID:        order.UserID,
		Name:          order.Name,
		Email:         order.Email,
		Phone:         order.Phone,
		Address:       order.Address,
		City:          order.City,
		Notes:         order.Notes,
		SubtotalCents: order.SubtotalCents,
		ShippingCents: order.ShippingCents,
		TotalCents:    order.TotalCents,
		Currency:      order.Currency,
		Status:        order.Status,
		TokenHash:     order.TokenHash,
		CreatedAt:     store.FormatTime(order.CreatedAt),
		UpdatedAt:     store.FormatTime(order.UpdatedAt),
		WilayaCode:    order.WilayaCode,
		WilayaName:    order.WilayaName,
		CommuneName:   order.CommuneName,
		DeliveryType:  order.DeliveryType,
		StopDeskID:    order.StopDeskID,
		StopDeskName:  order.StopDeskName,
	}
}

func wireItemsFrom(items []store.OrderItem) []wireItem {
	out := make([]wireItem, 0, len(items))
	for _, item := range items {
		out = append(out, wireItem{
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
	return out
}

func orderResult(order store.Order, items []store.OrderItem) map[string]any {
	return map[string]any{
		"ok":    true,
		"order": wireOrderFrom(order),
		"items": wireItemsFrom(items),
	}
}

func problemResult(problem *orderintake.Problem) map[string]any {
	return map[string]any{
		"ok": false,
		"problem": map[string]any{
			"status":  problem.Status,
			"code":    problem.Code,
			"message": problem.Message,
			"fields":  problem.Fields,
		},
	}
}

func errorResult(err error) map[string]any {
	return map[string]any{"ok": false, "error": err.Error()}
}

// fixturePlugin offers the product type the Rust fixture registry mirrors. It
// deliberately implements no hook interfaces.
type fixturePlugin struct{}

func (fixturePlugin) Manifest() plugins.Manifest {
	return plugins.Manifest{
		ID:      "itoopt",
		Name:    "Intake fixture",
		Version: "1.0.0",
		ProductType: &plugins.ProductType{
			Label: "Fixture product",
			Fields: []plugins.OptionField{
				{Key: "scent", Label: "Scent", Type: "select", Required: true},
				{Key: "finish", Label: "Finish", Type: "select"},
				{Key: "engraving", Label: "Engraving", Type: "text"},
			},
		},
	}
}

func (fixturePlugin) Routes() []plugins.Route { return nil }

func main() {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		dsn = defaultDSN
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	handle, err := db.Open(ctx, dsn)
	if err != nil {
		fmt.Fprintf(os.Stderr, "oracle-orderintake: cannot open AZAHAR_TEST_DATABASE_URL=%s: %v\nrun `make dev-db` or set AZAHAR_TEST_DATABASE_URL\n", dsn, err)
		os.Exit(1)
	}
	defer handle.Close()
	if err := db.Migrate(ctx, handle); err != nil {
		fmt.Fprintf(os.Stderr, "oracle-orderintake: migrate %s: %v\n", dsn, err)
		os.Exit(1)
	}

	repo := store.New(handle)
	registry := plugins.NewRegistry()
	if err := registry.Register(fixturePlugin{}); err != nil {
		fmt.Fprintf(os.Stderr, "oracle-orderintake: register fixture plugin: %v\n", err)
		os.Exit(1)
	}
	intake := orderintake.New(repo, registry)

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 4<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var req request
		if err := json.Unmarshal(scanner.Bytes(), &req); err != nil {
			_ = encoder.Encode(errorResult(err))
			continue
		}
		switch req.Op {
		case "place":
			_ = encoder.Encode(place(ctx, intake, req))
		case "status":
			_ = encoder.Encode(changeStatus(ctx, intake, req))
		case "options":
			_ = encoder.Encode(optionsResult(registry, req))
		case "plugin_state":
			if req.PluginEnabled == nil {
				_ = encoder.Encode(errorResult(errors.New("plugin_state needs plugin_enabled")))
				continue
			}
			_ = encoder.Encode(map[string]any{
				"ok":      true,
				"changed": registry.SetEnabled(req.PluginID, *req.PluginEnabled),
			})
		default:
			_ = encoder.Encode(errorResult(fmt.Errorf("unknown op %q", req.Op)))
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func place(ctx context.Context, intake *orderintake.Intake, req request) map[string]any {
	var channel orderintake.Channel
	switch req.Channel {
	case "checkout":
		channel = orderintake.CheckoutChannel(req.TermsVersion)
	case "counter":
		channel = orderintake.CounterChannel()
	default:
		return errorResult(fmt.Errorf("unknown channel %q", req.Channel))
	}
	lines := make([]orderintake.Line, 0, len(req.Lines))
	for _, line := range req.Lines {
		lines = append(lines, orderintake.Line{
			ProductID:          line.ProductID,
			Quantity:           line.Quantity,
			Options:            line.Options,
			ExpectedPriceCents: line.ExpectedPriceCents,
		})
	}
	result, err := intake.Place(ctx, orderintake.Request{
		Lines:   lines,
		Channel: channel,
		Customer: orderintake.Customer{
			UserID:  req.Customer.UserID,
			Name:    req.Customer.Name,
			Email:   req.Customer.Email,
			Phone:   req.Customer.Phone,
			Address: req.Customer.Address,
			Notes:   req.Customer.Notes,
		},
		Destination: orderintake.Destination{
			WilayaCode:   req.Destination.WilayaCode,
			WilayaName:   req.Destination.WilayaName,
			CommuneName:  req.Destination.CommuneName,
			DeliveryType: req.Destination.DeliveryType,
			StopDeskID:   req.Destination.StopDeskID,
			StopDeskName: req.Destination.StopDeskName,
		},
		Status:      req.Status,
		TokenHash:   req.TokenHash,
		ActorUserID: req.ActorUserID,
	})
	if err != nil {
		var refusal *orderintake.Problem
		if errors.As(err, &refusal) {
			return problemResult(refusal)
		}
		return errorResult(err)
	}
	return orderResult(result.Order, result.Items)
}

func changeStatus(ctx context.Context, intake *orderintake.Intake, req request) map[string]any {
	changed, err := intake.ChangeStatus(ctx, orderintake.Change{
		OrderID:     req.OrderID,
		To:          req.To,
		ActorUserID: req.ActorUserID,
		ActorRole:   req.ActorRole,
	})
	if err != nil {
		var refusal *orderintake.Problem
		if errors.As(err, &refusal) {
			return problemResult(refusal)
		}
		return errorResult(err)
	}
	return orderResult(changed.Order, changed.Items)
}
