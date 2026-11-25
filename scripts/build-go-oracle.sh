#!/usr/bin/env bash
# Build the Go oracle with a deterministic-captcha test seam, without editing
# the Go tree. The overlay replaces only `captcha.go` with a copy whose issue()
# honours AZAHAR_TEST_CAPTCHA_CODE. Used by the live checkout diff; normal Go
# builds are untouched.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="$ROOT/backend/internal/httpapi/captcha.go"
COPY="$(mktemp --suffix=.go)"
OVERLAY="$(mktemp --suffix=.json)"
trap 'rm -f "$COPY" "$OVERLAY"' EXIT

python3 - "$SOURCE" "$COPY" <<'PY'
import sys

source, target = sys.argv[1], sys.argv[2]
text = open(source, encoding="utf-8").read()
if '\n\t"os"\n' not in text:
    text = text.replace('\t"net/http"\n', '\t"net/http"\n\t"os"\n', 1)
old = '''\tcode, err := randomCaptchaCode(captchaCodeLength)
\tif err != nil {
\t\treturn "", "", 0, err
\t}'''
new = '''\tcode := ""
\tif injected := strings.TrimSpace(os.Getenv("AZAHAR_TEST_CAPTCHA_CODE")); injected != "" {
\t\tcode = injected
\t} else {
\t\tgenerated, err := randomCaptchaCode(captchaCodeLength)
\t\tif err != nil {
\t\t\treturn "", "", 0, err
\t\t}
\t\tcode = generated
\t}'''
if old not in text:
    raise SystemExit("captcha issue snippet not found")
text = text.replace(old, new, 1)
open(target, "w", encoding="utf-8").write(text)
PY

python3 - "$SOURCE" "$COPY" "$OVERLAY" <<'PY'
import json, sys
source, copy, overlay = sys.argv[1:]
json.dump({"Replace": {source: copy}}, open(overlay, "w"))
PY

cd "$ROOT/backend"
go build -overlay "$OVERLAY" -o "${1:-/tmp/azahar-go-seam}" ./cmd/api
echo "built Go oracle with deterministic captcha: ${1:-/tmp/azahar-go-seam}"
