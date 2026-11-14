import { defineEnvVars } from '@sveltejs/kit/env';

export const variables = defineEnvVars({
	APP_ENV: {
		public: false,
		description: 'development, test or production',
		schema: (value) => value ?? 'development'
	},
	INTERNAL_API_URL: {
		public: false,
		description: 'The Go API base URL used by the SvelteKit server',
		schema: (value) => value ?? 'http://127.0.0.1:8080'
	},
	ADMIN_ACCESS_TOKEN: {
		public: false,
		description: 'Random token for the hidden admin gate. Kiri prints the gate URL.',
		schema: (value) => value ?? 'devtoken'
	},
	PUBLIC_TURNSTILE_SITE_KEY: {
		public: true,
		description:
			'Cloudflare Turnstile site key. Empty keeps the local captcha as the development fallback.',
		schema: (value) => value ?? ''
	}
});
