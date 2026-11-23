// The renderer never navigates away or opens a second window. Links outside the
// app go back through the Go bridge, which applies its own URL allowlist. The
// guard is a function so node --test can drive it with a fake window and page.

/**
 * @param {string} href
 * @returns {boolean}
 */
export function isExternalHref(href) {
	return /^https?:\/\//i.test(href);
}

/**
 * @param {{
 *   window?: any,
 *   document?: any,
 *   openExternal: (href: string) => Promise<unknown> | unknown,
 *   warn: (message: string) => void
 * }} options
 */
export function installNavigationGuards({ window: win, document: doc, openExternal, warn }) {
	doc.addEventListener('click', (event) => {
		const target =
			event.target && typeof event.target.closest === 'function'
				? event.target.closest('a[href]')
				: null;
		if (!target || event.defaultPrevented) return;
		const href = target.getAttribute('href') || '';
		if (href.startsWith('#')) return;
		event.preventDefault();
		if (!isExternalHref(href)) {
			warn('That link is not available in the desktop app.');
			return;
		}
		Promise.resolve()
			.then(() => openExternal(href))
			.catch((error) => {
				warn((error && error.message) || 'The link could not be opened.');
			});
	});

	win.addEventListener('dragover', (event) => event.preventDefault());
	win.addEventListener('drop', (event) => event.preventDefault());

	win.open = () => {
		warn('External windows are disabled. Use a link in the page.');
		return null;
	};
}
