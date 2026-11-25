// Command oracle-store-basics is a line-oriented differential oracle for the
// Rust ports of the store pricing rules, users, sessions, settings,
// plugin-states and audit functions.
//
// It reads one JSON request per line from stdin and writes one JSON response
// per line to stdout. A malformed request or a store error produces an error
// response and does not stop the loop. The database defaults to the local
// libSQL container; set AZAHAR_TEST_DATABASE_URL to point somewhere else.
package main

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	"azahar/internal/db"
	"azahar/internal/store"
)

const defaultDSN = "http://127.0.0.1:8880"

type request struct {
	Op       string        `json:"op"`
	Cases    []pricingCase `json:"cases"`
	User     *userInput    `json:"user"`
	At       string        `json:"at"`
	Email    string        `json:"email"`
	ID       string        `json:"id"`
	Role     string        `json:"role"`
	Now      string        `json:"now"`
	Preset   string        `json:"preset"`
	Session  *sessionInput `json:"session"`
	Hash     string        `json:"hash"`
	UserID   string        `json:"user_id"`
	Agent    string        `json:"user_agent"`
	PluginID string        `json:"plugin_id"`
	Payload  string        `json:"payload"`
	Enabled  *bool         `json:"enabled"`
	Entry    *auditEntryIn `json:"entry"`
}

type pricingCase struct {
	PriceCents        *int64  `json:"price_cents"`
	StartsAt          *string `json:"starts"`
	EndsAt            *string `json:"ends"`
	RegularPriceCents int64   `json:"regular_price_cents"`
	Now               string  `json:"now"`
}

type pricingResult struct {
	Problems  []int `json:"problems"`
	Valid     bool  `json:"valid"`
	Active    bool  `json:"active"`
	Effective int64 `json:"effective"`
}

type userInput struct {
	ID           string `json:"id"`
	Name         string `json:"name"`
	Email        string `json:"email"`
	PasswordHash string `json:"password_hash"`
	Role         string `json:"role"`
	ThemePreset  string `json:"theme_preset"`
	CreatedAt    string `json:"created_at"`
	UpdatedAt    string `json:"updated_at"`
}

type sessionInput struct {
	ID        string `json:"id"`
	UserID    string `json:"user_id"`
	TokenHash string `json:"token_hash"`
	UserAgent string `json:"user_agent"`
	IP        string `json:"ip"`
	CreatedAt string `json:"created_at"`
	ExpiresAt string `json:"expires_at"`
}

type auditEntryIn struct {
	ID          string `json:"id"`
	ActorUserID string `json:"actor_user_id"`
	Action      string `json:"action"`
	EntityType  string `json:"entity_type"`
	EntityID    string `json:"entity_id"`
	Detail      string `json:"detail"`
	CreatedAt   string `json:"created_at"`
}

type userOut struct {
	ID           string `json:"id"`
	Name         string `json:"name"`
	Email        string `json:"email"`
	PasswordHash string `json:"password_hash"`
	Role         string `json:"role"`
	ThemePreset  string `json:"theme_preset"`
	CreatedAt    string `json:"created_at"`
	UpdatedAt    string `json:"updated_at"`
}

type sessionOut struct {
	ID        string `json:"id"`
	UserID    string `json:"user_id"`
	TokenHash string `json:"token_hash"`
	UserAgent string `json:"user_agent"`
	IP        string `json:"ip"`
	CreatedAt string `json:"created_at"`
	ExpiresAt string `json:"expires_at"`
}

type customerOut struct {
	ID              string  `json:"id"`
	Name            string  `json:"name"`
	Email           string  `json:"email"`
	OrdersCount     int     `json:"orders_count"`
	TotalSpentCents int64   `json:"total_spent_cents"`
	LastOrderAt     *string `json:"last_order_at"`
	CreatedAt       string  `json:"created_at"`
}

func optionalTime(value *string) time.Time {
	if value == nil {
		return time.Time{}
	}
	return store.ParseTime(*value)
}

func fallback(value string, fallback time.Time) time.Time {
	if value == "" {
		return fallback
	}
	return store.ParseTime(value)
}

func toUser(input userInput, at time.Time) store.User {
	return store.User{
		ID:           input.ID,
		Name:         input.Name,
		Email:        input.Email,
		PasswordHash: input.PasswordHash,
		Role:         input.Role,
		ThemePreset:  input.ThemePreset,
		CreatedAt:    fallback(input.CreatedAt, at),
		UpdatedAt:    fallback(input.UpdatedAt, at),
	}
}

func userJSON(user store.User) userOut {
	return userOut{
		ID:           user.ID,
		Name:         user.Name,
		Email:        user.Email,
		PasswordHash: user.PasswordHash,
		Role:         user.Role,
		ThemePreset:  user.ThemePreset,
		CreatedAt:    store.FormatTime(user.CreatedAt),
		UpdatedAt:    store.FormatTime(user.UpdatedAt),
	}
}

