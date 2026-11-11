export type Role = 'customer' | 'admin';

export type User = {
	id: string;
	name: string;
	email: string;
	role: Role;
};

export type Category = {
	slug: string;
	name: string;
	description: string;
};

export type Product = {
	id: string;
	slug: string;
	name: string;
	subtitle: string;
	description: string;
	price_cents: number;
	compare_at_cents: number | null;
	currency: string;
	size: string;
	category: { slug: string; name: string };
	concerns: string[];
	ingredients: string[];
	how_to: string;
	image: string;
	gallery: string[];
	stock: number;
	featured: boolean;
	active: boolean;
};

export type AdminProduct = Product & {
	created_at: string;
	updated_at: string;
};

export type OrderStatus =
	| 'pending'
	| 'confirmed'
	| 'packed'
	| 'shipped'
	| 'delivered'
	| 'cancelled';

export type OrderItem = {
	product_id: string;
	slug: string;
	name: string;
	price_cents: number;
	quantity: number;
	image: string;
};

export type CourierEvent = {
	status: string;
	rawStatus: string;
	timestamp: string;
};

export type Wilaya = {
	code: number;
	nameFr: string;
	nameAr: string;
	isDeepSouth: boolean;
	communeCount: number;
	courierSupported: boolean;
};

export type Commune = {
	wilayaCode: number;
	nameFr: string;
	nameAr: string;
};

export type StopDesk = {
	id: string;
	name: string;
	wilayaCode: number;
	communeName?: string;
	address?: string;
};

export type Order = {
	id: string;
	number: string;
	status: OrderStatus;
	email: string;
	name: string;
	phone: string;
	address: string;
	city: string;
	notes: string;
	wilaya_code?: number;
	wilaya_name?: string;
	commune_name?: string;
	delivery_type?: 'home' | 'stopdesk';
	stop_desk_id?: string;
	stop_desk_name?: string;
	courier_key?: string;
	tracking_number?: string;
	courier_status?: string;
	courier_events?: CourierEvent[];
	courier_synced_at?: string;
	subtotal_cents: number;
	shipping_cents: number;
	total_cents: number;
	items: OrderItem[];
	created_at: string;
	token?: string;
};

export type OrderSummary = Omit<Order, 'token' | 'address' | 'phone' | 'notes'>;

export type AdminOrder = Order & {
	user_id: string | null;
};

export type Customer = {
	id: string;
	name: string;
	email: string;
	orders_count: number;
	total_spent_cents: number;
	last_order_at: string | null;
	created_at: string;
};

export type MetricPoint = {
	date: string;
	revenue_cents: number;
	orders: number;
};

export type MetricStatus = {
	status: OrderStatus;
	count: number;
};

export type MetricProduct = {
	product_id: string;
	name: string;
	units: number;
	revenue_cents: number;
};

export type LowStock = {
	id: string;
	name: string;
	stock: number;
	image: string;
};

export type Metrics = {
	demo: boolean;
	range: { days: number; from: string; to: string };
	revenue_cents: number;
	previous_revenue_cents: number;
	delta_pct: number | null;
	orders_count: number;
	previous_orders_count: number;
	aov_cents: number;
	units_sold: number;
	new_customers: number;
	low_stock: LowStock[];
	revenue_series: MetricPoint[];
	status_split: MetricStatus[];
	top_products: MetricProduct[];
	recent_orders: {
		id: string;
		number: string;
		name: string;
		total_cents: number;
		status: OrderStatus;
		created_at: string;
	}[];
};

export type ThemePreset = 'plaster' | 'cellar' | 'terracotta' | 'lagoon';

export type ApiErrorBody = {
	error?: {
		code?: string;
		message?: string;
		fields?: Record<string, string>;
	};
};
