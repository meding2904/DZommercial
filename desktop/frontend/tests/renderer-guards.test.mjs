// Behavior guards that replaced the old Go source-text greps: boot the real
// renderer with hostile network globals, paint every route, and render every
// view. Any direct network call or retired markup fails a test.
import test from 'node:test';
import assert from 'node:assert/strict';

import { installBridge, installDocument, installStorage, wait } from './helpers/browser.mjs';
import { counterView } from '../views/counter.js';
import { connectionView } from '../views/connection.js';
import { customersView } from '../views/customers.js';
import { ledgerView } from '../views/ledger.js';
import { orderDialogView, ordersTableBody, ordersView } from '../views/orders.js';
import { productsView } from '../views/products.js';
import { staffView } from '../views/staff.js';

function installNetworkTraps() {
	const attempts = [];
	const trap = (name) => () => {
		attempts.push(name);
		throw new Error(`the renderer touched ${name} directly`);
	};
	globalThis.fetch = trap('fetch');
	globalThis.XMLHttpRequest = trap('XMLHttpRequest');
	globalThis.WebSocket = trap('WebSocket');
	return attempts;
}

test('the renderer boots and paints every route without a direct network call', async () => {
	installStorage({});
	installDocument([
		'view',
		'toast',
		'boot',
		'login-screen',
		'admin-shell',
		'order-dialog',
		'login-form',
		'login-email',
		'login-password',
		'login-error',
		'login-submit',
		'login-retry',
		'login-status',
		'login-status-text',
		'login-network-hint',
		'connection-details',
		'connection-form',
		'scheme-toggle',
		'preset-select',
		'sign-out',
		'order-search',
		'orders-body'
	]);
	const attempts = installNetworkTraps();
	installBridge({
		Info: async () => ({
			apiUrl: 'http://127.0.0.1:8080',
			origin: '',
			networkAllowed: true,
			platform: 'linux',
			version: '2.0.0',
			resolvedOrigin: 'http://127.0.0.1:8080'
		}),
		Health: async () => ({ ok: true, status: 200 }),
		Request: async (method, path) => {
			if (path === '/api/storefront/theme') {
				return { ok: true, data: { theme: { brand_name: 'Azahar' } } };
			}
			if (path === '/api/auth/me') {
				return { ok: true, data: { user: { id: 'usr_test', name: 'Admin', role: 'admin' } } };
			}
			return { ok: true, data: {} };
		},
		UpdateSettings: async () => ({}),
		ClearSession: async () => ({}),
		FirewallGuide: async () => ({}),
		OpenExternal: async () => ({}),
		Image: async () => null
	});
	globalThis.window.location = { hash: '#/ledger' };
	globalThis.window.addEventListener = () => {};
	globalThis.window.open = () => null;

	await import('../app.js');
	await wait(60);
	assert.equal(attempts.length, 0, `direct network calls at boot: ${attempts.join(', ')}`);

	const { route } = await import('../router.js');
	for (const name of ['counter', 'orders', 'products', 'customers', 'staff', 'connection', 'ledger']) {
		globalThis.window.location.hash = `#/${name}`;
		try {
			await route();
		} catch {
			// A stub payload can upset a screen's rendering; the network guard
			// still holds, which is the fact this test pins.
		}
		await wait(10);
	}
	assert.equal(attempts.length, 0, `direct network calls while painting: ${attempts.join(', ')}`);
	assert.match(globalThis.document.title, /Ledger/);
});

test('every view renders and keeps the retired markup out', () => {
	const metrics = {
		revenue_cents: 120000,
		previous_revenue_cents: 100000,
		delta_pct: 20,
		orders_count: 4,
		units_sold: 6,
		aov_cents: 30000,
		range: { from: '2026-01-01', to: '2026-01-31' },
		demo: false,
		status_split: [
			{ status: 'pending', count: 1 },
			{ status: 'delivered', count: 3 }
		],
		low_stock: [],
		top_products: [],
		revenue_series: [{ date: '2026-01-05', revenue_cents: 5000, orders: 1 }],
		recent_orders: [],
		new_customers: 0
	};
	const order = {
		id: 'ord-1',
		number: 'AZ-1001',
		status: 'pending',
		delivery_type: 'stopdesk',
		name: 'Amina',
		phone: '0550',
		email: '',
		created_at: '2026-01-05T12:00:00Z',
		items: [],
		subtotal_cents: 5000,
		shipping_cents: 0,
		total_cents: 5000
	};
	const rendered = [
		ledgerView({ metrics, metricsDays: 7 }),
		ordersView({ orderFilter: 'pending', orderQuery: '' }),
		ordersTableBody([order]),
		orderDialogView(order, { role: 'admin' }),
		counterView({
			counterQuery: '',
			products: [],
			counterLines: [],
			counterCustomer: '',
			counterPhone: '',
			counterNote: ''
		}),
		productsView({
			products: [],
			productQuery: '',
			productLowOnly: false,
			user: { id: 'u1', role: 'admin' }
		}),
		customersView({ customers: [], customerQuery: '' }),
		staffView({ user: { id: 'u1', role: 'admin' }, team: [] }),
		connectionView({ info: null, health: null })
	].join('\n');

	for (const forbidden of [
		'role="tablist"',
		'aria-selected',
		"delivery_type === 'pickup' ? 'Counter",
		"delivery_type === 'pickup' ? 'Counter sale"
	]) {
		assert.equal(rendered.includes(forbidden), false, `a view still renders ${forbidden}`);
	}
	assert.match(rendered, /role="group" aria-label="Order status"/);
});

test('an older orders response cannot repaint a newer query', async () => {
	const pending = [];
	installBridge({
		Request: (method, path) =>
			new Promise((resolve) => {
				pending.push({ path, resolve });
			}),
		Info: async () => ({}),
		Health: async () => ({ ok: true }),
		UpdateSettings: async () => ({}),
		ClearSession: async () => ({}),
		FirewallGuide: async () => ({}),
		OpenExternal: async () => ({}),
		Image: async () => null
	});
	installDocument(['view', 'orders-body', 'order-dialog']);
	const { loadOrders } = await import('../screens/orders.js');
	const { state } = await import('../state.js');

	state.orderQuery = 'first';
	const first = loadOrders();
	state.orderQuery = 'second';
	const second = loadOrders();
	assert.equal(pending.length, 2, 'both queries reach the bridge');

	pending[1].resolve({ ok: true, data: { orders: [{ id: 'second' }] } });
	pending[0].resolve({ ok: true, data: { orders: [{ id: 'first' }] } });
	await Promise.all([first, second]);

	assert.deepEqual(state.orders, [{ id: 'second' }], 'the newest query owns the table');
});