func sessionJSON(session store.Session) sessionOut {
	return sessionOut{
		ID:        session.ID,
		UserID:    session.UserID,
		TokenHash: session.TokenHash,
		UserAgent: session.UserAgent,
		IP:        session.IP,
		CreatedAt: store.FormatTime(session.CreatedAt),
		ExpiresAt: store.FormatTime(session.ExpiresAt),
	}
}

func sessionFromInput(input sessionInput, at time.Time) store.Session {
	return store.Session{
		ID:        input.ID,
		UserID:    input.UserID,
		TokenHash: input.TokenHash,
		UserAgent: input.UserAgent,
		IP:        input.IP,
		CreatedAt: fallback(input.CreatedAt, at),
		ExpiresAt: fallback(input.ExpiresAt, at),
	}
}

func evaluatePricing(c pricingCase) pricingResult {
	starts := optionalTime(c.StartsAt)
	ends := optionalTime(c.EndsAt)
	now := store.ParseTime(c.Now)
	window := store.SaleWindow{PriceCents: c.PriceCents, StartsAt: starts, EndsAt: ends}
	product := store.Product{
		PriceCents:     c.RegularPriceCents,
		SalePriceCents: c.PriceCents,
		SaleStartsAt:   starts,
		SaleEndsAt:     ends,
	}

	problems := window.Problems(c.RegularPriceCents)
	// A Go nil slice would marshal as null; the Rust port returns an empty
	// vector. Both are observably the same "no problems".
	out := make([]int, len(problems))
	for index, problem := range problems {
		out[index] = int(problem)
	}
	return pricingResult{
		Problems:  out,
		Valid:     window.Valid(c.RegularPriceCents),
		Active:    window.Active(now, c.RegularPriceCents),
		Effective: product.EffectivePriceCents(now),
	}
}

