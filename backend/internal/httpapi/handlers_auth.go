package httpapi

import (
	"database/sql"
	"errors"
	"net/http"
	"strings"
	"time"

	"azahar/internal/auth"
	"azahar/internal/ids"
	"azahar/internal/store"
)

type registerPayload struct {
	Name     string `json:"name"`
	Email    string `json:"email"`
	Password string `json:"password"`
}

func (s *Server) handleRegister(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[registerPayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	payload.Name = strings.TrimSpace(payload.Name)
	payload.Email = strings.ToLower(strings.TrimSpace(payload.Email))

	fields := make(map[string]string)
	if runeLen(payload.Name) < 2 || runeLen(payload.Name) > 80 {
		fields["name"] = "Use between 2 and 80 characters."
	}
	if !validEmail(payload.Email) {
		fields["email"] = "Enter a valid email."
	}
	if runeLen(payload.Password) < 10 || runeLen(payload.Password) > 200 {
		fields["password"] = "Use between 10 and 200 characters."
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	if _, err := s.store.UserByEmail(r.Context(), payload.Email); err == nil {
		writeProblem(w, problemFields(409, "conflict", "An account with this email already exists.",
			map[string]string{"email": "This email is already registered."}))
		return
	} else if !errors.Is(err, sql.ErrNoRows) {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	hash, err := auth.HashPassword(payload.Password)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	now := time.Now().UTC()
	user := store.User{
		ID:           ids.New("usr"),
		Name:         payload.Name,
		Email:        payload.Email,
		PasswordHash: hash,
		Role:         "customer",
		ThemePreset:  "cellar",
		CreatedAt:    now,
		UpdatedAt:    now,
	}
	token, session, err := s.newSession(user, r)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if err := s.store.CreateUser(r.Context(), tx, user); err != nil {
			return err
		}
		return s.store.CreateSession(r.Context(), tx, session)
	})
	if err != nil {
		if isUniqueViolation(err) {
			writeProblem(w, problemFields(409, "conflict", "An account with this email already exists.",
				map[string]string{"email": "This email is already registered."}))
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	s.setSessionCookie(w, token, session.ExpiresAt)
	writeJSON(w, http.StatusCreated, map[string]any{"user": toUser(user)})
}

type loginPayload struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

func (s *Server) handleLogin(w http.ResponseWriter, r *http.Request) {
	payload, _, bad := decodeJSON[loginPayload](w, r)
	if bad != nil {
		writeProblem(w, bad)
		return
	}
	payload.Email = strings.ToLower(strings.TrimSpace(payload.Email))

	fields := make(map[string]string)
	if !validEmail(payload.Email) {
		fields["email"] = "Enter a valid email."
	}
	if payload.Password == "" || runeLen(payload.Password) > 200 {
		fields["password"] = "Enter your password."
	}
	if len(fields) > 0 {
		writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.", fields))
		return
	}

	user, err := s.store.UserByEmail(r.Context(), payload.Email)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			auth.VerifyDummy(payload.Password)
			writeError(w, http.StatusUnauthorized, "unauthorized", "Email or password is incorrect.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	if !auth.VerifyPassword(payload.Password, user.PasswordHash) {
		writeError(w, http.StatusUnauthorized, "unauthorized", "Email or password is incorrect.")
		return
	}

	token, session, err := s.newSession(user, r)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	err = s.store.InTx(r.Context(), func(tx *sql.Tx) error {
		if cookie, cookieErr := r.Cookie(sessionCookieName); cookieErr == nil && cookie.Value != "" {
			if err := s.store.DeleteSessionByHash(r.Context(), tx, auth.HashToken(cookie.Value)); err != nil {
				return err
			}
		}
		if err := s.store.DeleteSessionsForUserAgent(r.Context(), tx, user.ID, r.UserAgent()); err != nil {
			return err
		}
		if err := s.store.DeleteExpiredSessions(r.Context(), tx, time.Now().UTC()); err != nil {
			return err
		}
		return s.store.CreateSession(r.Context(), tx, session)
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	s.setSessionCookie(w, token, session.ExpiresAt)
	writeJSON(w, http.StatusOK, map[string]any{"user": toUser(user)})
}

func (s *Server) handleLogout(w http.ResponseWriter, r *http.Request) {
	if cookie, err := r.Cookie(sessionCookieName); err == nil && cookie.Value != "" {
		if err := s.store.DeleteSessionByHash(r.Context(), s.store.DB(), auth.HashToken(cookie.Value)); err != nil {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
	}
	s.clearSessionCookie(w)
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) handleMe(w http.ResponseWriter, r *http.Request) {
	current, ok := currentAuth(r)
	if !ok {
		writeError(w, http.StatusUnauthorized, "unauthorized", "Sign in to continue.")
		return
	}
	_ = s.store.DeleteExpiredSessions(r.Context(), s.store.DB(), time.Now().UTC())
	writeJSON(w, http.StatusOK, map[string]any{"user": toUser(current.User)})
}

func (s *Server) handleAccountOrders(w http.ResponseWriter, r *http.Request) {
	current, ok := currentAuth(r)
	if !ok {
		writeError(w, http.StatusUnauthorized, "unauthorized", "Sign in to continue.")
		return
	}
	orders, err := s.store.OrdersByUser(r.Context(), current.User.ID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}
	result := make([]orderDTO, 0, len(orders))
	for _, order := range orders {
		items, err := s.store.OrderItems(r.Context(), order.ID)
		if err != nil {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		result = append(result, toOrder(order, items, "", false))
	}
	writeJSON(w, http.StatusOK, map[string]any{"orders": result})
}

func (s *Server) newSession(user store.User, r *http.Request) (string, store.Session, error) {
	token, err := auth.NewToken()
	if err != nil {
		return "", store.Session{}, err
	}
	now := time.Now().UTC()
	ttl := s.cfg.SessionTTL
	if user.Role == "admin" {
		ttl = s.cfg.AdminSessionTTL()
	}
	session := store.Session{
		ID:        ids.New("ses"),
		UserID:    user.ID,
		TokenHash: auth.HashToken(token),
		UserAgent: r.UserAgent(),
		IP:        clientIP(r, s.cfg.TrustProxy),
		CreatedAt: now,
		ExpiresAt: now.Add(ttl),
	}
	return token, session, nil
}

func isUniqueViolation(err error) bool {
	return err != nil && strings.Contains(strings.ToLower(err.Error()), "unique constraint")
}
