package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"path"
	"strings"
	"time"
)

const (
	defaultRequestTimeout = 20 * time.Second
	defaultImageTimeout   = 15 * time.Second
	maxRequestBodyBytes   = 2 << 20
	maxResponseBytes      = 16 << 20
)

var allowedMethods = map[string]bool{
	http.MethodGet:    true,
	http.MethodHead:   true,
	http.MethodPost:   true,
	http.MethodPut:    true,
	http.MethodPatch:  true,
	http.MethodDelete: true,
}

type requestOptions struct {
	method        string
	target        *url.URL
	generation    uint64
	payload       []byte
	timeout       time.Duration
	includeOrigin bool
	includeCSRF   bool
	maxBytes      int64
}

type httpResult struct {
	status      int
	body        []byte
	contentType string
}

func (c *Client) Request(method, path string, body any) Response {
	if isCSRFEndpoint(path) {
		return Response{OK: false, Error: invalidPath()}
	}
	data, requestErr := c.request(method, path, body)
	if requestErr != nil {
		return Response{OK: false, Error: requestErr}
	}
	return Response{OK: true, Data: data}
}

func isCSRFEndpoint(raw string) bool {
	parsed, err := validateAPIPath(raw)
	if err != nil {
		return false
	}
	cleaned := path.Clean(parsed.Path)
	rest := strings.TrimPrefix(cleaned, "/api/")
	segment, _, _ := strings.Cut(rest, "/")
	if index := strings.IndexByte(segment, ';'); index >= 0 {
		segment = segment[:index]
	}
	segment = strings.TrimRight(segment, ". ")
	return strings.EqualFold(segment, "csrf")
}

func (c *Client) request(method, path string, body any) (any, *Error) {
	method = strings.ToUpper(strings.TrimSpace(method))
	if method == "" {
		method = http.MethodGet
	}
	if !allowedMethods[method] {
		return nil, newError(0, "invalid_method", "That request method is not allowed.")
	}

	target, generation, err := c.targetURL(path)
	if err != nil {
		return nil, err
	}

	payload, bodyErr := encodeRequestBody(method, body)
	if bodyErr != nil {
		return nil, bodyErr
	}

	safe := method == http.MethodGet || method == http.MethodHead
	if !safe {
		if originErr := c.ensureOrigin(); originErr != nil {
			return nil, originErr
		}
		if csrfErr := c.ensureCSRF(); csrfErr != nil {
			return nil, csrfErr
		}
	}

	result, requestErr := c.execute(requestOptions{
		method:        method,
		target:        target,
		generation:    generation,
		payload:       payload,
		timeout:       c.timeout,
		includeOrigin: true,
		includeCSRF:   true,
		maxBytes:      maxResponseBytes,
	})
	if requestErr != nil {
		return nil, requestErr
	}

	if !safe && result.status == http.StatusForbidden && errorCode(result.body) == "csrf_failed" {
		target, retryGeneration, err := c.targetURL(path)
		if err != nil {
			return nil, err
		}
		if retryGeneration != generation {
			return nil, settingsChangedError()
		}
		c.clearCSRF()
		if csrfErr := c.ensureCSRF(); csrfErr != nil {
			return nil, csrfErr
		}
		result, requestErr = c.execute(requestOptions{
			method:        method,
			target:        target,
			generation:    generation,
			payload:       payload,
			timeout:       c.timeout,
			includeOrigin: true,
			includeCSRF:   true,
			maxBytes:      maxResponseBytes,
		})
		if requestErr != nil {
			return nil, requestErr
		}
	}

	if result.status < 200 || result.status > 299 {
		return nil, errorFromResult(result)
	}
	return decodeJSON(result.body), nil
}

func encodeRequestBody(method string, body any) ([]byte, *Error) {
	if method == http.MethodGet || method == http.MethodHead {
		return nil, nil
	}
	if body != nil {
		payload, err := json.Marshal(body)
		if err != nil {
			return nil, newError(0, "invalid_body", "The request body could not be sent.")
		}
		if len(payload) > maxRequestBodyBytes {
			return nil, newError(0, "payload_too_large", "The request body is too large.")
		}
		return payload, nil
	}
	return []byte("{}"), nil
}

