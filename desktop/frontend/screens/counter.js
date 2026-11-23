// Counter sale screen: the shelf, the ticket and the cash call. Ticket maths is
// pure (ticket.js); this module owns the Wails calls and the screen state.

import { state } from '../state.js';
import { call } from '../api.js';
import { captureRender } from '../stale.js';
import { setView, printOrderReceipt } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { errorView, loadingView, pageHead } from '../views/common.js';
import { counterReceiptView, counterView } from '../views/counter.js';
import { markRoute, registerScreen } from '../router.js';
import { escapeHtml } from '../format.js';
import { changeTicketQuantity, mergeTicketLine, stockAfterSale } from '../ticket.js';
import { toast } from '../toast.js';

export async function renderCounter() {
	markRoute('counter');
	setView(loadingView('Reading the shelf…'));
	const fresh = captureRender(state);
	try {
		const data = await call('GET', '/api/admin/products');
		if (!fresh()) return;
		state.products = data.products || [];
		if (state.counterReceipt) {
			drawReceipt();
			return;
		}
		drawCounter();
	} catch (error) {
		if (!fresh()) return;
		setView(
			pageHead('Counter sale', 'Cash at the counter.') +
				errorView('The products could not be read.', error.message)
		);
	}
}

function drawCounter() {
	markRoute('counter');
	setView(counterView(state));
}

function drawReceipt() {
	markRoute('counter');
	setView(counterReceiptView(state));
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
	const dialog = nodes.orderDialog;
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
	state.counterOptions = { product, fields, selected };
	if (!dialog.open) dialog.showModal();
}

function appendCounterLine(product, options) {
	const result = mergeTicketLine(state.counterLines, product, options);
	state.counterLines = result.lines;
	if (result.blocked) {
		toast('That is all the stock this line will take.', 'error');
		return;
	}
	drawCounter();
}

function changeCounterQuantity(index, delta) {
	const result = changeTicketQuantity(state.counterLines, index, delta);
	state.counterLines = result.lines;
	if (result.changed) drawCounter();
}

async function takeCash() {
	const button = document.getElementById('take-cash');
	const fresh = captureRender(state);
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
		if (!fresh()) return;
		state.counterReceipt = data.order;
		state.counterLines = [];
		state.counterCustomer = '';
		state.counterPhone = '';
		state.counterNote = '';
		state.products = stockAfterSale(state.products, data.order.items || []);
		drawReceipt();
		toast(`Sale ${data.order.number} recorded.`);
	} catch (error) {
		if (!fresh()) return;
		toast(error.message || 'The sale could not be recorded.', 'error');
		if (button) button.disabled = false;
	}
}

delegate(nodes.view, 'input', '#counter-search', (event, target) => {
	state.counterQuery = target.value;
	drawCounter();
	const search = document.getElementById('counter-search');
	search?.focus();
	search?.setSelectionRange(search.value.length, search.value.length);
});

delegate(nodes.view, 'click', '[data-add-product]', (event, target) => {
	addCounterProduct(target.dataset.addProduct);
});

delegate(nodes.view, 'click', '[data-line-down]', (event, target) => {
	changeCounterQuantity(Number(target.dataset.lineDown), -1);
});

delegate(nodes.view, 'click', '[data-line-up]', (event, target) => {
	changeCounterQuantity(Number(target.dataset.lineUp), 1);
});

delegate(nodes.view, 'input', '#counter-customer', (event, target) => {
	state.counterCustomer = target.value;
});

delegate(nodes.view, 'input', '#counter-phone', (event, target) => {
	state.counterPhone = target.value;
});

delegate(nodes.view, 'input', '#counter-note', (event, target) => {
	state.counterNote = target.value;
});

delegate(nodes.view, 'click', '#clear-ticket', () => {
	state.counterLines = [];
	drawCounter();
});

delegate(nodes.view, 'click', '#take-cash', () => void takeCash());

delegate(nodes.view, 'click', '#print-receipt', () => {
	if (state.counterReceipt) {
		printOrderReceipt(state.counterReceipt, state.user?.name || 'the counter');
	}
});

delegate(nodes.view, 'click', '#new-sale', () => {
	state.counterReceipt = null;
	drawCounter();
});

delegate(nodes.orderDialog, 'change', '.option-chip input', (event, target) => {
	if (!state.counterOptions) return;
	state.counterOptions.selected[target.name.replace(/^sale-/, '')] = target.value;
});

delegate(nodes.orderDialog, 'click', '#add-with-options', () => {
	const options = state.counterOptions;
	if (!options) return;
	const missing = options.fields.some((field) => !options.selected[field.key]);
	if (missing) {
		toast('Choose every option for this product.', 'error');
		return;
	}
	appendCounterLine(options.product, { ...options.selected });
	nodes.orderDialog.close();
});

registerScreen('counter', { render: renderCounter, refresh: renderCounter });
