import test from 'node:test';
import assert from 'node:assert/strict';

import { wait } from './helpers/browser.mjs';
import { installNavigationGuards, isExternalHref } from '../navigation.js';

function install({ openExternal = async () => {}, warn = () => {} } = {}) {
	const docListeners = {};
	const winListeners = {};
	const doc = {
		addEventListener(type, handler) {
			docListeners[type] = handler;
		}
	};
	const win = {
		addEventListener(type, handler) {
			winListeners[type] = handler;
		},
		open: null
	};
	installNavigationGuards({ window: win, document: doc, openExternal, warn });
	return { click: docListeners.click, winListeners, win };
}

function clickEvent(href) {
	const event = {
		defaultPrevented: false,
		prevented: false,
		target: {
			closest: (selector) => (selector === 'a[href]' ? { getAttribute: () => href } : null)
		},
		preventDefault() {
			event.prevented = true;
		}
	};
	return event;
}

test('external links open through the bridge instead of navigating', async () => {
	const opened = [];
	const { click } = install({ openExternal: async (href) => opened.push(href) });
	const event = clickEvent('https://shop.example.com/help');
	click(event);
	await wait(0);
	assert.equal(event.prevented, true);
	assert.deepEqual(opened, ['https://shop.example.com/help']);
});

test('hash links are left to the router', async () => {
	const opened = [];
	const { click } = install({ openExternal: async (href) => opened.push(href) });
	const event = clickEvent('#/orders');
	click(event);
	await wait(0);
	assert.equal(event.prevented, false);
	assert.deepEqual(opened, []);
});

test('non-http links are refused with a toast', async () => {
	const warned = [];
	const { click } = install({ warn: (message) => warned.push(message) });
	const event = clickEvent('mailto:admin@example.com');
	click(event);
	assert.equal(event.prevented, true);
	assert.deepEqual(warned, ['That link is not available in the desktop app.']);
});

test('a failed open reports through the toast', async () => {
	const warned = [];
	const { click } = install({
		openExternal: async () => {
			throw new Error('the bridge said no');
		},
		warn: (message) => warned.push(message)
	});
	click(clickEvent('https://example.com'));
	await wait(0);
	assert.deepEqual(warned, ['the bridge said no']);
});

test('drop and dragover are cancelled', () => {
	const { winListeners } = install();
	for (const type of ['dragover', 'drop']) {
		let prevented = false;
		winListeners[type]({ preventDefault: () => { prevented = true; } });
		assert.equal(prevented, true, `${type} must be cancelled`);
	}
});

test('window.open is replaced by a toast', () => {
	const warned = [];
	const { win } = install({ warn: (message) => warned.push(message) });
	assert.equal(win.open('https://example.com'), null);
	assert.deepEqual(warned, ['External windows are disabled. Use a link in the page.']);
	assert.equal(isExternalHref('http://example.com'), true);
	assert.equal(isExternalHref('/api/images/one'), false);
});
