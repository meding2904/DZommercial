// Package orderintake owns the one path that turns basket lines into a stored
// order and the one path that moves an order's status. The web checkout, the
// counter sale and the admin status change all call it instead of carrying
// their own copy of the product, option, price, stock, plugin-hook and audit
// steps. Every write runs inside one Store.InTx transaction, so plugin hooks
// receive the caller's *sql.Tx and the audit row joins the same commit.
package orderintake

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"azahar/internal/ids"
	"azahar/internal/orderstatus"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

const (
	statusDelivered = "delivered"
	statusCancelled = "cancelled"
)

// Intake runs the order writes.
type Intake struct {
	store   *store.Store
	plugins *plugins.Registry
}

// New returns an intake that writes through st and asks registry about the
// plugins that own products and order hooks.
func New(st *store.Store, registry *plugins.Registry) *Intake {
	return &Intake{store: st, plugins: registry}
}

// Request is one order to store. The caller has already validated its own
// payload fields and line count; BuildLines has trimmed and merged the lines.
type Request struct {
	Lines       []Line
	Channel     Channel
	Customer    Customer
	Destination Destination
	// Status is the status the row is created with.
	Status string
	// TokenHash is the hash of the order token the storefront shows.
	TokenHash string
	// ActorUserID is the account or employee behind the order.
	ActorUserID string
}

// Customer is the person on the order.
type Customer struct {
	UserID  string
	Name    string
	Email   string
	Phone   string
	Address string
	Notes   string
}

// Destination is where a delivered order goes. A counter sale leaves it zero
// apart from DeliveryType.
type Destination struct {
	WilayaCode   int
	WilayaName   string
	CommuneName  string
	DeliveryType string
	StopDeskID   string
	StopDeskName string
}

// Channel is what one way of selling changes about the shared order path.
type Channel struct {
	// Basket is the noun refusals use for the lines: "bag" for the web
	// checkout, "sale" for the counter.
	Basket string
	// Shipping returns the delivery fee for a subtotal. Nil is free delivery.
	Shipping func(subtotalCents int64) int64
	// Delivered records a paid sale as delivered in the same transaction.
	Delivered bool
	// Audit describes the audit row written with the order.
	Audit Audit
}

// Audit is the audit row one channel writes with an order.
type Audit struct {
	Action string
	Detail func(order store.Order, itemCount int, now time.Time) map[string]any
}

// Result is a stored order and its lines, ready for a response body.
type Result struct {
	Order store.Order
	Items []store.OrderItem
}

// CheckoutChannel is the storefront checkout: the bag, the standard shipping
// rule, an order left pending and the accepted terms recorded in the audit.
func CheckoutChannel(termsVersion string) Channel {
	return Channel{
		Basket:   "bag",
		Shipping: ShippingCents,
		Audit: Audit{
			Action: "order.terms_accepted",
			Detail: func(_ store.Order, _ int, now time.Time) map[string]any {
				return map[string]any{
					"version":     termsVersion,
					"accepted_at": store.FormatTime(now),
				}
			},
		},
	}
}

// CounterChannel is the in-store sale: no delivery, delivered in the same
// transaction, with the sale's own audit row.
func CounterChannel() Channel {
	return Channel{
		Basket:    "sale",
		Shipping:  NoShipping,
		Delivered: true,
		Audit: Audit{
			Action: "order.counter_sale",
			Detail: func(order store.Order, itemCount int, _ time.Time) map[string]any {
				return map[string]any{
					"channel":     "counter",
					"total_cents": order.TotalCents,
					"items":       itemCount,
				}
			},
		},
	}
}

