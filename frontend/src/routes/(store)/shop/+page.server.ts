import { serverGet } from '#lib/server/api';
import type { StorefrontTheme } from '#lib/theme/storefront';
import type { Category, Product } from '#lib/types';
import type { PageServerLoad } from './$types';

const SORTS = ['featured', 'price_asc', 'price_desc', 'new'] as const;
const CONCERNS = ['dry', 'sensitive', 'dull', 'oily', 'tired'];

type Sort = (typeof SORTS)[number];

function readSort(value: string | null): Sort {
	return SORTS.includes(value as Sort) ? (value as Sort) : 'featured';
}

export const load: PageServerLoad = async ({ url, getClientAddress }) => {
	const client = getClientAddress();
	const sort = readSort(url.searchParams.get('sort'));
	const rawConcern = url.searchParams.get('concern')?.trim() ?? '';
	const q = url.searchParams.get('q')?.trim() ?? '';
	const rawCategory = url.searchParams.get('category')?.trim() ?? '';

	try {
		const [categoryData, themeData] = await Promise.all([
			serverGet<{ categories: Category[] }>('/api/categories', client),
			serverGet<{ theme: StorefrontTheme }>('/api/storefront/theme', client).catch(() => null)
		]);
		const categories = categoryData.categories;
		const category = categories.some((item) => item.slug === rawCategory) ? rawCategory : '';
		const concernEnabled = themeData?.theme.concerns_enabled !== false;
		const concern =
			concernEnabled && CONCERNS.includes(rawConcern) ? rawConcern : '';

		const params = new URLSearchParams();
		if (category) params.set('category', category);
		if (concern) params.set('concern', concern);
		if (q) params.set('q', q);
		params.set('sort', sort);

		const productData = await serverGet<{ products: Product[] }>(`/api/products?${params}`, client);

		return {
			products: productData.products,
			categories,
			filters: { category, concern, q, sort },
			apiDown: false
		};
	} catch {
		return {
			products: [] as Product[],
			categories: [] as Category[],
			filters: { category: '', concern: '', q, sort },
			apiDown: true
		};
	}
};
