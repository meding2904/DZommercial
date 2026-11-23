// Counter sale ticket lines. Pure list operations: the counter screen owns the
// state object, this module decides when a line merges, caps or disappears.

export const TICKET_LINE_CAP = 10;

/**
 * Identifies a ticket line by product and chosen options.
 * @param {string} productId
 * @param {Record<string, string>} options
 * @returns {string}
 */
export function ticketKey(productId, options) {
	const encoded = Object.keys(options)
		.sort()
		.map((name) => `${name}=${encodeURIComponent(options[name])}`)
		.join('&');
	return `${productId}|${encoded}`;
}

/**
 * A line never takes more units than the shelf holds, and never more than ten.
 * @param {{ stock?: number | string }} product
 * @returns {number}
 */
export function ticketLimit(product) {
	return Math.min(Number(product.stock) || 1, TICKET_LINE_CAP);
}

/**
 * @param {{ effective_price_cents?: number, price_cents?: number }} product
 * @returns {number}
 */
export function ticketUnitPrice(product) {
	return Number(product.effective_price_cents ?? product.price_cents ?? 0);
}

/**
 * Adds one unit of a product to the ticket, merging into the line that already
 * holds the same product and options.
 * @template {{ key: string, product: object, quantity: number }} Line
 * @param {Line[]} lines
 * @param {object & { id: string }} product
 * @param {Record<string, string>} options
 * @returns {{ lines: Line[], blocked: boolean }}
 */
export function mergeTicketLine(lines, product, options) {
	const key = ticketKey(product.id, options);
	const existing = lines.find((line) => line.key === key);
	if (!existing) {
		return {
			lines: [
				...lines,
				{ key, product, options, quantity: 1, unit: ticketUnitPrice(product) }
			],
			blocked: false
		};
	}
	if (existing.quantity >= ticketLimit(product)) {
		return { lines, blocked: true };
	}
	return {
		lines: lines.map((line) =>
			line === existing ? { ...line, quantity: line.quantity + 1 } : line
		),
		blocked: false
	};
}

/**
 * Raises or lowers one line. Below one unit the line leaves the ticket; above
 * the stock limit the quantity stays at the limit.
 * @template {{ product: object, quantity: number }} Line
 * @param {Line[]} lines
 * @param {number} index
 * @param {number} delta
 * @returns {{ lines: Line[], changed: boolean }}
 */
export function changeTicketQuantity(lines, index, delta) {
	const line = lines[index];
	if (!line) return { lines, changed: false };
	const next = line.quantity + delta;
	if (next < 1) {
		return { lines: lines.filter((_, position) => position !== index), changed: true };
	}
	return {
		lines: lines.map((value, position) =>
			position === index
				? { ...value, quantity: Math.min(next, ticketLimit(line.product)) }
				: value
		),
		changed: true
	};
}

/**
 * @param {{ unit: number, quantity: number }[]} lines
 * @returns {number}
 */
export function ticketTotal(lines) {
	return lines.reduce((sum, line) => sum + line.unit * line.quantity, 0);
}

/**
 * @param {{ quantity: number }[]} lines
 * @returns {number}
 */
export function ticketCount(lines) {
	return lines.reduce((sum, line) => sum + line.quantity, 0);
}

/**
 * The shelf after a sale: quantities are summed per product, so a product that
 * appears on two ticket lines (different options) decrements by both lines.
 * @param {{ id: string, stock?: number | string }[]} products
 * @param {{ product_id: string, quantity?: number | string }[]} items
 * @returns {{ id: string, stock?: number | string }[]}
 */
export function stockAfterSale(products, items) {
	const sold = new Map();
	for (const item of items || []) {
		sold.set(item.product_id, (sold.get(item.product_id) || 0) + (Number(item.quantity) || 0));
	}
	return products.map((product) => {
		if (!sold.has(product.id)) return product;
		return { ...product, stock: Math.max(0, Number(product.stock) - sold.get(product.id)) };
	});
}
