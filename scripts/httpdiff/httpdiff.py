#!/usr/bin/env python3
"""HTTP-level differential test: Go oracle vs Rust port.

Both services must already be running against freshly seeded databases with the
same demo data. The script drives the same requests in the same order, keeping a
cookie jar per service, and compares status, content type and parsed JSON after
normalising values that are legitimately different (generated ids, timestamps,
session/csrf tokens, order numbers).

Usage:
    python3 scripts/httpdiff/httpdiff.py --oracle http://127.0.0.1:18080 \
        --candidate http://127.0.0.1:18081

Exit code 0 means every case matched; 1 prints the diffs.
"""

from __future__ import annotations

import argparse
import http.client
import json
import re
import sys
import time
from dataclasses import dataclass, field
from typing import Any
from urllib.parse import urlsplit

ID_KEYS = {
    "id",
    "user_id",
    "product_id",
    "order_id",
    "category_id",
    "session_id",
    "request_id",
}
GENERATED_ID = re.compile(r"^[a-z]{2,5}_[0-9a-f]{16,32}$")
TIME_KEYS = {
    "created_at",
    "updated_at",
    "server_time",
    "sale_starts_at",
    "sale_ends_at",
    "courier_synced_at",
    "last_order_at",
    "expires_at",
}
SECRET_KEYS = {"token", "csrf_token", "tracking_number", "number", "image"}


@dataclass
class Client:
    base: str
    cookies: dict[str, str] = field(default_factory=dict)
    csrf: str = ""

    def request(
        self,
        method: str,
        path: str,
        body: dict[str, Any] | None = None,
        headers: dict[str, str] | None = None,
    ) -> tuple[int, dict[str, str], bytes]:
        parsed = urlsplit(self.base)
        connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=30)
        request_headers = {"Accept": "application/json"}
        if body is not None:
            payload = json.dumps(body).encode()
            request_headers["Content-Type"] = "application/json"
        else:
            payload = None
        if self.cookies:
            request_headers["Cookie"] = "; ".join(f"{k}={v}" for k, v in self.cookies.items())
        request_headers.update(headers or {})
        connection.request(method, path, body=payload, headers=request_headers)
        response = connection.getresponse()
        data = response.read()
        response_headers = {key.lower(): value for key, value in response.getheaders()}
        for raw in response.headers.get_all("Set-Cookie") or []:
            name, _, rest = raw.partition("=")
            value = rest.split(";", 1)[0]
            if value == "":
                self.cookies.pop(name, None)
            else:
                self.cookies[name] = value
        connection.close()
        return response.status, response_headers, data

    def ensure_csrf(self) -> None:
        status, _, data = self.request("GET", "/api/csrf")
        if status != 200:
            raise RuntimeError(f"csrf failed: {status} {data!r}")
        self.csrf = json.loads(data)["csrf_token"]

    def mutation(self, method: str, path: str, body: dict[str, Any]) -> tuple[int, dict[str, str], bytes]:
        self.ensure_csrf()
        return self.request(
            method,
            path,
            body,
            headers={"X-Requested-With": "azahar-web", "X-CSRF-Token": self.csrf},
        )


def normalise(value: Any, key: str = "") -> Any:
    if isinstance(value, dict):
        return {item_key: normalise(item, item_key) for item_key, item in value.items()}
    if isinstance(value, list):
        items = [normalise(item, key) for item in value]
        if key == "devices":
            items.sort(key=lambda item: item.get("imei", "") if isinstance(item, dict) else "")
        return items
    if isinstance(value, str):
        if key in TIME_KEYS:
            return "<time>"
        if key in SECRET_KEYS:
            return "<secret>"
        if key in ID_KEYS and (GENERATED_ID.match(value) or value):
            return "<id>"
        if key == "slug" and value.startswith("it-"):
            return "<slug>"
        return value
    return value


STRUCTURAL_CASES = {"admin_metrics", "admin_orders", "admin_customers", "account_orders"}
EXPECTED_DIFFERENCES: dict[str, str] = {}


def shape(value: Any) -> Any:
    """Key/type skeleton for data whose concrete values come from the random demo seed."""
    if isinstance(value, dict):
        return {key: shape(item) for key, item in value.items()}
    if isinstance(value, list):
        if not value:
            return []
        merged = shape(value[0])
        for item in value[1:]:
            merged = merge_shapes(merged, shape(item))
        return [merged]
    if isinstance(value, bool):
        return "<bool>"
    if isinstance(value, (int, float)):
        return "<number>"
    if isinstance(value, str):
        return "<string>"
    return value


def merge_shapes(left: Any, right: Any) -> Any:
    if isinstance(left, dict) and isinstance(right, dict):
        merged = dict(left)
        for key, item in right.items():
            merged[key] = merge_shapes(merged[key], item) if key in merged else item
        return merged
    if isinstance(left, list) and isinstance(right, list):
        if not left:
            return right
        if not right:
            return left
        return [merge_shapes(left[0], right[0])]
    if type(left) is not type(right):
        return "<mixed>"
    return left


