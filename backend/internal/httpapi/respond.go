package httpapi

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"strings"
)

type apiProblem struct {
	Status  int
	Code    string
	Message string
	Fields  map[string]string
}

func (p *apiProblem) Error() string {
	return p.Message
}

func problem(status int, code, message string) *apiProblem {
	return &apiProblem{Status: status, Code: code, Message: message}
}

func problemFields(status int, code, message string, fields map[string]string) *apiProblem {
	return &apiProblem{Status: status, Code: code, Message: message, Fields: fields}
}

type errorEnvelope struct {
	Error errorPayload `json:"error"`
}

type errorPayload struct {
	Code    string            `json:"code"`
	Message string            `json:"message"`
	Fields  map[string]string `json:"fields,omitempty"`
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	if payload == nil {
		return
	}
	encoder := json.NewEncoder(w)
	encoder.SetEscapeHTML(true)
	_ = encoder.Encode(payload)
}

func writeProblem(w http.ResponseWriter, p *apiProblem) {
	writeJSON(w, p.Status, errorEnvelope{Error: errorPayload{
		Code:    p.Code,
		Message: p.Message,
		Fields:  p.Fields,
	}})
}

func writeError(w http.ResponseWriter, status int, code, message string) {
	writeProblem(w, problem(status, code, message))
}

func decodeJSON[T any](w http.ResponseWriter, r *http.Request) (T, map[string]json.RawMessage, *apiProblem) {
	var zero T

	contentType := r.Header.Get("Content-Type")
	mediaType, _, err := mime.ParseMediaType(contentType)
	if err != nil || mediaType != "application/json" {
		return zero, nil, problem(415, "unsupported_media_type", "Send the request body as application/json.")
	}

	body, err := io.ReadAll(r.Body)
	if err != nil {
		var maxErr *http.MaxBytesError
		if errors.As(err, &maxErr) {
			return zero, nil, problem(413, "payload_too_large", "The request body is too large.")
		}
		return zero, nil, problem(400, "invalid_json", "The request body could not be read.")
	}
	if len(bytes.TrimSpace(body)) == 0 {
		return zero, nil, problemFields(422, "validation_error", "Check the marked fields.",
			map[string]string{"body": "Send a JSON object."})
	}

	var raw map[string]json.RawMessage
	if err := json.Unmarshal(body, &raw); err != nil || raw == nil {
		return zero, nil, problem(422, "invalid_json", "The request body is not valid JSON.")
	}

	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	var payload T
	if err := decoder.Decode(&payload); err != nil {
		if strings.Contains(err.Error(), "unknown field") {
			return zero, nil, problem(422, "validation_error", "The request body contains an unknown field.")
		}
		return zero, nil, problem(422, "invalid_json", "The request body does not match the expected shape.")
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return zero, nil, problem(422, "invalid_json", "Send one JSON object.")
	}
	return payload, raw, nil
}

func clientIP(r *http.Request, trustProxy bool) string {
	if trustProxy {
		forwarded := r.Header.Get("X-Forwarded-For")
		if forwarded != "" {
			first := strings.TrimSpace(strings.Split(forwarded, ",")[0])
			if first != "" {
				return first
			}
		}
	}
	host := r.RemoteAddr
	if index := strings.LastIndex(host, ":"); index > 0 {
		host = host[:index]
	}
	return strings.Trim(host, "[]")
}
