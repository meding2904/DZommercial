// The Wails bridge adapter. Wails binds main.App methods on window.go.main.App;
// a Go error rejects the promise, so every call is normalized back to an Error
// carrying the Go message. This is the renderer's only way out of the page.

/**
 * @param {string} method
 * @param {...unknown} args
 * @returns {Promise<unknown>}
 */
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

/**
 * @param {unknown} error
 * @returns {string}
 */
export function bridgeMessage(error) {
	if (typeof error === 'string' && error) return error;
	if (error && typeof error.message === 'string' && error.message) return error.message;
	return 'The app bridge did not answer.';
}

export const api = {
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