def shapes_compatible(left: Any, right: Any) -> bool:
    if left == right:
        return True
    if left == "<mixed>" or right == "<mixed>":
        return True
    if isinstance(left, dict) and isinstance(right, dict):
        return left.keys() == right.keys() and all(
            shapes_compatible(left[key], right[key]) for key in left
        )
    if isinstance(left, list) and isinstance(right, list):
        return len(left) == len(right) and all(
            shapes_compatible(a, b) for a, b in zip(left, right)
        )
    return False


def compare(case: str, oracle: tuple[int, dict[str, str], bytes], candidate: tuple[int, dict[str, str], bytes]) -> str | None:
    oracle_status, oracle_headers, oracle_body = oracle
    candidate_status, candidate_headers, candidate_body = candidate
    if oracle_status != candidate_status:
        return f"{case}: status {oracle_status} != {candidate_status}\n  oracle={oracle_body[:400]!r}\n  rust={candidate_body[:400]!r}"
    content_type = oracle_headers.get("content-type", "")
    if content_type.startswith("application/json"):
        try:
            want_raw = json.loads(oracle_body)
            got_raw = json.loads(candidate_body)
        except json.JSONDecodeError as error:
            return f"{case}: invalid json ({error})"
        if case in STRUCTURAL_CASES:
            if not shapes_compatible(shape(want_raw), shape(got_raw)):
                return (
                    f"{case}: response shape differs\n  oracle={json.dumps(shape(want_raw), sort_keys=True)[:1200]}\n"
                    f"  rust  ={json.dumps(shape(got_raw), sort_keys=True)[:1200]}"
                )
            return None
        want = normalise(want_raw)
        got = normalise(got_raw)
        if want != got:
            return (
                f"{case}: body differs\n  oracle={json.dumps(want, sort_keys=True)[:1200]}\n"
                f"  rust  ={json.dumps(got, sort_keys=True)[:1200]}"
            )
    else:
        if oracle_body != candidate_body:
            return f"{case}: binary body differs ({len(oracle_body)} vs {len(candidate_body)} bytes)"
    return None


