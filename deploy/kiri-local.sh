#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v kiri >/dev/null 2>&1; then
	echo "kiri is not installed." >&2
	exit 1
fi

APP_ENV="${APP_ENV:-development}"
APP_ENV="${APP_ENV,,}"

if [[ "$APP_ENV" == "production" ]]; then
	: "${APP_ORIGIN:?set APP_ORIGIN to the public https origin before running production}"
	: "${ADMIN_EMAIL:?set ADMIN_EMAIL before running production}"
	: "${ADMIN_PASSWORD:?set a unique ADMIN_PASSWORD before running production}"
	: "${TURSO_AUTH_TOKEN:?set TURSO_AUTH_TOKEN before running production}"
	if [[ "$ADMIN_PASSWORD" == "azahar-dev-admin-2026" ]]; then
		echo "error: ADMIN_PASSWORD must not be the development default." >&2
		exit 1
	fi
	if [[ "${SEED_DEMO:-false}" == "true" ]]; then
		: "${CUSTOMER_PASSWORD:?set CUSTOMER_PASSWORD when SEED_DEMO=true}"
		if [[ "$CUSTOMER_PASSWORD" == "azahar-dev-client-2026" ]]; then
			echo "error: CUSTOMER_PASSWORD must not be the development default." >&2
			exit 1
		fi
	fi
	if [[ "${COOKIE_SECURE:-}" == "false" ]]; then
		echo "warning: COOKIE_SECURE=false is ignored in production." >&2
	fi
	SEED_DEMO="${SEED_DEMO:-false}"
	COOKIE_SECURE=true
fi

APP_ORIGIN="${APP_ORIGIN:-http://localhost:3000}"
ADMIN_EMAIL="${ADMIN_EMAIL:-admin@azahar.example}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-azahar-dev-admin-2026}"
CUSTOMER_EMAIL="${CUSTOMER_EMAIL:-client@azahar.example}"
CUSTOMER_PASSWORD="${CUSTOMER_PASSWORD:-azahar-dev-client-2026}"
COOKIE_SECURE="${COOKIE_SECURE:-false}"
SEED_DEMO="${SEED_DEMO:-true}"
TURSO_AUTH_TOKEN="${TURSO_AUTH_TOKEN:-}"

make_admin_token() {
	if command -v openssl >/dev/null 2>&1; then
		openssl rand -hex 24
	else
		node -e "process.stdout.write(require('node:crypto').randomBytes(24).toString('hex'))"
	fi
}

ADMIN_ACCESS_TOKEN="${ADMIN_ACCESS_TOKEN:-$(make_admin_token)}"
export ADMIN_ACCESS_TOKEN

echo "Building the Go API..."
mkdir -p deploy/bin
(cd backend && CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o ../deploy/bin/azahar-api ./cmd/api)

echo "Building the SvelteKit app..."
(cd frontend && pnpm build)

echo "Preparing the Kiri network and volume..."
kiri network create azahar-net >/dev/null 2>&1 || true
kiri volume create azahar-db >/dev/null 2>&1 || true

for name in azahar-web azahar-api azahar-db; do
	if kiri ps -a --format '{{.Names}}' 2>/dev/null | grep -qx "$name"; then
		kiri rm -f "$name" >/dev/null
	fi
done

echo "Starting the Turso/libSQL server..."
kiri run -d --name azahar-db --network azahar-net \
	-v azahar-db:/data \
	--entrypoint /bin/sqld ghcr.io/tursodatabase/libsql-server:latest \
	--http-listen-addr 0.0.0.0:8080 -d /data >/dev/null

sleep 3

echo "Starting the API..."
kiri run -d --name azahar-api --network azahar-net \
	-v "$ROOT/deploy/bin/azahar-api:/azahar-api:ro" \
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
	-e CUSTOMER_EMAIL="$CUSTOMER_EMAIL" \
	-e CUSTOMER_PASSWORD="$CUSTOMER_PASSWORD" \
	gcr.io/distroless/static-debian12:nonroot /azahar-api >/dev/null

sleep 2
if ! kiri ps --format '{{.Names}}' 2>/dev/null | grep -qx azahar-api; then
	echo "The API did not stay up. Logs:" >&2
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
	node:22-alpine node build >/dev/null

sleep 2
echo
echo "Azahar is running at $APP_ORIGIN"
echo "Admin gate: $APP_ORIGIN/ops-$ADMIN_ACCESS_TOKEN"
echo "Keep the gate link private. It rotates on every restart."
if [[ "$APP_ENV" != "production" ]]; then
	echo "Admin login: $ADMIN_EMAIL / $ADMIN_PASSWORD"
	echo "Customer: $CUSTOMER_EMAIL / $CUSTOMER_PASSWORD"
fi
echo "Stop with ./deploy/kiri-down.sh"
