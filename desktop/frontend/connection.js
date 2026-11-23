// The connection form: address parsing, the loopback and consent rules the
// server address obeys, and the save/test/clear actions. The address rules are
// exported as pure functions so node --test can pin them.

import { api } from './bridge.js';
import { readHealth } from './api.js';
import { state } from './state.js';
import { toast } from './toast.js';
import { connectionFieldsHtml } from './views/connection.js';

const IPV6_LOOPBACK_HOSTS = new Set(['::1', '0:0:0:0:0:0:0:1']);

/**
 * @param {unknown} value
 * @returns {URL | null}
 */
export function parseServerAddress(value) {
	try {
		const url = new URL(String(value || '').trim());
		return url.protocol === 'http:' || url.protocol === 'https:' ? url : null;
	} catch {
		return null;
	}
}

/**
 * @param {string} hostname
 * @returns {boolean}
 */
export function isLoopbackHost(hostname) {
	let host = String(hostname || '').toLowerCase();
	if (host.startsWith('[') && host.endsWith(']')) host = host.slice(1, -1);
	host = host.replace(/\.$/, '');
	if (host === 'localhost' || host.endsWith('.localhost')) return true;
	if (IPV6_LOOPBACK_HOSTS.has(host)) return true;
	return /^127(?:\.\d{1,3}){3}$/.test(host);
}

/**
 * @param {unknown} value
 * @returns {boolean}
 */
export function needsNetworkConsent(value) {
	const url = parseServerAddress(value);
	return Boolean(url) && !isLoopbackHost(url.hostname);
}

/**
 * @param {unknown} value
 * @returns {string}
 */
export function serverWarning(value) {
	const url = parseServerAddress(value);
	if (!url || url.protocol !== 'http:' || isLoopbackHost(url.hostname)) return '';
	return 'Plain HTTP is not encrypted. Use HTTPS unless this is a trusted local network.';
}

/**
 * Decides whether the typed address may be saved. Landing back on the origin
 * the app already trusts needs no fresh consent.
 * @param {{ apiUrl: unknown, allowNetwork: boolean, savedApiUrl?: unknown }} form
 * @returns {{ ok: boolean, message: string }}
 */
export function validateConnectionAddress({ apiUrl, allowNetwork, savedApiUrl }) {
	const previous = parseServerAddress(savedApiUrl);
	const next = parseServerAddress(apiUrl);
	const sameHost = Boolean(previous && next && previous.origin === next.origin);
	if (needsNetworkConsent(apiUrl) && !allowNetwork && !sameHost) {
		return {
			ok: false,
			message: 'Tick the network permission before saving an address that is not loopback.'
		};
	}
	return { ok: true, message: '' };
}

/**
 * @param {string} prefix
 * @param {any} info
 */
export function applyConnectionValues(prefix, info) {
	const address = document.getElementById(`${prefix}-api`);
	if (address) {
		address.value = info?.apiUrl || '';
		address.dataset.saved = info?.apiUrl || '';
	}
	const origin = document.getElementById(`${prefix}-origin`);
	if (origin) origin.value = info?.origin || '';
	const network = document.getElementById(`${prefix}-network`);
	if (network) {
		network.checked = needsNetworkConsent(info?.apiUrl) && Boolean(info?.networkAllowed);
	}
	updateConnectionWarning(prefix);
	syncNetworkControl(prefix);
}

/**
 * @param {string} prefix
 */
export function syncNetworkControl(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	const network = document.getElementById(`${prefix}-network`);
	if (!address || !network) return;
	const remote = needsNetworkConsent(address.value);
	network.disabled = !remote;
	if (!remote) network.checked = false;
}

/**
 * @param {string} prefix
 */
export function updateConnectionWarning(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	const warning = document.getElementById(`${prefix}-warning`);
	if (!address || !warning) return;
	const text = serverWarning(address.value);
	warning.textContent = text;
	warning.hidden = !text;
}

/**
 * The input handler for a connection form, shared by the login screen (bound
 * once) and the connection page (delegated on the view).
 * @param {string} prefix
 */
export function onConnectionAddressInput(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	if (!address) return;
	const network = document.getElementById(`${prefix}-network`);
	syncNetworkControl(prefix);
	if (network && address.value.trim() !== address.dataset.saved && needsNetworkConsent(address.value)) {
		network.checked = false;
	}
	updateConnectionWarning(prefix);
}

/**
 * @param {string} prefix
 */
export function bindConnectionFields(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	if (!address) return;
	address.addEventListener('input', () => onConnectionAddressInput(prefix));
	updateConnectionWarning(prefix);
	const help = document.getElementById(`${prefix}-firewall`);
	const guide = document.getElementById(`${prefix}-firewall-guide`);
	if (help && guide) bindFirewallHelp(help, guide);
}

