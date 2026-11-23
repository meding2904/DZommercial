// Order statuses and transitions. The API in docs/API.md is the enforcement
// authority; this table mirrors it once for both the web and desktop admin UIs.

/**
 * @typedef {'pending' | 'confirmed' | 'packed' | 'shipped' | 'delivered' | 'cancelled'} OrderStatus
 */

/** @type {OrderStatus[]} */
export const ORDER_STATUSES = [
	'pending',
	'confirmed',
	'packed',
	'shipped',
	'delivered',
	'cancelled'
];

/** @type {Record<OrderStatus, OrderStatus[]>} */
export const STATUS_TRANSITIONS = {
	pending: ['confirmed', 'cancelled'],
	confirmed: ['packed', 'cancelled'],
	packed: ['shipped', 'cancelled'],
	shipped: ['delivered'],
	delivered: [],
	cancelled: []
};
