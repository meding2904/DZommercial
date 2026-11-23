import test from 'node:test';
import assert from 'node:assert/strict';

import { installBridge, installDocument, installStorage } from './helpers/browser.mjs';

installStorage();
installDocument();
const { state } = await import('../state.js');
const { signOut, clearLocalSession } = await import('../session.js');

test('sign out and clear session both reset transient state', async () => {
	installBridge({
		Request: async () => ({ ok: true, data: {} }),
		ClearSession: async () => ({})
	});

	state.user = { name: 'Owner', role: 'admin' };
	state.orders = [{ id: 'old' }];
	state.metrics = { revenue_cents: 1 };
	state.counterQuery = 'old';
	const before = state.render;

	await signOut();

	assert.equal(state.user, null);
	assert.deepEqual(state.orders, []);
	assert.equal(state.metrics, null);
	assert.equal(state.counterQuery, '');
	assert.ok(state.render > before, 'sign out must invalidate in-flight renders');
	assert.equal(document.getElementById('login-screen').hidden, false);
	assert.equal(document.getElementById('admin-shell').hidden, true);

	state.user = { name: 'Owner', role: 'admin' };
	state.orders = [{ id: 'again' }];
	const mid = state.render;

	const cleared = await clearLocalSession();

	assert.equal(cleared, true);
	assert.equal(state.user, null);
	assert.deepEqual(state.orders, []);
	assert.ok(state.render > mid, 'clear session must invalidate in-flight renders');
});

test('sign out reports a logout the server did not confirm', async () => {
	installBridge({
		Request: async () => {
			throw new Error('offline');
		},
		ClearSession: async () => ({})
	});

	await signOut();

	assert.equal(state.user, null);
	assert.equal(document.getElementById('login-error').textContent, 'Signed out on this device. The server did not confirm the logout.');
});

test('a sign-out re-enables the sign-in button', async () => {
	installBridge({
		Request: async () => ({ ok: true, data: {} }),
		ClearSession: async () => ({})
	});

	state.user = { name: 'Owner', role: 'admin' };
	document.getElementById('login-submit').disabled = true;
	await signOut();
	assert.equal(state.user, null);
	assert.equal(document.getElementById('login-submit').disabled, false);
});
