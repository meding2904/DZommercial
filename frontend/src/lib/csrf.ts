export const CSRF_COOKIE = 'azahar_csrf';
export const CSRF_HEADER = 'X-CSRF-Token';

export function readCsrfToken(): string {
	if (typeof document === 'undefined') return '';
	const match = document.cookie.match(new RegExp(`(?:^|; )${CSRF_COOKIE}=([^;]*)`));
	return match ? match[1] : '';
}
