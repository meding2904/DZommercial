package httpapi

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

func (s *Server) handleAdminOrders(w http.ResponseWriter, r *http.Request) {
	query := r.URL.Query()
	status := strings.TrimSpace(query.Get("status"))
	if status != "" && !knownStatus(status) {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"status": "Unknown order status."}))
		return
	}
	search := strings.TrimSpace(query.Get("q"))
	if runeLen(search) > 120 {
		search = string([]rune(search)[:120])
	}

	orders, err := s.store.AdminOrders(r.Context(), store.AdminOrderFilter{
		Status: status,
		Query:  search,
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	result := make([]orderDTO, 0, len(orders))
	for _, order := range orders {
		items, err := s.store.OrderItems(r.Context(), order.ID)
		if err != nil {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		result = append(result, toOrder(order, items, "", true))
	}
	writeJSON(w, http.StatusOK, map[string]any{"orders": result})
}

type orderStatusPayload struct {
	Status string `json:"status"`
}

func (s *Server) handleOrderStatus(w http.ResponseWriter, r *http.Request) {
	id := chi.URLParam(r, "id")
	payload, _, bad := decodeJSON[orderStatusPayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	status := strings.TrimSpace(payload.Status)
	if !knownStatus(status) {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"status": "Unknown order status."}))
		return
	}

	order, err := s.store.OrderByID(r.Context(), id)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Order not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if !canTransition(order.Status, status) {
		writeProblem(w, problemFields(409, "invalid_transition", "That status change is not allowed.",
			map[string]string{"status": "This order cannot move from " + order.Status + " to " + status + "."}))
		return
	}

	now := time.Now().UTC()
	detail, _ := json.Marshal(map[string]string{"from": order.Status, "to": status})
	actor, _ := currentAuth(r)
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.UpdateOrderStatus(r.Context(), tx, order.ID, order.Status, status, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "order.status",
			EntityType:  "order",
			EntityID:    order.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeProblem(w, problemFields(409, "conflict", "The order changed. Reload and try again.",
				map[string]string{"status": "The status changed while this page was open."}))
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	order.Status = status
	order.UpdatedAt = now

	items, err := s.store.OrderItems(r.Context(), order.ID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"order": toOrder(order, items, "", true)})
}

type customerDTO struct {
	ID              string  `json:"id"`
	Name            string  `json:"name"`
	Email           string  `json:"email"`
	OrdersCount     int     `json:"orders_count"`
	TotalSpentCents int64   `json:"total_spent_cents"`
	LastOrderAt     *string `json:"last_order_at"`
	CreatedAt       string  `json:"created_at"`
}

func (s *Server) handleCustomers(w http.ResponseWriter, r *http.Request) {
	customers, err := s.store.ListCustomers(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	result := make([]customerDTO, 0, len(customers))
	for _, customer := range customers {
		var lastOrderAt *string
		if customer.LastOrderAt != nil {
			value := store.FormatTime(*customer.LastOrderAt)
			lastOrderAt = &value
		}
		result = append(result, customerDTO{
			ID:              customer.ID,
			Name:            customer.Name,
			Email:           customer.Email,
			OrdersCount:     customer.OrdersCount,
			TotalSpentCents: customer.TotalSpentCents,
			LastOrderAt:     lastOrderAt,
			CreatedAt:       store.FormatTime(customer.CreatedAt),
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{"customers": result})
}

func (s *Server) handleGetPreferences(w http.ResponseWriter, r *http.Request) {
	current, _ := currentAuth(r)
	preset := current.User.ThemePreset
	if !knownPreset(preset) {
		preset = "cellar"
	}
	writeJSON(w, http.StatusOK, map[string]any{"theme_preset": preset})
}

type preferencesPayload struct {
	ThemePreset string `json:"theme_preset"`
}

func (s *Server) handlePutPreferences(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[preferencesPayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	preset := strings.TrimSpace(payload.ThemePreset)
	if !knownPreset(preset) {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"theme_preset": "Choose plaster, cellar, terracotta or lagoon."}))
		return
	}

	now := time.Now().UTC()
	actor, _ := currentAuth(r)
	detail, _ := json.Marshal(map[string]string{"theme_preset": preset})
	err := s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.UpdateTheme(r.Context(), tx, actor.User.ID, preset, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "preferences.update",
			EntityType:  "user",
			EntityID:    actor.User.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"theme_preset": preset})
}
