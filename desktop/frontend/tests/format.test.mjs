import test from 'node:test';
import assert from 'node:assert/strict';

import {
	compactMoney,
	escapeHtml,
	formatDate,
	formatDateTime,
	formatDayShort,
	fromDinars,
	money,
	moneyValue,
	percent
} from '../format.js';

test('money prints whole dinars with a non-breaking space', () => {
	assert.equal(money(0), '0\u00a0DZD');
	assert.equal(money(null), '0\u00a0DZD');
	assert.equal(money(123456), '1,235\u00a0DZD');
});

test('compactMoney keeps the full amount below one thousand dinars', () => {
	assert.equal(compactMoney(99999), money(99999));
	assert.equal(compactMoney(100000), '1K\u00a0DZD');
});

test('moneyValue and fromDinars convert between dinars and cents', () => {
	assert.equal(moneyValue(12345), '123.45');
	assert.equal(moneyValue(null), '0.00');
	assert.equal(fromDinars('12.345'), 1235);
	assert.equal(fromDinars(''), 0);
});

test('formatDate falls back for an empty or unreadable date', () => {
	assert.equal(formatDate('2026-01-05T12:00:00Z'), '05 Jan 2026');
	assert.equal(formatDate(''), 'No date');
	assert.equal(formatDate('nonsense'), 'nonsense');
});

test('formatDateTime falls back for an empty or unreadable date', () => {
	assert.equal(formatDateTime(''), 'Not set');
	assert.equal(formatDateTime('nonsense'), 'nonsense');
});

test('formatDayShort renders a short calendar day', () => {
	assert.equal(formatDayShort('2026-01-05'), '05 Jan');
});

test('percent handles a missing previous period and signs the change', () => {
	assert.equal(percent(null), 'No previous period');
	assert.equal(percent(undefined), 'No previous period');
	assert.equal(percent(12.34), '+12.3%');
	assert.equal(percent(-1.5), '-1.5%');
	assert.equal(percent(0), '0.0%');
});

test('escapeHtml escapes the five markup characters', () => {
	assert.equal(escapeHtml(`<b>"x"&'`), '&lt;b&gt;&quot;x&quot;&amp;&#39;');
	assert.equal(escapeHtml(null), '');
});