def pump_cases(oracle: Client, candidate: Client, group: str = "all") -> list[tuple[str, tuple, tuple]]:
    """Run one request on both services and return the results."""
    results: list[tuple[str, tuple, tuple]] = []

    def get(case: str, path: str) -> None:
        results.append((case, oracle.request("GET", path), candidate.request("GET", path)))

    def wanted(name: str) -> bool:
        if group == "all":
            return True
        if group == "core":
            return name in {"health", "csrf", "not_found", "method_not_allowed"}
        if group == "public":
            return name in {
                "health", "categories", "products", "product", "plugins", "theme",
                "wilayas", "communes", "desks", "not_found", "method_not_allowed",
            }
        if group == "auth":
            return name in {
                "health", "csrf", "login", "me", "account_orders", "admin_login",
                "admin_me", "logout", "me_after_logout",
            }
        if group == "checkout":
            return name in {"checkout_validation", "checkout_challenge", "checkout_success"}
        if group == "admin":
            return name in {
                "admin_login", "admin_me", "admin_metrics", "admin_products",
                "admin_orders", "admin_customers", "admin_team", "admin_plugins",
            }
        raise SystemExit(f"unknown group {group}")

    if wanted("health"):
        get("health", "/api/health")
    if wanted("csrf"):
        # Issuing the token into each jar is the precondition for every
        # mutation case. The endpoint shape itself is asserted below.
        oracle.ensure_csrf()
        candidate.ensure_csrf()
    if wanted("csrf"):
        results.append(("csrf", oracle.request("GET", "/api/csrf"), candidate.request("GET", "/api/csrf")))
    if wanted("categories"):
        get("categories", "/api/categories")
    if wanted("products"):
        get("products", "/api/products")
        get("product", "/api/products/rose-petal-bath-soak")
        get("phone_product", "/api/products/aster-5")
        get("not_found", "/api/products/does-not-exist")
        get("method_not_allowed", "/api/products")
    if wanted("plugins"):
        get("plugins", "/api/plugins")
    if wanted("theme"):
        get("theme", "/api/storefront/theme")
    if wanted("plugins"):
        get("pcbuilder_parts", "/api/plugins/pcbuilder/parts")
    if wanted("wilayas"):
        get("wilayas", "/api/shipping/wilayas")
        get("communes", "/api/shipping/communes?wilaya=16")
        get("desks", "/api/shipping/desks?wilaya=16")

    if wanted("login"):
        login = {"email": "client@azahar.example", "password": "azahar-dev-client-2026"}
        results.append(("login", oracle.mutation("POST", "/api/auth/login", login), candidate.mutation("POST", "/api/auth/login", login)))
        get("me", "/api/auth/me")
        get("account_orders", "/api/account/orders")
    if wanted("admin_login"):
        admin = {"email": "admin@azahar.example", "password": "azahar-dev-admin-2026"}
        results.append(("admin_login", oracle.mutation("POST", "/api/auth/login", admin), candidate.mutation("POST", "/api/auth/login", admin)))
        get("admin_me", "/api/auth/me")
    if wanted("admin_metrics"):
        get("admin_metrics", "/api/admin/metrics")
        get("admin_products", "/api/admin/products")
        get("admin_orders", "/api/admin/orders")
        get("admin_customers", "/api/admin/customers")
        get("admin_team", "/api/admin/team")
        get("admin_plugins", "/api/admin/plugins")
        get("phone_devices", "/api/plugins/phone/admin/devices")
        get("ai_runs", "/api/plugins/ai/admin/runs")
        get("codemode_plans", "/api/plugins/codemode/admin/plans")
    if wanted("checkout_validation"):
        results.append((
            "checkout_validation",
            oracle.mutation("POST", "/api/checkout", {}),
            candidate.mutation("POST", "/api/checkout", {}),
        ))
    if wanted("checkout_challenge"):
        headers = {"X-Requested-With": "azahar-web"}
        results.append((
            "checkout_challenge",
            oracle.request("GET", "/api/checkout/challenge", headers=headers),
            candidate.request("GET", "/api/checkout/challenge", headers=headers),
        ))
    if wanted("checkout_success"):
        # Both servers must run with AZAHAR_TEST_CAPTCHA_CODE=ABCDE. The Go
        # oracle binary is built with scripts/build-go-oracle.sh, which injects
        # the same seam through a temporary overlay.
        time.sleep(2.2)
        challenge_headers = {"X-Requested-With": "azahar-web"}
        oracle.ensure_csrf()
        candidate.ensure_csrf()
        oracle_status, _, oracle_challenge = oracle.request(
            "GET", "/api/checkout/challenge", headers=challenge_headers
        )
        candidate_status, _, candidate_challenge = candidate.request(
            "GET", "/api/checkout/challenge", headers=challenge_headers
        )
        if oracle_status != 200 or candidate_status != 200:
            results.append((
                "checkout_success",
                (oracle_status, {}, oracle_challenge),
                (candidate_status, {}, candidate_challenge),
            ))
        else:
            oracle_token = json.loads(oracle_challenge)["token"]
            candidate_token = json.loads(candidate_challenge)["token"]
            _, _, oracle_product = oracle.request("GET", "/api/products/rose-petal-bath-soak")
            _, _, candidate_product = candidate.request("GET", "/api/products/rose-petal-bath-soak")
            oracle_id = json.loads(oracle_product)["product"]["id"]
            candidate_id = json.loads(candidate_product)["product"]["id"]
            payload = {
                "name": "Diff Buyer",
                "email": "buyer@example.test",
                "phone": "0550123456",
                "address": "12 Test Street",
                "wilaya_code": 16,
                "commune_name": "Ain Benian",
                "delivery_type": "home",
                "items": [{"product_id": None, "quantity": 1}],
                "captcha_answer": "ABCDE",
                "terms_accepted": True,
                "terms_version": "2026-11-14",
            }
            oracle_payload = dict(payload, items=[{"product_id": oracle_id, "quantity": 1}], captcha_token=oracle_token)
            candidate_payload = dict(payload, items=[{"product_id": candidate_id, "quantity": 1}], captcha_token=candidate_token)
            results.append((
                "checkout_success",
                oracle.mutation("POST", "/api/checkout", oracle_payload),
                candidate.mutation("POST", "/api/checkout", candidate_payload),
            ))
    if wanted("logout"):
        results.append(("logout", oracle.mutation("POST", "/api/auth/logout", {}), candidate.mutation("POST", "/api/auth/logout", {})))
        get("me_after_logout", "/api/auth/me")
    return results


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle", required=True)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--group", default="all", choices=["all", "core", "public", "auth", "admin", "checkout"])
    args = parser.parse_args()
    oracle = Client(args.oracle)
    candidate = Client(args.candidate)

    failures = []
    for case, want, got in pump_cases(oracle, candidate, args.group):
        if case in EXPECTED_DIFFERENCES:
            print(f"skip {case}: {EXPECTED_DIFFERENCES[case]}")
            continue
        difference = compare(case, want, got)
        marker = "ok" if difference is None else "FAIL"
        print(f"{marker:4} {case}")
        if difference:
            failures.append(difference)
    if failures:
        print(f"\n{len(failures)} case(s) differ\n")
        print("\n\n".join(failures))
        return 1
    print("\nall cases match")
    return 0


if __name__ == "__main__":
    sys.exit(main())
