// Command oracle-plugin-context exposes the Go plugin SDK context helpers for
// Rust differential tests.
//
//	go run ./cmd/oracle-plugin-context
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net/http/httptest"
	"os"
	"strings"

	"azahar/internal/plugins"
)

type request struct {
	Op          string            `json:"op"`
	Status      int               `json:"status"`
	Code        string            `json:"code"`
	Message     string            `json:"message"`
	Fields      map[string]string `json:"fields"`
	Role        string            `json:"role"`
	Payload     json.RawMessage   `json:"payload"`
	ContentType string            `json:"content_type"`
	Body        string            `json:"body"`
}

type testPayload struct {
	Name string `json:"name"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 4<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var request request
		if err := json.Unmarshal(scanner.Bytes(), &request); err != nil {
			_ = encoder.Encode(map[string]any{"error": err.Error()})
			continue
		}
		recorder := httptest.NewRecorder()
		ctx := &plugins.Context{W: recorder, UserRole: request.Role}
		response := map[string]any{}
		switch request.Op {
		case "is_admin":
			response["ok"] = ctx.IsAdmin()
		case "json":
			var payload any
			if len(request.Payload) > 0 {
				_ = json.Unmarshal(request.Payload, &payload)
			}
			ctx.JSON(request.Status, payload)
			response["status"] = recorder.Code
			response["body"] = recorder.Body.String()
			response["content_type"] = recorder.Header().Get("Content-Type")
		case "error":
			ctx.Error(request.Status, request.Code, request.Message)
			response["status"] = recorder.Code
			response["body"] = recorder.Body.String()
		case "problem":
			ctx.Problem(request.Status, request.Code, request.Message, request.Fields)
			response["status"] = recorder.Code
			response["body"] = recorder.Body.String()
		case "decode":
			ctx.R = httptest.NewRequest("POST", "/", nil)
			ctx.R.Header.Set("Content-Type", request.ContentType)
			ctx.R.Body = io.NopCloser(strings.NewReader(request.Body))
			value, problem := plugins.DecodeBody[testPayload](ctx)
			if problem != nil {
				response["problem"] = map[string]any{
					"status": problem.Status, "code": problem.Code,
					"message": problem.Message, "fields": problem.Fields,
				}
			} else {
				response["value"] = value
			}
		default:
			response["error"] = fmt.Sprintf("unknown op %q", request.Op)
		}
		_ = encoder.Encode(response)
	}
}
