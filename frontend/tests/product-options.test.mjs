import assert from 'node:assert/strict';
import test from 'node:test';
import {
	effectiveStock,
	isValueAvailable,
	missingSelectFields,
	optionLabel,
	resolveVariant
} from '../src/lib/product-options.ts';

const CONFIG = { key: 'config', label: 'Memory and storage', type: 'select', required: true };
const COLOR = { key: 'color', label: 'Color', type: 'select', required: true };

function phone(overrides = {}) {
	return {
		id: 'prd_phone',
		name: 'Aster 2',
		stock: 10,
		option_schema: [CONFIG, COLOR],
		attributes: {
			config: ['12 GB RAM + 256 GB', '16 GB RAM + 512 GB'],
			color: ['Midnight', 'Silver']
		},
		plugin_data: {
			devices_in_stock: 6,
			variants: [
				{ config: '12 GB RAM + 256 GB', color: 'Midnight', in_stock: 2 },
				{ config: '16 GB RAM + 512 GB', color: 'Silver', in_stock: 4 }
			]
		},
		...overrides
	};
}

function core(overrides = {}) {
	return {
		id: 'prd_oil',
		name: 'Green fig body oil',
		stock: 7,
		option_schema: [],
		attributes: {},
		...overrides
	};
}

test('values with no in-stock variant behind them are unavailable', () => {
	const product = phone();
	const cases = [
		['a fresh product offers every stocked config', {}, 'config', '12 GB RAM + 256 GB', true],
		['and every stocked colour', {}, 'color', 'Silver', true],
		['a matching held option keeps its partner', { config: '12 GB RAM + 256 GB' }, 'color', 'Midnight', true],
		['an unmatched held option disables the partner', { config: '12 GB RAM + 256 GB' }, 'color', 'Silver', false],
		['the cascade works from the colour side', { color: 'Silver' }, 'config', '16 GB RAM + 512 GB', true],
		['and from a value it cannot pair with', { color: 'Silver' }, 'config', '12 GB RAM + 256 GB', false],
		['a value outside the variant table is unavailable', {}, 'config', '8 GB RAM + 128 GB', false],
		['empty values in the selection are ignored', { config: '' }, 'color', 'Midnight', true]
	];
	for (const [name, selection, key, value, expected] of cases) {
		assert.equal(isValueAvailable(product, key, value, selection), expected, name);
	}
});

test('the unavailable-combination cascade', () => {
	const product = phone();
	const midnight = { color: 'Midnight' };
	const sixteen = { config: '16 GB RAM + 512 GB' };

	assert.equal(isValueAvailable(product, 'config', '16 GB RAM + 512 GB', midnight), false);
	assert.equal(isValueAvailable(product, 'config', '12 GB RAM + 256 GB', midnight), true);
	assert.equal(isValueAvailable(product, 'color', 'Midnight', sixteen), false);
	assert.equal(isValueAvailable(product, 'color', 'Silver', sixteen), true);

	const dead = { config: '16 GB RAM + 512 GB', color: 'Midnight' };
	assert.equal(resolveVariant(product, dead), null);
	assert.equal(effectiveStock(product, dead), 0);
});

test('a complete selection resolves to its variant stock', () => {
	const product = phone();
	const cases = [
		['midnight 256', { config: '12 GB RAM + 256 GB', color: 'Midnight' }, 2],
		['silver 512', { config: '16 GB RAM + 512 GB', color: 'Silver' }, 4],
		['unmatched pair', { config: '16 GB RAM + 512 GB', color: 'Midnight' }, 0],
		['unknown value', { config: '8 GB RAM + 128 GB', color: 'Silver' }, 0]
	];
	for (const [name, selection, expected] of cases) {
		assert.equal(effectiveStock(product, selection), expected, name);
	}

	assert.equal(
		effectiveStock(phone({ plugin_data: { devices_in_stock: 6, variants: [] } }), {
			config: '12 GB RAM + 256 GB',
			color: 'Midnight'
		}),
		0,
		'an empty variant list is sold out'
	);
	assert.equal(
		effectiveStock(phone({ stock: 3 }), { config: '16 GB RAM + 512 GB', color: 'Silver' }),
		3,
		'product stock caps the variant count'
	);
	assert.equal(
		effectiveStock(
			phone({
				plugin_data: {
					devices_in_stock: 6,
					variants: [{ config: '12 GB RAM + 256 GB', color: 'Midnight', in_stock: 0 }]
				}
			}),
			{ config: '12 GB RAM + 256 GB', color: 'Midnight' }
		),
		0,
		'a zero-stock variant is sold out'
	);
});

