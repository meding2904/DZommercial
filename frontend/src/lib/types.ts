export type Role = 'customer' | 'admin' | 'employee';

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
	plugin_id: string;
};

export type OptionField = {
	key: string;
	label: string;
	type: 'select' | 'text' | 'textarea' | 'list';
	required: boolean;
	help?: string;
	suggestions?: string[];
};

export type VariantStock = Record<string, string | number> & { in_stock: number };

export type Product = {
	id: string;
	slug: string;
	name: string;
	subtitle: string;
	description: string;
	price_cents: number;
	compare_at_cents: number | null;
	sale_price_cents: number | null;
	sale_starts_at: string;
	sale_ends_at: string;
	effective_price_cents: number;
	on_sale: boolean;
	currency: string;
	size: string;
	category: { slug: string; name: string; plugin_id?: string };
	concerns: string[];
	ingredients: string[];
	how_to: string;
	image: string;
	gallery: string[];
	stock: number;
	featured: boolean;
	active: boolean;
	plugin_id: string;
	plugin_name: string;
	plugin_summary: string;
	option_schema: OptionField[];
	attributes: Record<string, string[]>;
	plugin_data?: Record<string, unknown>;
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
	options?: Record<string, string>;
	options_label?: string;
	plugin_id?: string;
	id: string;
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
	delivery_type?: 'home' | 'stopdesk' | 'pickup';
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

export type ProductTypeManifest = {
	label: string;
	summary?: string;
	fields: OptionField[];
};

export type PluginManifest = {
	id: string;
	name: string;
	version: string;
	description: string;
	author?: string;
	dependencies?: string[];
	product_type?: ProductTypeManifest;
	categories?: PluginCategory[];
	storefront_pages?: StorefrontPage[];
	admin_path?: string;
	enabled?: boolean;
};

export type StorefrontPage = {
	path: string;
	label: string;
	sort_order?: number;
};

export type PluginCategory = {
	slug: string;
	name: string;
	description?: string;
	sort_order?: number;
};

export type PluginSettingOption = {
	value: string;
	label: string;
};

export type PluginSettingField = {
	key: string;
	label: string;
	type: 'text' | 'secret' | 'number' | 'boolean' | 'select';
	help?: string;
	placeholder?: string;
	options?: PluginSettingOption[];
	required: boolean;
	max_length?: number;
	default?: string;
};

export type TeamMember = {
	id: string;
	name: string;
	email: string;
	role: Role;
	created_at: string;
};

export type AdminPlugin = PluginManifest & {	settings?: PluginSettingField[];
	configured: boolean;
	settings_state: 'ready' | 'unreadable';
	updated_at?: string;
	enabled: boolean;
	effective_enabled: boolean;
	dependents?: string[];
	blocked_by?: string[];
};

export type PluginSettings = {
	plugin_id: string;
	settings: Record<string, string>;
	secrets_set: string[];
	configured: boolean;
	updated_at?: string;
};

export type ApiErrorBody = {
	error?: {
		code?: string;
		message?: string;
		fields?: Record<string, string>;
	};
};