// Place stores one order. It checks the catalog and the selected options in
// the write transaction, prices the lines, groups the plugin lines, allocates
// the order number, inserts the order and its items, moves stock, runs the
// checkout hooks, optionally marks the sale delivered, and writes the audit
// row. A refusal comes back as a *Problem; anything else is an infrastructure
// error the caller reports as an internal failure.
func (in *Intake) Place(ctx context.Context, request Request) (Result, error) {
	now := time.Now().UTC()
	order := store.Order{
		ID:           ids.New("ord"),
		Status:       request.Status,
		Name:         request.Customer.Name,
		Email:        request.Customer.Email,
		Phone:        request.Customer.Phone,
		Address:      request.Customer.Address,
		City:         request.Destination.CommuneName,
		Notes:        request.Customer.Notes,
		Currency:     "DZD",
		CreatedAt:    now,
		UpdatedAt:    now,
		WilayaCode:   request.Destination.WilayaCode,
		WilayaName:   request.Destination.WilayaName,
		CommuneName:  request.Destination.CommuneName,
		DeliveryType: request.Destination.DeliveryType,
		StopDeskID:   request.Destination.StopDeskID,
		StopDeskName: request.Destination.StopDeskName,
		TokenHash:    request.TokenHash,
	}
	if request.Customer.UserID != "" {
		userID := request.Customer.UserID
		order.UserID = &userID
	}

	items := make([]store.OrderItem, 0, len(request.Lines))
	pluginLines := make(map[string][]plugins.CheckoutLine)

	err := in.store.InTx(ctx, func(tx *sql.Tx) error {
		var subtotal int64
		for _, line := range request.Lines {
			product, err := in.store.ProductByIDTx(ctx, tx, line.ProductID, true)
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					return refuseFields(409, "conflict", "Some products are no longer available.",
						map[string]string{"items": "A product in the " + request.Channel.Basket + " is no longer available."})
				}
				return err
			}
			if !in.plugins.EffectiveEnabled(product.PluginID) {
				return refuseFields(409, "conflict", "Some products are no longer available.",
					map[string]string{"items": product.Name + " is no longer available."})
			}

			schema := OptionSchema(in.plugins, product)
			if fields := selectedOptionFields(schema, product.Attributes, line.Options); len(fields) > 0 {
				return refuseFields(409, "conflict", "Some product options have changed.", fields)
			}

			itemID := ids.New("itm")
			pluginLine := plugins.CheckoutLine{
				Product:  product,
				Options:  line.Options,
				Quantity: line.Quantity,
				OrderID:  order.ID,
				ItemID:   itemID,
			}
			if hook := in.plugins.CheckoutHook(product.PluginID); hook != nil {
				if hookProblem := hook.ValidateCheckoutLine(ctx, tx, pluginLine); hookProblem != nil {
					return refuseFields(hookProblem.Status, hookProblem.Code,
						hookProblem.Message, hookProblem.Fields)
				}
			}

			if product.Stock < line.Quantity {
				return refuseFields(409, "conflict", "Some products do not have enough stock.",
					map[string]string{"items": "Reduce the quantity for " + product.Name + "."})
			}
			unitPrice := product.EffectivePriceCents(now)
			if line.ExpectedPriceCents != nil && *line.ExpectedPriceCents != unitPrice {
				return refuseFields(409, "price_changed",
					"A price changed while the bag was open. Review the bag and try again.",
					map[string]string{"items": product.Name + " now costs a different amount."})
			}
			lineTotal := unitPrice * int64(line.Quantity)
			subtotal += lineTotal
			items = append(items, store.OrderItem{
				ID:             itemID,
				ProductID:      product.ID,
				Slug:           product.Slug,
				Name:           product.Name,
				Image:          product.Image,
				UnitPriceCents: unitPrice,
				Quantity:       line.Quantity,
				LineTotalCents: lineTotal,
				Options:        line.Options,
				OptionsLabel:   optionLabel(schema, line.Options),
				PluginID:       store.NormalizePluginID(product.PluginID),
			})
			if store.NormalizePluginID(product.PluginID) != store.CorePluginID {
				pluginLines[product.PluginID] = append(pluginLines[product.PluginID], pluginLine)
			}
		}

		order.SubtotalCents = subtotal
		order.ShippingCents = request.Channel.shippingCents(subtotal)
		order.TotalCents = order.SubtotalCents + order.ShippingCents

		number, err := in.store.NextOrderNumber(ctx, tx, now.Year())
		if err != nil {
			return err
		}
		order.Number = number
		if err := in.store.CreateOrder(ctx, tx, order, items); err != nil {
			return err
		}
		for _, line := range request.Lines {
			ok, err := in.store.DecrementStock(ctx, tx, line.ProductID, line.Quantity)
			if err != nil {
				return err
			}
			if !ok {
				return refuseFields(409, "conflict", "Some products do not have enough stock.",
					map[string]string{"items": "Reduce the quantity in the " + request.Channel.Basket + "."})
			}
		}
		for pluginID, hookLines := range pluginLines {
			hook := in.plugins.CheckoutHook(pluginID)
			if hook == nil {
				continue
			}
			if err := hook.OnOrderCreated(ctx, tx, plugins.OrderRef{
				ID:     order.ID,
				Number: order.Number,
				Status: order.Status,
			}, hookLines); err != nil {
				return err
			}
		}
		if request.Channel.Delivered {
			// A counter sale is born delivered: this is creation-time state, not
			// an admin lifecycle move, so it does not go through
			// orderstatus.CanTransition. Creating the row pending first keeps
			// UpdateOrderStatus and OnOrderStatusChanged in their existing
			// order inside the same transaction.
			from := order.Status
			if err := in.store.UpdateOrderStatus(ctx, tx, order.ID, from, statusDelivered, now); err != nil {
				return err
			}
			for _, hook := range in.plugins.OrderStatusHooks() {
				if err := hook.OnOrderStatusChanged(ctx, tx, order.ID, from, statusDelivered); err != nil {
					return err
				}
			}
			order.Status = statusDelivered
		}
		detail := map[string]any{}
		if describe := request.Channel.Audit.Detail; describe != nil {
			detail = describe(order, len(items), now)
		}
		encoded, err := json.Marshal(detail)
		if err != nil {
			return err
		}
		return in.store.InsertAudit(ctx, tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: request.ActorUserID,
			Action:      request.Channel.Audit.Action,
			EntityType:  "order",
			EntityID:    order.ID,
			Detail:      string(encoded),
			CreatedAt:   now,
		})
	})
	if err != nil {
		return Result{}, err
	}
	return Result{Order: order, Items: items}, nil
}

