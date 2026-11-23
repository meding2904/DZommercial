// The application shell: painting the view, hydrating images, printing receipts
// and the one dialog. Screens call setView with HTML from a pure view module.

import { state } from './state.js';
import { nodes } from './dom.js';
import { api } from './bridge.js';
import { toast } from './toast.js';
import { receiptHtml } from './domain/receipts.js';

/**
 * @param {string} html
 */
export function setView(html) {
	state.render += 1;
	nodes.view.innerHTML = html;
	nodes.view.classList.remove('view');
	void nodes.view.offsetWidth;
	nodes.view.classList.add('view');
	hydrateImages(nodes.view);
}

/**
 * @param {Element} root
 */
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

/**
 * @param {string} html
 */
export function printHtml(html) {
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

/**
 * @param {object} order
 * @param {string} [servedBy]
 */
export function printOrderReceipt(order, servedBy = '') {
	printHtml(receiptHtml(state.brand, order, servedBy));
}

// The dialog element survives every innerHTML swap, so its dismiss gestures are
// bound once here instead of after every draw.
export function initDialog() {
	nodes.orderDialog.addEventListener('click', (event) => {
		if (event.target === nodes.orderDialog) nodes.orderDialog.close();
	});
	nodes.orderDialog.addEventListener('click', (event) => {
		const close =
			event.target && typeof event.target.closest === 'function'
				? event.target.closest('[data-close-dialog]')
				: null;
		if (close) nodes.orderDialog.close();
	});
	window.addEventListener('keydown', (event) => {
		if (event.key === 'Escape' && nodes.orderDialog.open) nodes.orderDialog.close();
	});
}
