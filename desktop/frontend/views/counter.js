// The counter sale screen and its receipt. Pure: state in, HTML out. The
// ticket arithmetic lives in ticket.js.

import { formatDateTime, escapeHtml, money } from '../format.js';
import { pageHead, imageTag } from './common.js';
import { ticketCount, ticketTotal } from '../ticket.js';

/**
 * @param {any} state
 * @returns {string}
 */
export function counterView(state) {
	const query = state.counterQuery.trim().toLowerCase();
	const products = state.products.filter((product) => {
		if (!product.active || Number(product.stock) < 1) return false;
		if (!query) return true;
		return `${product.name} ${product.subtitle} ${product.category?.name || ''}`
			.toLowerCase()
			.includes(query);
	});
	const lines = state.counterLines
		.map((line, index) => {
			const label = Object.entries(line.options || {})
				.map(([key, value]) => `${key}: ${value}`)
				.join(' · ');
			return `<div class="ticket-line">
				<div><strong>${escapeHtml(line.product.name)}</strong><span class="meta">${escapeHtml(label || line.product.size || '')}</span></div>
				<div class="ticket-line-actions">
					<button type="button" data-line-down="${index}" aria-label="Reduce ${escapeHtml(line.product.name)}">−</button>
					<output>${line.quantity}</output>
					<button type="button" data-line-up="${index}" aria-label="Increase ${escapeHtml(line.product.name)}">+</button>
				</div>
				<span class="mono">${money(line.unit * line.quantity)}</span>
			</div>`;
		})
		.join('');
	const total = ticketTotal(state.counterLines);
	const count = ticketCount(state.counterLines);
	return `<div>
		${pageHead('Counter sale', 'Take cash, hand over the goods, and let the stock follow.')}
		<div class="counter-layout">
			<div class="counter-menu">
				<div class="counter-search">
					<input class="input" id="counter-search" type="search" placeholder="Find a product on the shelf" value="${escapeHtml(state.counterQuery)}" aria-label="Find a product" />
				</div>
				${
					products.length
						? `<div class="counter-products">${products
								.map(
									(product) => `<button class="counter-item" type="button" data-add-product="${escapeHtml(product.id)}" title="${escapeHtml(product.name)}"${Number(product.stock) < 1 ? ' disabled' : ''}>
										<span class="counter-item-image">${imageTag(product.image, product.name, { width: 92, height: 112 })}</span>
										<span class="counter-item-copy"><strong>${escapeHtml(product.name)}</strong><span>${money(product.effective_price_cents ?? product.price_cents)} · ${product.stock} left</span></span>
									</button>`
								)
								.join('')}</div>`
						: `<p class="meta">No active product matches that.</p>`
				}
			</div>
			<aside class="ticket" aria-labelledby="ticket-title">
				<div class="ticket-head"><h2 id="ticket-title">The ticket</h2><span class="mono">${count} ${count === 1 ? 'item' : 'items'}</span></div>
				<div class="ticket-lines">${lines || '<p class="ticket-empty">Add a product from the shelf.</p>'}</div>
				<div class="ticket-form">
					<div class="field"><label for="counter-customer">Name, optional</label><input class="input" id="counter-customer" autocomplete="off" value="${escapeHtml(state.counterCustomer)}" /></div>
					<div class="field"><label for="counter-phone">Phone, optional</label><input class="input" id="counter-phone" type="tel" autocomplete="off" value="${escapeHtml(state.counterPhone)}" /></div>
					<div class="field"><label for="counter-note">Note, optional</label><input class="input" id="counter-note" autocomplete="off" value="${escapeHtml(state.counterNote)}" /></div>
				</div>
				<div class="ticket-total"><span>Cash</span><span>${money(total)}</span></div>
				<div class="ticket-actions">
					<button class="button button-primary button-block" type="button" id="take-cash"${state.counterLines.length === 0 ? ' disabled' : ''}>Take cash and record the sale</button>
					<button class="button button-secondary button-block" type="button" id="clear-ticket"${state.counterLines.length === 0 ? ' disabled' : ''}>Clear the ticket</button>
				</div>
				<p class="ticket-note">The order is marked delivered, the stock drops, and the takings include it at once.</p>
			</aside>
		</div>
	</div>`;
}

/**
 * @param {any} state
 * @returns {string}
 */
export function counterReceiptView(state) {
	const order = state.counterReceipt;
	const lines = (order.items || [])
		.map(
			(item) =>
				`<div class="receipt-line"><span>${item.quantity} × ${escapeHtml(item.name)}${item.options_label ? `<span class="meta"> · ${escapeHtml(item.options_label)}</span>` : ''}</span><span class="mono">${money(item.line_total_cents ?? item.unit_price_cents * item.quantity)}</span></div>`
		)
		.join('');
	return `<div>
		${pageHead('Sale recorded', 'The stock and the takings are already updated.')}
		<div class="receipt">
			<div class="receipt-head"><div><p class="meta">${escapeHtml(state.brand)} counter · Receipt</p><h2>${escapeHtml(order.number)}</h2><p class="meta">${formatDateTime(order.created_at)}</p></div><span class="status" data-status="delivered">Paid in cash</span></div>
			<div><p class="meta">Customer</p><p>${escapeHtml(order.name)}${order.phone ? ` · ${escapeHtml(order.phone)}` : ''}</p></div>
			<div class="receipt-lines">${lines}</div>
			<div class="receipt-total"><span>Total</span><span>${money(order.total_cents)}</span></div>
			<div class="receipt-served"><span>Served by ${escapeHtml(state.user?.name || 'the counter')}</span><span>Payment: cash</span></div>
			${order.notes ? `<p class="meta">${escapeHtml(order.notes)}</p>` : ''}
			<p class="receipt-fine">Keep this receipt for the order and any courier claim.</p>
			<div class="receipt-actions">
				<button class="button button-secondary" type="button" id="print-receipt">Print the receipt</button>
				<button class="button button-primary" type="button" id="new-sale">New sale</button>
			</div>
		</div>
	</div>`;
}
