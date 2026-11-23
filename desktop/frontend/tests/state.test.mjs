import test from 'node:test';
import assert from 'node:assert/strict';

import { installStorage } from './helpers/browser.mjs';
import { captureRender } from '../stale.js';

installStorage();
const { state, resetTransientState } = await import('../state.js');

test('the shared staleness guard drops a result once a newer render starts', () => {
	const render = { render: 3 };
	const fresh = captureRender(render);
	if (!fresh()) throw new Error('a fresh render must pass');
	render.render += 1;
	if (fresh()) throw new Error('a later render must invalidate the captured token');
});

test('resetTransientState clears session data and invalidates in-flight renders', (t) => {
	t.mock.timers.enable({ apis: ['setTimeout'] });
	let fired = false;
	state.orderTimer = setTimeout(() => {
		fired = true;
	}, 5000);
	state.render = 5;
	state.metrics = { revenue_cents: 1 };
	state.orders = [{ id: 'o' }];
	state.products = [{ id: 'p' }];
	state.customers = [{ id: 'c' }];
	state.team = [{ id: 't' }];
	state.counterLines = [{ key: 'line' }];
	state.counterQuery = 'query';
	state.counterCustomer = 'name';
	state.counterPhone = 'phone';
	state.counterNote = 'note';
	state.counterReceipt = { number: 'AZ-1' };
	state.orderFilter = 'pending';
	state.orderQuery = 'search';
	state.productQuery = 'search';
	state.productLowOnly = true;
	state.pendingProduct = 'p';
	state.customerQuery = 'search';

	resetTransientState();

	assert.equal(state.counterQuery, '');
	assert.equal(state.render, 6);
	assert.equal(state.orderTimer, null);
	assert.equal(state.metrics, null);
	assert.deepEqual(state.orders, []);
	assert.deepEqual(state.products, []);
	assert.deepEqual(state.customers, []);
	assert.deepEqual(state.team, []);
	assert.deepEqual(state.counterLines, []);
	assert.equal(state.counterCustomer, '');
	assert.equal(state.counterPhone, '');
	assert.equal(state.counterNote, '');
	assert.equal(state.counterReceipt, null);
	assert.equal(state.orderFilter, '');
	assert.equal(state.orderQuery, '');
	assert.equal(state.productQuery, '');
	assert.equal(state.productLowOnly, false);
	assert.equal(state.pendingProduct, '');
	assert.equal(state.customerQuery, '');

	t.mock.timers.tick(6000);
	assert.equal(fired, false);
});
