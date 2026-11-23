import test from 'node:test';
import assert from 'node:assert/strict';

import { installStorage } from './helpers/browser.mjs';

installStorage();
const { parseRoute } = await import('../router.js');

test('parseRoute accepts a known route and reads only the first segment', () => {
	assert.equal(parseRoute('#/ledger'), 'ledger');
	assert.equal(parseRoute('#/orders/42'), 'orders');
	assert.equal(parseRoute('#counter'), 'counter');
	assert.equal(parseRoute('#/products'), 'products');
	assert.equal(parseRoute('#/customers'), 'customers');
	assert.equal(parseRoute('#/staff'), 'staff');
	assert.equal(parseRoute('#/connection'), 'connection');
});

test('parseRoute falls back to the ledger', () => {
	assert.equal(parseRoute(''), 'ledger');
	assert.equal(parseRoute('#/'), 'ledger');
	assert.equal(parseRoute('#/unknown'), 'ledger');
	assert.equal(parseRoute('#//orders'), 'ledger');
});
