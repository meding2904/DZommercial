package httpapi

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"azahar/internal/courier"
	"azahar/internal/ids"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

type resolvedDestination struct {
	Wilaya  courier.Wilaya
	Commune string
	Desk    *courier.Desk
}

func (s *Server) handleShippingWilayas(w http.ResponseWriter, r *http.Request) {
	wilayas, err := s.shipping.Wilayas(r.Context())
	if err != nil {
		s.writeDeliveryMapError(w, r, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"wilayas": wilayas})
}

func (s *Server) handleShippingCommunes(w http.ResponseWriter, r *http.Request) {
	wilaya, problem := parseWilayaCode(r)
	if problem != nil {
		writeProblem(w, problem)
		return
	}
	communes, err := s.shipping.Communes(r.Context(), wilaya)
	if err != nil {
		s.writeDeliveryMapError(w, r, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"communes": communes})
}

func (s *Server) handleShippingDesks(w http.ResponseWriter, r *http.Request) {
	wilaya, problem := parseWilayaCode(r)
	if problem != nil {
		writeProblem(w, problem)
		return
	}
	desks, err := s.shipping.Desks(r.Context(), wilaya)
	if err != nil {
		var apiErr *courier.APIError
		if errors.As(err, &apiErr) && apiErr.Status == http.StatusUnprocessableEntity {
			writeJSON(w, http.StatusOK, map[string]any{"desks": []courier.Desk{}})
			return
		}
		s.writeDeliveryMapError(w, r, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"desks": desks})
}

func (s *Server) resolveDestination(ctx context.Context, wilayaCode int, communeName, deliveryType, stopDeskID string) (resolvedDestination, *apiProblem) {
	wilaya, err := s.shipping.Wilaya(ctx, wilayaCode)
	if errors.Is(err, courier.ErrUnknownWilaya) {
		return resolvedDestination{}, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"wilaya_code": "Choose a wilaya from the list."})
	}
	if err != nil {
		return resolvedDestination{}, problem(503, "courier_unavailable",
			"The delivery map is unavailable. Try again in a moment.")
	}

	result := resolvedDestination{Wilaya: wilaya, Commune: strings.TrimSpace(communeName)}

	if deliveryType == "stopdesk" {
		desk, err := s.shipping.Desk(ctx, wilayaCode, stopDeskID)
		if errors.Is(err, courier.ErrUnknownDesk) {
			return resolvedDestination{}, problemFields(422, "validation_error", "Check the marked fields.",
				map[string]string{"stop_desk_id": "Choose a stop desk from the list."})
		}
		if err != nil {
			return resolvedDestination{}, problem(503, "courier_unavailable",
				"The delivery map is unavailable. Try again in a moment.")
		}
		result.Desk = &desk
		if strings.TrimSpace(desk.CommuneName) != "" {
			result.Commune = desk.CommuneName
		}
	}

	if strings.TrimSpace(result.Commune) == "" {
		return resolvedDestination{}, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"commune_name": "Choose a commune from the list."})
	}

	canonical, err := s.shipping.Commune(ctx, wilayaCode, result.Commune)
	if errors.Is(err, courier.ErrUnknownCommune) {
		return resolvedDestination{}, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"commune_name": "Choose a commune from the list."})
	}
	if err != nil {
		return resolvedDestination{}, problem(503, "courier_unavailable",
			"The delivery map is unavailable. Try again in a moment.")
	}
	result.Commune = canonical
	return result, nil
}

