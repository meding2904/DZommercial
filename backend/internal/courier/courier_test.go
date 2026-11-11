package courier

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

func testClient(t *testing.T, handler http.HandlerFunc) *Client {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	return New(Options{
		BaseURL:     server.URL,
		CourierKey:  "yalidine",
		Credentials: map[string]string{"apiId": "test-id", "apiToken": "test-token"},
		FromWilaya:  16,
		Timeout:     5 * time.Second,
	})
}

func TestCreateOrderSendsTheCourierContract(t *testing.T) {
	var seen map[string]any
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			t.Errorf("method = %s, want POST", r.Method)
		}
		if r.Header.Get("Content-Type") != "application/json" {
			t.Errorf("content type = %q", r.Header.Get("Content-Type"))
		}
		if err := json.NewDecoder(r.Body).Decode(&seen); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"trackingNumber":"yal-ABC123","status":"created","reference":"AZ-2026-0041"}`))
	})

	shipment, err := client.CreateOrder(context.Background(), OrderRequest{
		Reference: "AZ-2026-0041",
		Recipient: Recipient{
			FullName:    "Amina Bouzid",
			Phone:       "0551234567",
			WilayaCode:  16,
			CommuneName: "Bab Ezzouar",
			AddressLine: "12 rue des Oliviers",
		},
		DeliveryType: "home",
		ProductList:  "Orange blossom cleansing oil x1",
		CODAmount:    5400,
		FreeShipping: true,
		Notes:        "Call before delivery",
	})
	if err != nil {
		t.Fatalf("create order: %v", err)
	}
	if shipment.TrackingNumber != "yal-ABC123" {
		t.Fatalf("tracking = %q", shipment.TrackingNumber)
	}

	if seen["courier"] != "yalidine" {
		t.Errorf("courier = %v", seen["courier"])
	}
	credentials, ok := seen["credentials"].(map[string]any)
	if !ok || credentials["apiId"] != "test-id" || credentials["apiToken"] != "test-token" {
		t.Errorf("credentials = %v", seen["credentials"])
	}
	options, ok := seen["options"].(map[string]any)
	if !ok || options["fromWilaya"] != float64(16) {
		t.Errorf("options = %v", seen["options"])
	}
	order, ok := seen["order"].(map[string]any)
	if !ok {
		t.Fatalf("order = %v", seen["order"])
	}
	if order["deliveryType"] != "home" || order["codAmount"] != float64(5400) {
		t.Errorf("order = %v", order)
	}
	recipient, ok := order["recipient"].(map[string]any)
	if !ok || recipient["communeName"] != "Bab Ezzouar" || recipient["wilayaCode"] != float64(16) {
		t.Errorf("recipient = %v", order["recipient"])
	}
}

func TestAPIErrorsCarryStatusAndFields(t *testing.T) {
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnprocessableEntity)
		_, _ = w.Write([]byte(`{"error":{"code":"invalid_phone","message":"Invalid phone.","fields":{"recipient.phone":"Use 05, 06 or 07."}}}`))
	})

	_, err := client.CreateOrder(context.Background(), OrderRequest{})
	var apiErr *APIError
	if !errors.As(err, &apiErr) {
		t.Fatalf("error type = %T", err)
	}
	if apiErr.Status != http.StatusUnprocessableEntity || apiErr.Code != "invalid_phone" {
		t.Fatalf("api error = %+v", apiErr)
	}
	if apiErr.Fields["recipient.phone"] == "" {
		t.Fatalf("fields = %v", apiErr.Fields)
	}
}

func TestRateLimitRetryAfter(t *testing.T) {
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Retry-After", "7")
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusTooManyRequests)
		_, _ = w.Write([]byte(`{"error":{"code":"rate_limited","message":"Slow down."}}`))
	})

	_, err := client.CreateOrder(context.Background(), OrderRequest{})
	var apiErr *APIError
	if !errors.As(err, &apiErr) {
		t.Fatalf("error type = %T", err)
	}
	if apiErr.RetryAfter != 7*time.Second {
		t.Fatalf("retry after = %s", apiErr.RetryAfter)
	}
}

func TestReferenceDataIsCached(t *testing.T) {
	var calls atomic.Int32
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v1/wilayas":
			calls.Add(1)
			_, _ = w.Write([]byte(`[{"code":16,"nameFr":"Alger","nameAr":"الجزائر","courierSupported":true}]`))
		case "/v1/communes":
			_, _ = w.Write([]byte(`[{"wilayaCode":16,"nameFr":"Bab Ezzouar","nameAr":"باب الزوار"}]`))
		case "/v1/desks":
			_, _ = w.Write([]byte(`[{"id":"SBX-16","name":"Sandbox desk","wilayaCode":16,"communeName":"Bab Ezzouar"}]`))
		default:
			http.NotFound(w, r)
		}
	})

	first, err := client.Wilayas(context.Background())
	if err != nil || len(first) != 1 {
		t.Fatalf("wilayas = %v, %v", first, err)
	}
	second, err := client.Wilayas(context.Background())
	if err != nil || len(second) != 1 {
		t.Fatalf("cached wilayas = %v, %v", second, err)
	}
	if calls.Load() != 1 {
		t.Fatalf("wilaya calls = %d, want 1", calls.Load())
	}

	commune, err := client.Commune(context.Background(), 16, "bab ezzouar")
	if err != nil || commune != "Bab Ezzouar" {
		t.Fatalf("commune = %q, %v", commune, err)
	}
	desk, err := client.Desk(context.Background(), 16, "SBX-16")
	if err != nil || desk.Name != "Sandbox desk" {
		t.Fatalf("desk = %+v, %v", desk, err)
	}
}

func TestTrackUsesStoredCourierKey(t *testing.T) {
	var seen map[string]any
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&seen); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		_, _ = w.Write([]byte(`{"trackingNumber":"yal-ABC123","status":"in_transit","events":[{"status":"in_transit","rawStatus":"En transit","timestamp":"2026-11-11T10:00:00Z"}]}`))
	})

	tracking, err := client.Track(context.Background(), "yalidine", "yal-ABC123")
	if err != nil {
		t.Fatalf("track: %v", err)
	}
	if tracking.Status != "in_transit" || len(tracking.Events) != 1 {
		t.Fatalf("tracking = %+v", tracking)
	}
	if seen["courier"] != "yalidine" || seen["trackingNumber"] != "yal-ABC123" {
		t.Fatalf("payload = %v", seen)
	}
}

func TestSandboxFallsBackLocallyWhenTheAPIisUnavailable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	t.Cleanup(server.Close)

	client := New(Options{
		BaseURL:           server.URL,
		CourierKey:        "sandbox",
		FromWilaya:        16,
		AllowLocalSandbox: true,
		Timeout:           2 * time.Second,
	})

	wilayas, err := client.Wilayas(context.Background())
	if err != nil || len(wilayas) != 58 {
		t.Fatalf("fallback wilayas = %d, %v", len(wilayas), err)
	}
	communes, err := client.Communes(context.Background(), 16)
	if err != nil || len(communes) == 0 {
		t.Fatalf("fallback communes = %d, %v", len(communes), err)
	}
	desks, err := client.Desks(context.Background(), 16)
	if err != nil || len(desks) != 1 || desks[0].CommuneName == "" {
		t.Fatalf("fallback desks = %+v, %v", desks, err)
	}

	shipment, err := client.CreateOrder(context.Background(), OrderRequest{Reference: "AZ-2026-0041"})
	if err != nil {
		t.Fatalf("fallback shipment: %v", err)
	}
	if shipment.TrackingNumber != "SBX-AZ-2026-0041" {
		t.Fatalf("fallback tracking = %q", shipment.TrackingNumber)
	}

	tracking, err := client.Track(context.Background(), "sandbox", shipment.TrackingNumber)
	if err != nil || tracking.Status != "in_transit" || len(tracking.Events) != 3 {
		t.Fatalf("fallback tracking = %+v, %v", tracking, err)
	}
}

func TestRealCourierDoesNotUseTheLocalFallback(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	t.Cleanup(server.Close)

	client := New(Options{
		BaseURL:           server.URL,
		CourierKey:        "yalidine",
		AllowLocalSandbox: true,
		Timeout:           2 * time.Second,
	})
	if _, err := client.CreateOrder(context.Background(), OrderRequest{Reference: "AZ-2026-0041"}); err == nil {
		t.Fatal("a real courier must not create a local fake shipment")
	}
}
