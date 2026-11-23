import assert from 'node:assert/strict';
import test from 'node:test';
import {
	createDeliveryMap,
	createDeliveryMapState
} from '../src/lib/checkout/delivery-map.ts';
import { createHumanCheck, createHumanCheckState } from '../src/lib/checkout/human-check.ts';
import {
	normalizeAlgerianMobile,
	orderBody,
	pickFieldErrors,
	validAlgerianMobile,
	validEmail,
	validateOrder
} from '../src/lib/checkout/order-validation.ts';

function challenge(n) {
	return { token: `tok-${n}`, image: `/checkout/challenge/${n}.png`, expires_in: 300 };
}

// A fake fetch the test answers by hand: every path has its own queue, so a
// load, a retry and a reset can be driven in order.
function fakeFetch() {
	const calls = [];
	const queues = new Map();
	return {
		calls,
		queue(path, ...responses) {
			queues.set(path, [...(queues.get(path) ?? []), ...responses]);
			return this;
		},
		async fetch(path) {
			calls.push(path);
			const queue = queues.get(path) ?? [];
			if (queue.length === 0) throw new Error(`no response queued for ${path}`);
			const next = queue.shift();
			if (next instanceof Error) throw next;
			return structuredClone(next);
		}
	};
}

function abortError() {
	const error = new Error('The operation was aborted.');
	error.name = 'AbortError';
	return error;
}

// A transport that stays pending until the test settles it, so two responses
// can arrive out of order.
function deferredFetch({ honourAbort = true } = {}) {
	const calls = [];
	return {
		calls,
		fetch(path, signal) {
			return new Promise((resolve, reject) => {
				calls.push({ path, signal, resolve, reject });
				if (honourAbort) {
					signal?.addEventListener('abort', () => reject(abortError()), { once: true });
				}
			});
		}
	};
}

function flush() {
	return new Promise((resolve) => setTimeout(resolve, 0));
}

function fakeTurnstile() {
	const renders = [];
	const calls = { render: 0, reset: 0, remove: 0 };
	return {
		calls,
		options() {
			return renders.at(-1).options;
		},
		api: {
			render(container, options) {
				calls.render += 1;
				const id = `widget-${calls.render}`;
				renders.push({ id, container, options });
				return id;
			},
			reset() {
				calls.reset += 1;
			},
			remove() {
				calls.remove += 1;
			}
		},
		solve(token) {
			renders.at(-1).options.callback(token);
		},
		expire() {
			renders.at(-1).options['expired-callback']();
		},
		fail() {
			renders.at(-1).options['error-callback']();
		}
	};
}

function validForm(overrides = {}) {
	return {
		name: 'Amina Bensalem',
		email: 'amina@example.com',
		phone: '0550 12 34 56',
		wilayaCode: '16',
		communeName: 'Bab Ezzouar',
		deliveryType: 'home',
		stopDeskId: '',
		address: '12 rue des Oliviers',
		notes: '',
		...overrides
	};
}

const BAG = [
	{ product_id: 'prd_oil', quantity: 2, options: { size: '100 ml' }, price_cents: 540000 }
];

function submit({ form = validForm(), check, website = '', termsAccepted = true } = {}) {
	return orderBody({
		form,
		bag: BAG,
		check,
		website,
		termsAccepted,
		termsVersion: '2026-11-14'
	});
}

