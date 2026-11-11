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

	"azahar/internal/config"
	"azahar/internal/db"
	"azahar/internal/httpapi"
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

	if cfg.SeedDemo {
		if _, err := seed.Run(ctx, handle, logger, seed.Options{
			AdminEmail:       cfg.AdminEmail,
			AdminPassword:    cfg.AdminPassword,
			CustomerEmail:    cfg.CustomerEmail,
			CustomerPassword: cfg.CustomerPassword,
		}); err != nil {
			logger.Error("demo seed failed", "error", err)
			os.Exit(1)
		}
	}

	if cfg.IsDevelopment() {
		logger.Warn("development demo credentials",
			"admin_email", cfg.AdminEmail,
			"admin_password", cfg.AdminPassword,
			"customer_email", cfg.CustomerEmail,
			"customer_password", cfg.CustomerPassword,
		)
	}

	repo := store.New(handle)
	server := httpapi.New(cfg, repo, logger)
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
}
