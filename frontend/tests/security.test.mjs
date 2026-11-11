import assert from 'node:assert/strict';
import test from 'node:test';
import { gateConfigured, gateMatches } from '../src/lib/server/gate-policy.ts';
import { isAdminApiPath, isAdminPath, normalizePathname } from '../src/lib/server/path.ts';

test('normalizes path encodings used by the routers', () => {
	const cases = [
		['/%61dmin/login', '/admin/login'],
		['/admin%5Clogin', '/admin/login'],
		['/admin%2Flogin', '/admin/login'],
		['/admin%252Flogin', '/admin/login'],
		['/foo/%2e%2e/admin', '/admin'],
		['/foo/%252e%252e/admin', '/admin'],
		['/admin/../admin/login', '/admin/login'],
		['/api/%252e%252e/api/admin/metrics', '/api/admin/metrics'],
		['/api/admin%5Cmetrics', '/api/admin/metrics'],
		['/api//admin/metrics', '/api/admin/metrics'],
		['/api/./admin/metrics', '/api/admin/metrics'],
		['/api/admin/', '/api/admin'],
		['/', '/']
	];

	for (const [input, expected] of cases) {
		assert.equal(normalizePathname(input), expected, input);
	}
});

test('admin UI checks catch encoded and normalised paths', () => {
	for (const path of [
		'/admin',
		'/admin/login',
		'/%61dmin',
		'/%61dmin/login',
		'/admin%5Clogin',
		'/admin%2Flogin',
		'/admin%252Flogin',
		'/foo/%2e%2e/admin',
		'/admin/../admin/login'
	]) {
		assert.equal(isAdminPath(path), true, path);
	}

	for (const path of ['/', '/shop', '/administrator', '/Admin/login', '/api/admin/metrics']) {
		assert.equal(isAdminPath(path), false, path);
	}
});

test('admin API checks catch encoded and normalised paths', () => {
	for (const path of [
		'/api/admin',
		'/api/admin/metrics',
		'/api/%61dmin/metrics',
		'/api/admin%5Cmetrics',
		'/api/admin%2Fmetrics',
		'/api/admin%252Fmetrics',
		'/api/%252e%252e/api/admin/metrics',
		'/api//admin/metrics',
		'/api/./admin/metrics'
	]) {
		assert.equal(isAdminApiPath(path), true, path);
	}

	for (const path of ['/api', '/api/products', '/api/administrator', '/api/Admin/metrics']) {
		assert.equal(isAdminApiPath(path), false, path);
	}
});

test('a missing or weak gate token fails closed in production', () => {
	assert.equal(gateConfigured('development', 'devtoken'), true);
	assert.equal(gateConfigured('production', 'devtoken'), false);
	assert.equal(gateConfigured('Production', 'devtoken'), false);
	assert.equal(gateConfigured(' production ', 'devtoken'), false);
	assert.equal(gateConfigured('production', 'short'), false);
	assert.equal(gateConfigured('production', 'x'.repeat(32)), true);

	// The documented development token must not open a production gate.
	assert.equal(gateMatches('devtoken', 'devtoken', false), false);
	assert.equal(gateMatches('devtoken', 'devtoken', true), true);
	assert.equal(gateMatches(undefined, 'devtoken', true), false);
	assert.equal(gateMatches('', 'devtoken', true), false);
	assert.equal(gateMatches('wrong', 'devtoken', true), false);
});
