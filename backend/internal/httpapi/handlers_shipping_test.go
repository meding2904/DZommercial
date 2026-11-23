package httpapi

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"azahar/internal/config"
	"azahar/internal/courier"
)

// scriptedCourier answers the delivery-map and parcel verbs with canned
// answers, so the shipping handlers can be driven without a socket.
type scriptedCourier struct {
	wilayas  []courier.Wilaya
	desks    []courier.Desk
	commune  string
	shipment courier.Shipment
	tracking courier.Tracking
	err      error
}

func (f *scriptedCourier) Wilayas(context.Context) ([]courier.Wilaya, error) {
	if f.err != nil {
		return nil, f.err
	}
	return f.wilayas, nil
}

func (f *scriptedCourier) Wilaya(_ context.Context, code int) (courier.Wilaya, error) {
	if f.err != nil {
		return courier.Wilaya{}, f.err
	}
	for _, wilaya := range f.wilayas {
		if wilaya.Code == code {
			return wilaya, nil
		}
	}
	return courier.Wilaya{}, courier.ErrUnknownWilaya
}

func (f *scriptedCourier) Communes(context.Context, int) ([]courier.Commune, error) {
	if f.err != nil {
		return nil, f.err
	}
	if f.commune == "" {
		return nil, nil
	}
	return []courier.Commune{{NameFr: f.commune}}, nil
}

func (f *scriptedCourier) Commune(context.Context, int, string) (string, error) {
	if f.err != nil {
		return "", f.err
	}
	if f.commune == "" {
		return "", courier.ErrUnknownCommune
	}
	return f.commune, nil
}

func (f *scriptedCourier) Desks(context.Context, int) ([]courier.Desk, error) {
	if f.err != nil {
		return nil, f.err
	}
	return f.desks, nil
}

func (f *scriptedCourier) Desk(_ context.Context, _ int, id string) (courier.Desk, error) {
	if f.err != nil {
		return courier.Desk{}, f.err
	}
	for _, desk := range f.desks {
		if desk.ID == id {
			return desk, nil
		}
	}
	return courier.Desk{}, courier.ErrUnknownDesk
}

func (f *scriptedCourier) CreateOrder(context.Context, courier.OrderRequest) (courier.Shipment, error) {
	return f.shipment, f.err
}

func (f *scriptedCourier) Track(context.Context, string, string) (courier.Tracking, error) {
	return f.tracking, f.err
}

func newShippingTestServer(fake courier.Courier) *Server {
	return &Server{
		cfg:      config.Config{CourierKey: "sandbox"},
		shipping: fake,
		logger:   slog.New(slog.NewTextHandler(io.Discard, nil)),
	}
}

type problemBody struct {
	Error struct {
		Code    string            `json:"code"`
		Message string            `json:"message"`
		Fields  map[string]string `json:"fields"`
	} `json:"error"`
}

func decodeProblem(t *testing.T, body []byte) problemBody {
	t.Helper()
	var decoded problemBody
	if err := json.Unmarshal(body, &decoded); err != nil {
		t.Fatalf("decode problem: %v (body %s)", err, body)
	}
	return decoded
}

