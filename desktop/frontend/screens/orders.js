// Orders screen: the list, the debounced search and the order dialog with its
// status and courier actions. The dialog survives innerHTML swaps, so its
// actions are delegated once instead of re-bound after every draw.

import { state } from '../state.js';
import { call } from '../api.js';
import { setView, printOrderReceipt } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { errorView, loadingView } from '../views/common.js';
import { orderDialogView, ordersTableBody, ordersView } from '../views/orders.js';
import { markRoute, refreshCurrent, registerScreen } from '../router.js';
import { captureRender } from '../stale.js';
import { toast } from '../toast.js';

export async function renderOrders() {
	markRoute('orders');
	setView(ordersView(state));
	await loadOrders();
}

// The list can have several loads in flight (debounced search, Refresh, a status
// move). The newest attempt wins; the render epoch catches navigation away.
let loadAttempt = 0;

export async function loadOrders() {
	const body = document.getElementById('orders-body');
	if (!body) return;
	const fresh = captureRender(state);
	const attempt = ++loadAttempt;
	body.innerHTML = loadingView('Reading the orders…');
	const params = new URLSearchParams();
	if (state.orderFilter) params.set('status', state.orderFilter);
	if (state.orderQuery.trim()) params.set('q', state.orderQuery.trim());
	try {
		const data = await call('GET', `/api/admin/orders${params.toString() ? `?${params}` : ''}`);
		if (!fresh() || attempt !== loadAttempt) return;
		state.orders = data.orders || [];
		drawOrdersTable();
	} catch (error) {
		if (!fresh() || attempt !== loadAttempt) return;
		body.innerHTML = errorView(
			'The orders could not be read.',
			error.message,
			'<button class="button button-secondary" type="button" id="retry-orders">Try again</button>'
		);
	}
}

function drawOrdersTable() {
	const body = document.getElementById('orders-body');
	if (!body) return;
	body.innerHTML = ordersTableBody(state.orders);
}

export async function openOrder(id, order) {
	state.openOrder = order;
	const fresh = captureRender(state);
	nodes.orderDialog.innerHTML = `<div class="dialog-inner">${loadingView('Opening the order…')}</div>`;
	if (!nodes.orderDialog.open) nodes.orderDialog.showModal();
	try {
		const data = await call('GET', `/api/admin/orders?q=${encodeURIComponent(order.number)}`);
		if (!fresh() || state.openOrder?.id !== id) return;
		const loaded = (data.orders || []).find((item) => item.id === id) || order;
		drawOrderDialog(loaded);
	} catch (error) {
		if (!fresh() || state.openOrder?.id !== id) return;
		nodes.orderDialog.innerHTML = `<div class="dialog-inner"><div class="dialog-head"><h2 id="order-dialog-title">Order</h2><button class="dialog-close" type="button" data-close-dialog aria-label="Close">×</button></div>${errorView('The order could not be opened.', error.message)}</div>`;
	}
}

function drawOrderDialog(order, errorMessage = '') {
	state.openOrder = order;
	nodes.orderDialog.innerHTML = orderDialogView(order, { role: state.user.role, errorMessage });
}

delegate(nodes.view, 'click', '[data-order-filter]', (event, target) => {
	state.orderFilter = target.dataset.orderFilter;
	for (const other of nodes.view.querySelectorAll('[data-order-filter]')) {
		other.setAttribute('aria-pressed', String(other === target));
	}
	void loadOrders();
});

delegate(nodes.view, 'input', '#order-search', (event, target) => {
	state.orderQuery = target.value;
	clearTimeout(state.orderTimer);
	state.orderTimer = setTimeout(() => void loadOrders(), 260);
});

delegate(nodes.view, 'click', '#refresh-orders', () => void loadOrders());
delegate(nodes.view, 'click', '#retry-orders', () => void loadOrders());

delegate(nodes.view, 'click', '[data-order-id]', (event, target) => {
	const order = state.orders.find((item) => item.id === target.dataset.orderId);
	if (order) void openOrder(order.id, order);
});

delegate(nodes.orderDialog, 'click', '[data-order-status]', async (event, target) => {
	const order = state.openOrder;
	if (!order) return;
	const orderId = order.id;
	const fresh = captureRender(state);
	target.disabled = true;
	try {
		const data = await call('PATCH', `/api/admin/orders/${encodeURIComponent(order.id)}/status`, {
			status: target.dataset.orderStatus
		});
		if (!fresh() || state.openOrder?.id !== orderId) return;
		toast(`Order ${order.number} is ${data.order.status}.`);
		nodes.orderDialog.close();
		await refreshCurrent();
	} catch (error) {
		if (fresh() && state.openOrder?.id === orderId) drawOrderDialog(order, error.message);
	}
});

delegate(nodes.orderDialog, 'click', '[data-cancel-order]', (event, target) => {
	const order = state.openOrder;
	if (!order) return;
	const orderId = order.id;
	const fresh = captureRender(state);
	if (target.dataset.confirm !== 'true') {
		target.dataset.confirm = 'true';
		target.textContent = 'Confirm cancel';
		return;
	}
	target.disabled = true;
	call('PATCH', `/api/admin/orders/${encodeURIComponent(order.id)}/status`, { status: 'cancelled' })
		.then((data) => {
			if (!fresh() || state.openOrder?.id !== orderId) return;
			toast(`Order ${order.number} is cancelled and the stock is back.`);
			nodes.orderDialog.close();
			return refreshCurrent();
		})
		.catch((error) => {
			if (fresh() && state.openOrder?.id === orderId) drawOrderDialog(order, error.message);
		});
});

delegate(nodes.orderDialog, 'click', '[data-print-order]', () => {
	if (state.openOrder) printOrderReceipt(state.openOrder, state.user?.name || 'the counter');
});

delegate(nodes.orderDialog, 'click', '[data-create-shipment]', async (event, target) => {
	const order = state.openOrder;
	if (!order) return;
	const orderId = order.id;
	const fresh = captureRender(state);
	target.disabled = true;
	try {
		const data = await call('POST', `/api/admin/orders/${encodeURIComponent(order.id)}/shipment`);
		if (!fresh() || state.openOrder?.id !== orderId) return;
		toast(`Parcel ${data.order.tracking_number} created.`);
		drawOrderDialog(data.order);
		await refreshCurrent();
	} catch (error) {
		if (fresh() && state.openOrder?.id === orderId) drawOrderDialog(order, error.message);
	}
});

delegate(nodes.orderDialog, 'click', '[data-refresh-tracking]', async (event, target) => {
	const order = state.openOrder;
	if (!order) return;
	const orderId = order.id;
	const fresh = captureRender(state);
	target.disabled = true;
	try {
		const data = await call('POST', `/api/admin/orders/${encodeURIComponent(order.id)}/tracking`);
		if (!fresh() || state.openOrder?.id !== orderId) return;
		drawOrderDialog(data.order);
		toast('Tracking refreshed.');
		await refreshCurrent();
	} catch (error) {
		if (fresh() && state.openOrder?.id === orderId) drawOrderDialog(order, error.message);
	}
});

registerScreen('orders', { render: renderOrders, refresh: loadOrders });
