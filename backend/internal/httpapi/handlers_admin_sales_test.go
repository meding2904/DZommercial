package httpapi

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestCounterSaleRejectsAnEmptySale(t *testing.T) {
	server := newCSRFTestServer(t)
	request := httptest.NewRequest(http.MethodPost, "/api/admin/sales", strings.NewReader(`{"items":[]}`))
	request.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()

	server.handleCounterSale(recorder, request)

	if recorder.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusUnprocessableEntity)
	}
	var body struct {
		Error struct {
			Fields map[string]string `json:"fields"`
		} `json:"error"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if body.Error.Fields["items"] == "" {
		t.Fatalf("fields = %#v, want an items message", body.Error.Fields)
	}
}

func TestCounterSaleRejectsBadQuantitiesAndPhone(t *testing.T) {
	server := newCSRFTestServer(t)
	request := httptest.NewRequest(http.MethodPost, "/api/admin/sales",
		strings.NewReader(`{"phone":"123","items":[{"product_id":"prd_1","quantity":11}]}`))
	request.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()

	server.handleCounterSale(recorder, request)

	if recorder.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusUnprocessableEntity)
	}
	var body struct {
		Error struct {
			Fields map[string]string `json:"fields"`
		} `json:"error"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if body.Error.Fields["phone"] == "" || body.Error.Fields["items"] == "" {
		t.Fatalf("fields = %#v, want phone and items messages", body.Error.Fields)
	}
}
