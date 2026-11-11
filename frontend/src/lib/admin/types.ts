import type { OrderStatus } from '#lib/types';

export type AdminOrderItem = {
	id: string;
	product_id: string;
	slug: string;
	name: string;
	image: string;
	unit_price_cents: number;
	quantity: number;
	line_total_cents: number;
};

export type AdminOrder = {
	id: string;
	number: string;
	status: OrderStatus;
	name: string;
	email: string;
	phone: string;
	address: string;
	city: string;
	notes: string;
	subtotal_cents: number;
	shipping_cents: number;
	total_cents: number;
	currency: string;
	items: AdminOrderItem[];
	customer: {
		id?: string;
		name: string;
		email: string;
		phone: string;
	};
	created_at: string;
	updated_at: string;
};

export type ProductPayload = {
	name: string;
	slug: string;
	subtitle: string;
	description: string;
	price_cents: number;
	size: string;
	category_slug: string;
	concerns: string[];
	ingredients: string[];
	how_to: string;
	image: string;
	gallery: string[];
	stock: number;
	featured: boolean;
	active: boolean;
};

export type ProductPayloadField = keyof ProductPayload;
