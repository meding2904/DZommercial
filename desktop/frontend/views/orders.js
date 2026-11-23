// Orders: the tabbed list and the order dialog. Pure: state in, HTML out.

import { ORDER_STATUSES } from '../domain/statuses.js';
import { courierStatusLabel, deliveryTypeLabel, titleCase } from '../domain/labels.js';
import { escapeHtml, formatDate, formatDateTime, money } from '../format.js';
import { orderActionPlan } from '../order-actions.js';
import { loadingView, pageHead, statusTag } from './common.js';

/**
 * @param {{ orderFilter: string, orderQuery: string }} state
 * @returns {string}
 */
export function ordersView(state) {
	const tabs = ['', ...ORDER_STATUSES]
		.map(
			(status) =>
				`<button type="button" data-order-filter="${status}" aria-pressed="${state.orderFilter === status}">${status ? titleCase(status) : 'All'}</button>`
		)
		.join('');
	return `<div>
		${pageHead('Orders', 'Every sale, online and at the counter. Open one to move it.')}
		<div class="tabs" role="group" aria-label="Order status">${tabs}</div>
		<div class="toolbar">
			<div class="field field-grow"><label for="order-search">Search</label><input class="input" id="order-search" type="search" placeholder="Number, name, phone or tracking" value="${escapeHtml(state.orderQuery)}" /></div>
			<button class="button button-secondary" type="button" id="refresh-orders">Refresh</button>
		</div>
		<div id="orders-body">${loadingView('Reading the orders…')}</div>
	</div>`;
}

/**
 * @param {any[]} orders
 * @returns {string}
 */
export function ordersTableBody(orders) {
	if (orders.length === 0) {
		return `<div class="empty"><h2>No orders in this view.</h2><p>Try another status, or clear the search.</p></div>`;
	}
	return `<div class="table-wrap"><table>
		<thead><tr><th>Order</th><th>Customer</th><th>Delivery</th><th class="numeric">Total</th><th>Status</th><th>Date</th></tr></thead>
		<tbody>${orders
			.map(
				(order) => `<tr data-order-id="${escapeHtml(order.id)}" data-clickable="true">
					<td class="mono">${escapeHtml(order.number)}<span class="cell-sub">${escapeHtml(order.id)}</span></td>
					<td><span class="cell-strong">${escapeHtml(order.name)}</span><span class="cell-sub">${escapeHtml(order.phone || order.email || '')}</span></td>
					<td>${escapeHtml(deliveryTypeLabel(order.delivery_type))}</td>
					<td class="numeric">${money(order.total_cents)}</td>
					<td>${statusTag(order.status)}</td>
					<td class="meta">${formatDate(order.created_at)}</td>
				</tr>`
			)
			.join('')}</tbody>
	</table></div>`;
}

/**
 * @param {any} order
 * @param {{ role: string, errorMessage?: string }} options
 * @returns {string}
 */
export function orderDialogView(order, { role, errorMessage = '' }) {
	const items = (order.items || [])
		.map(
			(item) =>
				`<div class="order-item"><span>${item.quantity} × ${escapeHtml(item.name)}</span><span class="mono">${money(item.line_total_cents ?? item.unit_price_cents * item.quantity)}</span>${item.options_label ? `<span class="meta">${escapeHtml(item.options_label)}</span>` : ''}</div>`
		)
		.join('');
	return `<div class="dialog-inner">
		<div class="dialog-head">
			<div><p class="meta">${escapeHtml(titleCase(order.status))}</p><h2 id="order-dialog-title">${escapeHtml(order.number)}</h2><p class="meta">${formatDateTime(order.created_at)} · ${escapeHtml(order.id)}</p></div>
			<button class="dialog-close" type="button" data-close-dialog aria-label="Close">×</button>
		</div>
		${errorMessage ? `<div class="form-error" role="alert">${escapeHtml(errorMessage)}</div>` : ''}
		<div class="order-grid">
			<div><p class="field-label">Items</p><div class="order-items">${items || '<p class="meta">No items.</p>'}</div>
				<p class="meta" style="margin-top:10px">Subtotal ${money(order.subtotal_cents)} · Shipping ${order.shipping_cents === 0 ? 'free' : money(order.shipping_cents)} · <strong>Total ${money(order.total_cents)}</strong></p>
			</div>
			<div>
				<dl class="detail-list">
					<dt>Customer</dt><dd>${escapeHtml(order.name)}</dd>
					<dt>Phone</dt><dd>${escapeHtml(order.phone || 'Not given')}</dd>
					<dt>Email</dt><dd>${escapeHtml(order.email || 'Not given')}</dd>
					<dt>Delivery</dt><dd>${escapeHtml(deliveryTypeLabel(order.delivery_type))}</dd>
					${order.wilaya_name ? `<dt>Wilaya</dt><dd>${escapeHtml(order.wilaya_name)}</dd>` : ''}
					${order.commune_name ? `<dt>Commune</dt><dd>${escapeHtml(order.commune_name)}</dd>` : ''}
					${order.address ? `<dt>Address</dt><dd>${escapeHtml(order.address)}</dd>` : ''}
					${order.stop_desk_name ? `<dt>Stop desk</dt><dd>${escapeHtml(order.stop_desk_name)}</dd>` : ''}
					${order.notes ? `<dt>Notes</dt><dd>${escapeHtml(order.notes)}</dd>` : ''}
					${order.tracking_number ? `<dt>Tracking</dt><dd class="mono">${escapeHtml(order.tracking_number)}</dd>` : ''}
					${order.courier_status ? `<dt>Courier</dt><dd>${escapeHtml(courierStatusLabel(order.courier_status))}</dd>` : ''}
				</dl>
				<div class="order-actions">${orderActionsView(order, role)}</div>
			</div>
		</div>
	</div>`;
}

/**
 * @param {any} order
 * @param {string} role
 * @returns {string}
 */
function orderActionsView(order, role) {
	const plan = orderActionPlan(order, role);
	const buttons = plan.steps
		.map(
			(status) =>
				`<button class="button button-secondary" type="button" data-order-status="${status}">Mark ${titleCase(status)}</button>`
		)
		.join('');
	const courier = plan.showCourier
		? `<button class="button button-secondary" type="button" data-create-shipment ${order.tracking_number ? 'hidden' : ''}>Create parcel</button>${plan.hasTracking ? `<button class="button button-secondary" type="button" data-refresh-tracking>Refresh tracking</button>` : ''}`
		: '';
	return `${buttons}${courier}${plan.canCancel ? `<button class="button button-danger" type="button" data-cancel-order>Cancel order</button>` : ''}<button class="button button-secondary" type="button" data-print-order>Print receipt</button>`;
}
