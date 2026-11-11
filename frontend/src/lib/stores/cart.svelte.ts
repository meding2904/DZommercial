import { browser } from '$app/env';
import type { Product } from '#lib/types';

const STORAGE_KEY = 'azahar.cart.v1';
const MAX_PER_LINE = 10;

export type CartLine = {
	product_id: string;
	slug: string;
	name: string;
	price_cents: number;
	image: string;
	size: string;
	quantity: number;
	stock: number;
};

class Cart {
	items = $state<CartLine[]>([]);
	ready = $state(false);

	constructor() {
		if (browser) {
			try {
				const raw = localStorage.getItem(STORAGE_KEY);
				if (raw) {
					const parsed = JSON.parse(raw) as CartLine[];
					if (Array.isArray(parsed)) this.items = parsed.filter((line) => line && line.quantity > 0);
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

	add(product: Product, quantity = 1): boolean {
		if (product.stock < 1) return false;
		const limit = Math.min(product.stock, MAX_PER_LINE);
		const existing = this.items.find((line) => line.product_id === product.id);

		if (existing) {
			existing.quantity = Math.min(existing.quantity + quantity, limit);
			existing.stock = product.stock;
			existing.price_cents = product.price_cents;
		} else {
			this.items.push({
				product_id: product.id,
				slug: product.slug,
				name: product.name,
				price_cents: product.price_cents,
				image: product.image,
				size: product.size,
				quantity: Math.min(quantity, limit),
				stock: product.stock
			});
		}

		this.persist();
		return true;
	}

	setQty(productId: string, quantity: number): void {
		const line = this.items.find((item) => item.product_id === productId);
		if (!line) return;
		const next = Math.max(1, Math.min(quantity, line.stock, MAX_PER_LINE));
		line.quantity = next;
		this.persist();
	}

	remove(productId: string): void {
		this.items = this.items.filter((line) => line.product_id !== productId);
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
