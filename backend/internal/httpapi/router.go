package httpapi

import (
	"log/slog"
	"net/http"
	"time"

	"azahar/internal/auth"
	"azahar/internal/config"
	"azahar/internal/courier"
	"azahar/internal/orderintake"
	"azahar/internal/plugins"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

type Server struct {
	cfg             config.Config
	store           *store.Store
	shipping        courier.Courier
	plugins         *plugins.Registry
	intake          *orderintake.Intake
	pluginEnv       *plugins.Env
	logger          *slog.Logger
	generalLimiter  *rateLimiter
	authLimiter     *rateLimiter
	checkoutLimiter *rateLimiter
	captchaLimiter  *rateLimiter
	adminLimiter    *rateLimiter
	captcha         *captchaStore
	readCache       *readCache
	handler         http.Handler
}

func New(cfg config.Config, st *store.Store, logger *slog.Logger, shipping courier.Courier, registry *plugins.Registry) *Server {
	_ = auth.DummyHash()
	if registry == nil {
		registry = plugins.NewRegistry()
	}
	server := &Server{
		cfg:             cfg,
		store:           st,
		shipping:        shipping,
		plugins:         registry,
		intake:          orderintake.New(st, registry),
		logger:          logger,
		generalLimiter:  newRateLimiter(120, time.Minute),
		authLimiter:     newRateLimiter(10, 15*time.Minute),
		checkoutLimiter: newRateLimiter(20, 10*time.Minute),
		captchaLimiter:  newRateLimiter(30, 10*time.Minute),
		adminLimiter:    newRateLimiter(60, time.Minute),
		captcha:         newCaptchaStore(10 * time.Minute),
		readCache:       newReadCache(readCacheMaxEntries),
	}
	server.pluginEnv = &plugins.Env{
		DB:            st.DB(),
		Store:         st,
		Logger:        logger,
		SecretKey:     cfg.PluginSecretKey,
		AIBaseURL:     cfg.AIBaseURL,
		AITimeout:     cfg.AITimeout,
		IsDevelopment: cfg.IsDevelopment(),
		Actor: func(r *http.Request) (string, string) {
			current, ok := currentAuth(r)
			if !ok {
				return "", ""
			}
			return current.User.ID, current.User.Role
		},
		RequestID: requestID,
	}
	for _, plugin := range registry.All() {
		if aware, ok := plugin.(plugins.EnvAware); ok {
			aware.Init(server.pluginEnv)
		}
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
	s.captchaLimiter.close()
	s.adminLimiter.close()
	s.captcha.close()
}

func (s *Server) routes() http.Handler {
	r := chi.NewRouter()
	r.Use(s.securityHeaders)
	r.Use(s.withRequestID)
	r.Use(s.logRequests)
	r.Use(s.recoverPanic)
	r.Use(s.limitBody)
	r.Use(s.requireChallengeHeader)
	r.Use(s.rateLimit(s.generalLimiter))
	r.Use(s.enforceCSRF)
	r.Use(s.invalidateReadCacheOnWrite)

	r.NotFound(func(w http.ResponseWriter, r *http.Request) {
		writeError(w, http.StatusNotFound, "not_found", "Not found.")
	})
	r.MethodNotAllowed(func(w http.ResponseWriter, r *http.Request) {
		writeError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Method not allowed.")
	})

	r.Route("/api", func(r chi.Router) {
		r.Get("/health", s.handleHealth)
		r.Get("/csrf", s.handleCSRF)
		r.With(s.cacheRead(5*time.Second)).Get("/categories", s.handleCategories)
		r.With(s.cacheRead(2*time.Second)).Get("/products", s.handleProducts)
		r.With(s.cacheRead(2*time.Second)).Get("/products/{slug}", s.handleProduct)
		r.Get("/orders/{token}", s.handleOrderByToken)
		r.Get("/shipping/wilayas", s.handleShippingWilayas)
		r.Get("/shipping/communes", s.handleShippingCommunes)
		r.Get("/shipping/desks", s.handleShippingDesks)
		r.With(s.cacheRead(5*time.Second)).Get("/plugins", s.handlePluginManifests)
		r.With(s.cacheRead(2*time.Second)).Get("/storefront/theme", s.handleStorefrontTheme)
		r.Get("/images/{id}", s.handleProductImage)

		s.mountPluginRoutes(r)

		r.With(s.rateLimit(s.authLimiter)).Post("/auth/register", s.handleRegister)
		r.With(s.rateLimit(s.authLimiter)).Post("/auth/login", s.handleLogin)
		r.Post("/auth/logout", s.handleLogout)

		r.With(s.rateLimit(s.checkoutLimiter)).Post("/checkout", s.handleCheckout)
		r.With(s.rateLimit(s.captchaLimiter)).Get("/checkout/challenge", s.handleCheckoutChallenge)

		r.Group(func(r chi.Router) {
			r.Use(s.requireUser)
			r.Get("/auth/me", s.handleMe)
			r.Get("/account/orders", s.handleAccountOrders)
		})

		r.Route("/admin", func(r chi.Router) {
			r.Use(s.requireUser)
			r.Use(s.requireStaff)
			r.Use(s.adminMutationLimit)

			r.With(s.cacheRead(3*time.Second)).Get("/metrics", s.handleMetrics)
			r.Post("/sales", s.handleCounterSale)
			r.With(s.cacheRead(time.Second)).Get("/products", s.handleAdminProducts)
			r.With(s.cacheRead(time.Second)).Get("/orders", s.handleAdminOrders)
			r.Patch("/orders/{id}/status", s.handleOrderStatus)
			r.Post("/orders/{id}/shipment", s.handleCreateShipment)
			r.Post("/orders/{id}/tracking", s.handleRefreshTracking)
			r.With(s.cacheRead(5*time.Second)).Get("/customers", s.handleCustomers)
			r.Get("/preferences", s.handleGetPreferences)
			r.Put("/preferences", s.handlePutPreferences)

			r.Group(func(r chi.Router) {
				r.Use(s.requireAdmin)

				r.Post("/products", s.handleCreateProduct)
				r.Patch("/products/{id}", s.handleUpdateProduct)
				r.Delete("/products/{id}", s.handleArchiveProduct)
				r.Get("/storefront/theme", s.handleAdminStorefrontTheme)
				r.Put("/storefront/theme", s.handlePutStorefrontTheme)
				r.Get("/team", s.handleAdminTeam)
				r.Patch("/team/{id}", s.handleUpdateTeamRole)
				r.Get("/plugins", s.handleAdminPlugins)
				r.Put("/plugins/{pluginID}/state", s.handlePluginState)
				r.Post("/product-images", s.handleAdminProductImage)
				r.Route("/plugins/{pluginID}", func(r chi.Router) {
					r.Get("/settings", s.handleGetPluginSettings)
					r.Put("/settings", s.handlePutPluginSettings)
				})
			})
		})
	})

	return r
}

func (s *Server) mountPluginRoutes(r chi.Router) {
	for _, plugin := range s.plugins.All() {
		manifest := plugin.Manifest()
		routes := plugin.Routes()
		r.Route("/plugins/"+manifest.ID, func(r chi.Router) {
			for _, route := range routes {
				if route.Admin {
					continue
				}
				r.Method(route.Method, route.Pattern, s.pluginHandler(manifest.ID, route))
			}
			r.Group(func(r chi.Router) {
				r.Use(s.requireUser)
				r.Use(s.requireAdmin)
				r.Use(s.adminMutationLimit)
				for _, route := range routes {
					if !route.Admin {
						continue
					}
					r.Method(route.Method, "/admin"+route.Pattern, s.pluginHandler(manifest.ID, route))
				}
			})
		})
	}
}

func (s *Server) pluginHandler(pluginID string, route plugins.Route) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !s.plugins.EffectiveEnabled(pluginID) {
			writeError(w, http.StatusNotFound, "not_found", "Plugin not found.")
			return
		}
		ctx := &plugins.Context{
			W:        w,
			R:        r,
			Env:      s.pluginEnv,
			PluginID: pluginID,
		}
		if current, ok := currentAuth(r); ok {
			ctx.UserID = current.User.ID
			ctx.UserRole = current.User.Role
		}
		route.Handler(ctx)
	}
}