func (c *Client) execute(opts requestOptions) (httpResult, *Error) {
	now := time.Now()
	c.mu.RLock()
	if c.generation != opts.generation {
		c.mu.RUnlock()
		return httpResult{}, settingsChangedError()
	}
	headers := c.requestHeadersLocked(now, opts.includeOrigin, opts.includeCSRF, opts.payload != nil)
	c.mu.RUnlock()

	ctx, cancel := context.WithTimeout(context.Background(), opts.timeout)
	defer cancel()

	request, err := http.NewRequestWithContext(ctx, opts.method, opts.target.String(), bytes.NewReader(opts.payload))
	if err != nil {
		return httpResult{}, newError(0, "invalid_request", "The request could not be prepared.")
	}
	request.Header = headers

	response, err := c.httpClient.Do(request)
	if err != nil {
		return httpResult{}, networkFailure(err, ctx)
	}
	defer response.Body.Close()

	c.absorbCookies(response, opts.generation)

	if response.ContentLength > opts.maxBytes {
		return httpResult{}, responseTooLarge()
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, opts.maxBytes+1))
	if err != nil {
		return httpResult{}, networkFailure(err, ctx)
	}
	if int64(len(data)) > opts.maxBytes {
		return httpResult{}, responseTooLarge()
	}

	return httpResult{
		status:      response.StatusCode,
		body:        data,
		contentType: response.Header.Get("Content-Type"),
	}, nil
}

func (c *Client) requestHeadersLocked(now time.Time, includeOrigin, includeCSRF, hasBody bool) http.Header {
	headers := http.Header{}
	headers.Set("Accept", "application/json")
	headers.Set("User-Agent", c.userAgent)
	headers.Set("X-Requested-With", "azahar-web")

	var cookies []string
	if c.session.active(now) {
		cookies = append(cookies, sessionCookieName+"="+c.session.value)
	}
	if c.csrf.active(now) {
		cookies = append(cookies, csrfCookieName+"="+c.csrf.value)
		if includeCSRF {
			headers.Set("X-CSRF-Token", c.csrf.value)
		}
	}
	if len(cookies) > 0 {
		headers.Set("Cookie", strings.Join(cookies, "; "))
	}
	if includeOrigin {
		origin := c.settings.Origin
		if origin == "" {
			origin = c.resolvedOrigin
		}
		if origin != "" {
			headers.Set("Origin", origin)
		}
	}
	if hasBody {
		headers.Set("Content-Type", "application/json")
	}
	return headers
}

func (c *Client) ensureCSRF() *Error {
	if c.activeCSRF() != "" {
		return nil
	}
	target, generation, err := c.targetURL("/api/csrf")
	if err != nil {
		return err
	}
	result, requestErr := c.execute(requestOptions{
		method:        http.MethodGet,
		target:        target,
		generation:    generation,
		timeout:       c.timeout,
		includeOrigin: true,
		includeCSRF:   false,
		maxBytes:      maxResponseBytes,
	})
	if requestErr != nil {
		return requestErr
	}
	if result.status < 200 || result.status > 299 {
		return newError(result.status, "csrf_unavailable", "The API did not issue a security token. Try again.")
	}
	if c.activeCSRF() != "" {
		return nil
	}

	var body struct {
		Token string `json:"csrf_token"`
	}
	if json.Unmarshal(result.body, &body) == nil {
		token := strings.TrimSpace(body.Token)
		if validCookieValue(token) {
			c.storeCSRF(token, generation)
			return nil
		}
	}
	return newError(result.status, "csrf_unavailable", "The API did not issue a security token. Try again.")
}

func errorFromResult(result httpResult) *Error {
	code := "request_failed"
	message := "The API returned an error. Try again."
	fields := map[string]string{}

	var envelope struct {
		Error *struct {
			Code    string            `json:"code"`
			Message string            `json:"message"`
			Fields  map[string]string `json:"fields"`
		} `json:"error"`
	}
	if json.Unmarshal(result.body, &envelope) == nil && envelope.Error != nil {
		if value := strings.TrimSpace(envelope.Error.Code); value != "" {
			code = value
		}
		if value := strings.TrimSpace(envelope.Error.Message); value != "" {
			message = value
		}
		if envelope.Error.Fields != nil {
			fields = envelope.Error.Fields
		}
	}
	return &Error{Status: result.status, Code: code, Message: message, Fields: fields}
}

func errorCode(body []byte) string {
	var envelope struct {
		Error *struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if json.Unmarshal(body, &envelope) != nil || envelope.Error == nil {
		return ""
	}
	return envelope.Error.Code
}

func decodeJSON(body []byte) any {
	if len(body) == 0 {
		return nil
	}
	var value any
	if json.Unmarshal(body, &value) != nil {
		return nil
	}
	return value
}

func responseTooLarge() *Error {
	return newError(0, "response_too_large", "The API response was too large.")
}

func settingsChangedError() *Error {
	return newError(0, "settings_changed", "The server settings changed. Try again.")
}

func networkFailure(err error, ctx context.Context) *Error {
	message := "The API did not answer. Check the address and that the service is running."
	if errors.Is(err, context.DeadlineExceeded) || errors.Is(ctx.Err(), context.DeadlineExceeded) || isTimeout(err) {
		message = "The API did not answer in time. Check the service and try again."
	}
	return newError(0, "network_error", message)
}

func isTimeout(err error) bool {
	var timeout interface{ Timeout() bool }
	return errors.As(err, &timeout) && timeout.Timeout()
}
