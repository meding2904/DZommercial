// Wails binds main.App methods here. A Go error rejects the promise, so every call
// is normalized back to an Error with the Go message.
const api = {
	info: () => callBridge('Info'),
	health: () => callBridge('Health'),
	request: (method, path, body) =>
		callBridge('Request', method, path, body == null ? {} : body),
	image: (path) => callBridge('Image', path),
	updateSettings: (payload) => {
		const settings = payload || {};
		return callBridge(
			'UpdateSettings',
			settings.apiUrl,
			settings.origin,
			Boolean(settings.allowNetwork)
		);
	},
	clearSession: () => callBridge('ClearSession'),
	firewallGuide: () => callBridge('FirewallGuide'),
	openExternal: (url) => callBridge('OpenExternal', url)
};

function callBridge(method, ...args) {
	const app = window.go && window.go.main && window.go.main.App;
	if (!app || typeof app[method] !== 'function') {
		return Promise.reject(new Error('The app bridge is not ready. Restart the app.'));
	}
	try {
		return Promise.resolve(app[method](...args)).catch((error) => {
			throw new Error(bridgeMessage(error));
		});
	} catch (error) {
		return Promise.reject(new Error(bridgeMessage(error)));
	}
}

function bridgeMessage(error) {
	if (typeof error === 'string' && error) return error;
	if (error && typeof error.message === 'string' && error.message) return error.message;
	return 'The app bridge did not answer.';
}

const PRESET_TOKENS = {
	plaster: {
		light: {
			bg: '#f6f0eb', surface: '#ebe0d9', ink: '#2c2325', muted: '#6a5d5e',
			accent: '#9a3c4c', accentInk: '#ffffff', chart1: '#9a3c4c', chart2: '#476b5a',
			chart3: '#8a5a33', chart4: '#6a5d5e', pending: '#8a5a2b', confirmed: '#476b5a',
			packed: '#6a5d5e', shipped: '#7a4a2f', delivered: '#3f5f4b', cancelled: '#9a3c4c',
			danger: '#852f3f'
		},
		dark: {
			bg: '#171311', surface: '#221b18', ink: '#f4ece5', muted: '#b9aca4',
			accent: '#e0a0a8', accentInk: '#241316', chart1: '#e0a0a8', chart2: '#8fae8b',
			chart3: '#d8a44c', chart4: '#b9aca4', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#b9aca4', shipped: '#c46a4f', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#f0b0ae'
		}
	},
	cellar: {
		light: {
			bg: '#f5f0e2', surface: '#e9e1cd', ink: '#26271d', muted: '#5f6353',
			accent: '#8a5a12', accentInk: '#ffffff', chart1: '#8a5a12', chart2: '#4f6b57',
			chart3: '#a45a3a', chart4: '#6e6a5e', pending: '#8a5a12', confirmed: '#4f6b57',
			packed: '#6e6a5e', shipped: '#9a4a2f', delivered: '#3f5f4b', cancelled: '#9a3c4c',
			danger: '#7f3415'
		},
		dark: {
			bg: '#151a16', surface: '#1d241e', ink: '#f1ece1', muted: '#a3ab9f',
			accent: '#d8a44c', accentInk: '#1a1408', chart1: '#d8a44c', chart2: '#8fae8b',
			chart3: '#c46a4f', chart4: '#d8d2c4', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#a3ab9f', shipped: '#c46a4f', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#eda3a3'
		}
	},
	terracotta: {
		light: {
			bg: '#f2e5d8', surface: '#e5d1bf', ink: '#2b1d18', muted: '#6e554a',
			accent: '#a94e2b', accentInk: '#ffffff', chart1: '#a94e2b', chart2: '#4f6b57',
			chart3: '#8a5a33', chart4: '#6e554a', pending: '#8a5a29', confirmed: '#4f6b57',
			packed: '#6e554a', shipped: '#8f4a2f', delivered: '#3f5f4b', cancelled: '#a94e2b',
			danger: '#8a341e'
		},
		dark: {
			bg: '#1b1210', surface: '#271a16', ink: '#f3e7dd', muted: '#b9a396',
			accent: '#dc8a63', accentInk: '#2a120b', chart1: '#dc8a63', chart2: '#8fae8b',
			chart3: '#d8a44c', chart4: '#b9a396', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#b9a396', shipped: '#dc8a63', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#f0b0ae'
		}
	},
	lagoon: {
		light: {
			bg: '#eef5f4', surface: '#deebe9', ink: '#112528', muted: '#526b6c',
			accent: '#1d6f6b', accentInk: '#ffffff', chart1: '#1d6f6b', chart2: '#8a5a12',
			chart3: '#a45a3a', chart4: '#526b6c', pending: '#8a5a12', confirmed: '#3f5f4b',
			packed: '#526b6c', shipped: '#9a4a2f', delivered: '#2f6b4f', cancelled: '#9a3c4c',
			danger: '#7f3415'
		},
		dark: {
			bg: '#0e1d23', surface: '#152b33', ink: '#e9f1ef', muted: '#9ab0b1',
			accent: '#5fb7b2', accentInk: '#06201f', chart1: '#5fb7b2', chart2: '#d8a44c',
			chart3: '#c46a4f', chart4: '#d8e3de', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#9ab0b1', shipped: '#c46a4f', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#f0b0ae'
		}
	}
};

const DEFAULT_PRESET = 'cellar';
const PRESET_KEY = 'azahar.admin.preset';
const SCHEME_KEY = 'azahar.admin.scheme';
const ORDER_STATUSES = ['pending', 'confirmed', 'packed', 'shipped', 'delivered', 'cancelled'];
const TRANSITIONS = {
	pending: ['confirmed', 'cancelled'],
	confirmed: ['packed', 'cancelled'],
	packed: ['shipped', 'cancelled'],
	shipped: ['delivered'],
	delivered: [],
	cancelled: []
};

const boot = document.getElementById('boot');
const loginScreen = document.getElementById('login-screen');
const adminShell = document.getElementById('admin-shell');
const view = document.getElementById('view');
const toastNode = document.getElementById('toast');
const orderDialog = document.getElementById('order-dialog');

const state = {
	info: null,
	health: null,
	brand: 'Azahar',
	user: null,
	preset: localStorage.getItem(PRESET_KEY) || DEFAULT_PRESET,
	scheme: localStorage.getItem(SCHEME_KEY) || 'dark',
	route: 'ledger',
	render: 0,
	metrics: null,
	metricsDays: 30,
	orders: [],
	orderFilter: '',
	orderQuery: '',
	orderLoading: false,
	orderTimer: null,
	products: [],
	productQuery: '',
	productLowOnly: false,
	pendingProduct: '',
	customers: [],
	customerQuery: '',
	team: [],
	counterQuery: '',
	counterLines: [],
	counterCustomer: '',
	counterPhone: '',
	counterNote: '',
	counterReceipt: null,
	toastTimer: null
};

class RequestError extends Error {
	constructor(payload) {
		super(payload && payload.message ? payload.message : 'The request failed.');
		this.name = 'RequestError';
		this.status = payload && payload.status ? payload.status : 0;
		this.code = payload && payload.code ? payload.code : 'request_failed';
		this.fields = payload && payload.fields ? payload.fields : {};
	}
}

function call(method, path, body) {
	return api.request(method, path, body).then((result) => {
		if (!result || !result.ok) throw new RequestError(result && result.error);
		return result.data;
	});
}

async function optionalCall(path) {
	try {
		return await call('GET', path);
	} catch (error) {
		if (error instanceof RequestError && error.status === 401) return null;
		throw error;
	}
}

function escapeHtml(value) {
	return String(value ?? '').replace(/[&<>"']/g, (character) => {
		return {
			'&': '&amp;',
			'<': '&lt;',
			'>': '&gt;',
			'"': '&quot;',
			"'": '&#39;'
		}[character];
	});
}

function money(cents) {
	const amount = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(
		Math.round(Number(cents || 0) / 100)
	);
	return `${amount}\u00a0DZD`;
}

function compactMoney(cents) {
	if (Math.abs(Number(cents)) < 100000) return money(cents);
	return `${new Intl.NumberFormat('en-US', {
		notation: 'compact',
		maximumFractionDigits: 1
	}).format(Number(cents) / 100)}\u00a0DZD`;
}

function moneyValue(cents) {
	return (Number(cents || 0) / 100).toFixed(2);
}

function fromDinars(value) {
	return Math.round(Number(value || 0) * 100);
}

