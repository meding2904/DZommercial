// The stable shell nodes and the delegation helper. Screens bind one listener
// per event and action instead of re-binding after every innerHTML swap. The
// nodes are getters so importing this module never needs a live document.

const byId = (id) => document.getElementById(id);

export const nodes = {
	get boot() {
		return byId('boot');
	},
	get loginScreen() {
		return byId('login-screen');
	},
	get adminShell() {
		return byId('admin-shell');
	},
	get view() {
		return byId('view');
	},
	get toast() {
		return byId('toast');
	},
	get orderDialog() {
		return byId('order-dialog');
	}
};

/**
 * @param {EventTarget} root A stable container such as the view or the dialog.
 * @param {string} type
 * @param {string} selector
 * @param {(event: Event, target: Element) => void} handler
 */
export function delegate(root, type, selector, handler) {
	root.addEventListener(type, (event) => {
		const target =
			event.target && typeof event.target.closest === 'function'
				? event.target.closest(selector)
				: null;
		if (!target) return;
		handler(event, target);
	});
}
