// Minimal browser globals for the renderer's node --test suite. Tests install
// these before importing a module that reads localStorage or the document.

/**
 * @param {Record<string, string>} [seed]
 * @returns {Map<string, string>}
 */
export function installStorage(seed = {}) {
	const data = new Map(Object.entries(seed));
	globalThis.localStorage = {
		getItem(key) {
			return data.has(key) ? data.get(key) : null;
		},
		setItem(key, value) {
			data.set(key, String(value));
		},
		removeItem(key) {
			data.delete(key);
		},
		clear() {
			data.clear();
		}
	};
	return data;
}

/**
 * @param {string} id
 */
function createNode(id) {
	let html = '';
	let writes = 0;
	const node = {
		id,
		value: '',
		checked: false,
		hidden: false,
		open: false,
		disabled: false,
		textContent: '',
		className: '',
		dataset: {},
		style: { cssText: '', setProperty() {} },
		classList: { add() {}, remove() {}, toggle() {} },
		focused: false,
		focus() {
			node.focused = true;
		},
		close() {
			node.open = false;
		},
		showModal() {
			node.open = true;
		},
		addEventListener() {},
		removeEventListener() {},
		setAttribute() {},
		removeAttribute() {},
		append() {},
		replaceChildren() {},
		querySelector() {
			return null;
		},
		querySelectorAll() {
			return [];
		},
		contains() {
			return false;
		},
		setSelectionRange() {},
		remove() {}
	};
	Object.defineProperty(node, 'innerHTML', {
		get() {
			return html;
		},
		set(value) {
			html = String(value);
			writes += 1;
		}
	});
	Object.defineProperty(node, 'innerHTMLWrites', {
		get() {
			return writes;
		}
	});
	return node;
}

/**
 * Installs a document whose getElementById returns the same stub per id.
 * @param {string[]} [ids]
 * @returns {Map<string, ReturnType<typeof createNode>>}
 */
export function installDocument(ids = ['view', 'toast', 'boot', 'login-screen', 'admin-shell', 'order-dialog']) {
	const registry = new Map();
	const node = (id) => {
		if (!registry.has(id)) registry.set(id, createNode(id));
		return registry.get(id);
	};
	for (const id of ids) node(id);
	globalThis.document = {
		title: '',
		documentElement: { dataset: {}, style: { setProperty() {} } },
		body: createNode('body'),
		getElementById(id) {
			return node(id);
		},
		querySelectorAll() {
			return [];
		},
		createElement(tag) {
			return createNode(tag);
		},
		addEventListener() {},
		removeEventListener() {}
	};
	return registry;
}

/**
 * Stubs window.go.main.App. Handlers receive the bridge arguments.
 * @param {Record<string, (...args: any[]) => any>} handlers
 * @returns {Array<{ method: string, args: any[] }>}
 */
export function installBridge(handlers) {
	const calls = [];
	const app = {};
	for (const [method, handler] of Object.entries(handlers)) {
		app[method] = (...args) => {
			calls.push({ method, args });
			return handler(...args);
		};
	}
	globalThis.window = { go: { main: { App: app } } };
	return calls;
}

/**
 * @param {number} milliseconds
 * @returns {Promise<void>}
 */
export function wait(milliseconds = 0) {
	return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
