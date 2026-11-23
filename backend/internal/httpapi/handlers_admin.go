package httpapi

import (
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/ids"
	"azahar/internal/orderintake"
	"azahar/internal/orderstatus"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

func (s *Server) handleAdminOrders(w http.ResponseWriter, r *http.Request) {
	query := r.URL.Query()
	status := strings.TrimSpace(query.Get("status"))
	if status != "" && !orderstatus.Known(status) {
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
	ids := make([]string, 0, len(orders))
	for _, order := range orders {
		ids = append(ids, order.ID)
	}
	itemsByOrder, err := s.store.OrderItemsByOrderIDs(r.Context(), ids)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	result := make([]orderDTO, 0, len(orders))
	for _, order := range orders {
		result = append(result, toOrder(order, itemsByOrder[order.ID], "", true))
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
	actor, _ := currentAuth(r)
	changed, err := s.intake.ChangeStatus(r.Context(), orderintake.Change{
		OrderID:     id,
		To:          strings.TrimSpace(payload.Status),
		ActorUserID: actor.User.ID,
		ActorRole:   actor.User.Role,
	})
	if err != nil {
		writeIntakeError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"order": toOrder(changed.Order, changed.Items, "", true),
	})
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

type teamMemberDTO struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Email     string `json:"email"`
	Role      string `json:"role"`
	CreatedAt string `json:"created_at"`
}

func (s *Server) handleAdminTeam(w http.ResponseWriter, r *http.Request) {
	users, err := s.store.ListUsers(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	result := make([]teamMemberDTO, 0, len(users))
	for _, user := range users {
		result = append(result, teamMemberDTO{
			ID: user.ID, Name: user.Name, Email: user.Email, Role: user.Role,
			CreatedAt: store.FormatTime(user.CreatedAt),
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{"users": result})
}

type teamRolePayload struct {
	Role string `json:"role"`
}

var errLastAdmin = errors.New("at least one administrator is required")

func (s *Server) handleUpdateTeamRole(w http.ResponseWriter, r *http.Request) {
	id := chi.URLParam(r, "id")
	payload, _, bad := decodeJSON[teamRolePayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	role := strings.TrimSpace(payload.Role)
	if role != "admin" && role != "employee" && role != "customer" {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"role": "Choose admin, employee or customer."}))
		return
	}
	target, err := s.store.UserByID(r.Context(), id)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "User not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	actor, _ := currentAuth(r)
	if target.ID == actor.User.ID && role != "admin" {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"role": "You cannot remove your own administrator role."}))
		return
	}

	now := time.Now().UTC()
	detail, _ := json.Marshal(map[string]string{"from": target.Role, "to": role})
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if target.Role == "admin" && role != "admin" {
			admins, err := s.store.CountAdmins(r.Context(), tx)
			if err != nil {
				return err
			}
			if admins <= 1 {
				return errLastAdmin
			}
		}
		if err := s.store.UpdateUserRole(r.Context(), tx, target.ID, role, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: actor.User.ID,
			Action:      "user.role",
			EntityType:  "user",
			EntityID:    target.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if errors.Is(err, errLastAdmin) {
		writeProblem(w, problemFields(409, "last_admin", "Keep at least one administrator.",
			map[string]string{"role": "Promote another administrator first."}))
		return
	}
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	target.Role = role
	target.UpdatedAt = now
	writeJSON(w, http.StatusOK, map[string]any{"user": teamMemberDTO{
		ID: target.ID, Name: target.Name, Email: target.Email, Role: target.Role,
		CreatedAt: store.FormatTime(target.CreatedAt),
	}})
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
