import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { receiptHtml } from '../src/lib/admin/domain/receipts.js';
import { ORDER_STATUSES, STATUS_TRANSITIONS } from '../src/lib/admin/domain/statuses.js';
import {
	COURIER_STATUS_LABELS,
	courierLabel,
	courierStatusLabel,
	deliveryTypeLabel,
	sentenceCase,
	titleCase
} from '../src/lib/admin/domain/labels.js';

const order = {
	number: 'AZ-2026-0100',
	created_at: '2026-11-21T12:00:00Z',
	name: '<script>alert(1)</script>',
	email: 'buyer@example.com',
	phone: '0550123456',
	address: '12 rue des Oliviers',
	city: 'Bab Ezzouar',
	wilaya_name: 'Alger',
	commune_name: 'Bab Ezzouar',
	status: 'pending',
	delivery_type: 'home',
	currency: 'DZD',
	notes: 'Ring twice',
	items: [
		{
			name: 'Green fig body oil',
			quantity: 3,
			unit_price_cents: 4000,
			line_total_cents: 123400,
			options_label: 'Size: 100 ml'
		}
	],
	subtotal_cents: 123400,
	shipping_cents: 0,
	total_cents: 123400
};

test('a receipt keeps the order currency and escapes customer text', () => {
	const html = receiptHtml('Azahar', { ...order, currency: 'EUR' });
	assert.match(html, /1,234\u00a0EUR/);
	assert.doesNotMatch(html, /DZD/);
	assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/);
	assert.doesNotMatch(html, /<script>alert\(1\)<\/script>/);
	assert.match(html, /Ring twice/);
});

test('a receipt uses the stored line total, not unit price times quantity', () => {
	const html = receiptHtml('Azahar', order);
	assert.match(html, /1,234\u00a0DZD/);
	assert.doesNotMatch(html, /120\u00a0DZD/);
	assert.match(html, /Size: 100 ml/);
	assert.match(html, /<span>Shipping<\/span><span>Free<\/span>/);
});

test('a receipt names the counter person only when the caller passes one', () => {
	const plain = receiptHtml('Azahar', order);
	assert.doesNotMatch(plain, /Served by/);
	const plainStatus = plain.slice(plain.indexOf('<p class="label">Status</p>'));
	assert.match(plainStatus, /21 Nov 2026, 12:00/);

	const counter = receiptHtml('Azahar', order, 'Amina');
	assert.match(counter, /Served by Amina/);
	const counterStatus = counter.slice(counter.indexOf('<p class="label">Status</p>'));
	assert.doesNotMatch(counterStatus, /21 Nov 2026/);

	const paid = receiptHtml('Azahar', { ...order, delivery_type: 'pickup', status: 'delivered' });
	assert.match(paid, /Paid in cash/);
});

test('order statuses and transitions match docs/API.md', () => {
	const api = readFileSync(new URL('../../docs/API.md', import.meta.url), 'utf8');
	const start = api.indexOf('Allowed transitions:');
	const end = api.indexOf('Invalid transition', start);
	assert.ok(start > 0 && end > start, 'docs/API.md documents the allowed transitions');

	/** @type {Record<string, string[]>} */
	const documented = {};
	for (const line of api.slice(start, end).split('\n')) {
		const trimmed = line.trim();
		const transition = /^- `([a-z_]+)` to (.+)$/.exec(trimmed);
		if (transition) {
			documented[transition[1]] = [...transition[2].matchAll(/`([a-z_]+)`/g)].map(
				(match) => match[1]
			);
			continue;
		}
		const terminal = /^- (.+) are terminal$/.exec(trimmed);
		if (terminal) {
			for (const match of terminal[1].matchAll(/`([a-z_]+)`/g)) documented[match[1]] = [];
		}
	}

	assert.deepEqual(Object.keys(documented), ORDER_STATUSES);
	assert.deepEqual(STATUS_TRANSITIONS, documented);
	for (const [from, targets] of Object.entries(STATUS_TRANSITIONS)) {
		for (const target of targets) {
			assert.ok(ORDER_STATUSES.includes(target), `${from} -> ${target} is a known status`);
		}
	}
});

test('courier and delivery labels stay human', () => {
	assert.equal(Object.keys(COURIER_STATUS_LABELS).length, 13);
	for (const [status, label] of Object.entries(COURIER_STATUS_LABELS)) {
		assert.ok(label.length > 0, status);
		assert.notEqual(label, status, status);
	}

	assert.equal(courierStatusLabel('pending_pickup'), 'Waiting for pickup');
	assert.equal(courierStatusLabel('out_for_delivery'), 'Out for delivery');
	assert.equal(courierStatusLabel(undefined), 'Not tracked yet');
	assert.equal(courierStatusLabel('new_state'), 'New state');

	assert.equal(courierLabel('sandbox'), 'Sandbox courier');
	assert.equal(courierLabel(undefined), 'No courier');
	assert.equal(courierLabel('zr_express'), 'Zr express');

	assert.equal(deliveryTypeLabel('pickup'), 'Counter sale');
	assert.equal(deliveryTypeLabel('stopdesk'), 'Stop desk');
	assert.equal(deliveryTypeLabel('home'), 'Home delivery');
	assert.equal(titleCase('out_for_delivery'), 'Out For Delivery');
	assert.equal(sentenceCase('out_for_delivery'), 'Out for delivery');
	assert.equal(sentenceCase(''), '');
});
