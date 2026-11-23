// Human check: one interface over the two checks checkout can run — Cloudflare
// Turnstile when a site key is configured, the home-made image challenge
// otherwise. The page asks whether the check is satisfied, what the submit body
// carries and what a refused submit costs; the fork lives here.

import type { HumanCheckFields } from './order-validation';
import type { Request } from './delivery-map';

export type HumanCheckKind = 'turnstile' | 'captcha';

export type CaptchaChallenge = {
	token: string;
	image: string;
	expires_in: number;
};

export type CheckStatus = 'idle' | 'loading' | 'ready' | 'error';

export type HumanCheckState = {
	kind: HumanCheckKind;
	// The configured Turnstile key; empty when the image challenge runs.
	siteKey: string;
	status: CheckStatus;
	// The image challenge, when the image check is the one running.
	challenge: CaptchaChallenge | null;
	// What the customer typed for the image challenge.
	answer: string;
	// The Turnstile token, when Turnstile is the one running.
	token: string;
	// The check's own message, shown beside the widget.
	error: string;
};

type TurnstileOptions = {
	sitekey: string;
	theme: 'light' | 'dark';
	size: 'flexible';
	callback: (token: string) => void;
	'expired-callback': () => void;
	'error-callback': () => void;
	'timeout-callback': () => void;
};

export type TurnstileApi = {
	render: (container: HTMLElement, options: TurnstileOptions) => string;
	reset: (widgetId?: string) => void;
	remove: (widgetId?: string) => void;
};

export type HumanCheckOptions = {
	request: Request;
	// Where Turnstile renders. Called after the first paint, when the container
	// is in the DOM.
	container?: () => HTMLElement | null;
	// Defaults to window.turnstile; tests hand in a fake.
	turnstile?: () => TurnstileApi | undefined;
};

export type HumanCheck = {
	readonly kind: HumanCheckKind;
	readonly state: HumanCheckState;
	load(): Promise<void>;
	reset(): Promise<void>;
	// A 422 naming captcha, or any 409, spends the check: reset it and say so.
	recover(error: unknown): Promise<boolean>;
	satisfied(): boolean;
	// Empty when satisfied, otherwise the message the form marks the check with.
	validationError(): string;
	// The element that takes focus when the check is the first invalid field.
	focusId(): string;
	body(): HumanCheckFields;
	setAnswer(value: string): void;
	destroy(): void;
};

const CAPTCHA_LOAD_ERROR = 'The security check could not load. Check your connection and try again.';
const TURNSTILE_LOAD_ERROR =
	'The security check could not load. Check your connection and try again, or write to hello@azahar.example.';
const TURNSTILE_UNAVAILABLE =
	'The security check is unavailable. Try again, or write to hello@azahar.example.';

export function humanCheckKind(siteKey: string): HumanCheckKind {
	return siteKey.trim().length > 0 ? 'turnstile' : 'captcha';
}

export function createHumanCheckState(siteKey: string): HumanCheckState {
	return {
		kind: humanCheckKind(siteKey),
		siteKey: siteKey.trim(),
		status: 'idle',
		challenge: null,
		answer: '',
		token: '',
		error: ''
	};
}

// A refused submit arrives as the API's error shape; duck-typed so this module
// needs no import of the API client.
function refusalOf(error: unknown): { status?: number; fields?: Record<string, string> } | null {
	if (!error || typeof error !== 'object') return null;
	const candidate = error as { status?: unknown; fields?: unknown };
	return {
		status: typeof candidate.status === 'number' ? candidate.status : undefined,
		fields:
			candidate.fields && typeof candidate.fields === 'object'
				? (candidate.fields as Record<string, string>)
				: undefined
	};
}

function readTheme(): 'light' | 'dark' {
	if (typeof document === 'undefined') return 'light';
	return document.documentElement.getAttribute('data-store-theme') === 'dark' ? 'dark' : 'light';
}

function turnstileApi(): TurnstileApi | undefined {
	if (typeof window === 'undefined') return undefined;
	return (window as Window & { turnstile?: TurnstileApi }).turnstile;
}

function ensureTurnstileScript(): Promise<void> {
	if (typeof document === 'undefined') {
		return Promise.reject(new Error('Turnstile is unavailable.'));
	}
	if (turnstileApi()) return Promise.resolve();

	const prior = document.querySelector<HTMLScriptElement>('script[data-azahar-turnstile]');
	const script = prior ?? document.createElement('script');

	const loaded = new Promise<void>((resolve, reject) => {
		script.addEventListener('load', () => resolve(), { once: true });
		script.addEventListener(
			'error',
			() => {
				script.remove();
				reject(new Error('The Turnstile script failed to load.'));
			},
			{ once: true }
		);
	});

	if (!prior) {
		script.dataset.azaharTurnstile = 'true';
		script.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
		script.async = true;
		document.head.appendChild(script);
	}

	return loaded;
}

type Adapter = {
	load(): Promise<void>;
	reset(): Promise<void>;
	satisfied(): boolean;
	validationError(): string;
	focusId(): string;
	body(): HumanCheckFields;
	destroy(): void;
};

