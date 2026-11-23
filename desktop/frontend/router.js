// The hash router. Screens register themselves here; the router only knows the
// route names, so it never imports a screen (no import cycles). It also serves
// the "refresh what is on screen" call the order dialog makes after a move.

import { state } from './state.js';
import { titleCase } from './domain/labels.js';
import { showLogin } from './session.js';

const KNOWN_ROUTES = ['ledger', 'counter', 'orders', 'products', 'customers', 'staff', 'connection'];

/** @type {Map<string, { render: () => any, refresh?: () => any }>} */
const screens = new Map();

/**
 * @param {string} name
 * @param {{ render: () => any, refresh?: () => any }} screen
 */
export function registerScreen(name, screen) {
	screens.set(name, screen);
}

/**
 * @param {string} [hash]
 * @returns {string}
 */
export function parseRoute(hash) {
	const value = hash || '#/ledger';
	const name = value.replace(/^#\/?/, '').split('/')[0] || 'ledger';
	return KNOWN_ROUTES.includes(name) ? name : 'ledger';
}

/**
 * @param {string} routeName
 */
export function markRoute(routeName) {
	state.route = routeName;
	for (const link of document.querySelectorAll('[data-route]')) {
		if (link.dataset.route === routeName) link.setAttribute('aria-current', 'page');
		else link.removeAttribute('aria-current');
	}
}

export async function route() {
	if (!state.user) {
		showLogin();
		return;
	}
	const routeName = parseRoute(window.location.hash);
	markRoute(routeName);
	document.title = `${titleCase(routeName)} · ${state.brand} admin`;
	const screen = screens.get(routeName);
	if (!screen) return;
	return screen.render();
}

export function refreshCurrent() {
	const screen = screens.get(state.route);
	if (screen && screen.refresh) return screen.refresh();
	return Promise.resolve();
}
