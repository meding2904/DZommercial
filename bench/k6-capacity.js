// Saturation read mix for the Azahar API, driven by Grafana k6.
//
//   k6 run bench/k6-capacity.js
//
// Unlike k6-read.js, this script keeps no checks and discards response bodies,
// so the client spends its CPU on connections and requests rather than JS
// assertions and body retention. Use it with several k6 processes to saturate
// the server; k6-read.js remains the latency/read-mix comparison.

import http from 'k6/http';

const BASE = __ENV.BASE_URL || 'http://127.0.0.1:18120';
const VUS = Number(__ENV.VUS || 50);
const IP_POOL = Number(__ENV.IP_POOL || 200000);
const IP_OFFSET = Number(__ENV.IP_OFFSET || 0);

const PATHS = [
  '/api/products',
  '/api/categories',
  '/api/plugins',
  '/api/storefront/theme',
  '/api/products/rose-petal-bath-soak',
  '/api/health',
];

export const options = {
  scenarios: {
    capacity: {
      executor: 'constant-vus',
      vus: VUS,
      duration: __ENV.DURATION || '15s',
      gracefulStop: '5s',
    },
  },
  discardResponseBodies: true,
  summaryTrendStats: ['avg', 'min', 'med', 'p(95)', 'p(99)', 'max'],
};

function addressFor(serial) {
  return `10.${(serial >> 16) & 0xff}.${(serial >> 8) & 0xff}.${serial & 0xff}`;
}

export default function () {
  const serial = ((IP_OFFSET + __VU * 7919 + __ITER * 104729) % IP_POOL) + 1;
  const path = PATHS[(__VU * 7 + __ITER) % PATHS.length];
  http.get(`${BASE}${path}`, {
    headers: {
      Accept: 'application/json',
      'X-Forwarded-For': addressFor(serial),
    },
  });
}
