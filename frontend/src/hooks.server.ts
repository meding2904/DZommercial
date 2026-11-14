import { ADMIN_ACCESS_TOKEN, APP_ENV } from '$app/env/private';
import { CSRF_COOKIE } from '#lib/csrf';
import { GATE_COOKIE, GATE_PATH, GATE_CONFIGURED, hasGate } from '#lib/server/gate';
import { isAdminPath, normalizePathname } from '#lib/server/path';
import type { Handle } from '@sveltejs/kit/hooks';

const GATE_PATH_NORMALIZED = normalizePathname(GATE_PATH);
const IS_PRODUCTION = APP_ENV.trim().toLowerCase() === 'production';
const CSRF_MAX_AGE = 7 * 24 * 60 * 60;

function makeCsrfToken(): string {
	const bytes = new Uint8Array(32);
	crypto.getRandomValues(bytes);
	return Array.from(bytes, (value) => value.toString(16).padStart(2, '0')).join('');
}

function harden(response: Response): Response {
	response.headers.set('X-Content-Type-Options', 'nosniff');
	response.headers.set('X-Frame-Options', 'DENY');
	response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
	response.headers.set('Permissions-Policy', 'camera=(), microphone=(), geolocation=()');
	response.headers.set('Cross-Origin-Opener-Policy', 'same-origin');
	response.headers.set('Cross-Origin-Resource-Policy', 'same-origin');

	if (IS_PRODUCTION) {
		response.headers.set('Strict-Transport-Security', 'max-age=31536000; includeSubDomains');
	}

	return response;
}

function notFound(): Response {
	return harden(
		new Response('Not found', {
			status: 404,
			headers: {
				'content-type': 'text/plain; charset=utf-8',
				'cache-control': 'no-store'
			}
		})
	);
}

export const handle: Handle = async ({ event, resolve }) => {
	const pathname = normalizePathname(event.url.pathname);

	if (!GATE_CONFIGURED && (isAdminPath(pathname) || pathname === GATE_PATH_NORMALIZED)) {
		return notFound();
	}

	if (pathname === GATE_PATH_NORMALIZED) {
		const cookie = [
			`${GATE_COOKIE}=${encodeURIComponent(ADMIN_ACCESS_TOKEN)}`,
			'Path=/',
			'HttpOnly',
			'SameSite=Strict',
			'Max-Age=43200',
			...(IS_PRODUCTION ? ['Secure'] : [])
		].join('; ');

		return harden(
			new Response(null, {
				status: 302,
				headers: {
					location: '/admin/login',
					'set-cookie': cookie,
					'cache-control': 'no-store'
				}
			})
		);
	}

	if (isAdminPath(pathname) && !hasGate(event.cookies)) {
		return notFound();
	}

	const acceptsHtml = (event.request.headers.get('accept') ?? '').includes('text/html');
	if (event.request.method === 'GET' && acceptsHtml) {
		const existing = event.cookies.get(CSRF_COOKIE);
		if (!existing || existing.length < 32) {
			event.cookies.set(CSRF_COOKIE, makeCsrfToken(), {
				path: '/',
				httpOnly: false,
				sameSite: 'lax',
				secure: IS_PRODUCTION,
				maxAge: CSRF_MAX_AGE
			});
		}
	}

	return harden(await resolve(event));
};
