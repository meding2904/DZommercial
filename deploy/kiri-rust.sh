#!/usr/bin/env bash
# Run the Rust API and the unchanged SvelteKit app through Kiri.
#
# This is the Rust counterpart of deploy/kiri-local.sh: the API binary is built
# on the host and mounted into a distroless image, while the database and web
# app are ordinary Kiri containers. No Go toolchain is used.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v kiri >/dev/null 2>&1; then
	echo "kiri is not installed. See https://kiri.dev" >&2
	exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
	echo "cargo is not installed." >&2
	exit 1
fi
if ! command -v pnpm >/dev/null 2>&1; then
	echo "pnpm is not installed." >&2
	exit 1
fi

APP_ENV="${APP_ENV:-development}"
APP_ENV="${APP_ENV,,}"
APP_ORIGIN="${APP_ORIGIN:-http://localhost:3000}"
ADMIN_EMAIL="${ADMIN_EMAIL:-admin@azahar.example}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-azahar-dev-admin-2026}"
EMPLOYEE_EMAIL="${EMPLOYEE_EMAIL:-staff@azahar.example}"
EMPLOYEE_PASSWORD="${EMPLOYEE_PASSWORD:-azahar-dev-staff-2026}"
CUSTOMER_EMAIL="${CUSTOMER_EMAIL:-client@azahar.example}"
CUSTOMER_PASSWORD="${CUSTOMER_PASSWORD:-azahar-dev-client-2026}"
TURNSTILE_SECRET_KEY="${TURNSTILE_SECRET_KEY:-}"
PUBLIC_TURNSTILE_SITE_KEY="${PUBLIC_TURNSTILE_SITE_KEY:-}"
PLUGIN_SECRET_KEY="${PLUGIN_SECRET_KEY:-azahar-dev-plugin-key-2026}"
AI_BASE_URL="${AI_BASE_URL:-https://api.openai.com/v1}"
AI_TIMEOUT_SECONDS="${AI_TIMEOUT_SECONDS:-25}"
TURSO_AUTH_TOKEN="${TURSO_AUTH_TOKEN:-}"
COOKIE_SECURE="${COOKIE_SECURE:-false}"
SEED_DEMO="${SEED_DEMO:-true}"

if [[ "$APP_ENV" == "production" ]]; then
	: "${APP_ORIGIN:?set APP_ORIGIN before running production}"
	: "${ADMIN_PASSWORD:?set ADMIN_PASSWORD before running production}"
	: "${TURNSTILE_SECRET_KEY:?set TURNSTILE_SECRET_KEY before running production}"
	: "${PUBLIC_TURNSTILE_SITE_KEY:?set PUBLIC_TURNSTILE_SITE_KEY before running production}"
	: "${PLUGIN_SECRET_KEY:?set PLUGIN_SECRET_KEY before running production}"
	COOKIE_SECURE=true
fi

make_admin_token() {
	if command -v openssl >/dev/null 2>&1; then
		openssl rand -hex 24
	else
		node -e "process.stdout.write(require('node:crypto').randomBytes(24).toString('hex'))"
	fi
}
ADMIN_ACCESS_TOKEN="${ADMIN_ACCESS_TOKEN:-$(make_admin_token)}"
export ADMIN_ACCESS_TOKEN

echo "Building the Rust API..."
cargo build --release -p azahar-api

echo "Building the SvelteKit app..."
(cd frontend && APP_ENV="$APP_ENV" PUBLIC_TURNSTILE_SITE_KEY="$PUBLIC_TURNSTILE_SITE_KEY" pnpm build)

echo "Preparing the Kiri network and volume..."
kiri network create azahar-net >/dev/null 2>&1 || true
kiri volume create azahar-db >/dev/null 2>&1 || true

for name in azahar-web azahar-api azahar-db; do
	if kiri ps -a --format '{{.Names}}' 2>/dev/null | grep -qx "$name"; then
		kiri rm -f "$name" >/dev/null
	fi
done

