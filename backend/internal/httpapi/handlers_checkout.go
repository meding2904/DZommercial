package httpapi

import (
	"crypto/subtle"
	"database/sql"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/auth"
	"azahar/internal/orderintake"

	"github.com/go-chi/chi/v5"
)

// Keep checkoutTermsVersion in sync with frontend/src/lib/legal.ts.
const checkoutTermsVersion = "2026-11-14"

type checkoutPayload struct {
	Name           string         `json:"name"`
	Email          string         `json:"email"`
	Phone          string         `json:"phone"`
	Address        string         `json:"address"`
	WilayaCode     int            `json:"wilaya_code"`
	CommuneName    string         `json:"commune_name"`
	DeliveryType   string         `json:"delivery_type"`
	StopDeskID     string         `json:"stop_desk_id"`
	Notes          string         `json:"notes"`
	Items          []checkoutLine `json:"items"`
	Website        string         `json:"website"`
	CaptchaToken   string         `json:"captcha_token"`
	CaptchaAnswer  string         `json:"captcha_answer"`
	TurnstileToken string         `json:"turnstile_token"`
	TermsAccepted  bool           `json:"terms_accepted"`
	TermsVersion   string         `json:"terms_version"`
}

type checkoutLine struct {
	ProductID          string            `json:"product_id"`
	Quantity           int               `json:"quantity"`
	Options            map[string]string `json:"options"`
	ExpectedPriceCents *int64            `json:"expected_price_cents"`
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
	payload.Website = strings.TrimSpace(payload.Website)
	payload.CaptchaToken = strings.TrimSpace(payload.CaptchaToken)
	payload.CaptchaAnswer = strings.TrimSpace(payload.CaptchaAnswer)
	payload.TurnstileToken = strings.TrimSpace(payload.TurnstileToken)
	payload.TermsVersion = strings.TrimSpace(payload.TermsVersion)
	if payload.DeliveryType == "" {
		payload.DeliveryType = "home"
	}

	// A person never sees this field. A filled value is a bot signal, so reject
	// before doing any order work.
	if payload.Website != "" {
		writeError(w, http.StatusForbidden, "bot_detected", "The order could not be verified.")
		return
	}

	fields := make(map[string]string)
	if !payload.TermsAccepted || payload.TermsVersion != checkoutTermsVersion {
		fields["terms"] = "Accept the current terms of service before placing the order."
	}
	if s.cfg.TurnstileSecret == "" {
		if payload.CaptchaToken == "" || payload.CaptchaAnswer == "" {
			fields["captcha"] = "Enter the code from the security check."
		}
	} else if payload.TurnstileToken == "" {
		fields["captcha"] = "Complete the security check and try again."
	}
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

	items := make([]orderintake.Line, 0, len(payload.Items))
	for _, item := range payload.Items {
		items = append(items, orderintake.Line{
			ProductID:          item.ProductID,
			Quantity:           item.Quantity,
			Options:            item.Options,
			ExpectedPriceCents: item.ExpectedPriceCents,
		})
	}
	lines, itemFields := orderintake.BuildLines(items)
	if len(itemFields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", itemFields))
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

	if s.cfg.TurnstileSecret != "" {
		verified, err := verifyTurnstile(r.Context(), s.cfg.TurnstileSecret, payload.TurnstileToken,
			clientIP(r, s.cfg.TrustProxy))
		if err != nil {
			s.logger.Warn("turnstile verification failed", "error", err, "request_id", requestID(r))
			writeError(w, http.StatusServiceUnavailable, "captcha_unavailable",
				"The security check is unavailable. Try again in a moment.")
			return
		}
		if !verified {
			writeProblem(w, problemFields(422, "validation_error", "The security check failed.",
				map[string]string{"captcha": "Complete the security check and try again."}))
			return
		}
	} else if reason := s.captcha.verify(payload.CaptchaToken, payload.CaptchaAnswer,
		clientIP(r, s.cfg.TrustProxy), time.Now()); reason != "" {
		writeProblem(w, problemFields(422, "validation_error", "The security check failed.",
			map[string]string{"captcha": captchaErrorMessage(reason)}))
		return
	}

	customer := orderintake.Customer{
		Name:    payload.Name,
		Email:   payload.Email,
		Phone:   payload.Phone,
		Address: payload.Address,
		Notes:   payload.Notes,
	}
	target := orderintake.Destination{
		WilayaCode:   destination.Wilaya.Code,
		WilayaName:   destination.Wilaya.NameFr,
		CommuneName:  destination.Commune,
		DeliveryType: payload.DeliveryType,
	}
	if destination.Desk != nil {
		target.StopDeskID = destination.Desk.ID
		target.StopDeskName = destination.Desk.Name
		if customer.Address == "" {
			customer.Address = destination.Desk.Address
		}
	}
	if user := s.optionalUser(r); user != nil {
		customer.UserID = user.ID
	}

	token, err := auth.NewToken()
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	result, err := s.intake.Place(r.Context(), orderintake.Request{
		Lines:       lines,
		Channel:     orderintake.CheckoutChannel(payload.TermsVersion),
		Customer:    customer,
		Destination: target,
		Status:      "pending",
		TokenHash:   auth.HashToken(token),
		ActorUserID: customer.UserID,
	})
	if err != nil {
		writeIntakeError(w, err)
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"order": toOrder(result.Order, result.Items, token, true),
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
		"order": toOrder(order, items, "", true),
	})
}
