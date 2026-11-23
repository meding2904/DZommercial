// Order validation: the checkout matrix and the submit body, in one place.
// Pure functions only, so the page renders what they return and the tests can
// reach every rule. The field order mirrors the server so the first message a
// person sees is the first field they read; docs/API.md stays the authority.

export const FIELD_ORDER = [
	'name',
	'email',
	'phone',
	'wilaya_code',
	'commune_name',
	'delivery_type',
	'stop_desk_id',
	'address',
	'notes',
	'captcha',
	'terms'
] as const;

export type FieldKey = (typeof FIELD_ORDER)[number];

export const INVALID_FIELDS_MESSAGE = 'Check the marked fields.';

export type DeliveryType = 'home' | 'stopdesk';

// The delivery details an order is placed with. The page keeps contact and
// address, the delivery map keeps the wilaya, commune and stop desk.
export type OrderForm = {
	name: string;
	email: string;
	phone: string;
	wilayaCode: string;
	communeName: string;
	deliveryType: DeliveryType;
	stopDeskId: string;
	address: string;
	notes: string;
};

type BagLine = {
	product_id: string;
	quantity: number;
	options: Record<string, string>;
	price_cents: number;
};

// What the human check contributes to the body, whichever check ran.
export type HumanCheckFields = {
	captcha_token: string;
	captcha_answer: string;
	turnstile_token: string;
};

export type OrderSubmit = {
	form: OrderForm;
	bag: readonly BagLine[];
	check: HumanCheckFields;
	// The honeypot. A person never fills it, so it is never a field error: it
	// travels untouched and the server refuses the order.
	website: string;
	termsAccepted: boolean;
	termsVersion: string;
};

export type ValidationInput = {
	form: OrderForm;
	termsAccepted: boolean;
	// Empty when the human check is satisfied; the check owns its wording.
	checkError: string;
};

export type ValidationResult = {
	errors: Record<string, string>;
	// The first marked field in reading order, or null when the order is ready.
	field: FieldKey | null;
};

// The digits behind every accepted phone form. The plus and the 213 prefix are
// the same number as the leading zero, so fold them before matching.
export function normalizeAlgerianMobile(value: string): string {
	return value.replace(/[\s()+-]/g, '').replace(/^213/, '0');
}

export function validAlgerianMobile(value: string): boolean {
	return /^0[567]\d{8}$/.test(normalizeAlgerianMobile(value));
}

export function validEmail(value: string): boolean {
	return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim());
}

export function validateOrder({
	form,
	termsAccepted,
	checkError
}: ValidationInput): ValidationResult {
	const errors: Record<string, string> = {};

	if (!form.name.trim()) errors.name = 'Enter the name for the delivery.';
	if (!form.email.trim()) errors.email = 'Enter an email for the order.';
	else if (!validEmail(form.email)) errors.email = 'Enter a valid email.';
	if (!form.phone.trim()) errors.phone = 'Enter a phone number the courier can call.';
	else if (!validAlgerianMobile(form.phone)) {
		errors.phone = 'Use an Algerian mobile number, for example 0550 12 34 56.';
	}
	if (!form.wilayaCode) errors.wilaya_code = 'Choose a wilaya.';
	if (!form.communeName) errors.commune_name = 'Choose a commune.';
	// A stop desk carries the address; home delivery asks for one.
	if (form.deliveryType === 'stopdesk') {
		if (!form.stopDeskId) errors.stop_desk_id = 'Choose a stop desk.';
	} else if (!form.address.trim()) {
		errors.address = 'Enter the delivery address.';
	}
	if (form.notes.length > 500) {
		errors.notes = 'Keep notes to 500 characters or fewer.';
	}
	if (checkError) errors.captcha = checkError;
	if (!termsAccepted) {
		errors.terms = 'Accept the terms of service before placing the order.';
	}

	return {
		errors,
		field: FIELD_ORDER.find((key) => errors[key]) ?? null
	};
}

// The server answers with a flat field map; the page only renders the fields it
// owns, so anything else (items, a plugin-specific key) stays out of the form.
export function pickFieldErrors(fields: Record<string, string>): Record<string, string> {
	return Object.fromEntries(
		Object.entries(fields).filter(([key]) => FIELD_ORDER.includes(key as FieldKey))
	);
}

export function orderBody({
	form,
	bag,
	check,
	website,
	termsAccepted,
	termsVersion
}: OrderSubmit) {
	// Key order is the body the server has always received; keep it stable so a
	// captured request stays byte-identical.
	return {
		name: form.name.trim(),
		email: form.email.trim(),
		phone: form.phone.trim(),
		address: form.deliveryType === 'home' ? form.address.trim() : '',
		wilaya_code: Number(form.wilayaCode),
		commune_name: form.communeName,
		delivery_type: form.deliveryType,
		stop_desk_id: form.deliveryType === 'stopdesk' ? form.stopDeskId : '',
		notes: form.notes.trim(),
		items: bag.map((line) => ({
			product_id: line.product_id,
			quantity: line.quantity,
			options: line.options,
			expected_price_cents: line.price_cents
		})),
		website,
		captcha_token: check.captcha_token,
		captcha_answer: check.captcha_answer,
		turnstile_token: check.turnstile_token,
		terms_accepted: termsAccepted,
		terms_version: termsVersion
	};
}