function turnstileAdapter(state: HumanCheckState, options: HumanCheckOptions): Adapter {
	let widgetId: string | null = null;
	let scriptPromise: Promise<void> | null = null;

	function api(): TurnstileApi | undefined {
		return options.turnstile ? options.turnstile() : turnstileApi();
	}

	// The Cloudflare script is fetched once; a failed fetch clears the cache so
	// Try again reaches the network again.
	function ensureScript(): Promise<void> {
		if (api()) return Promise.resolve();
		if (scriptPromise) return scriptPromise;
		scriptPromise = ensureTurnstileScript().catch((error: unknown) => {
			scriptPromise = null;
			throw error;
		});
		return scriptPromise;
	}

	async function load(): Promise<void> {
		if (state.status === 'loading') return;
		state.status = 'loading';
		state.token = '';
		state.error = '';

		try {
			await ensureScript();
			const client = api();
			const container = options.container?.() ?? null;
			if (!client || !container) throw new Error('Turnstile is unavailable.');

			if (widgetId) {
				client.remove(widgetId);
				widgetId = null;
				container.replaceChildren();
			}

			widgetId = client.render(container, {
				sitekey: state.siteKey,
				theme: readTheme(),
				size: 'flexible',
				callback: (token) => {
					state.token = token;
					state.status = 'ready';
					state.error = '';
				},
				'expired-callback': () => {
					state.token = '';
					state.error = 'The security check expired. Complete it again.';
				},
				'error-callback': () => {
					state.token = '';
					state.status = 'error';
					state.error = 'The security check failed to load. Check your connection and try again.';
				},
				'timeout-callback': () => {
					state.token = '';
					state.error = 'The security check timed out. Complete it again.';
				}
			});

			state.status = 'ready';
		} catch {
			state.status = 'error';
			state.error = TURNSTILE_LOAD_ERROR;
		}
	}

	async function reset(): Promise<void> {
		state.token = '';
		state.error = '';

		const client = api();
		if (client && widgetId) {
			client.reset(widgetId);
			state.status = 'ready';
			return;
		}

		// No widget to reset: render it again, and let the page refocus the
		// check without waiting for the load.
		state.status = 'idle';
		void load();
	}

	return {
		load,
		reset,
		satisfied() {
			return state.token.length > 0;
		},
		validationError() {
			if (state.token) return '';
			if (state.status === 'error') return TURNSTILE_UNAVAILABLE;
			if (state.status === 'loading') return 'Wait for the security check to finish loading.';
			return 'Complete the security check before placing the order.';
		},
		focusId() {
			return 'turnstile-check';
		},
		body() {
			return { captcha_token: '', captcha_answer: '', turnstile_token: state.token };
		},
		destroy() {
			const client = api();
			if (client && widgetId) client.remove(widgetId);
			widgetId = null;
		}
	};
}

function captchaAdapter(state: HumanCheckState, options: HumanCheckOptions): Adapter {
	let attempt = 0;

	async function load(): Promise<void> {
		const id = ++attempt;
		state.status = 'loading';
		state.error = '';
		state.challenge = null;
		state.answer = '';

		try {
			const challenge = (await options.request('/checkout/challenge')) as CaptchaChallenge;
			if (id !== attempt) return;
			state.challenge = challenge;
			state.status = 'ready';
		} catch {
			if (id !== attempt) return;
			state.status = 'error';
			state.error = CAPTCHA_LOAD_ERROR;
		}
	}

	async function reset(): Promise<void> {
		await load();
	}

	return {
		load,
		reset,
		satisfied() {
			return Boolean(state.challenge?.token) && state.answer.trim().length > 0;
		},
		validationError() {
			if (!state.challenge?.token) {
				return 'The security check is not ready. Request a new code.';
			}
			if (!state.answer.trim()) return 'Enter the code from the security check.';
			return '';
		},
		focusId() {
			return 'captcha-answer';
		},
		body() {
			return {
				captcha_token: state.challenge?.token ?? '',
				captcha_answer: state.answer.trim(),
				turnstile_token: ''
			};
		},
		destroy() {
			attempt += 1;
		}
	};
}

export function createHumanCheck(state: HumanCheckState, options: HumanCheckOptions): HumanCheck {
	const adapter =
		state.kind === 'turnstile' ? turnstileAdapter(state, options) : captchaAdapter(state, options);

	return {
		get kind() {
			return state.kind;
		},
		get state() {
			return state;
		},
		load: adapter.load,
		reset: adapter.reset,
		async recover(error) {
			const refusal = refusalOf(error);
			if (!refusal) return false;
			if (!refusal.fields?.captcha && refusal.status !== 409) return false;
			await adapter.reset();
			return true;
		},
		satisfied: adapter.satisfied,
		validationError: adapter.validationError,
		focusId: adapter.focusId,
		body: adapter.body,
		setAnswer(value) {
			state.answer = value;
		},
		destroy: adapter.destroy
	};
}
