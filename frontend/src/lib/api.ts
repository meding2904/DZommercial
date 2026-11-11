import type { ApiErrorBody } from './types';

export class ApiError extends Error {
	status: number;
	code: string;
	fields: Record<string, string>;

	constructor(status: number, message: string, code = 'request_failed', fields = {}) {
		super(message);
		this.name = 'ApiError';
		this.status = status;
		this.code = code;
		this.fields = fields;
	}
}

type RequestOptions = Omit<RequestInit, 'body'> & {
	body?: unknown;
};

export async function apiFetch<T>(path: string, options: RequestOptions = {}): Promise<T> {
	const headers = new Headers(options.headers);
	headers.set('X-Requested-With', 'azahar-web');

	let body: BodyInit | undefined;
	if (options.body !== undefined) {
		headers.set('Content-Type', 'application/json');
		body = JSON.stringify(options.body);
	}

	const res = await fetch(`/api${path}`, {
		...options,
		headers,
		body,
		cache: 'no-store'
	});

	let data: unknown = null;
	try {
		data = await res.json();
	} catch {
		data = null;
	}

	if (!res.ok) {
		const parsed = (data ?? {}) as ApiErrorBody;
		throw new ApiError(
			res.status,
			parsed.error?.message ?? 'Something went wrong. Try again.',
			parsed.error?.code ?? 'request_failed',
			parsed.error?.fields ?? {}
		);
	}

	return data as T;
}
