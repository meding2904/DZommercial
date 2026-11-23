import test from 'node:test';
import assert from 'node:assert/strict';

import { counterView } from '../views/counter.js';
import { connectionView } from '../views/connection.js';
import { customersView } from '../views/customers.js';
import { ledgerView } from '../views/ledger.js';
import { orderDialogView, ordersTableBody, ordersView } from '../views/orders.js';
import { productsView } from '../views/products.js';
import { staffView } from '../views/staff.js';

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

test('ledger view tolerates a metrics payload without a status split', () => {
	const html = ledgerView({ metrics: { ...metrics, status_split: undefined }, metricsDays: 7 });
	assert.match(html, /0 orders still open/);
	assert.match(html, /Needs attention/);
});

test('orders view groups the status tabs and marks the active filter', () => {
	const html = ordersView({ orderFilter: 'pending', orderQuery: '' });
	assert.match(html, /role="group" aria-label="Order status"/);
	assert.match(html, /data-order-filter="pending" aria-pressed="true"/);
	assert.match(html, /data-order-filter="confirmed" aria-pressed="false"/);
	assert.match(html, /id="refresh-orders"/);
});

test('orders table and order dialog label delivery through the shared domain module', () => {
	const table = ordersTableBody([order]);
	assert.match(table, /Stop desk/);

	const dialog = orderDialogView(order, { role: 'employee' });
	assert.match(dialog, /<dt>Delivery<\/dt><dd>Stop desk<\/dd>/);
	assert.match(dialog, /id="order-dialog-title">AZ-1001/);
});

test('order dialog offers the status move and hides cancel from an employee', () => {
	const employee = orderDialogView(order, { role: 'employee' });
	assert.match(employee, /data-order-status="confirmed">Mark Confirmed/);
	assert.doesNotMatch(employee, /data-cancel-order/);
	assert.match(employee, /data-create-shipment/);

	const admin = orderDialogView(order, { role: 'admin' });
	assert.match(admin, /data-cancel-order/);
});

test('counter view disables take cash on an empty ticket', () => {
	const html = counterView({
		counterQuery: '',
		products: [],
		counterLines: [],
		counterCustomer: '',
		counterPhone: '',
		counterNote: ''
	});
	assert.match(html, /id="take-cash" disabled/);
	assert.match(html, /id="clear-ticket" disabled/);
	assert.match(html, /No active product matches that\./);
});

test('products view counts the lines at or below five units', () => {
	const product = {
		id: 'p1',
		name: 'Shirt',
		slug: 'shirt',
		stock: 2,
		active: true,
		price_cents: 5000,
		image: ''
	};
	const html = productsView({
		products: [product],
		productQuery: '',
		productLowOnly: false,
		user: { id: 'u1', role: 'admin' }
	});
	assert.match(html, /1 lines · 1 at or below five units/);
	assert.match(html, /data-stock-up="p1"/);
});

test('staff view offers a role select for other members to an administrator', () => {
	const html = staffView({
		user: { id: 'u1', role: 'admin' },
		team: [
			{ id: 'u1', name: 'Owner', email: 'owner@example.com', role: 'admin', created_at: '' },
			{ id: 'u2', name: 'Sam', email: 'sam@example.com', role: 'employee', created_at: '' }
		]
	});
	assert.match(html, /data-role-for="u2"/);
	assert.doesNotMatch(html, /data-role-for="u1"/);
	assert.match(html, /<span class="cell-sub">You<\/span>/);
});

test('customers view counts the accounts it lists', () => {
	const html = customersView({
		customers: [{ name: 'Amina', email: 'a@example.com', orders_count: 2, total_spent_cents: 1000 }],
		customerQuery: ''
	});
	assert.match(html, /1 accounts with an order history/);
	assert.match(html, /a@example.com/);
});

test('connection view keeps the live server state rows', () => {
	const html = connectionView({
		info: { version: '2.0.0', platform: 'linux', resolvedOrigin: 'http://localhost:3000' },
		health: { ok: true }
	});
	assert.match(html, /id="app-server-state">Connected/);
	assert.match(html, /id="app-resolved-origin">http:\/\/localhost:3000/);
	assert.match(html, /id="app-clear-session"/);
});

test('login connection fields are shared with the connection page', () => {
	const html = connectionView({ info: null, health: null });
	assert.match(html, /id="app-api"/);
	assert.match(html, /id="app-firewall-guide"/);
});
