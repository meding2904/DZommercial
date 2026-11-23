import test from 'node:test';
import assert from 'node:assert/strict';

import {
	changeTicketQuantity,
	mergeTicketLine,
	stockAfterSale,
	ticketCount,
	ticketKey,
	ticketLimit,
	ticketTotal
} from '../ticket.js';

const shirt = { id: 'p1', stock: 3, price_cents: 500 };
const cup = { id: 'p2', stock: 20, effective_price_cents: 1500, price_cents: 2000 };

test('merging the same product and options adds to the existing counter line', () => {
	const first = mergeTicketLine([], shirt, {});
	assert.equal(first.lines.length, 1);
	assert.equal(first.lines[0].key, ticketKey('p1', {}));
	assert.equal(first.lines[0].quantity, 1);
	assert.equal(first.lines[0].unit, 500);

	const second = mergeTicketLine(first.lines, shirt, {});
	assert.equal(second.lines.length, 1);
	assert.equal(second.lines[0].quantity, 2);
});

test('different options make a separate counter line', () => {
	const lines = mergeTicketLine([], shirt, { size: 'M' }).lines;
	const next = mergeTicketLine(lines, shirt, { size: 'L' });
	assert.equal(next.lines.length, 2);
});

test('a counter line stops at the smaller of stock and ten', () => {
	let lines = [];
	for (let unit = 0; unit < 3; unit += 1) lines = mergeTicketLine(lines, shirt, {}).lines;
	assert.equal(lines[0].quantity, 3);
	const blocked = mergeTicketLine(lines, shirt, {});
	assert.equal(blocked.blocked, true);
	assert.equal(blocked.lines, lines);

	assert.equal(ticketLimit(cup), 10);
	// A sold-out product never reaches the ticket: the counter view filters
	// stock below one before a line can be added.
});

test('reducing a line below one removes it', () => {
	const lines = mergeTicketLine([], shirt, {}).lines;
	const down = changeTicketQuantity(lines, 0, -1);
	assert.equal(down.lines.length, 0);
	const missing = changeTicketQuantity([], 0, -1);
	assert.equal(missing.changed, false);
});

test('raising a line clamps to the stock limit', () => {
	const lines = mergeTicketLine([], shirt, {}).lines;
	const up = changeTicketQuantity(lines, 0, 5);
	assert.equal(up.lines[0].quantity, 3);
});

test('ticket totals count units and cents', () => {
	const lines = mergeTicketLine(mergeTicketLine([], shirt, {}).lines, cup, {}).lines;
	assert.equal(ticketCount(lines), 2);
	assert.equal(ticketTotal(lines), 2000);
});

test('a sale decrements each product once, summing repeated lines', () => {
	const products = [
		{ id: 'p1', stock: 5 },
		{ id: 'p2', stock: 7 }
	];
	const after = stockAfterSale(products, [
		{ product_id: 'p1', quantity: 2 },
		{ product_id: 'p1', quantity: 1 },
		{ product_id: 'p2', quantity: 10 }
	]);
	assert.equal(after.find((product) => product.id === 'p1').stock, 2);
	assert.equal(after.find((product) => product.id === 'p2').stock, 0, 'stock never goes negative');
});
