import { apiFetch, ApiError } from '#lib/api';
import type { AdminProduct, Category, Customer, Metrics, ThemePreset } from '#lib/types';
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
		return apiFetch<{ categories: Category[] }>('/categories', { signal });
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
