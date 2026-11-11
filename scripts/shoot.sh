#!/usr/bin/env bash
set -euo pipefail

URL="${1:-http://localhost:3000}"
OUT="${2:-shots}"
CHROME="${CHROME_PATH:-/usr/bin/chromium}"

mkdir -p "$OUT"

for width in 1440 1024 768 390; do
	echo "Shooting ${URL} at ${width}px"
	"$CHROME" \
		--headless=new \
		--no-sandbox \
		--disable-gpu \
		--hide-scrollbars \
		--virtual-time-budget=3000 \
		--window-size="${width},1000" \
		--screenshot="${OUT}/home-${width}.png" \
		"$URL" >/dev/null 2>&1
done

echo "Saved to ${OUT}/"
