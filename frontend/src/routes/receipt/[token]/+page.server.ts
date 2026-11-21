import { error } from '@sveltejs/kit';
import { serverGet } from '#lib/server/api';
import { optionalClientAddress } from '#lib/server/address';
import { isOrderNotFound, loadOrderByToken } from '#lib/server/order';
import { DEFAULT_THEME, type StorefrontTheme } from '#lib/theme/storefront';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, getClientAddress, setHeaders }) => {
	setHeaders({
		'cache-control': 'no-store',
		'referrer-policy': 'no-referrer',
		'x-robots-tag': 'noindex'
	});

	const client = optionalClientAddress(getClientAddress);
	try {
		const [order, themeData] = await Promise.all([
			loadOrderByToken(params.token, client),
			serverGet<{ theme: StorefrontTheme }>('/api/storefront/theme', client).catch(() => null)
		]);
		return {
			order,
			theme: themeData?.theme ?? DEFAULT_THEME,
			orderPath: `/order/${encodeURIComponent(params.token)}`
		};
	} catch (err) {
		if (isOrderNotFound(err)) {
			error(404, 'That receipt could not be found.');
		}
		error(503, 'The receipt could not be loaded. Try again in a moment.');
	}
};