/**
 * @param {HTMLButtonElement} button
 * @param {HTMLElement} guide
 */
export function bindFirewallHelp(button, guide) {
	button.addEventListener('click', () => void toggleFirewallHelp(button, guide));
}

/**
 * @param {HTMLButtonElement} button
 * @param {HTMLElement} guide
 */
export async function toggleFirewallHelp(button, guide) {
	if (button.dataset.busy === 'true') return;
	if (!guide.hidden) {
		guide.hidden = true;
		button.setAttribute('aria-expanded', 'false');
		return;
	}
	button.dataset.busy = 'true';
	button.setAttribute('aria-busy', 'true');
	try {
		const data = await api.firewallGuide();
		renderFirewallGuide(guide, data);
	} catch (error) {
		renderFirewallGuide(guide, {
			title: 'Firewall help is not available.',
			note: error.message || 'The app could not read the firewall guide.'
		});
	} finally {
		button.dataset.busy = 'false';
		button.removeAttribute('aria-busy');
	}
	guide.hidden = false;
	button.setAttribute('aria-expanded', 'true');
}

/**
 * @param {HTMLElement} guide
 * @param {any} data
 */
export function renderFirewallGuide(guide, data) {
	guide.replaceChildren();
	const append = (tag, text, className = '') => {
		const node = document.createElement(tag);
		if (className) node.className = className;
		node.textContent = text;
		guide.append(node);
	};
	const title =
		typeof data?.title === 'string' && data.title
			? data.title
			: 'Allow the app through your firewall';
	append('p', title, 'firewall-title');
	if (typeof data?.intro === 'string' && data.intro) append('p', data.intro);
	const steps = Array.isArray(data?.steps)
		? data.steps.filter((step) => typeof step === 'string' && step)
		: [];
	if (steps.length) {
		const list = document.createElement('ol');
		list.className = 'firewall-steps';
		for (const step of steps) {
			const item = document.createElement('li');
			item.textContent = step;
			list.append(item);
		}
		guide.append(list);
	}
	if (typeof data?.note === 'string' && data.note) append('p', data.note, 'field-hint');
}

/**
 * @param {string} prefix
 * @param {HTMLElement} errorBox
 * @returns {Promise<boolean>}
 */
export async function saveConnection(prefix, errorBox) {
	const payload = {
		apiUrl: document.getElementById(`${prefix}-api`).value.trim(),
		origin: document.getElementById(`${prefix}-origin`).value.trim(),
		allowNetwork: Boolean(document.getElementById(`${prefix}-network`).checked)
	};
	errorBox.hidden = true;
	const verdict = validateConnectionAddress({
		apiUrl: payload.apiUrl,
		allowNetwork: payload.allowNetwork,
		savedApiUrl: state.info?.apiUrl
	});
	if (!verdict.ok) {
		errorBox.textContent = verdict.message;
		errorBox.hidden = false;
		document.getElementById(`${prefix}-network`).focus();
		return false;
	}
	try {
		const result = await api.updateSettings(payload);
		if (result?.warning) toast(result.warning, 'error');
		return true;
	} catch (error) {
		errorBox.textContent = error.message || 'The server address could not be saved.';
		errorBox.hidden = false;
		return false;
	}
}

// Updates the two live rows without re-rendering the form, so a typed address
// survives the test.
export async function testConnection() {
	const health = await readHealth();
	state.health = health;
	const serverState = document.getElementById('app-server-state');
	if (serverState) serverState.textContent = health.ok ? 'Connected' : 'Offline';
	const resolvedOrigin = document.getElementById('app-resolved-origin');
	if (resolvedOrigin) {
		resolvedOrigin.textContent = state.info?.resolvedOrigin || state.health?.origin || 'Not resolved';
	}
	toast(
		health.ok ? 'The server answered.' : health.message || 'The server did not answer.',
		health.ok ? 'info' : 'error'
	);
}

/**
 * The login screen's connection form is never re-rendered, so it binds once.
 * @param {{ onSaved: () => Promise<void> }} options
 */
export function initLoginConnection({ onSaved }) {
	document.getElementById('login-connection-fields').innerHTML = connectionFieldsHtml('connection');
	applyConnectionValues('connection', null);
	bindConnectionFields('connection');
	document.getElementById('connection-form').addEventListener('submit', async (event) => {
		event.preventDefault();
		const error = document.getElementById('connection-error');
		if (!(await saveConnection('connection', error))) return;
		await onSaved();
	});
}
