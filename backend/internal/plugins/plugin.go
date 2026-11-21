// Package plugins is the extension surface of the storefront framework.
//
// A plugin contributes a manifest, optional routes, optional database migrations,
// an optional product option schema, and optional checkout and order hooks. The
// core catalog, checkout and admin know only this interface, never a plugin's
// implementation.
package plugins

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"regexp"
	"strings"
	"time"

	"azahar/internal/store"
)

var (
	ErrUnreadable = errors.New("plugin settings cannot be decrypted with the current PLUGIN_SECRET_KEY")
	idPattern     = regexp.MustCompile(`^[a-z][a-z0-9_-]{1,31}$`)
	keyPattern    = regexp.MustCompile(`^[a-z][a-z0-9_]{1,31}$`)
)

type OptionField struct {
	Key         string   `json:"key"`
	Label       string   `json:"label"`
	Type        string   `json:"type"`
	Required    bool     `json:"required"`
	Help        string   `json:"help,omitempty"`
	Suggestions []string `json:"suggestions,omitempty"`
}

type ProductType struct {
	Label   string        `json:"label"`
	Summary string        `json:"summary,omitempty"`
	Fields  []OptionField `json:"fields"`
}

type SettingOption struct {
	Value string `json:"value"`
	Label string `json:"label"`
}

type SettingField struct {
	Key         string          `json:"key"`
	Label       string          `json:"label"`
	Type        string          `json:"type"`
	Help        string          `json:"help,omitempty"`
	Placeholder string          `json:"placeholder,omitempty"`
	Options     []SettingOption `json:"options,omitempty"`
	Required    bool            `json:"required"`
	MaxLength   int             `json:"max_length,omitempty"`
	Default     string          `json:"default,omitempty"`
}

type Category struct {
	Slug        string `json:"slug"`
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	SortOrder   int    `json:"sort_order,omitempty"`
}

type StorefrontPage struct {
	Path      string `json:"path"`
	Label     string `json:"label"`
	SortOrder int    `json:"sort_order,omitempty"`
}

type Manifest struct {
	ID              string           `json:"id"`
	Name            string           `json:"name"`
	Version         string           `json:"version"`
	Description     string           `json:"description"`
	Author          string           `json:"author,omitempty"`
	Dependencies    []string         `json:"dependencies,omitempty"`
	ProductType     *ProductType     `json:"product_type,omitempty"`
	Categories      []Category       `json:"categories,omitempty"`
	StorefrontPages []StorefrontPage `json:"storefront_pages,omitempty"`
	Settings        []SettingField   `json:"settings,omitempty"`
	AdminPath       string           `json:"admin_path,omitempty"`
}

type Route struct {
	Method  string
	Pattern string
	Admin   bool
	Handler func(*Context)
}

type Migration struct {
	Version string
	SQL     string
}

type Plugin interface {
	Manifest() Manifest
	Routes() []Route
}

type Migrator interface {
	Migrations() []Migration
}

type EnvAware interface {
	Init(*Env)
}

type ProductEnricher interface {
	EnrichProduct(ctx context.Context, product store.Product) (map[string]any, error)
}

type CheckoutLine struct {
	Product  store.Product
	Options  map[string]string
	Quantity int
	OrderID  string
	ItemID   string
}

type OrderRef struct {
	ID     string
	Number string
	Status string
}

type CheckoutHook interface {
	ValidateCheckoutLine(ctx context.Context, tx *sql.Tx, line CheckoutLine) *Problem
	OnOrderCreated(ctx context.Context, tx *sql.Tx, order OrderRef, lines []CheckoutLine) error
}

type OrderStatusHook interface {
	OnOrderStatusChanged(ctx context.Context, tx *sql.Tx, orderID, from, to string) error
}

type SettingsValidator interface {
	ValidateSettings(settings map[string]string) map[string]string
}

type Env struct {
	DB            *sql.DB
	Store         *store.Store
	Logger        *slog.Logger
	SecretKey     string
	AIBaseURL     string
	AITimeout     time.Duration
	IsDevelopment bool
	Actor         func(*http.Request) (id, role string)
	RequestID     func(*http.Request) string
}

type Context struct {
	W        http.ResponseWriter
	R        *http.Request
	Env      *Env
	PluginID string
	UserID   string
	UserRole string
}

type errorEnvelope struct {
	Error errorPayload `json:"error"`
}

type errorPayload struct {
	Code    string            `json:"code"`
	Message string            `json:"message"`
	Fields  map[string]string `json:"fields,omitempty"`
}

func (c *Context) IsAdmin() bool {
	return c.UserRole == "admin"
}

func (c *Context) JSON(status int, payload any) {
	writeJSON(c.W, status, payload)
}

func (c *Context) Error(status int, code, message string) {
	c.Problem(status, code, message, nil)
}

func (c *Context) Problem(status int, code, message string, fields map[string]string) {
	writeJSON(c.W, status, errorEnvelope{Error: errorPayload{
		Code:    code,
		Message: message,
		Fields:  fields,
	}})
}

func (c *Context) Settings(ctx context.Context) (map[string]string, error) {
	return c.Env.Settings(ctx, c.PluginID)
}

type Problem struct {
	Status  int
	Code    string
	Message string
	Fields  map[string]string
}

func NewProblem(status int, code, message string, fields map[string]string) *Problem {
	return &Problem{Status: status, Code: code, Message: message, Fields: fields}
}

func DecodeBody[T any](c *Context) (T, *Problem) {
	var zero T
	contentType := c.R.Header.Get("Content-Type")
	if !strings.HasPrefix(strings.ToLower(contentType), "application/json") {
		return zero, NewProblem(http.StatusUnsupportedMediaType, "unsupported_media_type",
			"Send the request body as application/json.", nil)
	}
	body, err := io.ReadAll(io.LimitReader(c.R.Body, 1<<20+1))
	if err != nil {
		return zero, NewProblem(http.StatusBadRequest, "invalid_json",
			"The request body could not be read.", nil)
	}
	if len(body) > 1<<20 {
		return zero, NewProblem(http.StatusRequestEntityTooLarge, "payload_too_large",
			"The request body is too large.", nil)
	}
	if len(strings.TrimSpace(string(body))) == 0 {
		return zero, NewProblem(http.StatusUnprocessableEntity, "validation_error",
			"Check the marked fields.", map[string]string{"body": "Send a JSON object."})
	}
	decoder := json.NewDecoder(strings.NewReader(string(body)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&zero); err != nil {
		return zero, NewProblem(http.StatusUnprocessableEntity, "invalid_json",
			"The request body does not match the expected shape.", nil)
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return zero, NewProblem(http.StatusUnprocessableEntity, "invalid_json",
			"Send one JSON object.", nil)
	}
	return zero, nil
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
