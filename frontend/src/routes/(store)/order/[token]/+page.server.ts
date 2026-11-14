import { error } from '@sveltejs/kit';
import { ServerApiError, serverGet } from '#lib/server/api';
import type { Order } from '#lib/types';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, getClientAddress, setHeaders }) => {
	setHeaders({
		'cache-control': 'no-store',
		'referrer-policy': 'no-referrer',
		'x-robots-tag': 'noindex'
	});

	try {
		const data = await serverGet<{ order: Order }>(
			`/api/orders/${encodeURIComponent(params.token)}`,
			getClientAddress()
		);

		const { token, ...orderData } = data.order;
		const order: Order = {
			...orderData,
			items: data.order.items.map((item) => {
				const line = item as Order['items'][number] & { unit_price_cents?: number };
				return {
					...item,
					price_cents: item.price_cents ?? line.unit_price_cents ?? 0
				};
			})
		};

		return { order };
	} catch (err) {
		if (err instanceof ServerApiError && err.status === 404) {
			error(404, 'That order could not be found.');
		}
		error(503, 'The order could not be loaded. Try again in a moment.');
	}
};
