// Pure formatting for the renderer: HTML escaping, money in dinars, dates and
// the numbers the ledger prints. No DOM and no bridge, so node --test imports
// this module directly.

const HTML_ESCAPES = {
	'&': '&amp;',
	'<': '&lt;',
	'>': '&gt;',
	'"': '&quot;',
	"'": '&#39;'
};

/**
 * @param {unknown} value
 * @returns {string}
 */
export function escapeHtml(value) {
	return String(value ?? '').replace(/[&<>"']/g, (character) => HTML_ESCAPES[character]);
}

/**
 * @param {number | string | null | undefined} cents
 * @returns {string}
 */
export function money(cents) {
	const amount = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(
		Math.round(Number(cents || 0) / 100)
	);
	return `${amount}\u00a0DZD`;
}

/**
 * @param {number | string | null | undefined} cents
 * @returns {string}
 */
export function compactMoney(cents) {
	if (Math.abs(Number(cents)) < 100000) return money(cents);
	return `${new Intl.NumberFormat('en-US', {
		notation: 'compact',
		maximumFractionDigits: 1
	}).format(Number(cents) / 100)}\u00a0DZD`;
}

/**
 * @param {number | string | null | undefined} cents
 * @returns {string}
 */
export function moneyValue(cents) {
	return (Number(cents || 0) / 100).toFixed(2);
}

/**
 * @param {number | string | null | undefined} value
 * @returns {number}
 */
export function fromDinars(value) {
	return Math.round(Number(value || 0) * 100);
}

/**
 * @param {string | null | undefined} value
 * @returns {string}
 */
export function formatDate(value) {
	if (!value) return 'No date';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return String(value);
	return new Intl.DateTimeFormat('en-GB', {
		day: '2-digit',
		month: 'short',
		year: 'numeric'
	}).format(date);
}

/**
 * @param {string | null | undefined} value
 * @returns {string}
 */
export function formatDateTime(value) {
	if (!value) return 'Not set';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return String(value);
	return new Intl.DateTimeFormat('en-GB', {
		day: '2-digit',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit'
	}).format(date);
}

/**
 * @param {string} value A calendar day such as 2026-01-05.
 * @returns {string}
 */
export function formatDayShort(value) {
	const date = new Date(`${value}T12:00:00Z`);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat('en-GB', { day: '2-digit', month: 'short' }).format(date);
}

/**
 * @param {number | string | null | undefined} value
 * @returns {string}
 */
export function percent(value) {
	if (value === null || value === undefined || Number.isNaN(Number(value))) {
		return 'No previous period';
	}
	const number = Number(value);
	return `${number > 0 ? '+' : ''}${number.toFixed(1)}%`;
}

/**
 * @param {string | null | undefined} value
 * @returns {string}
 */
export function toLocalInput(value) {
	if (!value) return '';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	const pad = (number) => String(number).padStart(2, '0');
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/**
 * @param {string | null | undefined} value
 * @returns {string}
 */
export function toRFC3339(value) {
	if (!value) return '';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	return date.toISOString();
}
