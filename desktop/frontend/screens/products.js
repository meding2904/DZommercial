// Products and stock screen: the shelf list, inline stock changes, the editor
// dialog and archive. The table re-renders on every change; its buttons are
// delegated on the stable view, including the row beneath them.

import { state } from '../state.js';
import { call } from '../api.js';
import { captureRender } from '../stale.js';
import { setView } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { errorView, loadingView, pageHead } from '../views/common.js';
import { productsView } from '../views/products.js';
import { markRoute, registerScreen } from '../router.js';
import { escapeHtml, fromDinars, moneyValue, toLocalInput, toRFC3339 } from '../format.js';
import { toast } from '../toast.js';

export async function renderProducts() {
	markRoute('products');
	setView(loadingView('Reading the products…'));
	const fresh = captureRender(state);
	try {
		const data = await call('GET', '/api/admin/products');
		if (!fresh()) return;
		state.products = data.products || [];
		drawProducts();
		if (state.pendingProduct) {
			const pending = state.pendingProduct;
			state.pendingProduct = '';
			void openProductEditor(pending);
		}
	} catch (error) {
		if (!fresh()) return;
		setView(
			pageHead('Products and stock', 'The shelf behind the counter.') +
				errorView('The products could not be read.', error.message)
		);
	}
}

function drawProducts() {
	setView(productsView(state));
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
	state.openProduct = product;
	const dialog = nodes.orderDialog;
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
}

delegate(nodes.view, 'input', '#product-search', (event, target) => {
	state.productQuery = target.value;
	drawProducts();
	const search = document.getElementById('product-search');
	search?.focus();
	search?.setSelectionRange(search.value.length, search.value.length);
});

delegate(nodes.view, 'change', '#low-only', (event, target) => {
	state.productLowOnly = target.checked;
	drawProducts();
});

delegate(
	nodes.view,
	'click',
	'[data-stock-down], [data-stock-up], [data-edit-product], [data-product-row]',
	(event, target) => {
		if (target.matches('[data-stock-down]')) {
			void adjustStock(target.dataset.stockDown, -1);
			return;
		}
		if (target.matches('[data-stock-up]')) {
			void adjustStock(target.dataset.stockUp, 1);
			return;
		}
		if (target.matches('[data-edit-product]')) {
			void openProductEditor(target.dataset.editProduct);
			return;
		}
		void openProductEditor(target.dataset.productRow);
	}
);

delegate(nodes.orderDialog, 'submit', '#product-form', async (event) => {
	event.preventDefault();
	const product = state.openProduct;
	if (!product) return;
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
		if (product.plugin_id !== 'phone') payload.stock = stock;
		const data = await call('PATCH', `/api/admin/products/${encodeURIComponent(product.id)}`, payload);
		state.products = state.products.map((item) => (item.id === product.id ? data.product : item));
		nodes.orderDialog.close();
		drawProducts();
		toast(`${data.product.name} saved.`);
	} catch (error) {
		errorBox.textContent = error.message;
		errorBox.hidden = false;
	}
});

delegate(nodes.orderDialog, 'click', '#archive-product', (event, target) => {
	const product = state.openProduct;
	if (!product) return;
	if (target.dataset.confirm !== 'true') {
		target.dataset.confirm = 'true';
		target.textContent = 'Confirm archive';
		return;
	}
	target.disabled = true;
	call('DELETE', `/api/admin/products/${encodeURIComponent(product.id)}`)
		.then(() => {
			state.products = state.products.map((item) =>
				item.id === product.id ? { ...item, active: false } : item
			);
			nodes.orderDialog.close();
			drawProducts();
			toast(`${product.name} archived.`);
		})
		.catch((error) => {
			target.disabled = false;
			toast(error.message || 'The product could not be archived.', 'error');
		});
});

registerScreen('products', { render: renderProducts, refresh: drawProducts });