test('the image challenge loads, is spent by a refused submit, and is replaced', async () => {
	const transport = fakeFetch().queue('/checkout/challenge', challenge(1), challenge(2), challenge(3));
	const state = createHumanCheckState('');
	const check = createHumanCheck(state, { request: transport.fetch });

	assert.equal(check.kind, 'captcha');
	assert.equal(check.focusId(), 'captcha-answer');

	await check.load();
	assert.equal(state.status, 'ready');
	assert.equal(state.challenge.token, 'tok-1');
	assert.equal(check.satisfied(), false);
	assert.equal(check.validationError(), 'Enter the code from the security check.');
	assert.deepEqual(check.body(), {
		captcha_token: 'tok-1',
		captcha_answer: '',
		turnstile_token: ''
	});

	check.setAnswer('  AB12c ');
	assert.equal(check.satisfied(), true);
	assert.equal(check.validationError(), '');
	assert.deepEqual(check.body(), {
		captcha_token: 'tok-1',
		captcha_answer: 'AB12c',
		turnstile_token: ''
	});

	// A 422 that names the check spends it: a fresh challenge, the answer cleared.
	assert.equal(await check.recover({ status: 422, fields: { captcha: 'Complete it again.' } }), true);
	assert.equal(transport.calls.length, 2);
	assert.equal(state.challenge.token, 'tok-2');
	assert.equal(state.answer, '');
	assert.equal(check.satisfied(), false);

	// Any 409 spends it, even when the body does not name the check.
	assert.equal(await check.recover({ status: 409, fields: {} }), true);
	assert.equal(state.challenge.token, 'tok-3');

	// A refusal that names another field leaves the challenge alone.
	assert.equal(
		await check.recover({ status: 422, fields: { email: 'Enter a valid email.' } }),
		false
	);
	assert.equal(transport.calls.length, 3);
	assert.equal(state.challenge.token, 'tok-3');
});

test('a challenge that fails to load says so and the retry clears it', async () => {
	const transport = fakeFetch()
		.queue('/checkout/challenge', new Error('offline'), challenge(1));
	const state = createHumanCheckState('');
	const check = createHumanCheck(state, { request: transport.fetch });

	await check.load();
	assert.equal(state.status, 'error');
	assert.equal(
		state.error,
		'The security check could not load. Check your connection and try again.'
	);
	assert.equal(check.satisfied(), false);
	assert.equal(check.validationError(), 'The security check is not ready. Request a new code.');

	await check.load();
	assert.equal(state.status, 'ready');
	assert.equal(state.error, '');
	assert.equal(state.challenge.token, 'tok-1');
	assert.equal(check.satisfied(), false);
	assert.equal(check.validationError(), 'Enter the code from the security check.');
});

test('the Turnstile branch answers the same interface as the image branch', async () => {
	const widget = fakeTurnstile();
	const container = { replaceChildren() {} };
	const state = createHumanCheckState(' site-key ');
	const check = createHumanCheck(state, {
		request: async () => {
			throw new Error('the image challenge must not run');
		},
		turnstile: () => widget.api,
		container: () => container
	});

	assert.equal(check.kind, 'turnstile');
	assert.equal(state.siteKey, 'site-key');
	assert.equal(check.focusId(), 'turnstile-check');

	await check.load();
	assert.equal(state.status, 'ready');
	assert.equal(widget.calls.render, 1);
	assert.equal(widget.options().sitekey, 'site-key');
	assert.equal(widget.options().size, 'flexible');
	assert.equal(check.satisfied(), false);
	assert.equal(check.validationError(), 'Complete the security check before placing the order.');
	assert.deepEqual(check.body(), {
		captcha_token: '',
		captcha_answer: '',
		turnstile_token: ''
	});

	widget.solve('turnstile-token');
	assert.equal(check.satisfied(), true);
	assert.equal(state.error, '');
	assert.deepEqual(check.body(), {
		captcha_token: '',
		captcha_answer: '',
		turnstile_token: 'turnstile-token'
	});

	// An expired widget asks for the check again without rendering a new one.
	widget.expire();
	assert.equal(check.satisfied(), false);
	assert.equal(check.validationError(), 'Complete the security check before placing the order.');
	assert.equal(state.error, 'The security check expired. Complete it again.');
	assert.equal(widget.calls.render, 1);

	// A widget that cannot render is an error the page can offer to retry.
	widget.fail();
	assert.equal(state.status, 'error');
	assert.equal(
		state.error,
		'The security check failed to load. Check your connection and try again.'
	);
	assert.equal(
		check.validationError(),
		'The security check is unavailable. Try again, or write to hello@azahar.example.'
	);

	// The reset after a refused submit keeps the same widget.
	assert.equal(
		await check.recover({
			status: 422,
			fields: { captcha: 'Complete the security check and try again.' }
		}),
		true
	);
	assert.equal(widget.calls.reset, 1);
	assert.equal(widget.calls.remove, 0);
	assert.equal(check.satisfied(), false);

	// Without a container or an api the check reports the load failure.
	const missing = createHumanCheckState('site-key');
	const unavailable = createHumanCheck(missing, {
		request: async () => ({}),
		turnstile: () => undefined,
		container: () => null
	});
	await unavailable.load();
	assert.equal(missing.status, 'error');
	assert.equal(
		missing.error,
		'The security check could not load. Check your connection and try again, or write to hello@azahar.example.'
	);
	assert.equal(
		unavailable.validationError(),
		'The security check is unavailable. Try again, or write to hello@azahar.example.'
	);
	assert.equal(unavailable.focusId(), 'turnstile-check');
	unavailable.destroy();
});

