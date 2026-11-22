SHELL := /bin/bash
ROOT := $(CURDIR)
API_URL ?= http://127.0.0.1:8080
WEB_URL ?= http://127.0.0.1:5173
TURSO_URL ?= http://127.0.0.1:8880
LOADTEST_ARGS ?= -c 20 -n 100
WAILS ?= $(shell command -v wails 2>/dev/null || echo "$$(go env GOPATH)/bin/wails")
WAILS_TAGS ?= $(shell pkg-config --exists webkit2gtk-4.1 2>/dev/null && echo webkit2_41)
WAILS_TAG_FLAGS = $(if $(strip $(WAILS_TAGS)),-tags "$(WAILS_TAGS)")
VERSION ?= 2.0.0
DESKTOP_GOARCH ?= $(shell go env GOARCH)

.PHONY: help dev-db dev-api dev-web check desktop-check desktop-deps build smoke kiri-up kiri-local kiri-down desktop desktop-build desktop-package loadtest

help:
	@echo "Azahar demo store"
	@echo "  make dev-db    start the local Turso/libSQL container"
	@echo "  make dev-api   run the Go API on :8080"
	@echo "  make dev-web   run the SvelteKit app on :5173"
	@echo "  make check     type-check the frontend, test the Go code and the Wails app"
	@echo "  make build     build both apps"
	@echo "  make smoke     call the public API endpoints"
	@echo "  make desktop-deps  install the Wails CLI"
	@echo "  make desktop   run the Wails admin app against the local API"
	@echo "  make desktop-build  build the Wails admin binary"
	@echo "  make desktop-package  build a Linux tarball with binary, desktop file and icon"
	@echo "  make loadtest  run a short read probe against the API"
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
	cd frontend && APP_ENV=development INTERNAL_API_URL=$(API_URL) pnpm dev --port 5173

check:
	cd frontend && APP_ENV=development pnpm check && pnpm test:security && pnpm test:framework
	cd backend && APP_ENV=development go vet ./... && APP_ENV=development go test ./...
	$(MAKE) desktop-check

desktop-check:
	node --check desktop/frontend/app.js
	cd desktop && CGO_ENABLED=0 go vet ./internal/api && CGO_ENABLED=0 go test ./internal/api
	@if pkg-config --exists gtk+-3.0 && pkg-config --exists webkit2gtk-4.1; then \
		cd desktop && go vet $(WAILS_TAG_FLAGS) ./... && go test $(WAILS_TAG_FLAGS) ./...; \
	else \
		echo "skipping Wails main package checks: install libgtk-3-dev and libwebkit2gtk-4.1-dev"; \
	fi

desktop-deps:
	go install github.com/wailsapp/wails/v2/cmd/wails@v2.17.1

build:
	cd frontend && APP_ENV=development pnpm build
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

desktop:
	cd desktop && $(WAILS) dev $(WAILS_TAG_FLAGS) -s

desktop-build:
	cd desktop && $(WAILS) build $(WAILS_TAG_FLAGS) -s -clean -trimpath -ldflags "-X main.version=$(VERSION)"

desktop-package: desktop-build
	rm -rf desktop/build/package
	mkdir -p desktop/build/package/bin
	mkdir -p desktop/build/package/share/applications
	mkdir -p desktop/build/package/share/icons/hicolor/512x512/apps
	cp desktop/build/bin/azahar-admin desktop/build/package/bin/
	cp desktop/build/azahar-admin.desktop desktop/build/package/share/applications/
	cp desktop/build/appicon.png desktop/build/package/share/icons/hicolor/512x512/apps/azahar-admin.png
	cp LICENSE NOTICE desktop/build/package/
	tar -C desktop/build/package -czf desktop/build/azahar-admin-linux-$(DESKTOP_GOARCH).tar.gz .

loadtest:
	cd scripts/loadtest && go run . -base $(API_URL) -paths /api/products,/api/categories,/api/plugins $(LOADTEST_ARGS)
