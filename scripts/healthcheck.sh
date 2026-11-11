#!/usr/bin/env bash
set -euo pipefail

BASE="${1:-http://localhost:3000}"
FAILED=0

check() {
	local path="$1"
	local expected="${2:-200}"
	local code
	code="$(curl -s -o /dev/null -w '%{http_code}' "${BASE}${path}")"
	if [[ "$code" == "$expected" ]]; then
		printf 'PASS %s %s\n' "$code" "$path"
	else
		printf 'FAIL %s (expected %s) %s\n' "$code" "$expected" "$path"
		FAILED=1
	fi
}

check /
check /shop
check /shop?category=face
check /product/orange-blossom-cleansing-oil
check /cart
check /checkout
check /about
check /contact
check /policies/shipping
check /credits
check /admin 404
check /admin/login 404
check /%61dmin/login 404
check /api/admin/metrics 404
check /api/admin%5Cmetrics 404
check /api/admin%2Fmetrics 404
check /api/%252e%252e/api/admin/metrics 404
check /api/foo%5C..%5Cadmin%5Cmetrics 404
check /api/%2e%2e%2fapi%2fadmin%2fmetrics 404
check /api/products
check /api/shipping/wilayas
check /api/shipping/communes?wilaya=16
check /api/shipping/desks?wilaya=16
check /nope 404
check /product/does-not-exist 404
check /policies/does-not-exist 404

if [[ "$FAILED" -ne 0 ]]; then
	exit 1
fi

echo "All checks passed at ${BASE}"
