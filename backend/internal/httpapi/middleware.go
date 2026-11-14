package httpapi

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"fmt"
	"log/slog"
	"net/http"
	"runtime/debug"
	"strings"
	"time"

	"azahar/internal/auth"
	"azahar/internal/store"
)

type contextKey string

const (
	requestIDKey contextKey = "request_id"
	authKey      contextKey = "auth"
)

type authContext struct {
	User      store.User
	Session   store.Session
	TokenHash string
}

func requestID(r *http.Request) string {
	value, _ := r.Context().Value(requestIDKey).(string)
	return value
}

func (s *Server) securityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("X-Frame-Options", "DENY")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("Permissions-Policy", "camera=(), microphone=(), geolocation=()")
		w.Header().Set("Cache-Control", "no-store")
		if s.cfg.IsProduction() {
			w.Header().Set("Strict-Transport-Security", "max-age=31536000; includeSubDomains")
		}
		next.ServeHTTP(w, r)
	})
}

func (s *Server) withRequestID(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		id := r.Header.Get("X-Request-Id")
		if id == "" || len(id) > 64 {
			buf := make([]byte, 8)
			if _, err := rand.Read(buf); err != nil {
				id = "request"
			} else {
				id = hex.EncodeToString(buf)
			}
		}
		w.Header().Set("X-Request-Id", id)
		next.ServeHTTP(w, r.WithContext(context.WithValue(r.Context(), requestIDKey, id)))
	})
}

type statusWriter struct {
	http.ResponseWriter
	status int
	wrote  bool
}

func (w *statusWriter) WriteHeader(status int) {
	if w.wrote {
		return
	}
	w.status = status
	w.wrote = true
	w.ResponseWriter.WriteHeader(status)
}

func (w *statusWriter) Write(body []byte) (int, error) {
	if !w.wrote {
		w.WriteHeader(http.StatusOK)
	}
	return w.ResponseWriter.Write(body)
}

func (s *Server) logRequests(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		writer := &statusWriter{ResponseWriter: w, status: http.StatusOK}
		next.ServeHTTP(writer, r)

		level := slog.LevelInfo
		if writer.status >= 500 {
			level = slog.LevelError
		} else if writer.status >= 400 {
			level = slog.LevelWarn
		}
		s.logger.Log(r.Context(), level, "http request",
			"method", r.Method,
			"path", redactedPath(r.URL.Path),
			"status", writer.status,
			"duration_ms", time.Since(start).Milliseconds(),
			"request_id", requestID(r),
			"ip", clientIP(r, s.cfg.TrustProxy),
		)
	})
}

func redactedPath(path string) string {
	if strings.HasPrefix(path, "/api/orders/") && len(path) > len("/api/orders/") {
		return "/api/orders/[redacted]"
	}
	return path
}

func (s *Server) recoverPanic(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if recovered := recover(); recovered != nil {
				s.logger.Error("panic recovered",
					"error", fmt.Sprint(recovered),
					"path", redactedPath(r.URL.Path),
					"request_id", requestID(r),
					"stack", string(debug.Stack()),
				)
				if writer, ok := w.(*statusWriter); !ok || !writer.wrote {
					writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
				}
			}
		}()
		next.ServeHTTP(w, r)
	})
}

func (s *Server) limitBody(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
		next.ServeHTTP(w, r)
	})
}

// Check the challenge header before any limiter runs. A cross-site image cannot
// set this header, so rejected requests must not consume a visitor's bucket.
func (s *Server) requireChallengeHeader(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/api/checkout/challenge" &&
			r.Header.Get("X-Requested-With") != "azahar-web" {
			writeError(w, http.StatusForbidden, "forbidden", "The request could not be verified.")
			return
		}
		next.ServeHTTP(w, r)
	})
}

const (
	sessionCookieName = "azahar_session"
	csrfCookieName    = "azahar_csrf"
	csrfHeaderName    = "X-CSRF-Token"
)

