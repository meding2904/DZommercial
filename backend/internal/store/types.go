package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"time"
)

type User struct {
	ID           string
	Name         string
	Email        string
	PasswordHash string
	Role         string
	ThemePreset  string
	CreatedAt    time.Time
	UpdatedAt    time.Time
}

type Session struct {
	ID        string
	UserID    string
	TokenHash string
	UserAgent string
	IP        string
	CreatedAt time.Time
	ExpiresAt time.Time
}

type Category struct {
	ID          string
	Slug        string
	Name        string
	Description string
	SortOrder   int
}

type Product struct {
	ID             string
	Slug           string
	Name           string
	Subtitle       string
	Description    string
	PriceCents     int64
	CompareAtCents *int64
	Currency       string
	Size           string
	Category       Category
	Concerns       []string
	Ingredients    []string
	HowTo          string
	Image          string
	Gallery        []string
	Stock          int
	Featured       bool
	Active         bool
	CreatedAt      time.Time
	UpdatedAt      time.Time
}

type OrderItem struct {
	ID             string
	OrderID        string
	ProductID      string
	Slug           string
	Name           string
	Image          string
	UnitPriceCents int64
	Quantity       int
	LineTotalCents int64
}

type Order struct {
	ID            string
	Number        string
	UserID        *string
	Name          string
	Email         string
	Phone         string
	Address       string
	City          string
	Notes         string
	SubtotalCents int64
	ShippingCents int64
	TotalCents    int64
	Currency      string
	Status        string
	TokenHash     string
	CreatedAt     time.Time
	UpdatedAt     time.Time
}

type CustomerRow struct {
	ID              string
	Name            string
	Email           string
	OrdersCount     int
	TotalSpentCents int64
	LastOrderAt     *time.Time
	CreatedAt       time.Time
}

type AuditEntry struct {
	ID          string
	ActorUserID string
	Action      string
	EntityType  string
	EntityID    string
	Detail      string
	CreatedAt   time.Time
}

func FormatTime(t time.Time) string {
	return t.UTC().Format(time.RFC3339)
}

func ParseTime(value string) time.Time {
	if value == "" {
		return time.Time{}
	}
	if parsed, err := time.Parse(time.RFC3339, value); err == nil {
		return parsed.UTC()
	}
	if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return parsed.UTC()
	}
	return time.Time{}
}

func NullTime(value string) *time.Time {
	parsed := ParseTime(value)
	if parsed.IsZero() {
		return nil
	}
	return &parsed
}

func encodeJSON(values []string) string {
	if values == nil {
		values = []string{}
	}
	buf, err := json.Marshal(values)
	if err != nil {
		return "[]"
	}
	return string(buf)
}

func decodeJSON(value string) []string {
	if value == "" {
		return []string{}
	}
	var values []string
	if err := json.Unmarshal([]byte(value), &values); err != nil {
		return []string{}
	}
	if values == nil {
		return []string{}
	}
	return values
}

type scanner interface {
	Scan(dest ...any) error
}

type querier interface {
	ExecContext(ctx context.Context, query string, args ...any) (sql.Result, error)
	QueryContext(ctx context.Context, query string, args ...any) (*sql.Rows, error)
	QueryRowContext(ctx context.Context, query string, args ...any) *sql.Row
}