func TestShippingWilayasServesTheDeliveryMap(t *testing.T) {
	server := newShippingTestServer(&scriptedCourier{wilayas: []courier.Wilaya{
		{Code: 16, NameFr: "Alger"},
		{Code: 31, NameFr: "Oran"},
	}})
	recorder := httptest.NewRecorder()

	server.handleShippingWilayas(recorder, httptest.NewRequest(http.MethodGet, "/api/shipping/wilayas", nil))

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusOK)
	}
	var body struct {
		Wilayas []courier.Wilaya `json:"wilayas"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if len(body.Wilayas) != 2 || body.Wilayas[0].Code != 16 || body.Wilayas[1].NameFr != "Oran" {
		t.Fatalf("wilayas = %+v", body.Wilayas)
	}
}

func TestShippingCommunesRejectsAWilayaOutsideTheMap(t *testing.T) {
	server := newShippingTestServer(&scriptedCourier{})
	recorder := httptest.NewRecorder()

	server.handleShippingCommunes(recorder, httptest.NewRequest(http.MethodGet, "/api/shipping/communes?wilaya=99", nil))

	if recorder.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusUnprocessableEntity)
	}
	problem := decodeProblem(t, recorder.Body.Bytes())
	if problem.Error.Code != "validation_error" || problem.Error.Fields["wilaya"] == "" {
		t.Fatalf("problem = %+v", problem.Error)
	}
}

func TestShippingDesksTreatACourierRejectionAsAnEmptyList(t *testing.T) {
	server := newShippingTestServer(&scriptedCourier{
		err: &courier.APIError{Status: http.StatusUnprocessableEntity, Code: "courier_error", Message: "No desks."},
	})
	recorder := httptest.NewRecorder()

	server.handleShippingDesks(recorder, httptest.NewRequest(http.MethodGet, "/api/shipping/desks?wilaya=16", nil))

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusOK)
	}
	if got := strings.TrimSpace(recorder.Body.String()); got != `{"desks":[]}` {
		t.Fatalf("body = %s, want an empty desk list", got)
	}
}

func TestShippingDeliveryMapFailureIs503(t *testing.T) {
	server := newShippingTestServer(&scriptedCourier{err: fmt.Errorf("%w: dial tcp: no route", courier.ErrTransport)})
	recorder := httptest.NewRecorder()

	server.handleShippingWilayas(recorder, httptest.NewRequest(http.MethodGet, "/api/shipping/wilayas", nil))

	if recorder.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusServiceUnavailable)
	}
	problem := decodeProblem(t, recorder.Body.Bytes())
	if problem.Error.Code != "courier_unavailable" ||
		problem.Error.Message != "The delivery map is unavailable. Try again in a moment." {
		t.Fatalf("problem = %+v", problem.Error)
	}
}

func TestResolveDestinationRejectsAnUnknownWilaya(t *testing.T) {
	server := newShippingTestServer(&scriptedCourier{})

	_, problem := server.resolveDestination(context.Background(), 99, "Bab Ezzouar", "home", "")

	if problem == nil || problem.Status != http.StatusUnprocessableEntity {
		t.Fatalf("problem = %+v, want a 422", problem)
	}
	if problem.Fields["wilaya_code"] == "" {
		t.Fatalf("fields = %v, want a wilaya_code message", problem.Fields)
	}
}

func TestResolveDestinationFollowsTheStopDeskCommune(t *testing.T) {
	server := newShippingTestServer(&scriptedCourier{
		wilayas: []courier.Wilaya{{Code: 16, NameFr: "Alger"}},
		desks:   []courier.Desk{{ID: "SBX-16", Name: "Sandbox desk Alger", WilayaCode: 16, CommuneName: "Bab Ezzouar"}},
		commune: "Bab Ezzouar",
	})

	destination, problem := server.resolveDestination(context.Background(), 16, "", "stopdesk", "SBX-16")

	if problem != nil {
		t.Fatalf("problem = %+v", problem)
	}
	if destination.Desk == nil || destination.Desk.ID != "SBX-16" {
		t.Fatalf("desk = %+v", destination.Desk)
	}
	if destination.Commune != "Bab Ezzouar" {
		t.Fatalf("commune = %q, want the desk's commune", destination.Commune)
	}
}

// The parcel handlers look the order up in the database first, so these tests
// drive courierCall, the one place both handlers turn a courier failure into a
// response.
func TestCreateShipmentRendersACourierRejectionAsAFieldProblem(t *testing.T) {
	fake := &scriptedCourier{err: &courier.APIError{
		Status:  http.StatusUnprocessableEntity,
		Code:    "invalid_phone",
		Message: "Invalid phone.",
		Fields:  map[string]string{"recipient.phone": "Use 05, 06 or 07."},
	}}
	server := newShippingTestServer(fake)
	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodPost, "/api/admin/orders/ord_1/shipment", nil)

	_, ok := courierCall(server, recorder, request, func() (courier.Shipment, error) {
		return server.shipping.CreateOrder(request.Context(), courier.OrderRequest{})
	})

	if ok {
		t.Fatal("courierCall reported success for a rejected parcel")
	}
	if recorder.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusUnprocessableEntity)
	}
	problem := decodeProblem(t, recorder.Body.Bytes())
	if problem.Error.Code != "courier_error" || problem.Error.Message != "Invalid phone." {
		t.Fatalf("problem = %+v", problem.Error)
	}
	if problem.Error.Fields["recipient.phone"] != "Use 05, 06 or 07." {
		t.Fatalf("fields = %v, want the courier's field map", problem.Error.Fields)
	}
}

func TestCreateShipmentRendersARateLimitWithRetryAfter(t *testing.T) {
	cases := []struct {
		name       string
		retryAfter time.Duration
		want       string
	}{
		{name: "from the courier", retryAfter: 7 * time.Second, want: "7"},
		{name: "never below one second", retryAfter: 0, want: "1"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			fake := &scriptedCourier{err: &courier.APIError{
				Status:     http.StatusTooManyRequests,
				Code:       "rate_limited",
				Message:    "Slow down.",
				RetryAfter: tc.retryAfter,
			}}
			server := newShippingTestServer(fake)
			recorder := httptest.NewRecorder()
			request := httptest.NewRequest(http.MethodPost, "/api/admin/orders/ord_1/shipment", nil)

			if _, ok := courierCall(server, recorder, request, func() (courier.Shipment, error) {
				return server.shipping.CreateOrder(request.Context(), courier.OrderRequest{})
			}); ok {
				t.Fatal("courierCall reported success for a rate limit")
			}

			if recorder.Code != http.StatusTooManyRequests {
				t.Fatalf("status = %d, want %d", recorder.Code, http.StatusTooManyRequests)
			}
			if got := recorder.Header().Get("Retry-After"); got != tc.want {
				t.Fatalf("Retry-After = %q, want %q", got, tc.want)
			}
			problem := decodeProblem(t, recorder.Body.Bytes())
			if problem.Error.Code != "courier_rate_limited" {
				t.Fatalf("problem = %+v", problem.Error)
			}
		})
	}
}

func TestCourierTransportFailureRenders502(t *testing.T) {
	fake := &scriptedCourier{err: fmt.Errorf("%w: dial tcp: connection refused", courier.ErrTransport)}
	server := newShippingTestServer(fake)
	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodPost, "/api/admin/orders/ord_1/tracking", nil)

	if _, ok := courierCall(server, recorder, request, func() (courier.Tracking, error) {
		return server.shipping.Track(request.Context(), "sandbox", "SBX-ORD-1")
	}); ok {
		t.Fatal("courierCall reported success for a transport failure")
	}

	if recorder.Code != http.StatusBadGateway {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusBadGateway)
	}
	problem := decodeProblem(t, recorder.Body.Bytes())
	if problem.Error.Code != "courier_error" ||
		problem.Error.Message != "The courier service is unavailable. Try again in a moment." {
		t.Fatalf("problem = %+v", problem.Error)
	}
}
