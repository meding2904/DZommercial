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

.PHONY: help dev-db dev-api dev-web check desktop-sync desktop-check desktop-deps build smoke kiri-up kiri-local kiri-down desktop desktop-build desktop-package desktop-rust desktop-rust-build desktop-smoke loadtest rust-check rust-test bench-baseline rust-bench oracle-diff

help:
	@echo "Azahar demo store"
	@echo "  make dev-db    start the local Turso/libSQL container"
	@echo "  make dev-api   run the Go API on :8080"
	@echo "  make dev-web   run the SvelteKit app on :5173"
	@echo "  make check     type-check the frontend, test the Go code and the Wails app"
	@echo "  make build     build both apps"
	@echo "  make smoke     call the public API endpoints"
	@echo "  make desktop-deps  install the Wails CLI"
	@echo "  make desktop-sync  copy the shared admin domain into the desktop renderer"
	@echo "  make desktop   run the Go Wails admin app (oracle)"
	@echo "  make desktop-rust  run the Rust Tauri admin app"
	@echo "  make desktop-smoke  headless Weston smoke test for the Tauri host"
	@echo "  make desktop-build  build the Wails admin binary"
	@echo "  make desktop-package  build a Linux tarball with binary, desktop file and icon"
	@echo "  make loadtest  run a short read probe against the API"
	@echo "  make rust-check  fmt, clippy, and the whole Rust test suite"
	@echo "  make rust-bench  build the Rust API and write bench/after.json"
	@echo "  make kiri-up   build and run the three containers with Kiri"
	@echo "  make kiri-local run host-built artifacts in Kiri containers (builder fallback)"
	@echo "  make kiri-down stop the containers, keep the database volume"

dev-db:
	@if ! command -v kiri >/dev/null; then echo "kiri is not installed"; exit 1; fi
	@if kiri ps --format '{{.Names}}' 2>/dev/null | grep -qx azahar-db; then \
		echo "azahar-db is already running on $(TURSO_URL)"; \
	elif kiri ps -a --format '{{.Names}}' 2>/dev/null | grep -qx azahar-db; then \
		kiri start azahar-db >/dev/null; \
		echo "azahar-db restarted on $(TURSO_URL)"; \
	else \
		kiri volume create azahar-db >/dev/null 2>&1 || true; \
		kiri run -d --name azahar-db -p 8880:8080 -v azahar-db:/data \
			--entrypoint /bin/sqld ghcr.io/tursodatabase/libsql-server:latest \
			--http-listen-addr 0.0.0.0:8080 -d /data; \
		echo "azahar-db started on $(TURSO_URL)"; \
	fi
	@for i in $$(seq 1 30); do \
		if curl -fsS $(TURSO_URL)/health >/dev/null 2>&1; then exit 0; fi; \
		sleep 1; \
	done; \
	echo "azahar-db did not answer on $(TURSO_URL)"; exit 1

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

rust-check:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

rust-test:
	cargo test --workspace

# Same harness and request counts as bench/after.json. The API must be able to
# reach the database at TURSO_DATABASE_URL (make dev-db).
bench-baseline:
	cd backend && go build -o /tmp/azahar-api ./cmd/api
	python3 bench/bench.py --bin /tmp/azahar-api --label go-oracle --out bench/baseline.json

rust-bench:
	cargo build --release -p azahar-api
	python3 bench/bench.py --bin target/release/azahar-api --label rust --out bench/after.json

oracle-diff:
	cargo test --workspace --tests -- --nocapture

check:
	cd frontend && APP_ENV=development pnpm check && pnpm test:security && pnpm test:framework
	cd backend && APP_ENV=development go vet ./... && APP_ENV=development go test ./...
	$(MAKE) desktop-check

desktop-sync:
	@mkdir -p desktop/frontend/domain
	cp frontend/src/lib/admin/domain/*.js desktop/frontend/domain/

desktop-check:
	@for file in frontend/src/lib/admin/domain/*.js; do \
		name=$$(basename "$$file"); \
		if ! cmp -s "$$file" "desktop/frontend/domain/$$name"; then \
			echo "desktop/frontend/domain/$$name differs from $$file; run make desktop-sync"; \
			exit 1; \
		fi; \
	done
	@for file in desktop/frontend/domain/*.js; do \
		name=$$(basename "$$file"); \
		if [ ! -f "frontend/src/lib/admin/domain/$$name" ]; then \
			echo "desktop/frontend/domain/$$name has no source in frontend/src/lib/admin/domain"; \
			exit 1; \
		fi; \
	done
	node --check desktop/frontend/app.js
	@for file in "desktop/frontend/tests/"*.test.mjs; do \
		if [ ! -f "$$file" ]; then \
			echo "no desktop frontend tests found; expected desktop/frontend/tests/*.test.mjs"; \
			exit 1; \
		fi; \
	done
	node --test desktop/frontend/tests/*.test.mjs
	cd desktop && CGO_ENABLED=0 go vet ./internal/api && CGO_ENABLED=0 go test ./internal/api
	@if pkg-config --exists gtk+-3.0 && pkg-config --exists webkit2gtk-4.1; then \
		cd desktop && go vet $(WAILS_TAG_FLAGS) ./... && go test $(WAILS_TAG_FLAGS) ./...; \
	else \
		echo "skipping Wails main package checks: install libgtk-3-dev and libwebkit2gtk-4.1-dev"; \
	fi

desktop-rust:
	cargo run -p azahar-desktop

desktop-rust-build:
	cargo build --release -p azahar-desktop

desktop-smoke:
	./scripts/desktop-smoke.sh

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

desktop: desktop-sync
	cd desktop && $(WAILS) dev $(WAILS_TAG_FLAGS) -s

desktop-build: desktop-sync
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
