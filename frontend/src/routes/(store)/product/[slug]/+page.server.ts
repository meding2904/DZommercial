import { error } from '@sveltejs/kit';
import { ServerApiError, serverGet } from '#lib/server/api';
import type { Product } from '#lib/types';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, getClientAddress }) => {
	try {
		const data = await serverGet<{ product: Product; related: Product[] }>(
			`/api/products/${encodeURIComponent(params.slug)}`,
			getClientAddress()
		);

		return {
			product: data.product,
			related: data.related ?? []
		};
	} catch (err) {
		if (err instanceof ServerApiError && err.status === 404) {
			error(404, 'This formula is not on the shelf.');
		}
		error(503, 'The shelf is offline. Try again in a moment.');
	}
};
