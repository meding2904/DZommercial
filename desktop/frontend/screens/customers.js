// Customers screen: the account list and its search. The view is pure; the
// search input is delegated on the stable view.

import { state } from '../state.js';
import { call } from '../api.js';
import { captureRender } from '../stale.js';
import { setView } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { errorView, loadingView, pageHead } from '../views/common.js';
import { customersView } from '../views/customers.js';
import { markRoute, registerScreen } from '../router.js';

export async function renderCustomers() {
	markRoute('customers');
	setView(loadingView('Reading the customers…'));
	const fresh = captureRender(state);
	try {
		const data = await call('GET', '/api/admin/customers');
		if (!fresh()) return;
		state.customers = data.customers || [];
		setView(customersView(state));
	} catch (error) {
		if (!fresh()) return;
		setView(
			pageHead('Customers', 'The people behind the orders.') +
				errorView('The customers could not be read.', error.message)
		);
	}
}

delegate(nodes.view, 'input', '#customer-search', (event, target) => {
	state.customerQuery = target.value;
	setView(customersView(state));
	const search = document.getElementById('customer-search');
	search?.focus();
	search.setSelectionRange(search.value.length, search.value.length);
});

registerScreen('customers', { render: renderCustomers });
