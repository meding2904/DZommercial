package main

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"azahar/internal/auditbatch"
	"azahar/internal/config"
	"azahar/internal/courier"
	"azahar/internal/db"
	"azahar/internal/httpapi"
	"azahar/internal/plugins"
	"azahar/internal/plugins/ai"
	"azahar/internal/plugins/codemode"
	"azahar/internal/plugins/oils"
	"azahar/internal/plugins/pcbuilder"
	"azahar/internal/plugins/pcparts"
	"azahar/internal/plugins/phone"
	"azahar/internal/seed"
	"azahar/internal/store"
)

func main() {
	logger := slog.New(slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{Level: slog.LevelInfo}))
	slog.SetDefault(logger)

	cfg, err := config.Load()
	if err != nil {
		logger.Error("invalid configuration", "error", err)
		os.Exit(1)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	handle, err := db.Open(ctx, cfg.DSN())
	if err != nil {
		logger.Error("database connection failed", "error", err)
		os.Exit(1)
	}
	defer handle.Close()

	if err := db.Migrate(ctx, handle); err != nil {
		logger.Error("database migration failed", "error", err)
		os.Exit(1)
	}

	registry := plugins.NewRegistry()
	for _, plugin := range []plugins.Plugin{
		oils.New(), pcparts.New(), pcbuilder.New(), codemode.New(registry), phone.New(), ai.New(),
	} {
		if err := registry.Register(plugin); err != nil {
			logger.Error("plugin registration failed", "error", err)
			os.Exit(1)
		}
	}
	if err := registry.ValidateDependencies(); err != nil {
		logger.Error("plugin dependency check failed", "error", err)
		os.Exit(1)
	}
	pluginMigrations := make([]db.PluginMigration, 0)
	for _, item := range registry.Migrations() {
		pluginMigrations = append(pluginMigrations, db.PluginMigration{
			PluginID: item.PluginID,
			Version:  item.Migration.Version,
			SQL:      item.Migration.SQL,
		})
	}
	if err := db.MigratePlugins(ctx, handle, pluginMigrations); err != nil {
		logger.Error("plugin migration failed", "error", err)
		os.Exit(1)
	}

	repo := store.New(handle)
	states, err := repo.PluginStates(ctx)
	if err != nil {
		logger.Error("plugin states could not be read", "error", err)
		os.Exit(1)
	}
	registry.SetStates(states)
	for _, plugin := range registry.All() {
		manifest := plugin.Manifest()
		for _, category := range manifest.Categories {
			if err := repo.UpsertCategory(ctx, manifest.ID, store.Category{
				Slug:        category.Slug,
				Name:        category.Name,
				Description: category.Description,
				SortOrder:   category.SortOrder,
			}); err != nil {
				logger.Error("category sync failed", "error", err)
				os.Exit(1)
			}
		}
	}
	if err := repo.ReconcileProductCategories(ctx); err != nil {
		logger.Error("product category reconciliation failed", "error", err)
		os.Exit(1)
	}

	if cfg.SeedDemo {
		if _, err := seed.Run(ctx, handle, logger, seed.Options{
			AdminEmail:       cfg.AdminEmail,
			AdminPassword:    cfg.AdminPassword,
			CustomerEmail:    cfg.CustomerEmail,
			CustomerPassword: cfg.CustomerPassword,
			EmployeeEmail:    cfg.EmployeeEmail,
			EmployeePassword: cfg.EmployeePassword,
		}); err != nil {
			logger.Error("demo seed failed", "error", err)
			os.Exit(1)
		}
	}

	if cfg.IsDevelopment() {
		logger.Warn("development demo credentials",
			"admin_email", cfg.AdminEmail,
			"admin_password", cfg.AdminPassword,
			"employee_email", cfg.EmployeeEmail,
			"employee_password", cfg.EmployeePassword,
			"customer_email", cfg.CustomerEmail,
			"customer_password", cfg.CustomerPassword,
		)
	}

	courierCredentials, credentialsParsed := cfg.CourierCredentialValues()
	if !credentialsParsed {
		logger.Warn("COURIER_CREDENTIALS must be a JSON object; ignoring the value")
	}
	if cfg.CourierKey != "sandbox" && len(courierCredentials) == 0 {
		logger.Warn("courier credentials are empty; shipment creation will fail until they are set",
			"courier", cfg.CourierKey)
	}
	if cfg.IsProduction() && cfg.CourierKey == "sandbox" {
		logger.Warn("the sandbox courier is active in production; no real parcels will be created")
	}

	shipping := courier.New(courier.Options{
		BaseURL:           cfg.CourierAPIURL,
		CourierKey:        cfg.CourierKey,
		Credentials:       courierCredentials,
		FromWilaya:        cfg.CourierFromWilaya,
		Timeout:           cfg.CourierTimeout,
		AllowLocalSandbox: cfg.IsDevelopment(),
		OnFallback: func(resource string) {
			logger.Warn("courier API unavailable; using the bundled fallback",
				"resource", resource, "courier", cfg.CourierKey)
		},
	})

	auditCtx, stopAudit := context.WithCancel(context.Background())
	auditDone := make(chan struct{})
	go func() {
		defer close(auditDone)
		auditbatch.Run(auditCtx, repo, cfg.AuditBatchInterval, logger)
	}()

	server := httpapi.New(cfg, repo, logger, shipping, registry)
	httpServer := &http.Server{
		Addr:              cfg.APIAddr,
		Handler:           server.Handler(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      30 * time.Second,
		IdleTimeout:       60 * time.Second,
		MaxHeaderBytes:    1 << 20,
	}

	go func() {
		logger.Info("api listening", "addr", cfg.APIAddr, "env", cfg.AppEnv)
		if err := httpServer.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			logger.Error("http server stopped", "error", err)
			stop()
		}
	}()

	<-ctx.Done()
	logger.Info("shutting down")

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := httpServer.Shutdown(shutdownCtx); err != nil {
		logger.Error("graceful shutdown failed", "error", err)
		_ = httpServer.Close()
	}
	server.Close()

	stopAudit()
	<-auditDone
}
