import { ServerApiError, serverGet } from './api';
import type { Order } from '#lib/types';

export async function loadOrderByToken(token: string, clientAddress?: string): Promise<Order> {
	const data = await serverGet<{ order: Order }>(
		`/api/orders/${encodeURIComponent(token)}`,
		clientAddress
	);
	const { token: _token, ...orderData } = data.order;
	return {
		...orderData,
		items: data.order.items.map((item) => {
			const line = item as Order['items'][number] & { unit_price_cents?: number };
			return {
				...item,
				price_cents: item.price_cents ?? line.unit_price_cents ?? 0
			};
		})
	};
}

export function isOrderNotFound(error: unknown): boolean {
	return error instanceof ServerApiError && error.status === 404;
}