test('an incomplete selection falls back to the device count', () => {
	const product = phone();
	const cases = [
		['nothing chosen', {}, 6],
		['one field chosen', { config: '12 GB RAM + 256 GB' }, 6]
	];
	for (const [name, selection, expected] of cases) {
		assert.equal(effectiveStock(product, selection), expected, name);
	}
	assert.deepEqual(
		resolveVariant(product, { config: '12 GB RAM + 256 GB' }),
		{ config: '12 GB RAM + 256 GB', color: 'Midnight', in_stock: 2 },
		'a partial selection still resolves to the first compatible variant'
	);
});

test('the registered device count caps plain stock', () => {
	const cases = [
		['devices below stock', phone({ plugin_data: { devices_in_stock: 6, variants: undefined } }), 6],
		['devices above stock', phone({ plugin_data: { devices_in_stock: 20, variants: undefined } }), 10],
		['no plugin data', phone({ plugin_data: undefined }), 10],
		['no registered devices', phone({ plugin_data: { variants: undefined } }), 10],
		['zero registered devices', phone({ plugin_data: { devices_in_stock: 0, variants: undefined } }), 0]
	];
	for (const [name, product, expected] of cases) {
		assert.equal(effectiveStock(product), expected, name);
	}
});

test('products without variants use plain stock', () => {
	const product = core();
	assert.equal(effectiveStock(product), 7);
	assert.equal(effectiveStock(product, {}), 7);
	assert.equal(resolveVariant(product, {}), null);
	assert.equal(isValueAvailable(product, 'size', '100 ml'), true);
	assert.deepEqual(missingSelectFields(product), []);

	const soldOut = core({ stock: 0 });
	assert.equal(effectiveStock(soldOut), 0);
});

test('required select fields drive completeness', () => {
	const product = phone({ option_schema: [CONFIG, COLOR, { ...COLOR, key: 'gift', required: false }] });
	assert.deepEqual(
		missingSelectFields(product).map((field) => field.key),
		['config', 'color']
	);
	assert.deepEqual(
		missingSelectFields(product, { config: '12 GB RAM + 256 GB' }).map((field) => field.key),
		['color']
	);
	assert.deepEqual(missingSelectFields(product, { config: '12 GB RAM + 256 GB', color: 'Silver' }), []);
});

test('the options label joins chosen fields and skips the rest', () => {
	const schema = [
		{ key: 'config', label: 'Memory and storage' },
		{ key: 'color', label: 'Color' }
	];
	const cases = [
		['both fields', { config: '12 GB RAM + 256 GB', color: 'Silver' }, 'Memory and storage: 12 GB RAM + 256 GB · Color: Silver'],
		['one field', { config: '12 GB RAM + 256 GB' }, 'Memory and storage: 12 GB RAM + 256 GB'],
		['an empty value', { config: '12 GB RAM + 256 GB', color: '' }, 'Memory and storage: 12 GB RAM + 256 GB'],
		['a key outside the schema', { weight: '1 kg' }, ''],
		['nothing chosen', {}, '']
	];
	for (const [name, selection, expected] of cases) {
		assert.equal(optionLabel(schema, selection), expected, name);
	}
});
