import { serverGet } from '#lib/server/api';
import type { Category, Product } from '#lib/types';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async () => {
	try {
		const [productData, categoryData] = await Promise.all([
			serverGet<{ products: Product[] }>('/api/products?sort=featured'),
			serverGet<{ categories: Category[] }>('/api/categories')
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