func handle(ctx context.Context, repo *store.Store, req request) (any, error) {
	at := store.ParseTime(req.At)
	if at.IsZero() {
		at = time.Now().UTC()
	}

	switch req.Op {
	case "pricing":
		results := make([]pricingResult, 0, len(req.Cases))
		for _, c := range req.Cases {
			results = append(results, evaluatePricing(c))
		}
		return results, nil

	case "create_user":
		if req.User == nil {
			return nil, errors.New("create_user: missing user")
		}
		user := toUser(*req.User, at)
		if err := repo.CreateUser(ctx, repo.DB(), user); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "user_by_email":
		user, err := repo.UserByEmail(ctx, req.Email)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return map[string]any{"not_found": true}, nil
			}
			return nil, err
		}
		return userJSON(user), nil

	case "user_by_id":
		user, err := repo.UserByID(ctx, req.ID)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return map[string]any{"not_found": true}, nil
			}
			return nil, err
		}
		return userJSON(user), nil

	case "list_users":
		users, err := repo.ListUsers(ctx)
		if err != nil {
			return nil, err
		}
		out := make([]userOut, 0, len(users))
		for _, user := range users {
			out = append(out, userJSON(user))
		}
		return map[string]any{"users": out}, nil

	case "update_user_role":
		if err := repo.UpdateUserRole(ctx, repo.DB(), req.ID, req.Role, store.ParseTime(req.Now)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "count_users":
		count, err := repo.CountUsers(ctx)
		if err != nil {
			return nil, err
		}
		return map[string]any{"count": count}, nil

	case "count_admins":
		count, err := repo.CountAdmins(ctx, repo.DB())
		if err != nil {
			return nil, err
		}
		return map[string]any{"count": count}, nil

	case "update_theme":
		if err := repo.UpdateTheme(ctx, repo.DB(), req.ID, req.Preset, store.ParseTime(req.Now)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "list_customers":
		rows, err := repo.ListCustomers(ctx)
		if err != nil {
			return nil, err
		}
		out := make([]customerOut, 0, len(rows))
		for _, row := range rows {
			var lastOrderAt *string
			if row.LastOrderAt != nil {
				formatted := store.FormatTime(*row.LastOrderAt)
				lastOrderAt = &formatted
			}
			out = append(out, customerOut{
				ID:              row.ID,
				Name:            row.Name,
				Email:           row.Email,
				OrdersCount:     row.OrdersCount,
				TotalSpentCents: row.TotalSpentCents,
				LastOrderAt:     lastOrderAt,
				CreatedAt:       store.FormatTime(row.CreatedAt),
			})
		}
		return map[string]any{"customers": out}, nil

	case "create_session":
		if req.Session == nil {
			return nil, errors.New("create_session: missing session")
		}
		if err := repo.CreateSession(ctx, repo.DB(), sessionFromInput(*req.Session, at)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "session_user_by_hash":
		user, session, found, err := repo.SessionUserByHash(ctx, req.Hash)
		if err != nil {
			return nil, err
		}
		if !found {
			return map[string]any{"found": false}, nil
		}
		return map[string]any{
			"found":   true,
			"user":    userJSON(user),
			"session": sessionJSON(session),
		}, nil

	case "delete_session_by_hash":
		if err := repo.DeleteSessionByHash(ctx, repo.DB(), req.Hash); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "delete_sessions_for_user_agent":
		if err := repo.DeleteSessionsForUserAgent(ctx, repo.DB(), req.UserID, req.Agent); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "delete_expired_sessions":
		if err := repo.DeleteExpiredSessions(ctx, repo.DB(), store.ParseTime(req.Now)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "plugin_settings":
		raw, updatedAt, err := repo.PluginSettings(ctx, req.PluginID)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return map[string]any{"found": false}, nil
			}
			return nil, err
		}
		return map[string]any{
			"found":      true,
			"settings":   raw,
			"updated_at": store.FormatTime(updatedAt),
		}, nil

	case "save_plugin_settings":
		if err := repo.SavePluginSettings(ctx, repo.DB(), req.PluginID, req.Payload, store.ParseTime(req.Now)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "storefront_settings":
		raw, updatedAt, err := repo.StorefrontSettings(ctx)
		if err != nil {
			if errors.Is(err, sql.ErrNoRows) {
				return map[string]any{"found": false}, nil
			}
			return nil, err
		}
		return map[string]any{
			"found":      true,
			"settings":   raw,
			"updated_at": store.FormatTime(updatedAt),
		}, nil

	case "save_storefront_settings":
		if err := repo.SaveStorefrontSettings(ctx, repo.DB(), req.Payload, store.ParseTime(req.Now)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "plugin_states":
		states, err := repo.PluginStates(ctx)
		if err != nil {
			return nil, err
		}
		return map[string]any{"states": states}, nil

	case "set_plugin_state":
		enabled := false
		if req.Enabled != nil {
			enabled = *req.Enabled
		}
		if err := repo.SetPluginState(ctx, repo.DB(), req.PluginID, enabled, store.ParseTime(req.Now)); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "insert_audit":
		if req.Entry == nil {
			return nil, errors.New("insert_audit: missing entry")
		}
		entry := store.AuditEntry{
			ID:          req.Entry.ID,
			ActorUserID: req.Entry.ActorUserID,
			Action:      req.Entry.Action,
			EntityType:  req.Entry.EntityType,
			EntityID:    req.Entry.EntityID,
			Detail:      req.Entry.Detail,
			CreatedAt:   store.ParseTime(req.Entry.CreatedAt),
		}
		if err := repo.InsertAudit(ctx, repo.DB(), entry); err != nil {
			return nil, err
		}
		return struct{}{}, nil

	case "close":
		return struct{}{}, nil

	default:
		return nil, fmt.Errorf("unknown op %q", req.Op)
	}
}

func main() {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		dsn = defaultDSN
	}

	ctx := context.Background()
	handleDB, err := db.Open(ctx, dsn)
	if err != nil {
		fmt.Fprintf(os.Stderr, "oracle-store-basics: %v\n", err)
		os.Exit(1)
	}
	defer handleDB.Close()
	repo := store.New(handleDB)

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 8*1024*1024)
	writer := bufio.NewWriter(os.Stdout)
	defer writer.Flush()

	write := func(value any) bool {
		encoded, err := json.Marshal(value)
		if err != nil {
			encoded, _ = json.Marshal(map[string]any{"error": "encode response: " + err.Error()})
		}
		if _, err := writer.Write(append(encoded, '\n')); err != nil {
			return false
		}
		return writer.Flush() == nil
	}

	for scanner.Scan() {
		line := scanner.Bytes()
		var req request
		if err := json.Unmarshal(line, &req); err != nil {
			if !write(map[string]any{"error": "invalid JSON: " + err.Error()}) {
				return
			}
			continue
		}

		requestCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
		resp, err := func() (out any, failure error) {
			defer func() {
				if recovered := recover(); recovered != nil {
					out = nil
					failure = fmt.Errorf("panic: %v", recovered)
				}
			}()
			return handle(requestCtx, repo, req)
		}()
		cancel()
		if err != nil {
			resp = map[string]any{"error": err.Error()}
		}
		if !write(resp) {
			return
		}
		if req.Op == "close" {
			return
		}
	}

	if err := scanner.Err(); err != nil {
		_ = write(map[string]any{"error": "read stdin: " + err.Error()})
	}
}