func (s *Server) handleCreateShipment(w http.ResponseWriter, r *http.Request) {
	id := chi.URLParam(r, "id")
	order, err := s.store.OrderByID(r.Context(), id)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Order not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if order.TrackingNumber != "" {
		writeProblem(w, problem(409, "conflict", "This order already has a shipment."))
		return
	}
	if order.Status == "cancelled" || order.Status == "delivered" {
		writeProblem(w, problem(409, "invalid_transition", "This order is closed and cannot be shipped."))
		return
	}

	deliveryType := order.DeliveryType
	if deliveryType == "" {
		deliveryType = "home"
	}
	fields := make(map[string]string)
	if order.WilayaCode < 1 || order.WilayaCode > 58 {
		fields["delivery"] = "This order has no wilaya."
	}
	if strings.TrimSpace(order.CommuneName) == "" {
		fields["commune_name"] = "This order has no commune."
	}
	if deliveryType == "stopdesk" {
		if strings.TrimSpace(order.StopDeskID) == "" {
			fields["stop_desk_id"] = "This order has no stop desk."
		}
	} else if runeLen(order.Address) < 5 {
		fields["address"] = "This order has no deliverable address."
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "This order needs delivery details first.", fields))
		return
	}

	phone, ok := normalizedAlgerianMobile(order.Phone)
	if !ok {
		writeProblem(w, problemFields(422, "validation_error", "This order needs a valid phone first.",
			map[string]string{"phone": "Use an Algerian mobile number."}))
		return
	}

	items, err := s.store.OrderItems(r.Context(), order.ID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	addressLine := ""
	if deliveryType != "stopdesk" {
		addressLine = order.Address
	}
	shipment, err := s.shipping.CreateOrder(r.Context(), courier.OrderRequest{
		Reference: order.Number,
		Recipient: courier.Recipient{
			FullName:    order.Name,
			Phone:       phone,
			WilayaCode:  order.WilayaCode,
			CommuneName: order.CommuneName,
			AddressLine: addressLine,
		},
		DeliveryType: deliveryType,
		StopDeskID:   order.StopDeskID,
		ProductList:  orderProductList(items),
		// The COD amount already contains the store's delivery charge. Tell the
		// courier not to add its own fee on top of the amount we collect.
		CODAmount:    order.TotalCents / 100,
		FreeShipping: true,
		Notes:        truncateRunes(order.Notes, 255),
	})
	if err != nil {
		s.writeCourierError(w, r, err)
		return
	}

	status := strings.TrimSpace(shipment.Status)
	if status == "" {
		status = "created"
	}
	now := time.Now().UTC()
	detail, _ := json.Marshal(map[string]string{
		"courier":         s.cfg.CourierKey,
		"tracking_number": shipment.TrackingNumber,
	})
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.UpdateShipment(r.Context(), tx, order.ID, s.cfg.CourierKey,
			shipment.TrackingNumber, status, nil, now, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: currentUserID(r),
			Action:      "order.shipment",
			EntityType:  "order",
			EntityID:    order.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	order.CourierKey = s.cfg.CourierKey
	order.TrackingNumber = shipment.TrackingNumber
	order.CourierStatus = status
	order.CourierEvents = []store.CourierEvent{}
	order.CourierSyncedAt = now
	order.UpdatedAt = now

	writeJSON(w, http.StatusOK, map[string]any{"order": toOrder(order, items, "", true)})
}

func (s *Server) handleRefreshTracking(w http.ResponseWriter, r *http.Request) {
	id := chi.URLParam(r, "id")
	order, err := s.store.OrderByID(r.Context(), id)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			writeError(w, http.StatusNotFound, "not_found", "Order not found.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if order.TrackingNumber == "" {
		writeProblem(w, problem(409, "conflict", "This order has no shipment yet."))
		return
	}

	tracking, err := s.shipping.Track(r.Context(), order.CourierKey, order.TrackingNumber)
	if err != nil {
		s.writeCourierError(w, r, err)
		return
	}

	events := make([]store.CourierEvent, 0, len(tracking.Events))
	for _, event := range tracking.Events {
		events = append(events, store.CourierEvent{
			Status:    event.Status,
			RawStatus: event.RawStatus,
			Timestamp: event.Timestamp,
		})
	}
	status := strings.TrimSpace(tracking.Status)
	if status == "" {
		status = "unknown"
	}
	now := time.Now().UTC()
	detail, _ := json.Marshal(map[string]string{"status": status})
	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.UpdateTracking(r.Context(), tx, order.ID, status, events, now, now); err != nil {
			return err
		}
		return s.store.InsertAudit(r.Context(), tx, store.AuditEntry{
			ID:          ids.New("aud"),
			ActorUserID: currentUserID(r),
			Action:      "order.tracking",
			EntityType:  "order",
			EntityID:    order.ID,
			Detail:      string(detail),
			CreatedAt:   now,
		})
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	items, err := s.store.OrderItems(r.Context(), order.ID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	order.CourierStatus = status
	order.CourierEvents = events
	order.CourierSyncedAt = now
	order.UpdatedAt = now
	writeJSON(w, http.StatusOK, map[string]any{"order": toOrder(order, items, "", true)})
}

func (s *Server) writeDeliveryMapError(w http.ResponseWriter, r *http.Request, err error) {
	s.logger.Warn("delivery map lookup failed", "error", err, "request_id", requestID(r))
	writeError(w, http.StatusServiceUnavailable, "courier_unavailable",
		"The delivery map is unavailable. Try again in a moment.")
}

func (s *Server) writeCourierError(w http.ResponseWriter, r *http.Request, err error) {
	var apiErr *courier.APIError
	if errors.As(err, &apiErr) {
		if apiErr.Status == http.StatusTooManyRequests {
			seconds := int(apiErr.RetryAfter / time.Second)
			if seconds < 1 {
				seconds = 1
			}
			w.Header().Set("Retry-After", strconv.Itoa(seconds))
			writeError(w, http.StatusTooManyRequests, "courier_rate_limited",
				"The courier service is busy. Try again shortly.")
			return
		}

		message := truncateRunes(strings.TrimSpace(apiErr.Message), 300)
		if message == "" {
			message = "The courier service rejected the request."
		}
		if apiErr.Status == http.StatusBadRequest || apiErr.Status == http.StatusUnprocessableEntity {
			writeProblem(w, problemFields(422, "courier_error", message, apiErr.Fields))
			return
		}
		writeError(w, http.StatusBadGateway, "courier_error", message)
		return
	}

	s.logger.Warn("courier request failed", "error", err, "request_id", requestID(r))
	writeError(w, http.StatusBadGateway, "courier_error",
		"The courier service is unavailable. Try again in a moment.")
}

func parseWilayaCode(r *http.Request) (int, *apiProblem) {
	raw := strings.TrimSpace(r.URL.Query().Get("wilaya"))
	code, err := strconv.Atoi(raw)
	if err != nil || code < 1 || code > 58 {
		return 0, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"wilaya": "Choose a wilaya between 1 and 58."})
	}
	return code, nil
}

func orderProductList(items []store.OrderItem) string {
	parts := make([]string, 0, len(items))
	for _, item := range items {
		parts = append(parts, fmt.Sprintf("%s x%d", item.Name, item.Quantity))
	}
	return truncateRunes(strings.Join(parts, ", "), 500)
}

func currentUserID(r *http.Request) string {
	current, ok := currentAuth(r)
	if !ok {
		return ""
	}
	return current.User.ID
}
