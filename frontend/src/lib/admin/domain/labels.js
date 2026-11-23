// Human labels for the order domain. Plain ES module so the dependency-free
// desktop renderer can import the same bytes the web app uses.

/**
 * Capitalizes every word and turns underscores into spaces.
 * @param {string | null | undefined} value
 * @returns {string}
 */
export function titleCase(value) {
	return String(value ?? '')
		.replaceAll('_', ' ')
		.split(/\s+/)
		.filter(Boolean)
		.map((word) => word.charAt(0).toUpperCase() + word.slice(1))
		.join(' ');
}

/**
 * Sentence case with underscores turned into spaces: the web's long-standing
 * behaviour for values the label maps do not know.
 * @param {string} value
 * @returns {string}
 */
export function sentenceCase(value) {
	const words = value.replaceAll('_', ' ');
	return words.charAt(0).toUpperCase() + words.slice(1);
}

/** @type {Record<string, string>} */
export const COURIER_STATUS_LABELS = {
	created: 'Created',
	pending_pickup: 'Waiting for pickup',
	picked_up: 'Picked up',
	in_transit: 'In transit',
	at_hub: 'At the hub',
	out_for_delivery: 'Out for delivery',
	delivery_attempted: 'Delivery attempted',
	on_hold: 'On hold',
	return_in_transit: 'Return in transit',
	returned: 'Returned',
	cancelled: 'Cancelled',
	lost: 'Lost',
	unknown: 'Unknown'
};

/**
 * @param {string | undefined} status
 * @returns {string}
 */
export function courierStatusLabel(status) {
	if (!status) return 'Not tracked yet';
	return COURIER_STATUS_LABELS[status] ?? sentenceCase(status);
}

/**
 * @param {string | undefined} key
 * @returns {string}
 */
export function courierLabel(key) {
	if (!key) return 'No courier';
	if (key === 'sandbox') return 'Sandbox courier';
	return sentenceCase(key);
}

/**
 * @param {string | undefined} type
 * @returns {string}
 */
export function deliveryTypeLabel(type) {
	if (type === 'stopdesk') return 'Stop desk';
	if (type === 'pickup') return 'Counter sale';
	return 'Home delivery';
}
