// Tauri initialization script that recreates the Wails binding surface the
// renderer already uses: window.go.main.App.<Method>. Wails passed positional
// arguments, Tauri invokes take one object with camelCase keys, so this shim
// owns that mapping. A Rust error rejects with its message string; it is turned
// into an Error here so bridge.js sees the same shape it saw under Wails.
;(function () {
	'use strict';

	const core = window.__TAURI__ && window.__TAURI__.core;
	if (!core || typeof core.invoke !== 'function') return;

	function text(value) {
		return value == null ? '' : String(value);
	}

	function call(command, args) {
		return core.invoke(command, args).catch(function (error) {
			const message =
				typeof error === 'string' && error
					? error
					: error && typeof error.message === 'string' && error.message
						? error.message
						: 'The app bridge did not answer.';
			throw new Error(message);
		});
	}

	const app = {
		Info: function () {
			return call('info', {});
		},
		Health: function () {
			return call('health', {});
		},
		Request: function (method, path, body) {
			return call('request', {
				method: text(method),
				path: text(path),
				body: body == null ? {} : body
			});
		},
		Image: function (path) {
			return call('image', { path: text(path) });
		},
		UpdateSettings: function (apiUrl, origin, allowNetwork) {
			return call('update_settings', {
				apiUrl: text(apiUrl),
				origin: text(origin),
				allowNetwork: Boolean(allowNetwork)
			});
		},
		ClearSession: function () {
			return call('clear_session', {});
		},
		FirewallGuide: function () {
			return call('firewall_guide', {});
		},
		OpenExternal: function (url) {
			return call('open_external', { url: text(url) });
		}
	};

	Object.freeze(app);
	window.go = { main: { App: app } };

	// Wails ran with DisableWebViewDrop. Keep the webview from navigating to a
	// file the user drags onto it, and do the same for the Tauri drag handler
	// configured on the window builder.
	window.addEventListener(
		'dragover',
		function (event) {
			event.preventDefault();
		},
		true
	);
	window.addEventListener(
		'drop',
		function (event) {
			event.preventDefault();
			event.stopPropagation();
		},
		true
	);
})();
