// Connection: the server address form and the live bridge facts. Pure: state
// in, HTML out. The form fields are shared with the login screen.

import { escapeHtml } from '../format.js';
import { pageHead } from './common.js';

/**
 * @param {string} prefix
 * @returns {string}
 */
export function connectionFieldsHtml(prefix) {
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

/**
 * @param {{ info: any, health: any }} state
 * @returns {string}
 */
export function connectionView(state) {
	return `<div>
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
				<dt>Resolved origin</dt><dd class="mono" id="app-resolved-origin">${escapeHtml(state.info?.resolvedOrigin || state.health?.origin || 'Not resolved')}</dd>
				<dt>Server state</dt><dd id="app-server-state">${state.health?.ok ? 'Connected' : 'Offline'}</dd>
			</dl>
		</form>
	</div>`;
}
