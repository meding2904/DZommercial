package httpapi

import (
	"errors"
	"net/http"
	"strings"

	"azahar/internal/auth"
	"azahar/internal/orderintake"
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

	items := make([]orderintake.Line, 0, len(payload.Items))
	for _, item := range payload.Items {
		items = append(items, orderintake.Line{
			ProductID: item.ProductID,
			Quantity:  item.Quantity,
			Options:   item.Options,
		})
	}
	lines, itemFields := orderintake.BuildLines(items)
	for key, message := range itemFields {
		fields[key] = message
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	actor, _ := currentAuth(r)
	name := payload.CustomerName
	if name == "" {
		name = "Counter sale"
	}
	token, err := auth.NewToken()
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	result, err := s.intake.Place(r.Context(), orderintake.Request{
		Lines:   lines,
		Channel: orderintake.CounterChannel(),
		Customer: orderintake.Customer{
			Name:  name,
			Phone: payload.Phone,
			Notes: payload.Note,
		},
		Destination: orderintake.Destination{DeliveryType: "pickup"},
		Status:      "pending",
		TokenHash:   auth.HashToken(token),
		ActorUserID: actor.User.ID,
	})
	if err != nil {
		var refusal *orderintake.Problem
		if errors.As(err, &refusal) {
			writeProblem(w, problemFields(refusal.Status, refusal.Code, refusal.Message, refusal.Fields))
			return
		}
		s.logger.Error("counter sale failed", "error", err, "request_id", requestID(r))
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"order": toOrder(result.Order, result.Items, token, true),
	})
}