function titleCase(value) {
	return String(value || '')
		.replaceAll('_', ' ')
		.replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function formatDate(value) {
	if (!value) return 'No date';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return String(value);
	return new Intl.DateTimeFormat('en-GB', {
		day: '2-digit',
		month: 'short',
		year: 'numeric'
	}).format(date);
}

function formatDateTime(value) {
	if (!value) return 'Not set';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return String(value);
	return new Intl.DateTimeFormat('en-GB', {
		day: '2-digit',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit'
	}).format(date);
}

function formatDayShort(value) {
	const date = new Date(`${value}T12:00:00Z`);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat('en-GB', { day: '2-digit', month: 'short' }).format(date);
}

function deliveryLabel(type) {
	if (type === 'pickup') return 'Counter sale';
	if (type === 'stopdesk') return 'Stop desk';
	return 'Home delivery';
}

function receiptHtml(order) {
	const items = (order.items || [])
		.map(
			(item) => `<tr>
				<td>${escapeHtml(item.name)}${item.options_label ? `<span class="muted">${escapeHtml(item.options_label)}</span>` : ''}</td>
				<td class="right">${escapeHtml(String(item.quantity))}</td>
				<td class="right mono">${escapeHtml(money(item.unit_price_cents ?? item.price_cents ?? 0))}</td>
				<td class="right mono">${escapeHtml(money(item.line_total_cents ?? (item.unit_price_cents ?? item.price_cents ?? 0) * item.quantity))}</td>
			</tr>`
		)
		.join('');
	const place = [order.commune_name || order.city, order.wilaya_name].filter(Boolean).join(', ');
	const paid = order.delivery_type === 'pickup' || order.status === 'delivered';
	return `<!doctype html>
<html lang="en"><head><meta charset="utf-8" /><title>Receipt ${escapeHtml(order.number)}</title>
<style>
  * { box-sizing: border-box; }
  body { margin: 0; padding: 28px; color: #171311; background: #fff; font: 13px/1.45 ui-sans-serif, system-ui, sans-serif; }
  .receipt { width: 100%; max-width: 700px; margin: 0 auto; }
  .head { display: flex; justify-content: space-between; gap: 24px; padding-bottom: 16px; border-bottom: 2px solid #171311; }
  .brand { font-size: 26px; font-weight: 650; letter-spacing: -0.04em; line-height: 1; }
  .brand span { color: #9a3c4c; }
  .label { margin: 0; color: #6a5d5e; font: 10px/1.2 ui-monospace, monospace; letter-spacing: 0.04em; text-transform: uppercase; }
  .number { margin: 4px 0 2px; font: 17px/1.2 ui-monospace, monospace; }
  .muted { color: #6a5d5e; }
  .parties { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 18px; padding: 16px 0; border-bottom: 1px solid #cfc6bd; }
  .parties p { margin: 0; }
  .parties section { display: grid; gap: 2px; align-content: start; }
  table { width: 100%; border-collapse: collapse; }
  th { padding: 14px 6px 6px; border-bottom: 1px solid #171311; color: #6a5d5e; font: 10px/1.2 ui-monospace, monospace; font-weight: 400; text-align: left; }
  td { padding: 7px 6px; border-bottom: 1px solid #e2dad2; vertical-align: top; }
  th:first-child, td:first-child { padding-left: 0; }
  th:last-child, td:last-child { padding-right: 0; }
  .right { text-align: right; }
  .mono { font-family: ui-monospace, monospace; font-variant-numeric: tabular-nums; }
  .totals { display: grid; gap: 5px; width: 250px; max-width: 100%; margin: 12px 0 0 auto; }
  .totals div { display: flex; justify-content: space-between; gap: 14px; color: #6a5d5e; }
  .totals .final { padding-top: 7px; border-top: 1px solid #171311; color: #171311; font: 16px/1.2 ui-monospace, monospace; }
  .payment { margin-top: 16px; padding-top: 10px; border-top: 1px dashed #9d9186; }
  .foot { margin-top: 22px; padding-top: 12px; border-top: 1px solid #cfc6bd; color: #6a5d5e; font-size: 11px; }
  .foot p { margin: 0 0 3px; }
  @media print { @page { margin: 12mm; } body { padding: 0; } }
</style></head><body><div class="receipt">
  <div class="head"><div><p class="brand">${escapeHtml(state.brand)}<span>.</span></p><p class="muted">${escapeHtml(deliveryLabel(order.delivery_type))}</p></div><div style="text-align:right"><p class="label">Receipt</p><p class="number">${escapeHtml(order.number)}</p><p class="muted">${escapeHtml(formatDateTime(order.created_at))}</p></div></div>
  <div class="parties">
    <section><p class="label">Customer</p><p>${escapeHtml(order.name)}</p><p class="muted">${escapeHtml(order.email || 'No email')}</p><p class="muted">${escapeHtml(order.phone || 'No phone')}</p></section>
    <section><p class="label">Delivery</p><p>${escapeHtml(deliveryLabel(order.delivery_type))}</p>${order.stop_desk_name ? `<p class="muted">${escapeHtml(order.stop_desk_name)}</p>` : ''}${order.address ? `<p class="muted">${escapeHtml(order.address)}</p>` : ''}${place ? `<p class="muted">${escapeHtml(place)}</p>` : ''}</section>
    <section><p class="label">Status</p><p>${escapeHtml(titleCase(order.status))}</p><p class="muted">Served by ${escapeHtml(state.user?.name || 'the counter')}</p></section>
  </div>
  <table><thead><tr><th>Item</th><th class="right">Qty</th><th class="right">Unit</th><th class="right">Line total</th></tr></thead><tbody>${items}</tbody></table>
  <div class="totals"><div><span>Subtotal</span><span>${escapeHtml(money(order.subtotal_cents))}</span></div><div><span>Shipping</span><span>${escapeHtml(order.shipping_cents === 0 ? 'Free' : money(order.shipping_cents))}</span></div><div class="final"><span>Total</span><span>${escapeHtml(money(order.total_cents))}</span></div></div>
  <p class="payment">${paid ? 'Paid in cash.' : 'Cash on delivery. The courier collects the total.'}</p>
  ${order.notes ? `<p class="muted">Note: ${escapeHtml(order.notes)}</p>` : ''}
  <div class="foot"><p>${escapeHtml(state.brand)} receipt.</p><p>Keep this receipt for the order and any courier claim.</p><p>This is a demonstration shop. No binding sale is created.</p></div>
</div></body></html>`;
}

function printHtml(html) {
	const frame = document.createElement('iframe');
	frame.setAttribute('aria-hidden', 'true');
	frame.style.cssText = 'position:fixed;right:0;bottom:0;width:0;height:0;border:0;';
	frame.srcdoc = html;
	frame.onload = () => {
		const target = frame.contentWindow;
		if (!target || typeof target.print !== 'function') {
			frame.remove();
			toast('The receipt could not be sent to the printer.', 'error');
			return;
		}
		target.focus();
		target.print();
		setTimeout(() => frame.remove(), 1000);
	};
	document.body.appendChild(frame);
}

function printOrderReceipt(order) {
	printHtml(receiptHtml(order));
}

function percent(value) {
	if (value === null || value === undefined || Number.isNaN(Number(value))) {
		return 'No previous period';
	}
	const number = Number(value);
	return `${number > 0 ? '+' : ''}${number.toFixed(1)}%`;
}

function toLocalInput(value) {
	if (!value) return '';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	const pad = (number) => String(number).padStart(2, '0');
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function toRFC3339(value) {
	if (!value) return '';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	return date.toISOString();
}

function toast(message, tone = 'info') {
	clearTimeout(state.toastTimer);
	toastNode.textContent = message;
	toastNode.dataset.tone = tone;
	toastNode.hidden = false;
	toastNode.style.animation = 'none';
	void toastNode.offsetWidth;
	toastNode.style.animation = '';
	state.toastTimer = setTimeout(() => {
		toastNode.hidden = true;
	}, 3800);
}

function applyTheme() {
	const preset = PRESET_TOKENS[state.preset] ? state.preset : DEFAULT_PRESET;
	const tokens = PRESET_TOKENS[preset][state.scheme === 'light' ? 'light' : 'dark'];
	const root = document.documentElement;
	root.dataset.preset = preset;
	root.dataset.scheme = state.scheme;
	const map = {
		bg: '--a-bg',
		surface: '--a-surface',
		ink: '--a-ink',
		muted: '--a-muted',
		accent: '--a-accent',
		accentInk: '--a-accent-ink',
		chart1: '--a-chart-1',
		chart2: '--a-chart-2',
		chart3: '--a-chart-3',
		chart4: '--a-chart-4',
		pending: '--a-status-pending',
		confirmed: '--a-status-confirmed',
		packed: '--a-status-packed',
		shipped: '--a-status-shipped',
		delivered: '--a-status-delivered',
		cancelled: '--a-status-cancelled',
		danger: '--a-danger-text'
	};
	for (const [key, cssVar] of Object.entries(map)) root.style.setProperty(cssVar, tokens[key]);
	const select = document.getElementById('preset-select');
	if (select) select.value = preset;
	const label = document.getElementById('scheme-label');
	if (label) label.textContent = state.scheme === 'dark' ? 'Dark' : 'Light';
	const toggle = document.getElementById('scheme-toggle');
	if (toggle) {
		toggle.setAttribute('aria-label', `Switch to ${state.scheme === 'dark' ? 'light' : 'dark'} mode`);
	}
}

async function savePreset(preset) {
	state.preset = preset;
	localStorage.setItem(PRESET_KEY, preset);
	applyTheme();
	if (!state.user) return;
	try {
		await call('PUT', '/api/admin/preferences', { theme_preset: preset });
	} catch (error) {
		toast(error.message || 'The style could not be saved.', 'error');
	}
}

function setLoginStatus(health) {
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

const IPV6_LOOPBACK_HOSTS = new Set(['::1', '0:0:0:0:0:0:0:1']);

function parseServerAddress(value) {
	try {
		const url = new URL(String(value || '').trim());
		return url.protocol === 'http:' || url.protocol === 'https:' ? url : null;
	} catch {
		return null;
	}
}

function isLoopbackHost(hostname) {
	let host = String(hostname || '').toLowerCase();
	if (host.startsWith('[') && host.endsWith(']')) host = host.slice(1, -1);
	host = host.replace(/\.$/, '');
	if (host === 'localhost' || host.endsWith('.localhost')) return true;
	if (IPV6_LOOPBACK_HOSTS.has(host)) return true;
	return /^127(?:\.\d{1,3}){3}$/.test(host);
}

function needsNetworkConsent(value) {
	const url = parseServerAddress(value);
	return Boolean(url) && !isLoopbackHost(url.hostname);
}

function serverWarning(value) {
	const url = parseServerAddress(value);
	if (!url || url.protocol !== 'http:' || isLoopbackHost(url.hostname)) return '';
	return 'Plain HTTP is not encrypted. Use HTTPS unless this is a trusted local network.';
}

function connectionFieldsHtml(prefix) {
	return `<div class="connection-fields">
		<div class="field">
			<label for="${prefix}-api">Server address</label>
			<input class="input" id="${prefix}-api" autocomplete="url" placeholder="http://127.0.0.1:8080" />
			<p class="field-hint">The local demo runs at http://127.0.0.1:8080.</p>
		</div>
		<div class="field">
			<label for="${prefix}-origin">Request origin, optional</label>
			<input class="input" id="${prefix}-origin" autocomplete="url" placeholder="Auto, read from the server" />
			<p class="field-hint">Sent with writes so the server can check it against its own origin. Empty reads it from the server.</p>
		</div>
		<label class="checkbox-row" for="${prefix}-network">
			<input type="checkbox" id="${prefix}-network" aria-describedby="${prefix}-network-hint" />
			<span>Allow this app to contact a server on the network</span>
		</label>
		<p class="field-hint" id="${prefix}-network-hint">Required for any address other than loopback. The app sends requests from Go, not from this page, so your operating system firewall may ask whether to allow it.</p>
		<p class="form-warning" id="${prefix}-warning" aria-live="polite" hidden></p>
		<div class="connection-help">
			<button class="button button-secondary" type="button" id="${prefix}-firewall" aria-expanded="false" aria-controls="${prefix}-firewall-guide">Firewall help</button>
			<div class="firewall-guide" id="${prefix}-firewall-guide" role="region" aria-label="Firewall help" aria-live="polite" hidden></div>
		</div>
	</div>`;
}

function applyConnectionValues(prefix, info) {
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

function syncNetworkControl(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	const network = document.getElementById(`${prefix}-network`);
	if (!address || !network) return;
	const remote = needsNetworkConsent(address.value);
	network.disabled = !remote;
	if (!remote) network.checked = false;
}

function updateConnectionWarning(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	const warning = document.getElementById(`${prefix}-warning`);
	if (!address || !warning) return;
	const text = serverWarning(address.value);
	warning.textContent = text;
	warning.hidden = !text;
}

function bindConnectionFields(prefix) {
	const address = document.getElementById(`${prefix}-api`);
	if (!address) return;
	address.addEventListener('input', () => {
		const network = document.getElementById(`${prefix}-network`);
		syncNetworkControl(prefix);
		if (network && address.value.trim() !== address.dataset.saved && needsNetworkConsent(address.value)) {
			network.checked = false;
		}
		updateConnectionWarning(prefix);
	});
	updateConnectionWarning(prefix);
	const help = document.getElementById(`${prefix}-firewall`);
	const guide = document.getElementById(`${prefix}-firewall-guide`);
	if (help && guide) bindFirewallHelp(help, guide);
}

function bindFirewallHelp(button, guide) {
	let busy = false;
	button.addEventListener('click', async () => {
		if (busy) return;
		if (!guide.hidden) {
			guide.hidden = true;
			button.setAttribute('aria-expanded', 'false');
			return;
		}
		busy = true;
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
			busy = false;
			button.removeAttribute('aria-busy');
		}
		guide.hidden = false;
		button.setAttribute('aria-expanded', 'true');
	});
}

function renderFirewallGuide(guide, data) {
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

async function saveConnection(prefix, errorBox) {
	const payload = {
		apiUrl: document.getElementById(`${prefix}-api`).value.trim(),
		origin: document.getElementById(`${prefix}-origin`).value.trim(),
		allowNetwork: Boolean(document.getElementById(`${prefix}-network`).checked)
	};
	errorBox.hidden = true;
	if (needsNetworkConsent(payload.apiUrl) && !payload.allowNetwork) {
		errorBox.textContent =
			'Tick the network permission before saving an address that is not loopback.';
		errorBox.hidden = false;
		document.getElementById(`${prefix}-network`).focus();
		return false;
	}
	try {
		await api.updateSettings(payload);
		return true;
	} catch (error) {
		errorBox.textContent = error.message || 'The server address could not be saved.';
		errorBox.hidden = false;
		return false;
	}
}

async function readHealth() {
	try {
		return await api.health();
	} catch (error) {
		return { ok: false, status: 0, message: error.message || 'The server is not answering.' };
	}
}

function showLogin(message = '') {
	boot.hidden = true;
	adminShell.hidden = true;
	loginScreen.hidden = false;
	const error = document.getElementById('login-error');
	if (message) {
		error.textContent = message;
		error.hidden = false;
	} else {
		error.hidden = true;
	}
	document.getElementById('login-email').focus();
}

function showApp() {
	boot.hidden = true;
	loginScreen.hidden = true;
	adminShell.hidden = false;
	document.getElementById('rail-user-name').textContent = state.user.name;
	document.getElementById('rail-user-role').textContent =
		state.user.role === 'admin' ? 'Administrator' : 'Employee';
}

function setView(html) {
	state.render += 1;
	view.innerHTML = html;
	view.classList.remove('view');
	void view.offsetWidth;
	view.classList.add('view');
	hydrateImages(view);
}

function imageTag(src, alt, options = {}) {
	if (!src) return '';
	return `<img data-image="${escapeHtml(src)}" alt="${escapeHtml(alt || '')}" width="${options.width || 400}" height="${options.height || 500}" loading="lazy" />`;
}

function hydrateImages(root) {
	for (const image of root.querySelectorAll('img[data-image]')) {
		const src = image.dataset.image;
		delete image.dataset.image;
		if (!src) continue;
		if (src.startsWith('/images/')) {
			image.src = src;
			continue;
		}
		if (src.startsWith('/api/images/')) {
			api.image(src).then((result) => {
				if (result && result.ok) image.src = result.data;
				else image.classList.add('image-missing');
			});
			continue;
		}
		image.classList.add('image-missing');
	}
}

function loadingView(label = 'Reading the ledger…') {
	return `<div class="loading"><span class="spinner" aria-hidden="true"></span><span>${escapeHtml(label)}</span></div>`;
}

function errorView(title, message, retry = '') {
	return `<div class="empty"><p class="meta">Out of reach</p><h2>${escapeHtml(title)}</h2><p>${escapeHtml(message)}</p>${retry}</div>`;
}

function pageHead(title, sub, actions = '') {
	return `<header class="page-head"><div><h1>${escapeHtml(title)}</h1><p class="page-sub">${sub}</p></div>${actions}</header>`;
}

function statusTag(status) {
	return `<span class="status" data-status="${escapeHtml(status)}">${escapeHtml(titleCase(status))}</span>`;
}

function markRoute(route) {
	state.route = route;
	for (const link of document.querySelectorAll('[data-route]')) {
		if (link.dataset.route === route) link.setAttribute('aria-current', 'page');
		else link.removeAttribute('aria-current');
	}
}

async function bootstrap() {
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

async function loadPreferences() {
	try {
		const data = await call('GET', '/api/admin/preferences');
		if (data && PRESET_TOKENS[data.theme_preset]) {
			state.preset = data.theme_preset;
			localStorage.setItem(PRESET_KEY, state.preset);
		}
	} catch {
		/* the saved local style is a good enough fallback */
	}
}

async function route() {
	if (!state.user) {
		showLogin();
		return;
	}
	const hash = window.location.hash || '#/ledger';
	const name = hash.replace(/^#\/?/, '').split('/')[0] || 'ledger';
	const known = ['ledger', 'counter', 'orders', 'products', 'customers', 'staff', 'connection'];
	const routeName = known.includes(name) ? name : 'ledger';
	markRoute(routeName);
	document.title = `${titleCase(routeName)} · ${state.brand} admin`;
	if (routeName === 'ledger') return renderLedger();
	if (routeName === 'counter') return renderCounter();
	if (routeName === 'orders') return renderOrders();
	if (routeName === 'products') return renderProducts();
	if (routeName === 'customers') return renderCustomers();
	if (routeName === 'staff') return renderStaff();
	return renderConnection();
}

/* Ledger */

async function renderLedger() {
	markRoute('ledger');
	setView(loadingView());
	const request = state.render;
	try {
		const params = new URLSearchParams({ days: String(state.metricsDays) });
		const metrics = await call('GET', `/api/admin/metrics?${params}`);
		if (request !== state.render) return;
		state.metrics = metrics;
		drawLedger(metrics);
	} catch (error) {
		if (request !== state.render) return;
		setView(
			pageHead('The ledger', 'The shop could not be read.') +
				errorView('The numbers are out of reach.', error.message, '<button class="button button-secondary" type="button" id="retry-ledger">Try again</button>')
		);
		document.getElementById('retry-ledger')?.addEventListener('click', () => renderLedger());
	}
}

function drawLedger(metrics) {
	const days = state.metricsDays;
	const toPack = (metrics.status_split || [])
		.filter((item) => ['pending', 'confirmed', 'packed'].includes(item.status))
		.reduce((sum, item) => sum + item.count, 0);
	const lowStock = metrics.low_stock || [];
	const top = metrics.top_products || [];
	const maxUnits = Math.max(1, ...top.map((item) => item.units));
	const topUnits = top.reduce((sum, item) => sum + item.units, 0);
	const topRevenue = top.reduce((sum, item) => sum + item.revenue_cents, 0);
	const mixTotal = (metrics.status_split || []).reduce((sum, item) => sum + item.count, 0);
	const chart = revenueChart(metrics.revenue_series || []);
	const peak = (metrics.revenue_series || []).reduce(
		(best, point) => (point.revenue_cents > best.revenue_cents ? point : best),
		{ date: '', revenue_cents: 0, orders: 0 }
	);
	const period = `<div class="period"><span class="period-label" id="period-label">Period</span><div class="segmented" role="group" aria-labelledby="period-label">${[7, 30, 90]
		.map(
			(value) =>
				`<button type="button" data-days="${value}" aria-pressed="${days === value}">${value} days</button>`
		)
		.join('')}</div></div>`;
	const sub = `${formatDate(metrics.range.from)} to ${formatDate(metrics.range.to)}${
		metrics.demo ? ' · <span class="demo-label">Demo dataset</span>' : ''
	}`;

	const mix = (metrics.status_split || [])
		.filter((item) => item.count > 0)
		.map((item) => {
			const width = mixTotal ? (item.count / mixTotal) * 100 : 0;
			return `<span style="width:${width.toFixed(2)}%;background:var(--a-status-${item.status})" title="${escapeHtml(titleCase(item.status))}: ${item.count}"></span>`;
		})
		.join('');
	const legend = (metrics.status_split || [])
		.map(
			(item) =>
				`<span><i style="background:var(--a-status-${item.status})"></i>${escapeHtml(titleCase(item.status))} ${item.count}</span>`
		)
		.join('');

	const ranked = top.length
		? top
				.map(
					(product, index) => `<div class="rank-row">
						<span class="rank-number">${String(index + 1).padStart(2, '0')}</span>
						<span class="rank-name">${escapeHtml(product.name)}</span>
						<span class="rank-units">${product.units} units</span>
						<span class="rank-money">${money(product.revenue_cents)}</span>
						<span class="rank-share"><span style="width:${((product.units / maxUnits) * 100).toFixed(1)}%"></span></span>
					</div>`
				)
				.join('')
		: '<p class="meta">No products have sold in this period.</p>';

	const recent = (metrics.recent_orders || []).length
		? `<div class="table-wrap"><table><thead><tr><th>Order</th><th>Customer</th><th class="numeric">Total</th><th>Status</th><th>Date</th></tr></thead><tbody>${metrics.recent_orders
				.map(
					(order) => `<tr data-open-order="${escapeHtml(order.id)}" data-clickable="true">
						<td class="mono">${escapeHtml(order.number)}</td>
						<td>${escapeHtml(order.name)}</td>
						<td class="numeric">${money(order.total_cents)}</td>
						<td>${statusTag(order.status)}</td>
						<td class="meta">${formatDate(order.created_at)}</td>
					</tr>`
				)
				.join('')}</tbody></table></div>`
		: '<p class="meta">No orders yet. The first checkout fills this table.</p>';

	setView(`<div>
		${pageHead('The ledger', sub, period)}
		<div class="ledger-top">
			<section class="panel revenue-panel" aria-labelledby="revenue-title">
				<div class="revenue-head">
					<div>
						<p class="revenue-title" id="revenue-title">Takings</p>
						<p class="revenue-figure">${compactMoney(metrics.revenue_cents)}</p>
						<p class="revenue-exact">${money(metrics.revenue_cents)}</p>
						<p class="revenue-delta" data-down="${Number(metrics.delta_pct) < 0}">${
							metrics.delta_pct === null
								? '<span>No previous period to compare.</span>'
								: `<strong>${percent(metrics.delta_pct)}</strong> <span>against ${money(metrics.previous_revenue_cents)} in the previous ${days} days</span>`
						}</p>
					</div>
					<dl class="revenue-summary">
						<div><dt>Orders</dt><dd>${metrics.orders_count}</dd></div>
						<div><dt>Units</dt><dd>${metrics.units_sold}</dd></div>
						<div><dt>Average order</dt><dd>${money(metrics.aov_cents)}</dd></div>
					</dl>
				</div>
				<div class="chart-wrap">${chart}<div class="chart-foot"><span>Best day ${formatDayShort(peak.date)} · ${money(peak.revenue_cents)}</span><span>${peak.orders} orders that day</span></div></div>
			</section>
			<section class="panel attention" aria-labelledby="attention-title">
				<div class="attention-head"><h2 id="attention-title">Needs attention</h2><p class="meta">The work before the numbers.</p></div>
				<div class="attention-cell">
					<span class="attention-number">01</span>
					<div class="attention-copy"><strong>${toPack} ${toPack === 1 ? 'order' : 'orders'} to pack</strong><span>Paid or confirmed, still on the shelf.</span></div>
					<button class="button button-secondary button-small" type="button" data-goto-orders="pending">Pack</button>
				</div>
				<div class="attention-cell">
					<span class="attention-number">02</span>
					<div class="attention-copy"><strong>${lowStock.length} ${lowStock.length === 1 ? 'line' : 'lines'} low on stock</strong><span>${lowStock.length ? escapeHtml(lowStock.slice(0, 2).map((item) => item.name).join(', ')) : 'Every line is above the restock mark.'}</span></div>
					<button class="button button-secondary button-small" type="button" data-goto-low-stock>Review</button>
				</div>
				<div class="attention-cell">
					<span class="attention-number">03</span>
					<div class="attention-copy"><strong>${metrics.new_customers} new ${metrics.new_customers === 1 ? 'customer' : 'customers'}</strong><span>Accounts opened in this period.</span></div>
					<button class="button button-secondary button-small" type="button" data-goto-customers>See</button>
				</div>
			</section>
		</div>

		<section class="band" aria-labelledby="ranked-title">
			<div class="band-head"><div><h2 id="ranked-title">What moved</h2><p>Ranked by units sold in this period.</p></div><span class="meta">${topRevenue ? `${topUnits} units total` : ''}</span></div>
			<div class="ranked">${ranked}</div>
			<div class="rank-foot"><span>${top.length} products with a sale</span><span>${money(topRevenue)}</span></div>
		</section>

		<div class="two-band">
			<section class="band" aria-labelledby="mix-title">
				<div class="band-head"><div><h2 id="mix-title">Order mix</h2><p>The pipeline at a glance.</p></div></div>
				<div class="mix"><div class="mix-rule" role="img" aria-label="Order status mix">${mix}</div><div class="mix-legend">${legend}</div></div>
				<p class="mix-foot">${metrics.status_split.reduce((sum, item) => sum + (['delivered', 'cancelled'].includes(item.status) ? 0 : item.count), 0)} orders still open.</p>
			</section>
			<section class="band" aria-labelledby="low-title">
				<div class="band-head"><div><h2 id="low-title">Low stock</h2><p>Lines the shelf cannot cover for long.</p></div></div>
				${
					lowStock.length
						? `<div class="table-wrap"><table><tbody>${lowStock
								.slice(0, 6)
								.map(
									(item) => `<tr><td class="cell-strong">${escapeHtml(item.name)}</td><td class="numeric ${item.stock === 0 ? 'stock-out' : 'stock-low'}">${item.stock === 0 ? 'Out' : `${item.stock} left`}</td><td class="numeric"><button class="button button-secondary button-small" type="button" data-restock="${escapeHtml(item.id)}">Restock</button></td></tr>`
								)
								.join('')}</tbody></table></div>`
						: '<p class="meta">Nothing sits below the restock mark.</p>'
				}
			</section>
		</div>

		<section class="band" aria-labelledby="recent-title">
			<div class="band-head"><div><h2 id="recent-title">Recent orders</h2><p>The last five, newest first.</p></div><a class="button button-secondary button-small" href="#/orders">All orders</a></div>
			${recent}
		</section>
	</div>`);

	for (const button of view.querySelectorAll('[data-days]')) {
		button.addEventListener('click', () => {
			state.metricsDays = Number(button.dataset.days);
			renderLedger();
		});
	}
	view.querySelector('[data-goto-orders]')?.addEventListener('click', () => {
		state.orderFilter = 'pending';
		state.orderQuery = '';
		location.hash = '#/orders';
	});
	view.querySelector('[data-goto-low-stock]')?.addEventListener('click', () => {
		state.productLowOnly = true;
		state.productQuery = '';
		location.hash = '#/products';
	});
	view.querySelector('[data-goto-customers]')?.addEventListener('click', () => {
		location.hash = '#/customers';
	});
	for (const row of view.querySelectorAll('[data-open-order]')) {
		row.addEventListener('click', () => {
			const order = (metrics.recent_orders || []).find((item) => item.id === row.dataset.openOrder);
			if (order) void openOrder(order.id, order);
		});
	}
	for (const button of view.querySelectorAll('[data-restock]')) {
		button.addEventListener('click', () => {
			state.productQuery = '';
			state.productLowOnly = false;
			state.pendingProduct = button.dataset.restock;
			location.hash = '#/products';
		});
	}
}

function revenueChart(series) {
	if (!series.length) return '<p class="meta">No revenue yet.</p>';
	const width = 760;
	const height = 190;
	const bottom = 160;
	const top = 10;
	const max = Math.max(1, ...series.map((point) => point.revenue_cents));
	const gap = series.length > 1 ? width / series.length : width;
	const barWidth = Math.max(2, gap * 0.62);
	const peakIndex = series.reduce(
		(best, point, index) => (point.revenue_cents > series[best].revenue_cents ? index : best),
		0
	);
	const bars = series
		.map((point, index) => {
			const value = point.revenue_cents;
			const barHeight = value > 0 ? Math.max(2, ((bottom - top) * value) / max) : 0;
			const x = index * gap + (gap - barWidth) / 2;
			return `<rect class="bar" data-peak="${index === peakIndex}" x="${x.toFixed(1)}" y="${(bottom - barHeight).toFixed(1)}" width="${barWidth.toFixed(1)}" height="${barHeight.toFixed(1)}"><title>${escapeHtml(formatDayShort(point.date))}: ${money(value)}, ${point.orders} orders</title></rect>`;
		})
		.join('');
	const labelEvery = Math.max(1, Math.ceil(series.length / 6));
	const labels = series
		.map((point, index) => {
			if (index % labelEvery !== 0 && index !== series.length - 1) return '';
			const x = index * gap + gap / 2;
			return `<text class="axis-label" x="${x.toFixed(1)}" y="182" text-anchor="middle">${escapeHtml(formatDayShort(point.date))}</text>`;
		})
		.join('');
	const grid = [0, 0.5, 1]
		.map((fraction) => {
			const y = bottom - (bottom - top) * fraction;
			return `<line class="grid" x1="0" x2="${width}" y1="${y}" y2="${y}" />`;
		})
		.join('');
	return `<svg class="chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="Revenue by day">${grid}${bars}${labels}</svg>`;
}

/* Counter sale */

async function renderCounter() {
	markRoute('counter');
	setView(loadingView('Reading the shelf…'));
	const request = state.render;
	try {
		const data = await call('GET', '/api/admin/products');
		if (request !== state.render) return;
		state.products = data.products || [];
		if (state.counterReceipt) {
			drawReceipt(state.counterReceipt);
			return;
		}
		drawCounter();
	} catch (error) {
		if (request !== state.render) return;
		setView(pageHead('Counter sale', 'Cash at the counter.') + errorView('The products could not be read.', error.message));
	}
}

function drawCounter() {
	markRoute('counter');
	const query = state.counterQuery.trim().toLowerCase();
	const products = state.products.filter((product) => {
		if (!product.active || Number(product.stock) < 1) return false;
		if (!query) return true;
		return `${product.name} ${product.subtitle} ${product.category?.name || ''}`
			.toLowerCase()
			.includes(query);
	});
	const lines = state.counterLines
		.map((line, index) => {
			const label = Object.entries(line.options || {})
				.map(([key, value]) => `${key}: ${value}`)
				.join(' · ');
			return `<div class="ticket-line">
				<div><strong>${escapeHtml(line.product.name)}</strong><span class="meta">${escapeHtml(label || line.product.size || '')}</span></div>
				<div class="ticket-line-actions">
					<button type="button" data-line-down="${index}" aria-label="Reduce ${escapeHtml(line.product.name)}">−</button>
					<output>${line.quantity}</output>
					<button type="button" data-line-up="${index}" aria-label="Increase ${escapeHtml(line.product.name)}">+</button>
				</div>
				<span class="mono">${money(line.unit * line.quantity)}</span>
			</div>`;
		})
		.join('');
	const total = state.counterLines.reduce((sum, line) => sum + line.unit * line.quantity, 0);
	const count = state.counterLines.reduce((sum, line) => sum + line.quantity, 0);
	setView(`<div>
		${pageHead('Counter sale', 'Take cash, hand over the goods, and let the stock follow.')}
		<div class="counter-layout">
			<div class="counter-menu">
				<div class="counter-search">
					<input class="input" id="counter-search" type="search" placeholder="Find a product on the shelf" value="${escapeHtml(state.counterQuery)}" aria-label="Find a product" />
				</div>
				${
					products.length
						? `<div class="counter-products">${products
								.map(
									(product) => `<button class="counter-item" type="button" data-add-product="${escapeHtml(product.id)}" title="${escapeHtml(product.name)}"${Number(product.stock) < 1 ? ' disabled' : ''}>
										<span class="counter-item-image">${imageTag(product.image, product.name, { width: 92, height: 112 })}</span>
										<span class="counter-item-copy"><strong>${escapeHtml(product.name)}</strong><span>${money(product.effective_price_cents ?? product.price_cents)} · ${product.stock} left</span></span>
									</button>`
								)
								.join('')}</div>`
						: `<p class="meta">No active product matches that.</p>`
				}
			</div>
			<aside class="ticket" aria-labelledby="ticket-title">
				<div class="ticket-head"><h2 id="ticket-title">The ticket</h2><span class="mono">${count} ${count === 1 ? 'item' : 'items'}</span></div>
				<div class="ticket-lines">${lines || '<p class="ticket-empty">Add a product from the shelf.</p>'}</div>
				<div class="ticket-form">
					<div class="field"><label for="counter-customer">Name, optional</label><input class="input" id="counter-customer" autocomplete="off" value="${escapeHtml(state.counterCustomer)}" /></div>
					<div class="field"><label for="counter-phone">Phone, optional</label><input class="input" id="counter-phone" type="tel" autocomplete="off" value="${escapeHtml(state.counterPhone)}" /></div>
					<div class="field"><label for="counter-note">Note, optional</label><input class="input" id="counter-note" autocomplete="off" value="${escapeHtml(state.counterNote)}" /></div>
				</div>
				<div class="ticket-total"><span>Cash</span><span>${money(total)}</span></div>
				<div class="ticket-actions">
					<button class="button button-primary button-block" type="button" id="take-cash"${state.counterLines.length === 0 ? ' disabled' : ''}>Take cash and record the sale</button>
					<button class="button button-secondary button-block" type="button" id="clear-ticket"${state.counterLines.length === 0 ? ' disabled' : ''}>Clear the ticket</button>
				</div>
				<p class="ticket-note">The order is marked delivered, the stock drops, and the takings include it at once.</p>
			</aside>
		</div>
	</div>`);

	const search = document.getElementById('counter-search');
	search?.addEventListener('input', () => {
		state.counterQuery = search.value;
		drawCounter();
		document.getElementById('counter-search')?.focus();
		document.getElementById('counter-search')?.setSelectionRange(search.value.length, search.value.length);
	});
	for (const button of view.querySelectorAll('[data-add-product]')) {
		button.addEventListener('click', () => addCounterProduct(button.dataset.addProduct));
	}
	for (const button of view.querySelectorAll('[data-line-down]')) {
		button.addEventListener('click', () => changeCounterQuantity(Number(button.dataset.lineDown), -1));
	}
	for (const button of view.querySelectorAll('[data-line-up]')) {
		button.addEventListener('click', () => changeCounterQuantity(Number(button.dataset.lineUp), 1));
	}
	document.getElementById('counter-customer')?.addEventListener('input', (event) => {
		state.counterCustomer = event.target.value;
	});
	document.getElementById('counter-phone')?.addEventListener('input', (event) => {
		state.counterPhone = event.target.value;
	});
	document.getElementById('counter-note')?.addEventListener('input', (event) => {
		state.counterNote = event.target.value;
	});
	document.getElementById('clear-ticket')?.addEventListener('click', () => {
		state.counterLines = [];
		drawCounter();
	});
	document.getElementById('take-cash')?.addEventListener('click', () => void takeCash());
}

function addCounterProduct(productId) {
	const product = state.products.find((item) => item.id === productId);
	if (!product) return;
	const fields = (product.option_schema || []).filter((field) => field.type === 'select');
	if (fields.length > 0) {
		openOptionDialog(product, fields);
		return;
	}
	appendCounterLine(product, {});
}

function openOptionDialog(product, fields) {
	const dialog = document.getElementById('order-dialog');
	const selected = {};
	const choices = fields
		.map(
			(field) => `<fieldset class="option-set">
				<legend>${escapeHtml(field.label)}</legend>
				<div class="option-chips">${(product.attributes?.[field.key] || [])
					.map(
						(value) =>
							`<label class="option-chip"><input type="radio" name="sale-${escapeHtml(field.key)}" value="${escapeHtml(value)}" /><span>${escapeHtml(value)}</span></label>`
					)
					.join('')}</div>
			</fieldset>`
		)
		.join('');
	dialog.innerHTML = `<div class="dialog-inner">
		<div class="dialog-head"><div><p class="meta">Counter sale</p><h2 id="order-dialog-title">${escapeHtml(product.name)}</h2></div><button class="dialog-close" type="button" data-close-dialog aria-label="Close">×</button></div>
		<div class="product-editor">${choices}</div>
		<div class="dialog-actions"><button class="button button-primary" type="button" id="add-with-options">Add to the ticket</button></div>
	</div>`;
	if (!dialog.open) dialog.showModal();
	dialog.querySelector('[data-close-dialog]').addEventListener('click', () => dialog.close());
	for (const input of dialog.querySelectorAll('.option-chip input')) {
		input.addEventListener('change', () => {
			selected[input.name.replace(/^sale-/, '')] = input.value;
		});
	}
	dialog.querySelector('#add-with-options').addEventListener('click', () => {
		const missing = fields.some((field) => !selected[field.key]);
		if (missing) {
			toast('Choose every option for this product.', 'error');
			return;
		}
		appendCounterLine(product, { ...selected });
		dialog.close();
	});
}

function appendCounterLine(product, options) {
	const key = `${product.id}|${Object.keys(options).sort().map((name) => `${name}=${encodeURIComponent(options[name])}`).join('&')}`;
	const existing = state.counterLines.find((line) => line.key === key);
	const limit = Math.min(Number(product.stock) || 1, 10);
	if (existing) {
		if (existing.quantity >= limit) {
			toast('That is all the stock this line will take.', 'error');
			return;
		}
		existing.quantity += 1;
	} else {
		state.counterLines.push({
			key,
			product,
			options,
			quantity: 1,
			unit: Number(product.effective_price_cents ?? product.price_cents ?? 0)
		});
	}
	drawCounter();
}

function changeCounterQuantity(index, delta) {
	const line = state.counterLines[index];
	if (!line) return;
	const limit = Math.min(Number(line.product.stock) || 1, 10);
	const next = line.quantity + delta;
	if (next < 1) {
		state.counterLines.splice(index, 1);
		drawCounter();
		return;
	}
	line.quantity = Math.min(next, limit);
	drawCounter();
}

async function takeCash() {
	const button = document.getElementById('take-cash');
	if (button) button.disabled = true;
	try {
		const data = await call('POST', '/api/admin/sales', {
			customer_name: state.counterCustomer.trim(),
			phone: state.counterPhone.trim(),
			note: state.counterNote.trim(),
			items: state.counterLines.map((line) => ({
				product_id: line.product.id,
				quantity: line.quantity,
				options: line.options
			}))
		});
		state.counterReceipt = data.order;
		state.counterLines = [];
		state.counterCustomer = '';
		state.counterPhone = '';
		state.counterNote = '';
		state.products = state.products.map((product) => {
			const line = (data.order.items || []).find((item) => item.product_id === product.id);
			if (!line) return product;
			return { ...product, stock: Math.max(0, Number(product.stock) - line.quantity) };
		});
		drawReceipt(data.order);
		toast(`Sale ${data.order.number} recorded.`);
	} catch (error) {
		toast(error.message || 'The sale could not be recorded.', 'error');
		if (button) button.disabled = false;
	}
}

function drawReceipt(order) {
	markRoute('counter');
	const lines = (order.items || [])
		.map(
			(item) =>
				`<div class="receipt-line"><span>${item.quantity} × ${escapeHtml(item.name)}${item.options_label ? `<span class="meta"> · ${escapeHtml(item.options_label)}</span>` : ''}</span><span class="mono">${money(item.line_total_cents ?? item.unit_price_cents * item.quantity)}</span></div>`
		)
		.join('');
	setView(`<div>
		${pageHead('Sale recorded', 'The stock and the takings are already updated.')}
		<div class="receipt">
			<div class="receipt-head"><div><p class="meta">${escapeHtml(state.brand)} counter · Receipt</p><h2>${escapeHtml(order.number)}</h2><p class="meta">${formatDateTime(order.created_at)}</p></div><span class="status" data-status="delivered">Paid in cash</span></div>
			<div><p class="meta">Customer</p><p>${escapeHtml(order.name)}${order.phone ? ` · ${escapeHtml(order.phone)}` : ''}</p></div>
			<div class="receipt-lines">${lines}</div>
			<div class="receipt-total"><span>Total</span><span>${money(order.total_cents)}</span></div>
			<div class="receipt-served"><span>Served by ${escapeHtml(state.user?.name || 'the counter')}</span><span>Payment: cash</span></div>
			${order.notes ? `<p class="meta">${escapeHtml(order.notes)}</p>` : ''}
			<p class="receipt-fine">Keep this receipt for the order and any courier claim.</p>
			<div class="receipt-actions">
				<button class="button button-secondary" type="button" id="print-receipt">Print the receipt</button>
				<button class="button button-primary" type="button" id="new-sale">New sale</button>
			</div>
		</div>
	</div>`);
	document.getElementById('print-receipt')?.addEventListener('click', () => printOrderReceipt(order));
	document.getElementById('new-sale')?.addEventListener('click', () => {
		state.counterReceipt = null;
		drawCounter();
	});
}

/* Orders */

async function renderOrders() {
	markRoute('orders');
	const tabs = ['', ...ORDER_STATUSES]
		.map(
			(status) =>
				`<button type="button" data-order-filter="${status}" aria-selected="${state.orderFilter === status}">${status ? titleCase(status) : 'All'}</button>`
		)
		.join('');
	setView(`<div>
		${pageHead('Orders', 'Every sale, online and at the counter. Open one to move it.')}
		<div class="tabs" role="tablist" aria-label="Order status">${tabs}</div>
		<div class="toolbar">
			<div class="field field-grow"><label for="order-search">Search</label><input class="input" id="order-search" type="search" placeholder="Number, name, phone or tracking" value="${escapeHtml(state.orderQuery)}" /></div>
			<button class="button button-secondary" type="button" id="refresh-orders">Refresh</button>
		</div>
		<div id="orders-body">${loadingView('Reading the orders…')}</div>
	</div>`);

	for (const tab of view.querySelectorAll('[data-order-filter]')) {
		tab.addEventListener('click', () => {
			state.orderFilter = tab.dataset.orderFilter;
			for (const other of view.querySelectorAll('[data-order-filter]')) {
				other.setAttribute('aria-selected', String(other === tab));
			}
			void loadOrders();
		});
	}
	const search = document.getElementById('order-search');
	search?.addEventListener('input', () => {
		state.orderQuery = search.value;
		clearTimeout(state.orderTimer);
		state.orderTimer = setTimeout(() => void loadOrders(), 260);
	});
	document.getElementById('refresh-orders')?.addEventListener('click', () => void loadOrders());
	await loadOrders();
}

async function loadOrders() {
	const body = document.getElementById('orders-body');
	if (!body) return;
	body.innerHTML = loadingView('Reading the orders…');
	const params = new URLSearchParams();
	if (state.orderFilter) params.set('status', state.orderFilter);
	if (state.orderQuery.trim()) params.set('q', state.orderQuery.trim());
	try {
		const data = await call('GET', `/api/admin/orders${params.toString() ? `?${params}` : ''}`);
		state.orders = data.orders || [];
		drawOrdersTable();
	} catch (error) {
		body.innerHTML = errorView('The orders could not be read.', error.message, '<button class="button button-secondary" type="button" id="retry-orders">Try again</button>');
		document.getElementById('retry-orders')?.addEventListener('click', () => void loadOrders());
	}
}

function drawOrdersTable() {
	const body = document.getElementById('orders-body');
	if (!body) return;
	if (state.orders.length === 0) {
		body.innerHTML = `<div class="empty"><h2>No orders in this view.</h2><p>Try another status, or clear the search.</p></div>`;
		return;
	}
	body.innerHTML = `<div class="table-wrap"><table>
		<thead><tr><th>Order</th><th>Customer</th><th>Delivery</th><th class="numeric">Total</th><th>Status</th><th>Date</th></tr></thead>
		<tbody>${state.orders
			.map(
				(order) => `<tr data-order-id="${escapeHtml(order.id)}" data-clickable="true">
					<td class="mono">${escapeHtml(order.number)}<span class="cell-sub">${escapeHtml(order.id)}</span></td>
					<td><span class="cell-strong">${escapeHtml(order.name)}</span><span class="cell-sub">${escapeHtml(order.phone || order.email || '')}</span></td>
					<td>${escapeHtml(order.delivery_type === 'pickup' ? 'Counter' : order.delivery_type === 'stopdesk' ? 'Stop desk' : 'Home')}</td>
					<td class="numeric">${money(order.total_cents)}</td>
					<td>${statusTag(order.status)}</td>
					<td class="meta">${formatDate(order.created_at)}</td>
				</tr>`
			)
			.join('')}</tbody>
	</table></div>`;
	for (const row of body.querySelectorAll('[data-order-id]')) {
		row.addEventListener('click', () => {
			const order = state.orders.find((item) => item.id === row.dataset.orderId);
			if (order) void openOrder(order.id, order);
		});
	}
}

async function openOrder(id, order) {
	orderDialog.innerHTML = `<div class="dialog-inner">${loadingView('Opening the order…')}</div>`;
	if (!orderDialog.open) orderDialog.showModal();
	try {
		const data = await call('GET', `/api/admin/orders?q=${encodeURIComponent(order.number)}`);
		const fresh = (data.orders || []).find((item) => item.id === id) || order;
		drawOrderDialog(fresh);
	} catch (error) {
		orderDialog.innerHTML = `<div class="dialog-inner"><div class="dialog-head"><h2 id="order-dialog-title">Order</h2><button class="dialog-close" type="button" data-close-dialog aria-label="Close">×</button></div>${errorView('The order could not be opened.', error.message)}</div>`;
		attachDialogClose();
	}
}

function orderActions(order) {
	const allowed = TRANSITIONS[order.status] || [];
	const canCancel = allowed.includes('cancelled') && state.user.role === 'admin';
	const buttons = allowed
		.filter((status) => status !== 'cancelled')
		.map(
			(status) =>
				`<button class="button button-secondary" type="button" data-order-status="${status}">Mark ${titleCase(status)}</button>`
		)
		.join('');
	const courier = order.delivery_type !== 'pickup'
		? `<button class="button button-secondary" type="button" data-create-shipment ${order.tracking_number ? 'hidden' : ''}>Create parcel</button>${order.tracking_number ? `<button class="button button-secondary" type="button" data-refresh-tracking>Refresh tracking</button>` : ''}`
		: '';
	return `${buttons}${courier}${canCancel ? `<button class="button button-danger" type="button" data-cancel-order>Cancel order</button>` : ''}<button class="button button-secondary" type="button" data-print-order>Print receipt</button>`;
}

function drawOrderDialog(order, errorMessage = '') {
	const items = (order.items || [])
		.map(
			(item) =>
				`<div class="order-item"><span>${item.quantity} × ${escapeHtml(item.name)}</span><span class="mono">${money(item.line_total_cents ?? item.unit_price_cents * item.quantity)}</span>${item.options_label ? `<span class="meta">${escapeHtml(item.options_label)}</span>` : ''}</div>`
		)
		.join('');
	orderDialog.innerHTML = `<div class="dialog-inner">
		<div class="dialog-head">
			<div><p class="meta">${escapeHtml(titleCase(order.status))}</p><h2 id="order-dialog-title">${escapeHtml(order.number)}</h2><p class="meta">${formatDateTime(order.created_at)} · ${escapeHtml(order.id)}</p></div>
			<button class="dialog-close" type="button" data-close-dialog aria-label="Close">×</button>
		</div>
		${errorMessage ? `<div class="form-error" role="alert">${escapeHtml(errorMessage)}</div>` : ''}
		<div class="order-grid">
			<div><p class="field-label">Items</p><div class="order-items">${items || '<p class="meta">No items.</p>'}</div>
				<p class="meta" style="margin-top:10px">Subtotal ${money(order.subtotal_cents)} · Shipping ${order.shipping_cents === 0 ? 'free' : money(order.shipping_cents)} · <strong>Total ${money(order.total_cents)}</strong></p>
			</div>
			<div>
				<dl class="detail-list">
					<dt>Customer</dt><dd>${escapeHtml(order.name)}</dd>
					<dt>Phone</dt><dd>${escapeHtml(order.phone || 'Not given')}</dd>
					<dt>Email</dt><dd>${escapeHtml(order.email || 'Not given')}</dd>
					<dt>Delivery</dt><dd>${escapeHtml(order.delivery_type === 'pickup' ? 'Counter sale' : order.delivery_type === 'stopdesk' ? 'Stop desk' : 'Home delivery')}</dd>
					${order.wilaya_name ? `<dt>Wilaya</dt><dd>${escapeHtml(order.wilaya_name)}</dd>` : ''}
					${order.commune_name ? `<dt>Commune</dt><dd>${escapeHtml(order.commune_name)}</dd>` : ''}
					${order.address ? `<dt>Address</dt><dd>${escapeHtml(order.address)}</dd>` : ''}
					${order.stop_desk_name ? `<dt>Stop desk</dt><dd>${escapeHtml(order.stop_desk_name)}</dd>` : ''}
					${order.notes ? `<dt>Notes</dt><dd>${escapeHtml(order.notes)}</dd>` : ''}
					${order.tracking_number ? `<dt>Tracking</dt><dd class="mono">${escapeHtml(order.tracking_number)}</dd>` : ''}
					${order.courier_status ? `<dt>Courier</dt><dd>${escapeHtml(titleCase(order.courier_status))}</dd>` : ''}
				</dl>
				<div class="order-actions">${orderActions(order)}</div>
			</div>
		</div>
	</div>`;
	attachDialogClose();
	const reload = async () => {
		await loadOrders();
		await openOrder(order.id, order);
	};
	for (const button of orderDialog.querySelectorAll('[data-order-status]')) {
		button.addEventListener('click', async () => {
			button.disabled = true;
			try {
				const data = await call('PATCH', `/api/admin/orders/${encodeURIComponent(order.id)}/status`, {
					status: button.dataset.orderStatus
				});
				toast(`Order ${order.number} is ${data.order.status}.`);
				orderDialog.close();
				await refreshCurrent();
			} catch (error) {
				drawOrderDialog(order, error.message);
			}
		});
	}
	orderDialog.querySelector('[data-cancel-order]')?.addEventListener('click', (event) => {
		const button = event.currentTarget;
		if (button.dataset.confirm !== 'true') {
			button.dataset.confirm = 'true';
			button.textContent = 'Confirm cancel';
			return;
		}
		button.disabled = true;
		call('PATCH', `/api/admin/orders/${encodeURIComponent(order.id)}/status`, { status: 'cancelled' })
			.then((data) => {
				toast(`Order ${order.number} is cancelled and the stock is back.`);
				orderDialog.close();
				return refreshCurrent();
			})
			.catch((error) => drawOrderDialog(order, error.message));
	});
	orderDialog.querySelector('[data-print-order]')?.addEventListener('click', () => printOrderReceipt(order));
	orderDialog.querySelector('[data-create-shipment]')?.addEventListener('click', async (event) => {
		const button = event.currentTarget;
		button.disabled = true;
		try {
			const data = await call('POST', `/api/admin/orders/${encodeURIComponent(order.id)}/shipment`);
			toast(`Parcel ${data.order.tracking_number} created.`);
			drawOrderDialog(data.order);
			await refreshCurrent();
		} catch (error) {
			drawOrderDialog(order, error.message);
		}
	});
	orderDialog.querySelector('[data-refresh-tracking]')?.addEventListener('click', async (event) => {
		const button = event.currentTarget;
		button.disabled = true;
		try {
			const data = await call('POST', `/api/admin/orders/${encodeURIComponent(order.id)}/tracking`);
			drawOrderDialog(data.order);
			toast('Tracking refreshed.');
			await refreshCurrent();
		} catch (error) {
			drawOrderDialog(order, error.message);
		}
	});
}

function refreshCurrent() {
	if (state.route === 'orders') return loadOrders();
	if (state.route === 'ledger') return renderLedger();
	if (state.route === 'products') {
		drawProducts();
	}
	return Promise.resolve();
}

function attachDialogClose() {
	for (const button of orderDialog.querySelectorAll('[data-close-dialog]')) {
		button.addEventListener('click', () => orderDialog.close());
	}
}

/* Products and stock */

async function renderProducts() {
	markRoute('products');
	setView(loadingView('Reading the products…'));
	const request = state.render;
	try {
		const data = await call('GET', '/api/admin/products');
		if (request !== state.render) return;
		state.products = data.products || [];
		drawProducts();
		if (state.pendingProduct) {
			const pending = state.pendingProduct;
			state.pendingProduct = '';
			void openProductEditor(pending);
		}
	} catch (error) {
		if (request !== state.render) return;
		setView(pageHead('Products and stock', 'The shelf behind the counter.') + errorView('The products could not be read.', error.message));
	}
}

function filteredProducts() {
	const query = state.productQuery.trim().toLowerCase();
	return state.products.filter((product) => {
		if (state.productLowOnly && Number(product.stock) > 5) return false;
		if (!query) return true;
		return `${product.name} ${product.slug} ${product.category?.name || ''}`
			.toLowerCase()
			.includes(query);
	});
}

function drawProducts() {
	const products = filteredProducts();
	const low = state.products.filter((product) => Number(product.stock) <= 5).length;
	setView(`<div>
		${pageHead('Products and stock', `${state.products.length} lines · ${low} at or below five units`, '')}
		<div class="toolbar">
			<div class="field field-grow"><label for="product-search">Search</label><input class="input" id="product-search" type="search" placeholder="Name, slug or category" value="${escapeHtml(state.productQuery)}" /></div>
			<label class="checkbox-row" style="min-height:40px;align-items:center"><input type="checkbox" id="low-only"${state.productLowOnly ? ' checked' : ''} /> Low stock only</label>
		</div>
		${
			products.length
				? `<div class="table-wrap"><table>
						<thead><tr><th>Product</th><th>Category</th><th class="numeric">Price</th><th>Stock</th><th>State</th><th></th></tr></thead>
						<tbody>${products
							.map(
								(product) => (() => {
									const imeiStock = product.plugin_id === 'phone';
									return `<tr data-product-row="${escapeHtml(product.id)}" data-clickable="true">
									<td><div class="product-cell"><span class="product-thumb">${imageTag(product.image, product.name, { width: 72, height: 88 })}</span><span><span class="cell-strong">${escapeHtml(product.name)}</span><span class="cell-sub">${escapeHtml(product.slug)}</span></span></div></td>
									<td>${escapeHtml(product.category?.name || '')}</td>
									<td class="numeric">${money(product.effective_price_cents ?? product.price_cents)}${product.on_sale ? `<span class="cell-sub">was ${money(product.price_cents)}</span>` : ''}</td>
									<td><div class="inline-stock" data-stock-control="${escapeHtml(product.id)}" data-clickable="false"><button type="button" data-stock-down="${escapeHtml(product.id)}" aria-label="Reduce stock for ${escapeHtml(product.name)}"${state.user.role === 'admin' && !imeiStock ? '' : ' disabled'}>−</button><output class="${Number(product.stock) === 0 ? 'stock-out' : Number(product.stock) <= 5 ? 'stock-low' : ''}" title="${imeiStock ? 'Stock follows the registered IMEIs' : ''}">${product.stock}</output><button type="button" data-stock-up="${escapeHtml(product.id)}" aria-label="Add stock for ${escapeHtml(product.name)}"${state.user.role === 'admin' && !imeiStock ? '' : ' disabled'}>+</button></div>${imeiStock ? '<span class="cell-sub">IMEI stock</span>' : ''}</td>
									<td>${product.active ? '<span class="status" data-status="delivered">Active</span>' : '<span class="status" data-status="cancelled">Archived</span>'}</td>
									<td><button class="button button-secondary button-small" type="button" data-edit-product="${escapeHtml(product.id)}">${state.user.role === 'admin' ? 'Edit' : 'View'}</button></td>
								</tr>`;
								})()
							)
							.join('')}</tbody>
					</table></div>`
				: `<div class="empty"><h2>No product in this view.</h2><p>Clear the filter or the search.</p></div>`
		}
	</div>`);

	const search = document.getElementById('product-search');
	search?.addEventListener('input', () => {
		state.productQuery = search.value;
		drawProducts();
		document.getElementById('product-search')?.focus();
		document.getElementById('product-search')?.setSelectionRange(search.value.length, search.value.length);
	});
	document.getElementById('low-only')?.addEventListener('change', (event) => {
		state.productLowOnly = event.target.checked;
		drawProducts();
	});
	for (const button of view.querySelectorAll('[data-edit-product]')) {
		button.addEventListener('click', (event) => {
			event.stopPropagation();
			void openProductEditor(button.dataset.editProduct);
		});
	}
	for (const row of view.querySelectorAll('[data-product-row]')) {
		row.addEventListener('click', () => void openProductEditor(row.dataset.productRow));
	}
	for (const button of view.querySelectorAll('[data-stock-down]')) {
		button.addEventListener('click', (event) => {
			event.stopPropagation();
			void adjustStock(button.dataset.stockDown, -1);
		});
	}
	for (const button of view.querySelectorAll('[data-stock-up]')) {
		button.addEventListener('click', (event) => {
			event.stopPropagation();
			void adjustStock(button.dataset.stockUp, 1);
		});
	}
}

async function adjustStock(productId, delta) {
	const product = state.products.find((item) => item.id === productId);
	if (!product) return;
	if (state.user.role !== 'admin') {
		toast('Only an administrator can change stock.', 'error');
		return;
	}
	if (product.plugin_id === 'phone') {
		toast('Phone stock follows the registered IMEIs.', 'error');
		return;
	}
	const next = Math.max(0, Number(product.stock) + delta);
	try {
		const data = await call('PATCH', `/api/admin/products/${encodeURIComponent(productId)}`, {
			stock: next
		});
		state.products = state.products.map((item) => (item.id === productId ? data.product : item));
		drawProducts();
		toast(`${product.name}: ${next} in stock.`);
	} catch (error) {
		toast(error.message || 'The stock could not be changed.', 'error');
	}
}

async function openProductEditor(productId) {
	const product = state.products.find((item) => item.id === productId);
	if (!product) return;
	const dialog = document.getElementById('order-dialog');
	const admin = state.user.role === 'admin';
	const imeiStock = product.plugin_id === 'phone';
	dialog.innerHTML = `<div class="dialog-inner">
		<div class="dialog-head"><div><p class="meta">${escapeHtml(product.category?.name || 'Product')}</p><h2 id="order-dialog-title">${escapeHtml(product.name)}</h2><p class="meta">${escapeHtml(product.slug)}</p></div><button class="dialog-close" type="button" data-close-dialog aria-label="Close">×</button></div>
		<form class="product-editor" id="product-form" novalidate>
			<div class="product-editor-grid">
				<div class="field"><label for="product-price">Price, DZD</label><input class="input" id="product-price" type="number" min="1" step="0.01" value="${moneyValue(product.price_cents)}"${admin ? '' : ' disabled'} /></div>
				<div class="field"><label for="product-stock">Stock</label><input class="input" id="product-stock" type="number" min="0" step="1" value="${product.stock}"${admin && !imeiStock ? '' : ' disabled'} />${imeiStock ? '<p class="field-hint">Phone stock follows the registered IMEIs. Register units on the web plugin screen.</p>' : ''}</div>
				<div class="field"><label for="product-sale">Sale price, DZD</label><input class="input" id="product-sale" type="number" min="0" step="0.01" value="${product.sale_price_cents ? moneyValue(product.sale_price_cents) : ''}" placeholder="No sale"${admin ? '' : ' disabled'} /></div>
				<div class="field"><label for="product-sale-start">Sale starts</label><input class="input" id="product-sale-start" type="datetime-local" value="${toLocalInput(product.sale_starts_at)}"${admin ? '' : ' disabled'} /></div>
				<div class="field"><label for="product-sale-end">Sale ends</label><input class="input" id="product-sale-end" type="datetime-local" value="${toLocalInput(product.sale_ends_at)}"${admin ? '' : ' disabled'} /></div>
				<div class="field"><label>State</label><label class="checkbox-row"><input type="checkbox" id="product-active"${product.active ? ' checked' : ''}${admin ? '' : ' disabled'} /> Active on the shelf</label></div>
			</div>
			<div class="form-error" id="product-error" role="alert" hidden></div>
			<div class="dialog-actions">
				${admin ? '<button class="button button-primary" type="submit">Save the line</button>' : '<p class="meta">Only an administrator can edit this line.</p>'}
				<button class="button button-secondary" type="button" data-close-dialog>Close</button>
				${admin && product.active ? '<button class="button button-danger" type="button" id="archive-product">Archive</button>' : ''}
			</div>
		</form>
	</div>`;
	if (!dialog.open) dialog.showModal();
	attachDialogClose();
	dialog.querySelector('#product-form')?.addEventListener('submit', async (event) => {
		event.preventDefault();
		const price = fromDinars(document.getElementById('product-price').value);
		const stock = Number(document.getElementById('product-stock').value);
		const sale = fromDinars(document.getElementById('product-sale').value);
		const start = toRFC3339(document.getElementById('product-sale-start').value);
		const end = toRFC3339(document.getElementById('product-sale-end').value);
		const errorBox = document.getElementById('product-error');
		if (price < 1) {
			errorBox.textContent = 'Enter a price above zero.';
			errorBox.hidden = false;
			return;
		}
		if (!Number.isInteger(stock) || stock < 0) {
			errorBox.textContent = 'Stock must be a whole number at zero or above.';
			errorBox.hidden = false;
			return;
		}
		if (sale > 0 && (!start || !end)) {
			errorBox.textContent = 'A sale needs a start and an end.';
			errorBox.hidden = false;
			return;
		}
		try {
			const payload = {
				price_cents: price,
				active: document.getElementById('product-active').checked,
				sale_price_cents: sale,
				sale_starts_at: sale > 0 ? start : '',
				sale_ends_at: sale > 0 ? end : ''
			};
			if (!imeiStock) payload.stock = stock;
			const data = await call('PATCH', `/api/admin/products/${encodeURIComponent(product.id)}`, payload);
			state.products = state.products.map((item) => (item.id === product.id ? data.product : item));
			dialog.close();
			drawProducts();
			toast(`${data.product.name} saved.`);
		} catch (error) {
			errorBox.textContent = error.message;
			errorBox.hidden = false;
		}
	});
	const archive = dialog.querySelector('#archive-product');
	archive?.addEventListener('click', () => {
		if (archive.dataset.confirm !== 'true') {
			archive.dataset.confirm = 'true';
			archive.textContent = 'Confirm archive';
			return;
		}
		archive.disabled = true;
		call('DELETE', `/api/admin/products/${encodeURIComponent(product.id)}`)
			.then(() => {
				state.products = state.products.map((item) =>
					item.id === product.id ? { ...item, active: false } : item
				);
				dialog.close();
				drawProducts();
				toast(`${product.name} archived.`);
			})
			.catch((error) => {
				archive.disabled = false;
				toast(error.message || 'The product could not be archived.', 'error');
			});
	});
}

/* Customers */

async function renderCustomers() {
	markRoute('customers');
	setView(loadingView('Reading the customers…'));
	const request = state.render;
	try {
		const data = await call('GET', '/api/admin/customers');
		if (request !== state.render) return;
		state.customers = data.customers || [];
		drawCustomers();
	} catch (error) {
		if (request !== state.render) return;
		setView(pageHead('Customers', 'The people behind the orders.') + errorView('The customers could not be read.', error.message));
	}
}

function drawCustomers() {
	const query = state.customerQuery.trim().toLowerCase();
	const customers = state.customers.filter((customer) =>
		`${customer.name} ${customer.email}`.toLowerCase().includes(query)
	);
	setView(`<div>
		${pageHead('Customers', `${state.customers.length} accounts with an order history`)}
		<div class="toolbar"><div class="field field-grow"><label for="customer-search">Search</label><input class="input" id="customer-search" type="search" placeholder="Name or email" value="${escapeHtml(state.customerQuery)}" /></div></div>
		${
			customers.length
				? `<div class="table-wrap"><table><thead><tr><th>Customer</th><th class="numeric">Orders</th><th class="numeric">Lifetime spend</th><th>Last order</th><th>Joined</th></tr></thead><tbody>${customers
						.map(
							(customer) => `<tr><td><span class="cell-strong">${escapeHtml(customer.name)}</span><span class="cell-sub">${escapeHtml(customer.email)}</span></td><td class="numeric">${customer.orders_count}</td><td class="numeric">${money(customer.total_spent_cents)}</td><td>${customer.last_order_at ? formatDate(customer.last_order_at) : 'No orders yet'}</td><td class="meta">${formatDate(customer.created_at)}</td></tr>`
						)
						.join('')}</tbody></table></div>`
				: `<div class="empty"><h2>No customer matches that.</h2><p>Clear the search to see every account.</p></div>`
		}
	</div>`);
	const search = document.getElementById('customer-search');
	search?.addEventListener('input', () => {
		state.customerQuery = search.value;
		drawCustomers();
		document.getElementById('customer-search')?.focus();
		document.getElementById('customer-search').setSelectionRange(search.value.length, search.value.length);
	});
}

/* Staff */

async function renderStaff() {
	markRoute('staff');
	setView(loadingView('Reading the team…'));
	const request = state.render;
	try {
		const data = await call('GET', '/api/admin/team');
		if (request !== state.render) return;
		state.team = data.users || [];
		drawStaff();
	} catch (error) {
		if (request !== state.render) return;
		setView(pageHead('Staff', 'Who can open this ledger.') + errorView('The team could not be read.', error.message));
	}
}

function drawStaff() {
	const admin = state.user.role === 'admin';
	setView(`<div>
		${pageHead('Staff', 'Administrators run the shop. Employees sell and fulfil orders.')}
		<div class="table-wrap"><table><thead><tr><th>Name</th><th>Email</th><th>Role</th><th>Joined</th></tr></thead><tbody>${state.team
			.map(
				(member) => `<tr><td class="cell-strong">${escapeHtml(member.name)}${member.id === state.user.id ? '<span class="cell-sub">You</span>' : ''}</td><td>${escapeHtml(member.email)}</td><td>${
					admin && member.id !== state.user.id
						? `<select class="select" data-role-for="${escapeHtml(member.id)}" aria-label="Role for ${escapeHtml(member.name)}">${['admin', 'employee', 'customer']
								.map(
									(role) =>
										`<option value="${role}"${member.role === role ? ' selected' : ''}>${titleCase(role)}</option>`
								)
								.join('')}</select>`
						: `<span class="status" data-status="${member.role === 'admin' ? 'confirmed' : member.role === 'employee' ? 'packed' : 'cancelled'}">${titleCase(member.role)}</span>`
				}</td><td class="meta">${formatDate(member.created_at)}</td></tr>`
			)
			.join('')}</tbody></table></div>
	</div>`);
	for (const select of view.querySelectorAll('[data-role-for]')) {
		select.addEventListener('change', async () => {
			const id = select.dataset.roleFor;
			const previous = state.team.find((member) => member.id === id)?.role;
			select.disabled = true;
			try {
				const data = await call('PATCH', `/api/admin/team/${encodeURIComponent(id)}`, {
					role: select.value
				});
				state.team = state.team.map((member) => (member.id === id ? data.user : member));
				toast(`${data.user.name} is now ${data.user.role}.`);
				drawStaff();
			} catch (error) {
				if (previous) select.value = previous;
				select.disabled = false;
				toast(error.message || 'The role could not be changed.', 'error');
			}
		});
	}
}

/* Connection */

function renderConnection() {
	markRoute('connection');
	setView(`<div>
		${pageHead('Connection', 'The app calls your server from Go, not from this page. No web server sits in the middle.')}
		<form class="product-editor" id="app-connection-form" novalidate style="max-width:620px">
			${connectionFieldsHtml('app')}
			<div class="form-error" id="app-connection-error" role="alert" hidden></div>
			<div class="connection-actions">
				<button class="button button-primary" type="submit">Save and reconnect</button>
				<button class="button button-secondary" type="button" id="app-test">Test connection</button>
				<button class="button button-secondary" type="button" id="app-clear-session">Clear session</button>
			</div>
			<dl class="detail-list">
				<dt>Version</dt><dd class="mono">${escapeHtml(state.info?.version || 'Unknown')}</dd>
				<dt>Platform</dt><dd>${escapeHtml(state.info?.platform || 'Unknown')}</dd>
				<dt>Resolved origin</dt><dd class="mono">${escapeHtml(state.info?.resolvedOrigin || state.health?.origin || 'Not resolved')}</dd>
				<dt>Server state</dt><dd>${state.health?.ok ? 'Connected' : 'Offline'}</dd>
			</dl>
		</form>
	</div>`);
	applyConnectionValues('app', state.info);
	bindConnectionFields('app');
	const errorBox = document.getElementById('app-connection-error');
	document.getElementById('app-connection-form')?.addEventListener('submit', async (event) => {
		event.preventDefault();
		if (!(await saveConnection('app', errorBox))) return;
		toast('Server address saved.');
		await bootstrap();
		location.hash = '#/connection';
	});
	document.getElementById('app-test')?.addEventListener('click', async () => {
		const health = await readHealth();
		state.health = health;
		toast(
			health.ok ? 'The server answered.' : health.message || 'The server did not answer.',
			health.ok ? 'info' : 'error'
		);
		renderConnection();
	});
	document.getElementById('app-clear-session')?.addEventListener('click', async () => {
		try {
			await api.clearSession();
		} catch (error) {
			toast(error.message || 'The local session could not be cleared.', 'error');
			return;
		}
		state.user = null;
		showLogin('The local session is cleared.');
	});
}

/* Boot */

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

document.getElementById('connection-form').addEventListener('submit', async (event) => {
	event.preventDefault();
	const error = document.getElementById('connection-error');
	if (!(await saveConnection('connection', error))) return;
	await bootstrap();
});

document.getElementById('login-retry')?.addEventListener('click', () => {
	void bootstrap();
});

document.getElementById('sign-out').addEventListener('click', async () => {
	let serverConfirmed = true;
	try {
		await call('POST', '/api/auth/logout');
	} catch {
		serverConfirmed = false;
	}
	let localCleared = true;
	try {
		await api.clearSession();
	} catch (error) {
		localCleared = false;
		toast(error.message || 'The local session could not be cleared.', 'error');
	}
	state.user = null;
	state.metrics = null;
	state.orders = [];
	state.products = [];
	state.customers = [];
	state.team = [];
	state.counterLines = [];
	state.counterReceipt = null;
	if (!localCleared) {
		showLogin(
			'Signed out here, but the saved session could not be removed. Sign in again and use Clear session.'
		);
		return;
	}
	if (!serverConfirmed) {
		showLogin('Signed out on this device. The server did not confirm the logout.');
		return;
	}
	showLogin();
});

document.getElementById('scheme-toggle').addEventListener('click', () => {
	state.scheme = state.scheme === 'dark' ? 'light' : 'dark';
	localStorage.setItem(SCHEME_KEY, state.scheme);
	applyTheme();
});

document.getElementById('preset-select').addEventListener('change', (event) => {
	void savePreset(event.target.value);
});

orderDialog.addEventListener('click', (event) => {
	if (event.target === orderDialog) orderDialog.close();
});

window.addEventListener('hashchange', () => void route());
window.addEventListener('keydown', (event) => {
	if (event.key === 'Escape' && orderDialog.open) orderDialog.close();
});

document.getElementById('login-connection-fields').innerHTML = connectionFieldsHtml('connection');
applyConnectionValues('connection', null);
bindConnectionFields('connection');

document.addEventListener('click', (event) => {
	const target = event.target instanceof Element ? event.target.closest('a[href]') : null;
	if (!target || event.defaultPrevented) return;
	const href = target.getAttribute('href') || '';
	if (href.startsWith('#')) return;
	event.preventDefault();
	if (!/^https?:\/\//i.test(href)) {
		toast('That link is not available in the desktop app.', 'error');
		return;
	}
	api.openExternal(href).catch((error) => {
		toast(error.message || 'The link could not be opened.', 'error');
	});
});

window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('drop', (event) => event.preventDefault());

window.open = () => {
	toast('External windows are disabled. Use a link in the page.', 'error');
	return null;
};

applyTheme();
void bootstrap();
