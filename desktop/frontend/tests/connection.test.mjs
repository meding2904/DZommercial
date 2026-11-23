import test from 'node:test';
import assert from 'node:assert/strict';

import { installBridge, installDocument, installStorage } from './helpers/browser.mjs';

installStorage();
const registry = installDocument([
	'app-api',
	'app-origin',
	'app-network',
	'app-warning',
	'app-connection-error',
	'app-server-state',
	'app-resolved-origin',
	'app-firewall-guide',
	'toast',
	'view'
]);
const connection = await import('../connection.js');
const { state } = await import('../state.js');

test('a loopback address needs no network consent', () => {
	assert.equal(connection.needsNetworkConsent('http://127.0.0.1:8080'), false);
	assert.equal(connection.needsNetworkConsent('http://localhost:8080'), false);
	assert.equal(connection.needsNetworkConsent('http://[::1]:8080'), false);
	assert.equal(connection.needsNetworkConsent('http://192.168.1.4:8080'), true);
	assert.equal(connection.needsNetworkConsent('not a url'), false);
	assert.equal(connection.parseServerAddress('ftp://example.com'), null);
});

test('plain http on a remote host warns about encryption', () => {
	assert.equal(
		connection.serverWarning('http://192.168.1.4:8080'),
		'Plain HTTP is not encrypted. Use HTTPS unless this is a trusted local network.'
	);
	assert.equal(connection.serverWarning('https://shop.example.com'), '');
	assert.equal(connection.serverWarning('http://127.0.0.1:8080'), '');
});

test('a remote address needs consent unless it is the saved origin', () => {
	const blocked = connection.validateConnectionAddress({
		apiUrl: 'http://10.0.0.9:8080',
		allowNetwork: false,
		savedApiUrl: 'http://192.168.1.4:8080'
	});
	assert.equal(blocked.ok, false);
	assert.equal(
		blocked.message,
		'Tick the network permission before saving an address that is not loopback.'
	);

	const sameOrigin = connection.validateConnectionAddress({
		apiUrl: 'http://192.168.1.4:8080/',
		allowNetwork: false,
		savedApiUrl: 'http://192.168.1.4:8080'
	});
	assert.equal(sameOrigin.ok, true);

	const consented = connection.validateConnectionAddress({
		apiUrl: 'http://10.0.0.9:8080',
		allowNetwork: true,
		savedApiUrl: 'http://192.168.1.4:8080'
	});
	assert.equal(consented.ok, true);
});

test('saveConnection blocks an unconsented remote address and focuses the tick', () => {
	installBridge({ UpdateSettings: async () => ({}) });
	state.info = { apiUrl: 'http://192.168.1.4:8080' };
	document.getElementById('app-api').value = 'http://10.0.0.9:8080';
	document.getElementById('app-origin').value = '';
	document.getElementById('app-network').checked = false;
	const errorBox = document.getElementById('app-connection-error');
	const network = registry.get('app-network');

	return connection.saveConnection('app', errorBox).then((saved) => {
		assert.equal(saved, false);
		assert.equal(errorBox.hidden, false);
		assert.equal(network.focused, true);
	});
});

test('saveConnection toasts the warning the bridge returns', async () => {
	const calls = installBridge({
		UpdateSettings: async () => ({ warning: 'Plain HTTP is not encrypted.' })
	});
	state.info = { apiUrl: 'http://192.168.1.4:8080' };
	document.getElementById('app-api').value = 'http://192.168.1.4:8080/';
	document.getElementById('app-origin').value = '';
	document.getElementById('app-network').checked = false;
	const errorBox = document.getElementById('app-connection-error');

	const saved = await connection.saveConnection('app', errorBox);
	assert.equal(saved, true);
	assert.equal(errorBox.hidden, true);
	assert.equal(document.getElementById('toast').textContent, 'Plain HTTP is not encrypted.');
	assert.equal(calls.length, 1);
	assert.deepEqual(calls[0].args, ['http://192.168.1.4:8080/', '', false]);
});

test('testing the connection updates the live rows without re-rendering the page', async () => {
	installBridge({ Health: async () => ({ ok: true, origin: 'http://localhost:3000' }) });
	state.info = { resolvedOrigin: 'http://localhost:3000' };
	const view = document.getElementById('view');
	const before = view.innerHTMLWrites;

	await connection.testConnection();

	assert.equal(view.innerHTMLWrites, before);
	assert.equal(document.getElementById('app-server-state').textContent, 'Connected');
	assert.equal(document.getElementById('app-resolved-origin').textContent, 'http://localhost:3000');
});
