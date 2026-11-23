// The renderer's state. One object holds the session, the theme and the slices
// each screen reads; screens write their own slice and the pure views render it.

import { DEFAULT_PRESET, PRESET_KEY, SCHEME_KEY } from './presets.js';

export const state = {
	info: null,
	health: null,
	brand: 'Azahar',
	user: null,
	preset: localStorage.getItem(PRESET_KEY) || DEFAULT_PRESET,
	scheme: localStorage.getItem(SCHEME_KEY) || 'dark',
	route: 'ledger',
	render: 0,
	metrics: null,
	metricsDays: 30,
	orders: [],
	orderFilter: '',
	orderQuery: '',
	orderTimer: null,
	openOrder: null,
	products: [],
	productQuery: '',
	productLowOnly: false,
	pendingProduct: '',
	openProduct: null,
	customers: [],
	customerQuery: '',
	team: [],
	counterQuery: '',
	counterLines: [],
	counterCustomer: '',
	counterPhone: '',
	counterNote: '',
	counterReceipt: null,
	counterOptions: null,
	toastTimer: null
};

// Signing out must not leave a previous account's rows on screen or let an
// in-flight response from that account repaint it.
export function resetTransientState() {
	clearTimeout(state.orderTimer);
	state.orderTimer = null;
	state.render += 1;
	state.metrics = null;
	state.orders = [];
	state.products = [];
	state.customers = [];
	state.team = [];
	state.counterLines = [];
	state.counterQuery = '';
	state.counterCustomer = '';
	state.counterPhone = '';
	state.counterNote = '';
	state.counterReceipt = null;
	state.orderFilter = '';
	state.orderQuery = '';
	state.productQuery = '';
	state.productLowOnly = false;
	state.pendingProduct = '';
	state.customerQuery = '';
}
