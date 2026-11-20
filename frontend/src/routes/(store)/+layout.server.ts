import { serverGet } from '#lib/server/api';
import { DEFAULT_THEME, type StorefrontTheme } from '#lib/theme/storefront';
import type { PluginManifest, StorefrontPage } from '#lib/types';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ getClientAddress }) => {
	const client = getClientAddress();
	const [themeData, pluginData] = await Promise.all([
		serverGet<{ theme: StorefrontTheme }>('/api/storefront/theme', client).catch(() => null),
		serverGet<{ plugins: PluginManifest[] }>('/api/plugins', client).catch(() => null)
	]);

	const pages: StorefrontPage[] = (pluginData?.plugins ?? [])
		.filter((plugin) => plugin.enabled !== false)
		.flatMap((plugin) => plugin.storefront_pages ?? [])
		.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.label.localeCompare(b.label));

	return { theme: themeData?.theme ?? DEFAULT_THEME, pages };
};
