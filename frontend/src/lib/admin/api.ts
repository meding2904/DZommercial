import { apiFetch, apiUpload, ApiError } from '#lib/api';
import type {
	AdminPlugin,
	AdminProduct,
	Category,
	Customer,
	Metrics,
	PluginManifest,
	PluginSettings,
	Role,
	TeamMember,
	ThemePreset
} from '#lib/types';
import type { StorefrontTheme } from '#lib/theme/storefront';
import type { AdminOrder, ProductPayload } from './types';

type Signal = AbortSignal | undefined;

export const adminApi = {
	metrics(days: number, signal?: Signal) {
		return apiFetch<Metrics>(`/admin/metrics?days=${days}`, { signal });
	},

	orders(status: string, query: string, signal?: Signal) {
		const params = new URLSearchParams();
		if (status) params.set('status', status);
		if (query) params.set('q', query);
		const suffix = params.toString();
		return apiFetch<{ orders: AdminOrder[] }>(`/admin/orders${suffix ? `?${suffix}` : ''}`, {
			signal
		});
	},

	updateOrderStatus(id: string, status: string) {
		return apiFetch<{ order: AdminOrder }>(`/admin/orders/${encodeURIComponent(id)}/status`, {
			method: 'PATCH',
			body: { status }
		});
	},

	products(signal?: Signal) {
		return apiFetch<{ products: AdminProduct[] }>('/admin/products', { signal });
	},

	categories(signal?: Signal) {
		return apiFetch<{ categories: Category[] }>('/categories?all=1', { signal });
	},

	createProduct(payload: ProductPayload) {
		return apiFetch<{ product: AdminProduct }>('/admin/products', {
			method: 'POST',
			body: payload
		});
	},

	updateProduct(id: string, payload: ProductPayload) {
		return apiFetch<{ product: AdminProduct }>(`/admin/products/${encodeURIComponent(id)}`, {
			method: 'PATCH',
			body: payload
		});
	},

	archiveProduct(id: string) {
		return apiFetch<void>(`/admin/products/${encodeURIComponent(id)}`, { method: 'DELETE' });
	},

	uploadProductImage(file: File) {
		const form = new FormData();
		form.set('image', file, file.name);
		return apiUpload<{ image: { id: string; url: string; mime: string; bytes: number } }>(
			'/admin/product-images',
			form
		);
	},

	customers(signal?: Signal) {
		return apiFetch<{ customers: Customer[] }>('/admin/customers', { signal });
	},

	createShipment(id: string) {
		return apiFetch<{ order: AdminOrder }>(`/admin/orders/${encodeURIComponent(id)}/shipment`, {
			method: 'POST'
		});
	},

	refreshTracking(id: string) {
		return apiFetch<{ order: AdminOrder }>(`/admin/orders/${encodeURIComponent(id)}/tracking`, {
			method: 'POST'
		});
	},

	preferences() {
		return apiFetch<{ theme_preset: ThemePreset }>('/admin/preferences');
	},

	savePreferences(themePreset: ThemePreset) {
		return apiFetch<{ theme_preset: ThemePreset }>('/admin/preferences', {
			method: 'PUT',
			body: { theme_preset: themePreset }
		});
	},

	pluginManifests(signal?: Signal) {
		return apiFetch<{ plugins: PluginManifest[] }>('/plugins', { signal });
	},

	adminPlugins(signal?: Signal) {
		return apiFetch<{ plugins: AdminPlugin[] }>('/admin/plugins', { signal });
	},

	team(signal?: Signal) {
		return apiFetch<{ users: TeamMember[] }>('/admin/team', { signal });
	},

	updateTeamRole(id: string, role: Role) {
		return apiFetch<{ user: TeamMember }>(`/admin/team/${encodeURIComponent(id)}`, {
			method: 'PATCH',
			body: { role }
		});
	},

	setPluginState(id: string, enabled: boolean, confirmDependents = false) {
		return apiFetch<{ plugin: AdminPlugin }>(
			`/admin/plugins/${encodeURIComponent(id)}/state`,
			{
				method: 'PUT',
				body: { enabled, confirm_dependents: confirmDependents }
			}
		);
	},

	pluginSettings(id: string) {
		return apiFetch<PluginSettings>(`/admin/plugins/${encodeURIComponent(id)}/settings`);
	},

	savePluginSettings(id: string, settings: Record<string, string>) {
		return apiFetch<PluginSettings>(`/admin/plugins/${encodeURIComponent(id)}/settings`, {
			method: 'PUT',
			body: { settings }
		});
	},

	storefrontTheme() {
		return apiFetch<{ theme: StorefrontTheme; defaults: StorefrontTheme }>(
			'/admin/storefront/theme'
		);
	},

	saveStorefrontTheme(theme: StorefrontTheme) {
		return apiFetch<{ theme: StorefrontTheme }>('/admin/storefront/theme', {
			method: 'PUT',
			body: theme
		});
	}
};

export function isAbortError(error: unknown): boolean {
	return error instanceof Error && error.name === 'AbortError';
}

export function errorMessage(error: unknown): string {
	if (error instanceof ApiError) return error.message;
	if (error instanceof Error) {
		if (error.name === 'AbortError') return '';
		if (error.name === 'TypeError') {
			return 'The shop API is not answering. Check that it is running, then try again.';
		}
		return error.message || 'Something went wrong. Try again.';
	}
	return 'Something went wrong. Try again.';
}
