package plugins_test

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"azahar/internal/plugins"
)

func TestProblemOmitsNilFields(t *testing.T) {
	recorder := httptest.NewRecorder()
	ctx := &plugins.Context{
		W: recorder,
		R: httptest.NewRequest(http.MethodGet, "/api/plugins/sample/items", nil),
	}
	ctx.Problem(http.StatusNotFound, "not_found", "Not found.", nil)

	body := recorder.Body.String()
	if strings.Contains(body, `"fields"`) {
		t.Fatalf("nil fields must be omitted like the core envelope: %s", body)
	}
	if !strings.Contains(body, `"code":"not_found"`) {
		t.Fatalf("unexpected envelope: %s", body)
	}
}

func TestProblemIncludesFieldMessages(t *testing.T) {
	recorder := httptest.NewRecorder()
	ctx := &plugins.Context{
		W: recorder,
		R: httptest.NewRequest(http.MethodPost, "/api/plugins/sample/items", nil),
	}
	ctx.Problem(http.StatusUnprocessableEntity, "validation_error", "Check the marked fields.",
		map[string]string{"imei": "Enter a valid IMEI."})

	body := recorder.Body.String()
	if !strings.Contains(body, `"fields":{"imei":"Enter a valid IMEI."}`) {
		t.Fatalf("field messages are missing: %s", body)
	}
}