test('the Algerian mobile rule folds the +213 form and rejects the rest', () => {
	const accepted = [
		'0550123456',
		'0550 12 34 56',
		'+213 550 12 34 56',
		'213550123456',
		'(0550) 12-34-56',
		'0661 23 45 67',
		'0770 00 00 00'
	];
	for (const value of accepted) assert.equal(validAlgerianMobile(value), true, value);

	const rejected = [
		'',
		'05501234',
		'00213550123456',
		'0450123456',
		'550123456',
		'0550 12 34 5a',
		'+33 6 12 34 56 78'
	];
	for (const value of rejected) assert.equal(validAlgerianMobile(value), false, value);

	assert.equal(normalizeAlgerianMobile('+213 (550) 12-34-56'), '0550123456');
	assert.equal(normalizeAlgerianMobile('213 550 12 34 56'), '0550123456');
});

test('validation marks fields in reading order, terms last', () => {
	const form = validForm();
	let result = validateOrder({
		form: { ...form, name: '', email: '', phone: '' },
		termsAccepted: true,
		checkError: ''
	});
	assert.equal(result.field, 'name');
	assert.equal(result.errors.name, 'Enter the name for the delivery.');
	assert.equal(result.errors.email, 'Enter an email for the order.');

	result = validateOrder({
		form: { ...form, email: 'amina@example' },
		termsAccepted: true,
		checkError: ''
	});
	assert.equal(result.field, 'email');
	assert.equal(result.errors.email, 'Enter a valid email.');

	result = validateOrder({
		form: { ...form, phone: '+33 6 12 34 56 78' },
		termsAccepted: true,
		checkError: ''
	});
	assert.equal(result.field, 'phone');
	assert.equal(result.errors.phone, 'Use an Algerian mobile number, for example 0550 12 34 56.');

	result = validateOrder({ form, termsAccepted: false, checkError: '' });
	assert.deepEqual(Object.keys(result.errors), ['terms']);
	assert.equal(result.field, 'terms');

	// The human check's own message lands on its field, before the terms.
	result = validateOrder({
		form,
		termsAccepted: false,
		checkError: 'Enter the code from the security check.'
	});
	assert.deepEqual(Object.keys(result.errors), ['captcha', 'terms']);
	assert.equal(result.field, 'captcha');

	result = validateOrder({ form, termsAccepted: true, checkError: '' });
	assert.deepEqual(result.errors, {});
	assert.equal(result.field, null);

	assert.equal(validEmail('amina@example.com'), true);
	assert.equal(validEmail(' amina@example.com '), true);
	assert.equal(validEmail('amina@example'), false);
	assert.equal(validEmail('amina @example.com'), false);
	assert.equal(validEmail(''), false);
});

test('a stop desk order needs a desk; home delivery needs an address', () => {
	const home = validateOrder({
		form: validForm({ address: '   ' }),
		termsAccepted: true,
		checkError: ''
	});
	assert.equal(home.field, 'address');
	assert.equal(home.errors.address, 'Enter the delivery address.');

	const desk = validateOrder({
		form: validForm({ deliveryType: 'stopdesk', address: '', stopDeskId: '' }),
		termsAccepted: true,
		checkError: ''
	});
	assert.equal(desk.field, 'stop_desk_id');
	assert.equal(desk.errors.stop_desk_id, 'Choose a stop desk.');
	assert.equal(desk.errors.address, undefined);

	// The stop desk arrives from the delivery map with its commune.
	const chosen = { ...validForm(), deliveryType: 'stopdesk', address: '', stopDeskId: 'desk_16_01' };
	assert.equal(validateOrder({ form: chosen, termsAccepted: true, checkError: '' }).field, null);
	assert.equal(
		validateOrder({
			form: { ...chosen, notes: 'x'.repeat(501) },
			termsAccepted: true,
			checkError: ''
		}).errors.notes,
		'Keep notes to 500 characters or fewer.'
	);
});

