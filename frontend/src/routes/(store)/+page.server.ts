import { serverGet } from '#lib/server/api';
import type { Category, Product } from '#lib/types';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ getClientAddress }) => {
	const client = getClientAddress();
	try {
		const [productData, categoryData] = await Promise.all([
			serverGet<{ products: Product[] }>('/api/products?sort=featured', client),
			serverGet<{ categories: Category[] }>('/api/categories', client)
		]);

		return {
			products: productData.products,
			categories: categoryData.categories,
			apiDown: false
		};
	} catch {
		return {
			products: [] as Product[],
			categories: [] as Category[],
			apiDown: true
		};
	}
};
