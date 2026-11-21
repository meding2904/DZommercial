package httpapi

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/auth"
	"azahar/internal/ids"
	"azahar/internal/plugins"
	"azahar/internal/store"
)

type counterSalePayload struct {
	Items        []counterSaleLine `json:"items"`
	CustomerName string            `json:"customer_name"`
	Phone        string            `json:"phone"`
	Note         string            `json:"note"`
}

type counterSaleLine struct {
	ProductID string            `json:"product_id"`
	Quantity  int               `json:"quantity"`
	Options   map[string]string `json:"options"`
}

// handleCounterSale records a paid in-store sale. It is the one admin write
// that also moves stock, so it runs the same product, option, plugin and
// transaction path as a checkout before marking the order delivered.
func (s *Server) handleCounterSale(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[counterSalePayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	payload.CustomerName = strings.TrimSpace(payload.CustomerName)
	payload.Phone = strings.TrimSpace(payload.Phone)
	payload.Note = strings.TrimSpace(payload.Note)

	fields := make(map[string]string)
	if payload.CustomerName != "" && (runeLen(payload.CustomerName) < 2 || runeLen(payload.CustomerName) > 120) {
		fields["customer_name"] = "Use between 2 and 120 characters, or leave it empty."
	}
	if payload.Phone != "" && !validAlgerianMobile(payload.Phone) {
		fields["phone"] = "Use an Algerian mobile number, for example 0550 12 34 56."
	}
	if runeLen(payload.Note) > 500 {
		fields["note"] = "Keep the note under 500 characters."
	}
	if len(payload.Items) == 0 || len(payload.Items) > 20 {
		fields["items"] = "Add between 1 and 20 products."
	}

	type saleLine struct {
		productID string
		quantity  int
		options   map[string]string
	}
	lines := make([]saleLine, 0, len(payload.Items))
	index := make(map[string]int)
	for _, item := range payload.Items {
		productID := strings.TrimSpace(item.ProductID)
		if productID == "" {
			fields["items"] = "Choose a product for every line."
			continue
		}
		if item.Quantity < 1 || item.Quantity > 10 {
			fields["items"] = "Use between 1 and 10 units per product."
			continue
		}
		options := normalizeOptions(item.Options)
		key := productID + "|" + optionsKey(options)
		if existing, ok := index[key]; ok {
			lines[existing].quantity += item.Quantity
			if lines[existing].quantity > 10 {
				fields["items"] = "Use between 1 and 10 units per product."
			}
			continue
		}
		index[key] = len(lines)
		lines = append(lines, saleLine{productID: productID, quantity: item.Quantity, options: options})
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	actor, _ := currentAuth(r)
	now := time.Now().UTC()
	order := store.Order{
		ID:           ids.New("ord"),
		Status:       "pending",
		Name:         payload.CustomerName,
		Phone:        payload.Phone,
		Notes:        payload.Note,
		Currency:     "DZD",
		DeliveryType: "pickup",
		CreatedAt:    now,
		UpdatedAt:    now,
	}
	if order.Name == "" {
		order.Name = "Counter sale"
	}
	token, err := auth.NewToken()
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	order.TokenHash = auth.HashToken(token)

	items := make([]store.OrderItem, 0, len(lines))
	pluginLines := make(map[string][]plugins.CheckoutLine)
	var problem *apiProblem

	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		var subtotal int64
		for _, line := range lines {
			product, err := s.store.ProductByIDTx(r.Context(), tx, line.productID, true)
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					problem = problemFields(409, "conflict", "Some products are no longer available.",
						map[string]string{"items": "A product in the sale is no longer available."})
					return problem
				}
				return err
			}
			if !s.plugins.EffectiveEnabled(product.PluginID) {
				problem = problemFields(409, "conflict", "Some products are no longer available.",
					map[string]string{"items": product.Name + " is no longer available."})
				return problem
			}
			schema := s.optionSchema(product)
			for _, message := range validateSelectedOptions(schema, product.Attributes, line.options) {
				problem = problemFields(409, "conflict", "Some product options have changed.",
					map[string]string{"items": message})
				return problem
			}
			if product.Stock < line.quantity {
				problem = problemFields(409, "conflict", "Some products do not have enough stock.",
					map[string]string{"items": "Reduce the quantity for " + product.Name + "."})
				return problem
			}
			unitPrice := product.EffectivePriceCents(now)
			lineTotal := unitPrice * int64(line.quantity)
			subtotal += lineTotal
			itemID := ids.New("itm")
			items = append(items, store.OrderItem{
				ID:             itemID,
				ProductID:      product.ID,
				Slug:           product.Slug,
				Name:           product.Name,
				Image:          product.Image,
				UnitPriceCents: unitPrice,
				Quantity:       line.quantity,
				LineTotalCents: lineTotal,
				Options:        line.options,
				OptionsLabel:   optionLabel(schema, line.options),
				PluginID:       store.NormalizePluginID(product.PluginID),
			})
			if store.NormalizePluginID(product.PluginID) != store.CorePluginID {
				pluginLines[product.PluginID] = append(pluginLines[product.PluginID], plugins.CheckoutLine{
					Product:  product,
					Options:  line.options,
					Quantity: line.quantity,
					OrderID:  order.ID,
					ItemID:   itemID,
				})
			}
		}

		order.SubtotalCents = subtotal
		order.ShippingCents = 0
		order.TotalCents = subtotal
		number, err := s.store.NextOrderNumber(r.Context(), tx, now.Year())
		if err != nil {
			return err
		}
		order.Number = number
		if err := s.store.CreateOrder(r.Context(), tx, order, items); err != nil {
			return err
		}
		for _, item := range items {
			ok, err := s.store.DecrementStock(r.Context(), tx, item.ProductID, item.Quantity)
			if err != nil {
				return err
			}
			if !ok {
				problem = problemFields(409, "conflict", "Some products do not have enough stock.",
					map[string]string{"items": "Reduce the quantity in the sale."})
				return problem
			}
		}
		for pluginID, hookLines := range pluginLines {
			hook := s.plugins.CheckoutHook(pluginID)
			if hook == nil {
				continue
			}
			if err := hook.OnOrderCreated(r.Context(), tx, plugins.OrderRef{
				ID:     order.ID,
				Number: order.Number,
				Status: order.Status,
			}, hookLines); err != nil {
				return err
			}
		}
		if err := s.store.UpdateOrderStatus(r.Context(), tx, order.ID, "pending", "delivered", now); err != nil {
			return err
		}
		for _, hook := range s.plugins.OrderStatusHooks() {
			if err := hook.OnOrderStatusChanged(r.Context(), tx, order.ID, "pending", "delivered"); err != nil {
				return err
			}
		}
		detail, err := json.Marshal(map[string]any{
			"channel":     "counter",
			"total_cents": order.TotalCents,
			"items":       len(items),
		})
		if err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "order.counter_sale",
			EntityType:  "order",
			EntityID:    order.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		if problem != nil {
			writeProblem(w, problem)
			return
		}
		s.logger.Error("counter sale failed", "error", err, "request_id", requestID(r))
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	order.Status = "delivered"
	order.UpdatedAt = now
	writeJSON(w, http.StatusCreated, map[string]any{
		"order": toOrder(order, items, token, true),
	})
}
