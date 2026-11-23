import { formatDate } from '#lib/format';

export { ORDER_STATUSES, STATUS_TRANSITIONS } from './domain/statuses.js';
export {
	courierLabel,
	courierStatusLabel,
	deliveryTypeLabel,
	titleCase
} from './domain/labels.js';

const dateTime = new Intl.DateTimeFormat('en-GB', {
	day: '2-digit',
	month: 'short',
	year: 'numeric',
	hour: '2-digit',
	minute: '2-digit'
});

export function formatDateTime(value: string | null | undefined): string {
	if (!value) return 'No date';
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? value : dateTime.format(date);
}

export function formatRange(from: string, to: string): string {
	return `${formatDate(from)} to ${formatDate(to)}`;
}

export function parseList(value: string): string[] {
	return value
		.split(',')
		.map((part) => part.trim())
		.filter(Boolean);
}

export function countLabel(count: number, singular: string, plural = `${singular}s`): string {
	return `${count} ${count === 1 ? singular : plural}`;
}

export const SLUG_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
