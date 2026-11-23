// Customers: the people behind the orders. Pure: state in, HTML out.

import { escapeHtml, formatDate, money } from '../format.js';
import { pageHead } from './common.js';

/**
 * @param {any} state
 * @returns {string}
 */
export function customersView(state) {
	const query = state.customerQuery.trim().toLowerCase();
	const customers = state.customers.filter((customer) =>
		`${customer.name} ${customer.email}`.toLowerCase().includes(query)
	);
	return `<div>
		${pageHead('Customers', `${state.customers.length} accounts with an order history`)}
		<div class="toolbar"><div class="field field-grow"><label for="customer-search">Search</label><input class="input" id="customer-search" type="search" placeholder="Name or email" value="${escapeHtml(state.customerQuery)}" /></div></div>
		${
			customers.length
				? `<div class="table-wrap"><table><thead><tr><th>Customer</th><th class="numeric">Orders</th><th class="numeric">Lifetime spend</th><th>Last order</th><th>Joined</th></tr></thead><tbody>${customers
						.map(
							(customer) => `<tr><td><span class="cell-strong">${escapeHtml(customer.name)}</span><span class="cell-sub">${escapeHtml(customer.email)}</span></td><td class="numeric">${customer.orders_count}</td><td class="numeric">${money(customer.total_spent_cents)}</td><td>${customer.last_order_at ? formatDate(customer.last_order_at) : 'No orders yet'}</td><td class="meta">${formatDate(customer.created_at)}</td></tr>`
						)
						.join('')}</tbody></table></div>`
				: `<div class="empty"><h2>No customer matches that.</h2><p>Clear the search to see every account.</p></div>`
		}
	</div>`;
}
