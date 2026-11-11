package httpapi

import (
	"crypto/subtle"
	"database/sql"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/auth"
	"azahar/internal/ids"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

type checkoutPayload struct {
	Name         string         `json:"name"`
	Email        string         `json:"email"`
	Phone        string         `json:"phone"`
	Address      string         `json:"address"`
	WilayaCode   int            `json:"wilaya_code"`
	CommuneName  string         `json:"commune_name"`
	DeliveryType string         `json:"delivery_type"`
	StopDeskID   string         `json:"stop_desk_id"`
	Notes        string         `json:"notes"`
	Items        []checkoutLine `json:"items"`
}

type checkoutLine struct {
	ProductID string `json:"product_id"`
	Quantity  int    `json:"quantity"`
}

func (s *Server) handleCheckout(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[checkoutPayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	payload.Name = strings.TrimSpace(payload.Name)
	payload.Email = strings.ToLower(strings.TrimSpace(payload.Email))
	payload.Phone = strings.TrimSpace(payload.Phone)
	payload.Address = strings.TrimSpace(payload.Address)
	payload.CommuneName = strings.TrimSpace(payload.CommuneName)
	payload.DeliveryType = strings.ToLower(strings.TrimSpace(payload.DeliveryType))
	payload.StopDeskID = strings.TrimSpace(payload.StopDeskID)
	payload.Notes = strings.TrimSpace(payload.Notes)
	if payload.DeliveryType == "" {
		payload.DeliveryType = "home"
	}

	fields := make(map[string]string)
	if runeLen(payload.Name) < 2 || runeLen(payload.Name) > 120 {
		fields["name"] = "Use between 2 and 120 characters."
	}
	if !validEmail(payload.Email) {
		fields["email"] = "Enter a valid email."
	}
	if !validAlgerianMobile(payload.Phone) {
		fields["phone"] = "Use an Algerian mobile number, for example 0550 12 34 56."
	}
	if payload.WilayaCode < 1 || payload.WilayaCode > 58 {
		fields["wilaya_code"] = "Choose a wilaya."
	}
	if runeLen(payload.CommuneName) < 2 || runeLen(payload.CommuneName) > 80 {
		fields["commune_name"] = "Choose a commune."
	}
	if payload.DeliveryType != "home" && payload.DeliveryType != "stopdesk" {
		fields["delivery_type"] = "Choose home delivery or a stop desk."
	}
	if payload.DeliveryType == "stopdesk" {
		if runeLen(payload.StopDeskID) < 1 || runeLen(payload.StopDeskID) > 100 {
			fields["stop_desk_id"] = "Choose a stop desk."
		}
	} else if runeLen(payload.Address) < 5 || runeLen(payload.Address) > 200 {
		fields["address"] = "Use between 5 and 200 characters."
	}
	if runeLen(payload.Address) > 200 {
		fields["address"] = "Keep the address under 200 characters."
	}
	if runeLen(payload.Notes) > 500 {
		fields["notes"] = "Keep the notes under 500 characters."
	}
	if len(payload.Items) == 0 || len(payload.Items) > 20 {
		fields["items"] = "Add between 1 and 20 products."
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	type line struct {
		productID string
		quantity  int
	}
	lines := make([]line, 0, len(payload.Items))
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
		if existing, ok := index[productID]; ok {
			lines[existing].quantity += item.Quantity
			if lines[existing].quantity > 10 {
				fields["items"] = "Use between 1 and 10 units per product."
			}
			continue
		}
		index[productID] = len(lines)
		lines = append(lines, line{productID: productID, quantity: item.Quantity})
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}
	if len(lines) == 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"items": "Add at least one product."}))
		return
	}

	destination, destinationProblem := s.resolveDestination(
		r.Context(), payload.WilayaCode, payload.CommuneName, payload.DeliveryType, payload.StopDeskID)
	if destinationProblem != nil {
		writeProblem(w, destinationProblem)
		return
	}

	now := time.Now().UTC()
	order := store.Order{
		ID:           ids.New("ord"),
		Status:       "pending",
		Name:         payload.Name,
		Email:        payload.Email,
		Phone:        payload.Phone,
		Address:      payload.Address,
		City:         destination.Commune,
		Notes:        payload.Notes,
		Currency:     "DZD",
		CreatedAt:    now,
		UpdatedAt:    now,
		WilayaCode:   destination.Wilaya.Code,
		WilayaName:   destination.Wilaya.NameFr,
		CommuneName:  destination.Commune,
		DeliveryType: payload.DeliveryType,
	}
	if destination.Desk != nil {
		order.StopDeskID = destination.Desk.ID
		order.StopDeskName = destination.Desk.Name
		if order.Address == "" {
			order.Address = destination.Desk.Address
		}
	}
	if user := s.optionalUser(r); user != nil {
		order.UserID = &user.ID
	}
	token, err := auth.NewToken()
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	order.TokenHash = auth.HashToken(token)

	items := make([]store.OrderItem, 0, len(lines))
	var problem *apiProblem

	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		var subtotal int64
		for _, line := range lines {
			product, err := s.store.ProductByIDTx(r.Context(), tx, line.productID, true)
			if err != nil {
				if errors.Is(err, sql.ErrNoRows) {
					problem = problemFields(409, "conflict", "Some products are no longer available.",
						map[string]string{"items": "A product in the bag is no longer available."})
					return problem
				}
				return err
			}
			if product.Stock < line.quantity {
				problem = problemFields(409, "conflict", "Some products do not have enough stock.",
					map[string]string{"items": "Reduce the quantity for " + product.Name + "."})
				return problem
			}
			lineTotal := product.PriceCents * int64(line.quantity)
			subtotal += lineTotal
			items = append(items, store.OrderItem{
				ID:             ids.New("itm"),
				ProductID:      product.ID,
				Slug:           product.Slug,
				Name:           product.Name,
				Image:          product.Image,
				UnitPriceCents: product.PriceCents,
				Quantity:       line.quantity,
				LineTotalCents: lineTotal,
			})
		}

		order.SubtotalCents = subtotal
		if subtotal > 800000 {
			order.ShippingCents = 0
		} else {
			order.ShippingCents = 60000
		}
		order.TotalCents = order.SubtotalCents + order.ShippingCents

		number, err := s.store.NextOrderNumber(r.Context(), tx, now.Year())
		if err != nil {
			return err
		}
		order.Number = number

		if err := s.store.CreateOrder(r.Context(), tx, order, items); err != nil {
			return err
		}
		for _, line := range lines {
			ok, err := s.store.DecrementStock(r.Context(), tx, line.productID, line.quantity)
			if err != nil {
				return err
			}
			if !ok {
				problem = problemFields(409, "conflict", "Some products do not have enough stock.",
					map[string]string{"items": "Reduce the quantity in the bag."})
				return problem
			}
		}
		return nil
	})
	if err != nil {
		if problem != nil {
			writeProblem(w, problem)
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"order": toOrder(order, items, token, true),
	})
}

func (s *Server) handleOrderByToken(w http.ResponseWriter, r *http.Request) {
	token := chi.URLParam(r, "token")
	if token == "" || len(token) > 200 {
		writeError(w, http.StatusNotFound, "not_found", "Order not found.")
		return
	}
	hash := auth.HashToken(token)
	order, err := s.store.OrderByTokenHash(r.Context(), hash)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Order not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if subtle.ConstantTimeCompare([]byte(order.TokenHash), []byte(hash)) != 1 {
		writeError(w, http.StatusNotFound, "not_found", "Order not found.")
		return
	}
	items, err := s.store.OrderItems(r.Context(), order.ID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"order": toOrder(order, items, token, true),
	})
}
