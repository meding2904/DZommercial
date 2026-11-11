import { serverGet } from '#lib/server/api';
import type { Category, Product } from '#lib/types';
import type { PageServerLoad } from './$types';

const SORTS = ['featured', 'price_asc', 'price_desc', 'new'] as const;
const CONCERNS = ['dry', 'sensitive', 'dull', 'oily', 'tired'];

const FEATURED_ORDER = [
	'orange-blossom-cleansing-oil',
	'rosehip-and-olive-face-serum',
	'rose-clay-mask',
	'fig-and-sesame-body-balm',
	'prickly-pear-face-oil',
	'cedar-and-myrrh-hand-cream',
	'green-fig-body-oil',
	'rose-petal-bath-soak'
];

type Sort = (typeof SORTS)[number];

function readSort(value: string | null): Sort {
	return SORTS.includes(value as Sort) ? (value as Sort) : 'featured';
}

export const load: PageServerLoad = async ({ url }) => {
	const sort = readSort(url.searchParams.get('sort'));
	const rawConcern = url.searchParams.get('concern')?.trim() ?? '';
	const concern = CONCERNS.includes(rawConcern) ? rawConcern : '';
	const q = url.searchParams.get('q')?.trim() ?? '';
	const rawCategory = url.searchParams.get('category')?.trim() ?? '';

	try {
		const categoryData = await serverGet<{ categories: Category[] }>('/api/categories');
		const categories = categoryData.categories;
		const category = categories.some((item) => item.slug === rawCategory) ? rawCategory : '';

		const params = new URLSearchParams();
		if (category) params.set('category', category);
		if (concern) params.set('concern', concern);
		if (q) params.set('q', q);
		params.set('sort', sort);

		const productData = await serverGet<{ products: Product[] }>(`/api/products?${params}`);
		const products =
			sort === 'featured'
				? [...productData.products].sort((a, b) => {
						const ai = FEATURED_ORDER.indexOf(a.slug);
						const bi = FEATURED_ORDER.indexOf(b.slug);
						return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
					})
				: productData.products;

		return {
			products,
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
