SHELL := /bin/bash
ROOT := $(CURDIR)
API_URL ?= http://127.0.0.1:8080
WEB_URL ?= http://127.0.0.1:5173
TURSO_URL ?= http://127.0.0.1:8880

.PHONY: help dev-db dev-api dev-web check build smoke kiri-up kiri-local kiri-down

help:
	@echo "Azahar demo store"
	@echo "  make dev-db    start the local Turso/libSQL container"
	@echo "  make dev-api   run the Go API on :8080"
	@echo "  make dev-web   run the SvelteKit app on :5173"
	@echo "  make check     type-check the frontend and test the backend"
	@echo "  make build     build both apps"
	@echo "  make smoke     call the public API endpoints"
	@echo "  make kiri-up   build and run the three containers with Kiri"
	@echo "  make kiri-local run host-built artifacts in Kiri containers (builder fallback)"
	@echo "  make kiri-down stop the containers, keep the database volume"

dev-db:
	@if ! command -v kiri >/dev/null; then echo "kiri is not installed"; exit 1; fi
	@if kiri ps --format '{{.Names}}' 2>/dev/null | grep -qx azahar-db; then \
		echo "azahar-db is already running on $(TURSO_URL)"; \
	else \
		kiri volume create azahar-db >/dev/null 2>&1 || true; \
		kiri run -d --name azahar-db -p 8880:8080 -v azahar-db:/data \
			--entrypoint /bin/sqld ghcr.io/tursodatabase/libsql-server:latest \
			--http-listen-addr 0.0.0.0:8080 -d /data; \
		echo "azahar-db started on $(TURSO_URL)"; \
	fi

dev-api:
	cd backend && \
	APP_ENV=development \
	API_ADDR=:8080 \
	APP_ORIGIN=$(WEB_URL) \
	TURSO_DATABASE_URL=$(TURSO_URL) \
	COOKIE_SECURE=false \
	SEED_DEMO=true \
	go run ./cmd/api

dev-web:
	cd frontend && INTERNAL_API_URL=$(API_URL) pnpm dev --port 5173

check:
	cd frontend && pnpm check && pnpm test:security
	cd backend && go vet ./... && go test ./...

build:
	cd frontend && pnpm build
	cd backend && go build ./...

smoke:
	curl -fsS $(API_URL)/api/products | head -c 200; echo
	curl -fsS $(API_URL)/api/categories | head -c 200; echo

kiri-up:
	./deploy/kiri-up.sh

kiri-local:
	./deploy/kiri-local.sh

kiri-down:
	./deploy/kiri-down.sh
