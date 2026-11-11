#!/usr/bin/env bash
set -euo pipefail

BACKEND_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
TMP_DIR="$(mktemp -d)"
JAR="$TMP_DIR/cookies.txt"
LOG="$TMP_DIR/api.log"
BIN="$TMP_DIR/azahar-api"
PID=""

cleanup() {
	if [[ -n "$PID" ]]; then
		kill "$PID" 2>/dev/null || true
		wait "$PID" 2>/dev/null || true
	fi
	rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cd "$BACKEND_DIR"

if curl -fsS "$BASE_URL/api/products" >/dev/null 2>&1; then
	echo "A server is already listening at $BASE_URL. Stop it before running this script." >&2
	exit 1
fi

go build -o "$BIN" ./cmd/api
"$BIN" >"$LOG" 2>&1 &
PID=$!

for _ in $(seq 1 60); do
	if curl -fsS "$BASE_URL/api/products" >/dev/null 2>&1; then
		break
	fi
	if ! kill -0 "$PID" 2>/dev/null; then
		echo "API exited during startup:" >&2
		cat "$LOG" >&2
		exit 1
	fi
	sleep 0.5
done

if ! curl -fsS "$BASE_URL/api/products" >/dev/null 2>&1; then
	echo "API did not become ready:" >&2
	cat "$LOG" >&2
	exit 1
fi

fail() {
	echo "FAIL: $1" >&2
	if [[ -f "$TMP_DIR/last-response.json" ]]; then
		cat "$TMP_DIR/last-response.json" >&2
	fi
	exit 1
}

products_json="$(curl -fsS "$BASE_URL/api/products")"
printf '%s' "$products_json" >"$TMP_DIR/last-response.json"
product_count="$(printf '%s' "$products_json" | grep -o '"id":"prd_' | wc -l | tr -d ' ')"
[[ "$product_count" == "8" ]] || fail "GET /api/products returned $product_count products, want 8"
echo "PASS GET /api/products returns 8 products"

login_status="$(curl -sS -o "$TMP_DIR/login.json" -D "$TMP_DIR/login.headers" -w '%{http_code}' \
	-c "$JAR" -X POST "$BASE_URL/api/auth/login" \
	-H 'Content-Type: application/json' \
	-H 'X-Requested-With: azahar-web' \
	-H "Origin: ${APP_ORIGIN:-http://localhost:5173}" \
	-d '{"email":"admin@azahar.example","password":"azahar-dev-admin-2026"}')"
[[ "$login_status" == "200" ]] || fail "admin login returned $login_status, want 200"
grep -qi '^set-cookie: azahar_session=' "$TMP_DIR/login.headers" || fail "login did not set azahar_session"
echo "PASS POST /api/auth/login returns 200 and Set-Cookie"

metrics_status="$(curl -sS -o "$TMP_DIR/metrics.json" -w '%{http_code}' \
	-b "$JAR" "$BASE_URL/api/admin/metrics?days=30")"
[[ "$metrics_status" == "200" ]] || fail "metrics returned $metrics_status, want 200"
series_days="$(grep -o '"date":"' "$TMP_DIR/metrics.json" | wc -l | tr -d ' ')"
[[ "$series_days" == "30" ]] || fail "revenue_series has $series_days days, want 30"
echo "PASS GET /api/admin/metrics?days=30 returns 200 with 30 revenue points"

checkout_status="$(curl -sS -o "$TMP_DIR/checkout.json" -w '%{http_code}' \
	-X POST "$BASE_URL/api/checkout" \
	-H 'Content-Type: application/json' \
	-H 'X-Requested-With: azahar-web' \
	-H "Origin: ${APP_ORIGIN:-http://localhost:5173}" \
	-d '{"name":"Amina B.","email":"amina@example.com","phone":"0550 00 00 00","address":"12 rue des Oliviers","city":"Algiers","notes":"Smoke test","items":[{"product_id":"prd_01","quantity":1}]}')"
[[ "$checkout_status" == "201" ]] || fail "checkout returned $checkout_status, want 201"
token="$(sed -n 's/.*"token":"\([^"]*\)".*/\1/p' "$TMP_DIR/checkout.json" | head -n 1)"
[[ -n "$token" ]] || fail "checkout did not return an order token"
echo "PASS POST /api/checkout returns 201 and an order token"

order_status="$(curl -sS -o "$TMP_DIR/order.json" -w '%{http_code}' "$BASE_URL/api/orders/$token")"
[[ "$order_status" == "200" ]] || fail "order lookup returned $order_status, want 200"
grep -q '"number":"AZ-' "$TMP_DIR/order.json" || fail "order lookup did not return an order number"
echo "PASS GET /api/orders/{token} returns 200"

unauth_status="$(curl -sS -o /dev/null -w '%{http_code}' "$BASE_URL/api/admin/metrics")"
[[ "$unauth_status" == "401" ]] || fail "unauthenticated metrics returned $unauth_status, want 401"
echo "PASS GET /api/admin/metrics without a session returns 401"

echo "All smoke checks passed."
