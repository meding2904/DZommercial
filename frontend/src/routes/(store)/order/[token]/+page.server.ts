import { error } from '@sveltejs/kit';
import { optionalClientAddress } from '#lib/server/address';
import { isOrderNotFound, loadOrderByToken } from '#lib/server/order';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, getClientAddress, setHeaders }) => {
	setHeaders({
		'cache-control': 'no-store',
		'referrer-policy': 'no-referrer',
		'x-robots-tag': 'noindex'
	});

	try {
		const order = await loadOrderByToken(
			params.token,
			optionalClientAddress(getClientAddress)
		);
		return {
			order,
			receiptPath: `/receipt/${encodeURIComponent(params.token)}`
		};
	} catch (err) {
		if (isOrderNotFound(err)) {
			error(404, 'That order could not be found.');
		}
		error(503, 'The order could not be loaded. Try again in a moment.');
	}
};