test('the honeypot is never a field error but travels to the server', () => {
	const result = validateOrder({ form: validForm(), termsAccepted: true, checkError: '' });
	assert.deepEqual(result.errors, {});
	assert.equal('website' in result.errors, false);

	const body = submit({
		check: { captcha_token: 'tok-1', captcha_answer: 'AB12c', turnstile_token: '' },
		website: 'http://spam.example'
	});
	assert.equal(body.website, 'http://spam.example');

	// The server names fields the page does not own; they never reach the form.
	assert.deepEqual(
		pickFieldErrors({
			website: 'The order could not be verified.',
			items: 'Add between 1 and 20 products.',
			email: 'Enter a valid email.'
		}),
		{ email: 'Enter a valid email.' }
	);
});

test('the delivery map discards a response that lost the race', async () => {
	const transport = deferredFetch({ honourAbort: false });
	const state = createDeliveryMapState();
	const map = createDeliveryMap(state, { request: transport.fetch });
	const wilaya = (code, nameFr) => ({ code, nameFr, nameAr: '', communeCount: 1 });

	// Two reads of the map: the slow first one cannot overwrite the second.
	const first = map.load();
	const second = map.load();
	assert.equal(transport.calls[0].signal.aborted, true);
	transport.calls[1].resolve({ wilayas: [wilaya(31, 'Oran')] });
	await second;
	transport.calls[0].resolve({ wilayas: [wilaya(16, 'Alger')] });
	await first;
	assert.deepEqual(state.wilayas.map((entry) => entry.code), [31]);
	assert.equal(state.wilayaStatus, 'ready');
	assert.equal(state.wilayaError, '');

	// A commune read for the old wilaya is discarded after the choice changes.
	map.chooseWilaya('16');
	const oldCommunes = transport.calls.at(-1);
	assert.equal(oldCommunes.path, '/shipping/communes?wilaya=16');
	map.chooseWilaya('31');
	const newCommunes = transport.calls.at(-1);
	assert.equal(newCommunes.path, '/shipping/communes?wilaya=31');

	newCommunes.resolve({ communes: [{ wilayaCode: 31, nameFr: 'Bir El Djir', nameAr: '' }] });
	await flush();
	oldCommunes.resolve({ communes: [{ wilayaCode: 16, nameFr: 'Bab Ezzouar', nameAr: '' }] });
	await flush();
	assert.deepEqual(state.communes.map((entry) => entry.nameFr), ['Bir El Djir']);
	assert.equal(state.communeStatus, 'ready');
	assert.equal(state.communeError, '');

	// A stop desk read for a delivery type already left behind never lands.
	map.chooseDeliveryType('stopdesk');
	const staleDesks = transport.calls.at(-1);
	assert.equal(staleDesks.path, '/shipping/desks?wilaya=31');
	map.chooseDeliveryType('home');
	staleDesks.resolve({ desks: [{ id: 'desk_31_01', name: 'Desk', wilayaCode: 31 }] });
	await flush();
	assert.deepEqual(state.desks, []);
	assert.equal(state.deskStatus, 'idle');
	assert.equal(state.deskError, '');

	// The failure that does belong to the latest request is the one shown.
	map.chooseDeliveryType('stopdesk');
	transport.calls.at(-1).reject(new Error('the shop API is not answering'));
	await flush();
	assert.equal(state.deskStatus, 'error');
	assert.equal(
		state.deskError,
		'The stop desks could not be read. Try again, or choose home delivery.'
	);
	assert.equal(state.wilayaError, '');
});