func (s *Server) enforceCSRF(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.Method {
		case http.MethodGet, http.MethodHead, http.MethodOptions:
			next.ServeHTTP(w, r)
			return
		}
		if r.Header.Get("X-Requested-With") != "azahar-web" {
			writeError(w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
			return
		}
		origin := r.Header.Get("Origin")
		if origin != "" {
			if origin != s.cfg.AppOrigin {
				writeError(w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
				return
			}
		} else if !s.cfg.IsDevelopment() {
			writeError(w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
			return
		}
		cookie, err := r.Cookie(csrfCookieName)
		header := r.Header.Get(csrfHeaderName)
		if err != nil || cookie.Value == "" || header == "" ||
			len(cookie.Value) > 128 || len(header) > 128 ||
			subtle.ConstantTimeCompare([]byte(cookie.Value), []byte(header)) != 1 {
			writeError(w, http.StatusForbidden, "csrf_failed", "The request could not be verified.")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (s *Server) setSessionCookie(w http.ResponseWriter, token string, expires time.Time) {
	http.SetCookie(w, &http.Cookie{
		Name:     sessionCookieName,
		Value:    token,
		Path:     "/",
		HttpOnly: true,
		Secure:   s.cfg.CookieSecure,
		SameSite: http.SameSiteLaxMode,
		Expires:  expires,
		MaxAge:   int(time.Until(expires).Seconds()),
	})
}

func (s *Server) clearSessionCookie(w http.ResponseWriter) {
	http.SetCookie(w, &http.Cookie{
		Name:     sessionCookieName,
		Value:    "",
		Path:     "/",
		HttpOnly: true,
		Secure:   s.cfg.CookieSecure,
		SameSite: http.SameSiteLaxMode,
		Expires:  time.Unix(0, 0),
		MaxAge:   -1,
	})
}

func (s *Server) requireUser(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		cookie, err := r.Cookie(sessionCookieName)
		if err != nil || cookie.Value == "" {
			writeError(w, http.StatusUnauthorized, "unauthorized", "Sign in to continue.")
			return
		}
		hash := auth.HashToken(cookie.Value)
		user, session, found, err := s.store.SessionUserByHash(r.Context(), hash)
		if err != nil {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		if !found {
			s.clearSessionCookie(w)
			writeError(w, http.StatusUnauthorized, "unauthorized", "Sign in to continue.")
			return
		}
		if !session.ExpiresAt.After(time.Now()) {
			_ = s.store.DeleteSessionByHash(r.Context(), s.store.DB(), hash)
			s.clearSessionCookie(w)
			writeError(w, http.StatusUnauthorized, "unauthorized", "Your session has expired. Sign in again.")
			return
		}
		ctx := context.WithValue(r.Context(), authKey, authContext{
			User:      user,
			Session:   session,
			TokenHash: hash,
		})
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func (s *Server) requireAdmin(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		current, ok := r.Context().Value(authKey).(authContext)
		if !ok {
			writeError(w, http.StatusUnauthorized, "unauthorized", "Sign in to continue.")
			return
		}
		if current.User.Role != "admin" {
			writeError(w, http.StatusForbidden, "forbidden", "This account does not have access.")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func currentAuth(r *http.Request) (authContext, bool) {
	current, ok := r.Context().Value(authKey).(authContext)
	return current, ok
}

// optionalUser returns the signed-in user when a valid cookie is present. It
// never fails the request.
func (s *Server) optionalUser(r *http.Request) *store.User {
	cookie, err := r.Cookie(sessionCookieName)
	if err != nil || cookie.Value == "" {
		return nil
	}
	user, session, found, err := s.store.SessionUserByHash(r.Context(), auth.HashToken(cookie.Value))
	if err != nil || !found || !session.ExpiresAt.After(time.Now()) {
		return nil
	}
	return &user
}
