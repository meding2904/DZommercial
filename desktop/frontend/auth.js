// Bootstrapping the session: read the bridge facts, paint the connection form,
// check the server, then either enter the app or show the login screen.

import { api } from './bridge.js';
import { call, optionalCall, readHealth, RequestError } from './api.js';
import { state } from './state.js';
import { showLogin, showApp } from './session.js';
import { applyTheme, loadPreferences } from './theme.js';
import { applyConnectionValues } from './connection.js';
import { route } from './router.js';

export async function bootstrap() {
	let info = null;
	try {
		info = await api.info();
	} catch (error) {
		state.info = null;
		state.health = { ok: false, status: 0, message: error.message };
		applyConnectionValues('connection', null);
		setLoginStatus(state.health);
		showLogin('The app could not read its connection settings. Enter a server address below.');
		return;
	}
	state.info = info;
	applyConnectionValues('connection', info);
	const health = await readHealth();
	state.health = health;
	setLoginStatus(health);

	call('GET', '/api/storefront/theme')
		.then((data) => {
			if (data?.theme?.brand_name) {
				state.brand = data.theme.brand_name;
				for (const node of document.querySelectorAll('[data-brand]')) {
					node.textContent = state.brand;
				}
				document.title = `${state.brand} admin`;
			}
		})
		.catch(() => {
			// The ledger keeps the framework wordmark when the theme cannot be read.
		});

	let me = null;
	try {
		me = await optionalCall('/api/auth/me');
	} catch {
		showLogin(
			health.ok ? 'The server answered the health check but not the session request.' : ''
		);
		return;
	}
	if (me && me.user) {
		if (me.user.role === 'admin' || me.user.role === 'employee') {
			state.user = me.user;
			await enterApp();
			return;
		}
		await call('POST', '/api/auth/logout').catch(() => {});
		await api.clearSession().catch(() => {});
		showLogin('That account is not part of the shop team.');
		return;
	}
	showLogin();
}

async function enterApp() {
	showApp();
	await loadPreferences();
	applyTheme();
	await route();
}

export function initAuth() {
	document.getElementById('login-form').addEventListener('submit', async (event) => {
		event.preventDefault();
		const email = document.getElementById('login-email').value.trim();
		const password = document.getElementById('login-password').value;
		const error = document.getElementById('login-error');
		const submit = document.getElementById('login-submit');
		error.hidden = true;
		submit.disabled = true;
		try {
			const data = await call('POST', '/api/auth/login', { email, password });
			if (data.user.role !== 'admin' && data.user.role !== 'employee') {
				await call('POST', '/api/auth/logout').catch(() => {});
				await api.clearSession().catch(() => {});
				throw new RequestError({
					status: 403,
					code: 'not_staff',
					message: 'That account is not part of the shop team.'
				});
			}
			state.user = data.user;
			await enterApp();
		} catch (problem) {
			error.textContent =
				problem instanceof RequestError ? problem.message : 'Could not sign in. Try again.';
			error.hidden = false;
			submit.disabled = false;
		}
	});

	document.getElementById('login-retry')?.addEventListener('click', () => {
		void bootstrap();
	});
}

/**
 * @param {any} health
 */
export function setLoginStatus(health) {
	const node = document.getElementById('login-status');
	const text = document.getElementById('login-status-text');
	if (!node || !text) return;
	const ok = Boolean(health && health.ok);
	node.dataset.state = ok ? 'online' : 'offline';
	text.textContent = ok
		? `Server connected${health.env ? ` (${health.env})` : ''}`
		: health?.message || 'The server is not answering.';
	const hint = document.getElementById('login-network-hint');
	if (hint) hint.hidden = ok;
	const retry = document.getElementById('login-retry');
	if (retry) retry.hidden = ok;
	const details = document.getElementById('connection-details');
	if (details && !ok) details.open = true;
}
