import test from 'node:test';
import assert from 'node:assert/strict';

const { api } = await import('../bridge.js');

function installApp() {
	const calls = [];
	const App = {};
	for (const method of [
		'Info',
		'Health',
		'Request',
		'Image',
		'UpdateSettings',
		'ClearSession',
		'FirewallGuide',
		'OpenExternal'
	]) {
		App[method] = (...args) => {
			calls.push({ method, args });
			return { method };
		};
	}
	globalThis.window = { go: { main: { App } } };
	return calls;
}

test('bridge exposes every typed Wails call', async () => {
	const calls = installApp();

	assert.deepEqual(await api.info(), { method: 'Info' });
	assert.deepEqual(await api.health(), { method: 'Health' });
	assert.deepEqual(await api.request('GET', '/api/products'), { method: 'Request' });
	assert.deepEqual(calls.at(-1).args, ['GET', '/api/products', {}]);

	assert.deepEqual(await api.request('POST', '/api/auth/login', { email: 'a' }), {
		method: 'Request'
	});
	assert.deepEqual(calls.at(-1).args, ['POST', '/api/auth/login', { email: 'a' }]);

	assert.deepEqual(await api.image('/api/images/one'), { method: 'Image' });
	assert.deepEqual(await api.updateSettings({
		apiUrl: 'http://127.0.0.1:8080',
		origin: 'http://localhost:3000',
		allowNetwork: true
	}), { method: 'UpdateSettings' });
	assert.deepEqual(calls.at(-1).args, [
		'http://127.0.0.1:8080',
		'http://localhost:3000',
		true
	]);

	assert.deepEqual(await api.clearSession(), { method: 'ClearSession' });
	assert.deepEqual(await api.firewallGuide(), { method: 'FirewallGuide' });
	assert.deepEqual(await api.openExternal('https://example.com'), { method: 'OpenExternal' });
	assert.deepEqual(calls.at(-1).args, ['https://example.com']);
});

test('a missing bridge rejects with a readable error', async () => {
	globalThis.window = {};
	await assert.rejects(api.info(), /The app bridge is not ready\. Restart the app\./);
});

test('a Go error keeps the Go message', async () => {
	globalThis.window = {
		go: { main: { App: { Health: async () => { throw new Error('server is down'); } } } }
	};
	await assert.rejects(api.health(), /server is down/);

	globalThis.window = {
		go: { main: { App: { Health: async () => { throw 'denied'; } } } }
	};
	await assert.rejects(api.health(), /denied/);
});