test('the delivery map resets the cascade in one step', async () => {
	const transport = deferredFetch({ honourAbort: false });
	const state = createDeliveryMapState();
	const map = createDeliveryMap(state, { request: transport.fetch });

	map.load();
	transport.calls[0].resolve({ wilayas: [{ code: 16, nameFr: 'Alger', nameAr: '', communeCount: 57 }] });
	await flush();

	map.chooseWilaya('16');
	map.chooseCommune('Bab Ezzouar');
	assert.equal(state.communeName, 'Bab Ezzouar');
	map.chooseDeliveryType('stopdesk');
	const desks = transport.calls.at(-1);
	desks.resolve({
		desks: [{ id: 'desk_16_01', name: 'Agence Bab Ezzouar', wilayaCode: 16, communeName: 'Bab Ezzouar', address: 'Rue des Frères' }]
	});
	await flush();
	map.chooseDesk('desk_16_01');
	assert.equal(state.communeName, 'Bab Ezzouar');
	assert.equal(map.communeLocked, true);
	assert.equal(map.selectedDesk.address, 'Rue des Frères');

	// Choosing another wilaya drops the commune, the desk and both lists.
	const before = transport.calls.length;
	map.chooseWilaya('31');
	assert.equal(state.wilayaCode, '31');
	assert.equal(state.communeName, '');
	assert.equal(state.stopDeskId, '');
	assert.deepEqual(state.desks, []);
	assert.deepEqual(state.communes, []);
	assert.equal(map.communeLocked, false);
	assert.equal(map.selectedDesk, undefined);
	// The commune and the stop desk for the new wilaya both start reading.
	assert.deepEqual(
		transport.calls.slice(before).map((call) => call.path),
		['/shipping/communes?wilaya=31', '/shipping/desks?wilaya=31']
	);

	// The options are the map's own read of the state.
	assert.deepEqual(map.wilayaOptions[0], { value: '', label: 'Choose a wilaya' });
	assert.equal(map.wilayaOptions[1].label, 'Alger');
	assert.deepEqual(map.deskOptions, [{ value: '', label: 'Reading stop desks…' }]);
	map.destroy();
});

test('the submit body keeps its field names for both checks', () => {
	const captchaBody = submit({
		check: { captcha_token: 'tok-1', captcha_answer: 'AB12c', turnstile_token: '' }
	});
	assert.deepEqual(Object.keys(captchaBody), [
		'name',
		'email',
		'phone',
		'address',
		'wilaya_code',
		'commune_name',
		'delivery_type',
		'stop_desk_id',
		'notes',
		'items',
		'website',
		'captcha_token',
		'captcha_answer',
		'turnstile_token',
		'terms_accepted',
		'terms_version'
	]);
	assert.equal(captchaBody.wilaya_code, 16);
	assert.equal(captchaBody.terms_version, '2026-11-14');
	assert.equal(captchaBody.captcha_token, 'tok-1');
	assert.equal(captchaBody.captcha_answer, 'AB12c');
	assert.equal(captchaBody.turnstile_token, '');
	assert.deepEqual(captchaBody.items, [
		{ product_id: 'prd_oil', quantity: 2, options: { size: '100 ml' }, expected_price_cents: 540000 }
	]);

	const turnstileBody = submit({
		check: { captcha_token: '', captcha_answer: '', turnstile_token: 'turnstile-token' }
	});
	assert.equal(turnstileBody.turnstile_token, 'turnstile-token');
	assert.equal(turnstileBody.captcha_token, '');
	assert.equal(turnstileBody.captcha_answer, '');

	// A stop desk order sends the desk, not the street address.
	const deskBody = submit({
		form: validForm({ deliveryType: 'stopdesk', stopDeskId: 'desk_16_01' }),
		check: { captcha_token: 'tok-1', captcha_answer: 'AB12c', turnstile_token: '' }
	});
	assert.equal(deskBody.delivery_type, 'stopdesk');
	assert.equal(deskBody.stop_desk_id, 'desk_16_01');
	assert.equal(deskBody.address, '');

	const trimmed = submit({
		form: validForm({ name: '  Amina  ', notes: '  gate 3  ' }),
		check: { captcha_token: '', captcha_answer: '', turnstile_token: '' }
	});
	assert.equal(trimmed.name, 'Amina');
	assert.equal(trimmed.notes, 'gate 3');
	assert.equal(trimmed.terms_accepted, true);
});
