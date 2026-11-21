package httpapi

import (
	"crypto/rand"
	"encoding/hex"
	"net/http"
	"time"
)

const (
	csrfTokenBytes  = 32
	csrfCookieAge   = 7 * 24 * time.Hour
	csrfTokenLength = csrfTokenBytes * 2
)

// handleCSRF issues the double-submit token used by native clients. The web
// app gets the same cookie from the SvelteKit hook; a desktop client has no
// document to set it, so it reads this endpoint once and mirrors the value in
// X-CSRF-Token on every unsafe request.
func (s *Server) handleCSRF(w http.ResponseWriter, r *http.Request) {
	token := ""
	if cookie, err := r.Cookie(csrfCookieName); err == nil && validCSRFToken(cookie.Value) {
		token = cookie.Value
	}
	if token == "" {
		generated, err := newCSRFToken()
		if err != nil {
			writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
			return
		}
		token = generated
	}
	http.SetCookie(w, &http.Cookie{
		Name:     csrfCookieName,
		Value:    token,
		Path:     "/",
		HttpOnly: false,
		Secure:   s.cfg.CookieSecure,
		SameSite: http.SameSiteLaxMode,
		Expires:  time.Now().UTC().Add(csrfCookieAge),
		MaxAge:   int(csrfCookieAge.Seconds()),
	})
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, map[string]any{
		"csrf_token": token,
		"expires_in": int(csrfCookieAge.Seconds()),
	})
}

// handleHealth is the small public probe used by the desktop shell and by
// deployment checks. It deliberately does not touch the database: the process
// refuses to start without a working connection, and a probe that writes load
// into Turso is not a health check.
func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"status":      "ok",
		"service":     "azahar-api",
		"env":         s.cfg.AppEnv,
		"origin":      s.cfg.AppOrigin,
		"server_time": time.Now().UTC().Format(time.RFC3339),
	})
}

func newCSRFToken() (string, error) {
	buf := make([]byte, csrfTokenBytes)
	if _, err := rand.Read(buf); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf), nil
}

func validCSRFToken(value string) bool {
	if len(value) != csrfTokenLength {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}