// Change is one status move on an existing order.
type Change struct {
	OrderID     string
	To          string
	ActorUserID string
	ActorRole   string
}

// Changed is an order after a status move, with its lines.
type Changed struct {
	Order store.Order
	Items []store.OrderItem
}

// ChangeStatus moves an order through the lifecycle. It refuses an unknown
// status, an order that is missing, a move the lifecycle does not allow and a
// cancellation by anyone who is not an administrator. An allowed move updates
// the row, restocks a cancellation, writes the audit row and runs the status
// hooks in the same transaction.
func (in *Intake) ChangeStatus(ctx context.Context, change Change) (Changed, error) {
	if !orderstatus.Known(change.To) {
		return Changed{}, refuseFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"status": "Unknown order status."})
	}
	order, err := in.store.OrderByID(ctx, change.OrderID)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Changed{}, refuse(404, "not_found", "Order not found.")
		}
		return Changed{}, err
	}
	if !orderstatus.CanTransition(order.Status, change.To) {
		return Changed{}, refuseFields(409, "invalid_transition", "That status change is not allowed.",
			map[string]string{"status": "This order cannot move from " + order.Status + " to " + change.To + "."})
	}
	if change.To == statusCancelled && change.ActorRole != "admin" {
		return Changed{}, refuse(403, "forbidden", "Only an administrator can cancel an order.")
	}

	now := time.Now().UTC()
	var restock []store.OrderItem
	if change.To == statusCancelled {
		restock, err = in.store.OrderItems(ctx, order.ID)
		if err != nil {
			return Changed{}, err
		}
	}
	detail, _ := json.Marshal(map[string]string{"from": order.Status, "to": change.To})

	err = in.store.InTx(ctx, func(tx *sql.Tx) error {
		if err := in.store.UpdateOrderStatus(ctx, tx, order.ID, order.Status, change.To, now); err != nil {
			return err
		}
		for _, item := range restock {
			if err := in.store.IncrementStock(ctx, tx, item.ProductID, item.Quantity); err != nil {
				return err
			}
		}
		if err := in.store.InsertAudit(ctx, tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: change.ActorUserID,
			Action:      "order.status",
			EntityType:  "order",
			EntityID:    order.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		}); err != nil {
			return err
		}
		for _, hook := range in.plugins.OrderStatusHooks() {
			if err := hook.OnOrderStatusChanged(ctx, tx, order.ID, order.Status, change.To); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Changed{}, refuseFields(409, "conflict", "The order changed. Reload and try again.",
				map[string]string{"status": "The status changed while this page was open."})
		}
		return Changed{}, err
	}
	order.Status = change.To
	order.UpdatedAt = now

	items, err := in.store.OrderItems(ctx, order.ID)
	if err != nil {
		return Changed{}, err
	}
	return Changed{Order: order, Items: items}, nil
}

// shippingCents applies a channel's delivery rule.
func (c Channel) shippingCents(subtotalCents int64) int64 {
	if c.Shipping == nil {
		return 0
	}
	return c.Shipping(subtotalCents)
}
