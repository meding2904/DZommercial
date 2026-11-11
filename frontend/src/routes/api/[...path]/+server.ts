import { INTERNAL_API_URL } from '$app/env/private';
import { hasGate } from '#lib/server/gate';
import { isAdminApiPath, isPathWithin, normalizePathname } from '#lib/server/path';
import type { RequestHandler } from './$types';

const base = INTERNAL_API_URL;
const MAX_BODY_BYTES = 1 << 20;

function errorResponse(status: number, code: string, message: string): Response {
	return new Response(JSON.stringify({ error: { code, message } }), {
		status,
		headers: {
			'content-type': 'application/json; charset=utf-8',
			'cache-control': 'no-store'
		}
	});
}

function notFound(): Response {
	return errorResponse(404, 'not_found', 'Not found.');
}

function payloadTooLarge(): Response {
	return errorResponse(413, 'payload_too_large', 'The request body is too large.');
}

/**
 * Read at most MAX_BODY_BYTES before forwarding. The API has the same limit,
 * but it must not be possible to make the web process buffer an unbounded body
 * first.
 */
async function readLimitedBody(request: Request): Promise<ArrayBuffer | null> {
	if (!request.body) return new ArrayBuffer(0);

	const reader = request.body.getReader();
	const chunks: Uint8Array[] = [];
	let total = 0;

	for (;;) {
		const { done, value } = await reader.read();
		if (done) break;
		if (!value) continue;
		total += value.byteLength;
		if (total > MAX_BODY_BYTES) {
			await reader.cancel();
			return null;
		}
		chunks.push(value);
	}

	const buffer = new ArrayBuffer(total);
	const body = new Uint8Array(buffer);
	let offset = 0;
	for (const chunk of chunks) {
		body.set(chunk, offset);
		offset += chunk.byteLength;
	}
	return buffer;
}

const handler: RequestHandler = async ({
	request,
	params,
	url,
	getClientAddress,
	cookies: requestCookies
}) => {
	// Resolve the URL first: the URL parser is what turns backslashes and
	// encoded dot segments into the path the API will actually receive. The
	// raw `params.path` cannot be trusted for the gate check.
	let target: URL;
	try {
		target = new URL(`/api/${params.path}${url.search}`, base);
	} catch {
		return notFound();
	}

	const targetPath = normalizePathname(target.pathname);

	// Keep the proxy inside its documented namespace. Without this, a crafted
	// path can escape /api and reach another handler on the API host.
	if (!isPathWithin(targetPath, '/api')) {
		return notFound();
	}

	if (isAdminApiPath(targetPath) && !hasGate(requestCookies)) {
		return notFound();
	}

	const declaredLength = Number(request.headers.get('content-length') ?? '');
	if (Number.isFinite(declaredLength) && declaredLength > MAX_BODY_BYTES) {
		return payloadTooLarge();
	}

	const headers = new Headers(request.headers);

	headers.delete('host');
	headers.delete('connection');
	headers.delete('content-length');
	headers.delete('accept-encoding');

	const client = getClientAddress();
	if (client) headers.set('x-forwarded-for', client);
	headers.set('x-forwarded-proto', url.protocol.replace(':', ''));

	const init: RequestInit = {
		method: request.method,
		headers,
		redirect: 'manual',
		signal: AbortSignal.timeout(15000)
	};

	if (request.method !== 'GET' && request.method !== 'HEAD') {
		const body = await readLimitedBody(request);
		if (body === null) return payloadTooLarge();
		init.body = body;
	}

	let upstream: Response;
	try {
		upstream = await fetch(target, init);
	} catch {
		return errorResponse(
			502,
			'api_unavailable',
			'The shop API is not answering. Try again in a moment.'
		);
	}

	const responseHeaders = new Headers();
	for (const [key, value] of upstream.headers) {
		if (['content-encoding', 'content-length', 'transfer-encoding', 'connection'].includes(key)) {
			continue;
		}
		if (key === 'set-cookie') continue;
		responseHeaders.append(key, value);
	}

	const cookies = upstream.headers.getSetCookie?.() ?? [];
	for (const cookie of cookies) responseHeaders.append('set-cookie', cookie);

	responseHeaders.set('cache-control', 'no-store');

	return new Response(upstream.body, {
		status: upstream.status,
		statusText: upstream.statusText,
		headers: responseHeaders
	});
};

export const GET = handler;
export const POST = handler;
export const PUT = handler;
export const PATCH = handler;
export const DELETE = handler;
export const HEAD = handler;
export const OPTIONS = handler;
