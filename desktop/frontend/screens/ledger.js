// Ledger screen: fetches the period metrics, paints the loading state, then the
// ledger view. The view is pure; the listeners are delegated on the stable view.

import { state } from '../state.js';
import { call } from '../api.js';
import { captureRender } from '../stale.js';
import { setView } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { errorView, loadingView, pageHead } from '../views/common.js';
import { ledgerView } from '../views/ledger.js';
import { markRoute, registerScreen } from '../router.js';
import { openOrder } from './orders.js';

export async function renderLedger() {
	markRoute('ledger');
	setView(loadingView());
	const fresh = captureRender(state);
	try {
		const params = new URLSearchParams({ days: String(state.metricsDays) });
		const metrics = await call('GET', `/api/admin/metrics?${params}`);
		if (!fresh()) return;
		state.metrics = metrics;
		setView(ledgerView(state));
	} catch (error) {
		if (!fresh()) return;
		setView(
			pageHead('The ledger', 'The shop could not be read.') +
				errorView(
					'The numbers are out of reach.',
					error.message,
					'<button class="button button-secondary" type="button" id="retry-ledger">Try again</button>'
				)
		);
	}
}

delegate(nodes.view, 'click', '#retry-ledger', () => void renderLedger());

delegate(nodes.view, 'click', '[data-days]', (event, target) => {
	state.metricsDays = Number(target.dataset.days);
	void renderLedger();
});

delegate(nodes.view, 'click', '[data-goto-orders]', () => {
	state.orderFilter = 'pending';
	state.orderQuery = '';
	location.hash = '#/orders';
});

delegate(nodes.view, 'click', '[data-goto-low-stock]', () => {
	state.productLowOnly = true;
	state.productQuery = '';
	location.hash = '#/products';
});

delegate(nodes.view, 'click', '[data-goto-customers]', () => {
	location.hash = '#/customers';
});

delegate(nodes.view, 'click', '[data-open-order]', (event, target) => {
	const order = (state.metrics?.recent_orders || []).find(
		(item) => item.id === target.dataset.openOrder
	);
	if (order) void openOrder(order.id, order);
});

delegate(nodes.view, 'click', '[data-restock]', (event, target) => {
	state.productQuery = '';
	state.productLowOnly = false;
	state.pendingProduct = target.dataset.restock;
	location.hash = '#/products';
});

registerScreen('ledger', { render: renderLedger, refresh: renderLedger });
