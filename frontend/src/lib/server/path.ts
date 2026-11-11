const MAX_DECODE_ROUNDS = 5;

/**
 * Normalise a request path so that security checks see the path the router
 * will resolve.
 *
 * SvelteKit routes percent-encoded segments after decoding, while
 * `event.url.pathname` keeps them encoded. The WHATWG URL parser also turns
 * backslashes into slashes and resolves dot segments. Decoding only once is not
 * enough: the router and the Go API may each decode a layer. This function
 * applies the same operations repeatedly, then resolves `.` and `..`.
 */
export function normalizePathname(pathname: string): string {
	let decoded = pathname;

	for (let round = 0; round < MAX_DECODE_ROUNDS; round++) {
		let next: string;
		try {
			next = decodeURIComponent(decoded);
		} catch {
			// A malformed escape is left as-is; it cannot resolve to a route.
			break;
		}
		if (next === decoded) break;
		decoded = next;
	}

	const segments: string[] = [];
	for (const segment of decoded.replace(/\\/g, '/').split('/')) {
		if (segment === '' || segment === '.') continue;
		if (segment === '..') {
			segments.pop();
			continue;
		}
		segments.push(segment);
	}

	return `/${segments.join('/')}`;
}

/** True when the normalised path is `base` or a child of `base`. */
export function isPathWithin(pathname: string, base: string): boolean {
	const normalized = normalizePathname(pathname);
	const prefix = normalizePathname(base);
	return normalized === prefix || normalized.startsWith(`${prefix}/`);
}

/** The hidden admin UI lives at /admin and its children. */
export function isAdminPath(pathname: string): boolean {
	return isPathWithin(pathname, '/admin');
}

/** The proxied admin API lives at /api/admin and its children. */
export function isAdminApiPath(pathname: string): boolean {
	return isPathWithin(pathname, '/api/admin');
}