DB_PUBLISH=()
if [[ "$APP_ENV" != "production" ]]; then
	DB_PUBLISH=(-p 127.0.0.1:8880:8080)
fi

echo "Starting the Turso/libSQL server..."
kiri run -d --name azahar-db --network azahar-net \
	"${DB_PUBLISH[@]}" \
	-v azahar-db:/data \
	--entrypoint /bin/sqld ghcr.io/tursodatabase/libsql-server:latest \
	--http-listen-addr 0.0.0.0:8080 -d /data >/dev/null

sleep 4

API_PUBLISH=()
if [[ "$APP_ENV" != "production" ]]; then
	API_PUBLISH=(-p 127.0.0.1:8080:8080)
fi

echo "Starting the Rust API..."
kiri run -d --name azahar-api --network azahar-net \
	"${API_PUBLISH[@]}" \
	-v "$ROOT/target/release/azahar-api:/azahar-api:ro" \
	-e APP_ENV="$APP_ENV" \
	-e API_ADDR=:8080 \
	-e APP_ORIGIN="$APP_ORIGIN" \
	-e TURSO_DATABASE_URL=http://azahar-db:8080 \
	-e TURSO_AUTH_TOKEN="$TURSO_AUTH_TOKEN" \
	-e COOKIE_SECURE="$COOKIE_SECURE" \
	-e TRUST_PROXY=true \
	-e SEED_DEMO="$SEED_DEMO" \
	-e ADMIN_EMAIL="$ADMIN_EMAIL" \
	-e ADMIN_PASSWORD="$ADMIN_PASSWORD" \
	-e EMPLOYEE_EMAIL="$EMPLOYEE_EMAIL" \
	-e EMPLOYEE_PASSWORD="$EMPLOYEE_PASSWORD" \
	-e CUSTOMER_EMAIL="$CUSTOMER_EMAIL" \
	-e CUSTOMER_PASSWORD="$CUSTOMER_PASSWORD" \
	-e TURNSTILE_SECRET_KEY="$TURNSTILE_SECRET_KEY" \
	-e PLUGIN_SECRET_KEY="$PLUGIN_SECRET_KEY" \
	-e AI_BASE_URL="$AI_BASE_URL" \
	-e AI_TIMEOUT_SECONDS="$AI_TIMEOUT_SECONDS" \
	gcr.io/distroless/cc-debian12:nonroot /azahar-api >/dev/null

sleep 2
if ! kiri ps --format '{{.Names}}' 2>/dev/null | grep -qx azahar-api; then
	echo "The Rust API did not stay up. Logs:" >&2
	kiri logs azahar-api >&2 || true
	exit 1
fi

echo "Starting the web app..."
kiri run -d --name azahar-web --network azahar-net -p 3000:3000 \
	-v "$ROOT/frontend:/app:ro" \
	-w /app \
	--user node \
	-e APP_ENV="$APP_ENV" \
	-e PORT=3000 \
	-e HOST=0.0.0.0 \
	-e INTERNAL_API_URL=http://azahar-api:8080 \
	-e ORIGIN="$APP_ORIGIN" \
	-e ADMIN_ACCESS_TOKEN="$ADMIN_ACCESS_TOKEN" \
	-e PUBLIC_TURNSTILE_SITE_KEY="$PUBLIC_TURNSTILE_SITE_KEY" \
	node:22-alpine node build >/dev/null

sleep 2
echo
echo "Azahar (Rust API) is running at $APP_ORIGIN"
echo "Admin gate: $APP_ORIGIN/ops-$ADMIN_ACCESS_TOKEN"
if [[ "$APP_ENV" != "production" ]]; then
	echo "Desktop admin API: http://127.0.0.1:8080 (development only)"
	echo "Admin login: $ADMIN_EMAIL / $ADMIN_PASSWORD"
	echo "Employee login: $EMPLOYEE_EMAIL / $EMPLOYEE_PASSWORD"
	echo "Customer: $CUSTOMER_EMAIL / $CUSTOMER_PASSWORD"
fi
echo "Stop with ./deploy/kiri-down.sh"
