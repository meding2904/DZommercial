import { error } from '@sveltejs/kit';
import { serverGet } from '#lib/server/api';
import type { PluginManifest } from '#lib/types';
import type { PageServerLoad } from './$types';

type PartProduct = {
	id: string;
	slug: string;
	name: string;
	subtitle: string;
	description: string;
	price_cents: number;
	sale_price_cents: number | null;
	sale_starts_at: string;
	sale_ends_at: string;
	effective_price_cents: number;
	on_sale: boolean;
	currency: string;
	image: string;
	stock: number;
	plugin_id: string;
	attributes: Record<string, string[]>;
	category: { slug: string; name: string; plugin_id: string };
};

type PartCategory = {
	slug: string;
	name: string;
	products: PartProduct[];
};

export const load: PageServerLoad = async ({ getClientAddress }) => {
	const client = getClientAddress();
	const [pluginData, parts] = await Promise.all([
		serverGet<{ plugins: PluginManifest[] }>('/api/plugins', client).catch(() => null),
		serverGet<{ categories: PartCategory[] }>(
			'/api/plugins/pcbuilder/parts',
			client
		).catch(() => null)
	]);

	const builder = pluginData?.plugins.find((plugin) => plugin.id === 'pcbuilder');
	if (builder && builder.enabled === false) {
		error(404, 'Not found');
	}

	if (!parts) {
		return {
			categories: [] as PartCategory[],
			apiDown: true
		};
	}
	return {
		categories: parts.categories ?? [],
		apiDown: false
	};
};
