package httpapi

import (
	"log/slog"
	"net/http"
	"time"

	"azahar/internal/auth"
	"azahar/internal/config"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

type Server struct {
	cfg             config.Config
	store           *store.Store
	logger          *slog.Logger
	generalLimiter  *rateLimiter
	authLimiter     *rateLimiter
	checkoutLimiter *rateLimiter
	adminLimiter    *rateLimiter
	handler         http.Handler
}

func New(cfg config.Config, st *store.Store, logger *slog.Logger) *Server {
	_ = auth.DummyHash()
	server := &Server{
		cfg:             cfg,
		store:           st,
		logger:          logger,
		generalLimiter:  newRateLimiter(120, time.Minute),
		authLimiter:     newRateLimiter(10, 15*time.Minute),
		checkoutLimiter: newRateLimiter(20, 10*time.Minute),
		adminLimiter:    newRateLimiter(60, time.Minute),
	}
	server.handler = server.routes()
	return server
}

func (s *Server) Handler() http.Handler {
	return s.handler
}

func (s *Server) Close() {
	s.generalLimiter.close()
	s.authLimiter.close()
	s.checkoutLimiter.close()
	s.adminLimiter.close()
}

func (s *Server) routes() http.Handler {
	r := chi.NewRouter()
	r.Use(s.securityHeaders)
	r.Use(s.withRequestID)
	r.Use(s.logRequests)
	r.Use(s.recoverPanic)
	r.Use(s.limitBody)
	r.Use(s.rateLimit(s.generalLimiter))
	r.Use(s.enforceCSRF)

	r.NotFound(func(w http.ResponseWriter, r *http.Request) {
		writeError(w, http.StatusNotFound, "not_found", "Not found.")
	})
	r.MethodNotAllowed(func(w http.ResponseWriter, r *http.Request) {
		writeError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Method not allowed.")
	})

	r.Route("/api", func(r chi.Router) {
		r.Get("/categories", s.handleCategories)
		r.Get("/products", s.handleProducts)
		r.Get("/products/{slug}", s.handleProduct)
		r.Get("/orders/{token}", s.handleOrderByToken)

		r.With(s.rateLimit(s.authLimiter)).Post("/auth/register", s.handleRegister)
		r.With(s.rateLimit(s.authLimiter)).Post("/auth/login", s.handleLogin)
		r.Post("/auth/logout", s.handleLogout)

		r.With(s.rateLimit(s.checkoutLimiter)).Post("/checkout", s.handleCheckout)

		r.Group(func(r chi.Router) {
			r.Use(s.requireUser)
			r.Get("/auth/me", s.handleMe)
			r.Get("/account/orders", s.handleAccountOrders)
		})

		r.Route("/admin", func(r chi.Router) {
			r.Use(s.requireUser)
			r.Use(s.requireAdmin)
			r.Use(s.adminMutationLimit)

			r.Get("/metrics", s.handleMetrics)
			r.Get("/products", s.handleAdminProducts)
			r.Post("/products", s.handleCreateProduct)
			r.Patch("/products/{id}", s.handleUpdateProduct)
			r.Delete("/products/{id}", s.handleArchiveProduct)
			r.Get("/orders", s.handleAdminOrders)
			r.Patch("/orders/{id}/status", s.handleOrderStatus)
			r.Get("/customers", s.handleCustomers)
			r.Get("/preferences", s.handleGetPreferences)
			r.Put("/preferences", s.handlePutPreferences)
		})
	})

	return r
}
