import { browser } from '$app/env';
import { apiFetch } from '#lib/api';
import type { OptionField, Product } from '#lib/types';

const STORAGE_KEY = 'azahar.cart.v1';
const MAX_PER_LINE = 10;

export type CartLine = {
	key: string;
	product_id: string;
	slug: string;
	name: string;
	price_cents: number;
	image: string;
	size: string;
	quantity: number;
	stock: number;
	options: Record<string, string>;
	options_label: string;
};

export function canonicalOptions(options: Record<string, string>): string {
	return Object.keys(options)
		.sort()
		.map((key) => `${key}=${encodeURIComponent(options[key])}`)
		.join('&');
}

export function cartLineKey(productId: string, options: Record<string, string>): string {
	const suffix = canonicalOptions(options);
	return suffix ? `${productId}::${suffix}` : productId;
}

export function optionLabel(
	schema: OptionField[],
	options: Record<string, string>
): string {
	return schema
		.map((field) => {
			const value = options[field.key];
			return value ? `${field.label}: ${value}` : '';
		})
		.filter(Boolean)
		.join(' · ');
}

function normalizeStoredLine(line: Partial<CartLine>): CartLine | null {
	if (!line || typeof line.product_id !== 'string' || !line.quantity || line.quantity < 1) {
		return null;
	}
	const options =
		line.options && typeof line.options === 'object' ? (line.options as Record<string, string>) : {};
	return {
		key: typeof line.key === 'string' && line.key ? line.key : cartLineKey(line.product_id, options),
		product_id: line.product_id,
		slug: String(line.slug ?? ''),
		name: String(line.name ?? ''),
		price_cents: Number(line.price_cents ?? 0),
		image: String(line.image ?? ''),
		size: String(line.size ?? ''),
		quantity: Math.min(Number(line.quantity), MAX_PER_LINE),
		stock: Number(line.stock ?? 0),
		options,
		options_label: String(line.options_label ?? '')
	};
}

class Cart {
	items = $state<CartLine[]>([]);
	ready = $state(false);

	constructor() {
		if (browser) {
			try {
				const raw = localStorage.getItem(STORAGE_KEY);
				if (raw) {
					const parsed = JSON.parse(raw) as Partial<CartLine>[];
					if (Array.isArray(parsed)) {
						this.items = parsed
							.map((line) => normalizeStoredLine(line))
							.filter((line): line is CartLine => line !== null);
					}
				}
			} catch {
				this.items = [];
			}
			this.ready = true;
		}
	}

	get count(): number {
		return this.items.reduce((sum, line) => sum + line.quantity, 0);
	}

	get subtotal(): number {
		return this.items.reduce((sum, line) => sum + line.price_cents * line.quantity, 0);
	}

	// Checkout verifies that the line price still matches the server. When a
	// sale starts or ends mid-session, refresh the displayed prices so the
	// shopper can retry instead of being stuck with a stale cart.
	async refreshPrices(): Promise<boolean> {
		if (!browser || this.items.length === 0) return false;
		const updates = await Promise.all(
			this.items.map(async (line) => {
				try {
					const data = await apiFetch<{ product: Product }>(
						`/products/${encodeURIComponent(line.slug)}`
					);
					return { key: line.key, product: data.product };
				} catch {
					return null;
				}
			})
		);
		let changed = false;
		for (const update of updates) {
			if (!update) continue;
			const line = this.items.find((item) => item.key === update.key);
			if (!line) continue;
			const price = update.product.effective_price_cents ?? update.product.price_cents;
			if (line.price_cents !== price) {
				line.price_cents = price;
				changed = true;
			}
		}
		if (changed) this.persist();
		return changed;
	}

	add(product: Product, quantity = 1, options: Record<string, string> = {}): boolean {
		const registered =
			typeof product.plugin_data?.devices_in_stock === 'number'
				? product.plugin_data.devices_in_stock
				: product.stock;
		let available = Math.min(product.stock, registered);
		const variants = product.plugin_data?.variants;
		if (Array.isArray(variants) && Object.keys(options).length > 0) {
			const match = variants.find(
				(variant) =>
					typeof variant === 'object' &&
					variant !== null &&
					Object.entries(options).every(
						([key, value]) => (variant as Record<string, unknown>)[key] === value
					)
			) as { in_stock?: number } | undefined;
			available =
				typeof match?.in_stock === 'number'
					? Math.min(match.in_stock, product.stock)
					: 0;
		}
		if (available < 1) return false;
		const chosen = { ...options };
		for (const field of product.option_schema) {
			if (field.type === 'select' && field.required && !chosen[field.key]) return false;
		}
		const key = cartLineKey(product.id, chosen);
		const limit = Math.min(available, MAX_PER_LINE);
		const price = product.effective_price_cents ?? product.price_cents;
		const existing = this.items.find((line) => line.key === key);

		if (existing) {
			existing.quantity = Math.min(existing.quantity + quantity, limit);
			existing.stock = available;
			existing.price_cents = price;
		} else {
			this.items.push({
				key,
				product_id: product.id,
				slug: product.slug,
				name: product.name,
				price_cents: price,
				image: product.image,
				size: product.attributes?.size?.[0] ?? product.size,
				quantity: Math.min(quantity, limit),
				stock: available,
				options: chosen,
				options_label: optionLabel(product.option_schema, chosen)
			});
		}

		this.persist();
		return true;
	}

	setQty(key: string, quantity: number): void {
		const line = this.items.find((item) => item.key === key);
		if (!line) return;
		const next = Math.max(1, Math.min(quantity, line.stock, MAX_PER_LINE));
		line.quantity = next;
		this.persist();
	}

	remove(key: string): void {
		this.items = this.items.filter((line) => line.key !== key);
		this.persist();
	}

	clear(): void {
		this.items = [];
		this.persist();
	}

	private persist(): void {
		if (!browser) return;
		localStorage.setItem(STORAGE_KEY, JSON.stringify(this.items));
	}
}

export const cart = new Cart();
