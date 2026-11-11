const number = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 });
const compact = new Intl.NumberFormat('en-US', {
	notation: 'compact',
	maximumFractionDigits: 1
});
const dateShort = new Intl.DateTimeFormat('en-GB', {
	day: '2-digit',
	month: 'short',
	year: 'numeric'
});
const dateOnly = new Intl.DateTimeFormat('en-GB', {
	day: '2-digit',
	month: 'short'
});

export function money(cents: number, currency = 'DZD'): string {
	return `${number.format(Math.round(cents / 100))} ${currency}`;
}

export function compactMoney(cents: number, currency = 'DZD'): string {
	if (cents < 100000) return money(cents, currency);
	return `${compact.format(cents / 100)} ${currency}`;
}

export function formatDate(value: string | null | undefined): string {
	if (!value) return 'No orders yet';
	const d = new Date(value);
	return Number.isNaN(d.getTime()) ? value : dateShort.format(d);
}

export function formatDay(value: string): string {
	const d = new Date(`${value}T12:00:00Z`);
	return Number.isNaN(d.getTime()) ? value : dateOnly.format(d);
}

export function percent(value: number | null): string {
	if (value === null || Number.isNaN(value)) return 'No previous period';
	const sign = value > 0 ? '+' : '';
	return `${sign}${value.toFixed(1)}%`;
}

export function titleCase(value: string): string {
	return value.charAt(0).toUpperCase() + value.slice(1);
}
