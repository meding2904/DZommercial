// Login flow for the Azahar API, driven by Grafana k6.
//
//   k6 run bench/k6-login.js
//   BASE_URL=http://127.0.0.1:18100 VUS=4 ITERATIONS=10 k6 run bench/k6-login.js
//
// Each iteration fetches a CSRF token, signs in with the demo customer, and
// calls /api/auth/me. A per-VU user agent keeps the session layer from
// deleting another VU's session, as in bench.py.

import http from 'k6/http';
import { check } from 'k6';
import { Trend } from 'k6/metrics';

const BASE = __ENV.BASE_URL || 'http://127.0.0.1:18100';
const VUS = Number(__ENV.VUS || 4);
const ITERATIONS = Number(__ENV.ITERATIONS || 10);
const ORIGIN = __ENV.APP_ORIGIN || 'http://127.0.0.1:5173';
const EMAIL = __ENV.LOGIN_EMAIL || 'client@azahar.example';
const PASSWORD = __ENV.LOGIN_PASSWORD || 'azahar-dev-client-2026';

const loginDuration = new Trend('login_flow_duration', true);

export const options = {
  scenarios: {
    login: {
      executor: 'per-vu-iterations',
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: '5m',
      gracefulStop: '30s',
    },
  },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

function addressFor(serial) {
  return `10.${(serial >> 16) & 0xff}.${(serial >> 8) & 0xff}.${serial & 0xff}`;
}

export default function () {
  const userAgent = `azahar-k6/${__VU}-${__ITER}`;
  const clientIp = addressFor(__VU * 1000003 + __ITER + 1);
  const common = {
    'X-Forwarded-For': clientIp,
    'User-Agent': userAgent,
  };

  const csrfResponse = http.get(`${BASE}/api/csrf`, { headers: common, tags: { step: 'csrf' } });
  const csrfToken = csrfResponse.json('csrf_token') || '';

  const started = Date.now();
  const loginResponse = http.post(
    `${BASE}/api/auth/login`,
    JSON.stringify({ email: EMAIL, password: PASSWORD }),
    {
      headers: {
        ...common,
        'Content-Type': 'application/json',
        'X-Requested-With': 'azahar-web',
        Origin: ORIGIN,
        'X-CSRF-Token': csrfToken,
      },
      tags: { step: 'login' },
    },
  );
  const meResponse = http.get(`${BASE}/api/auth/me`, {
    headers: common,
    tags: { step: 'me' },
  });
  loginDuration.add(Date.now() - started);

  check(loginResponse, { 'login is 200': (result) => result.status === 200 });
  check(meResponse, { 'me is 200': (result) => result.status === 200 });
}
