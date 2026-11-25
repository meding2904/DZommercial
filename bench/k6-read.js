// Keep-alive read mix for the Azahar API, driven by Grafana k6.
//
//   k6 run bench/k6-read.js
//   BASE_URL=http://127.0.0.1:18100 VUS=50 DURATION=20s \
//     IP_POOL=8192 k6 run bench/k6-read.js
//
// Every request carries a rotating X-Forwarded-For address. Keep IP_POOL
// below the 10,000-bucket rate-limiter cap to measure the normal read path;
// raise it above that to exercise bucket eviction.

import http from 'k6/http';
import { check } from 'k6';

const BASE = __ENV.BASE_URL || 'http://127.0.0.1:18100';
const VUS = Number(__ENV.VUS || 50);
const IP_POOL = Number(__ENV.IP_POOL || 8192);
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
    reads: {
      executor: 'constant-vus',
      vus: VUS,
      duration: __ENV.DURATION || '20s',
      gracefulStop: '5s',
    },
  },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
  discardResponseBodies: __ENV.DISCARD_BODIES === '1',
};

function addressFor(serial) {
  return `10.${(serial >> 16) & 0xff}.${(serial >> 8) & 0xff}.${serial & 0xff}`;
}

export default function () {
  // 104729 is prime, so multiplying ITER by it walks the whole pool before
  // repeating and keeps one VU's window from overlapping another's. IP_OFFSET
  // lets several k6 processes share the pool without sharing a counter.
  const serial = ((IP_OFFSET + __VU * 7919 + __ITER * 104729) % IP_POOL) + 1;
  const path = PATHS[(__VU * 7 + __ITER) % PATHS.length];
  const response = http.get(`${BASE}${path}`, {
    headers: {
      Accept: 'application/json',
      'X-Forwarded-For': addressFor(serial),
    },
    tags: { path },
  });
  check(response, { 'status is 200': (result) => result.status === 200 });
}
