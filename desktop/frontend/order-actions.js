// Which moves an order offers. The transition table lives in domain/statuses.js
// and mirrors docs/API.md; this module only answers what the renderer may show.

import { STATUS_TRANSITIONS } from './domain/statuses.js';

/**
 * @typedef {object} OrderActionPlan
 * @property {string[]} steps Status buttons other than cancel, in table order.
 * @property {boolean} canCancel Administrators only.
 * @property {boolean} showCourier True for delivery and stop-desk orders.
 * @property {boolean} hasTracking A courier parcel already exists.
 */

/**
 * @param {{ status?: string, delivery_type?: string, tracking_number?: string }} order
 * @param {string} role
 * @returns {OrderActionPlan}
 */
export function orderActionPlan(order, role) {
	const allowed = STATUS_TRANSITIONS[order.status] || [];
	return {
		steps: allowed.filter((status) => status !== 'cancelled'),
		canCancel: allowed.includes('cancelled') && role === 'admin',
		showCourier: order.delivery_type !== 'pickup',
		hasTracking: Boolean(order.tracking_number)
	};
}
