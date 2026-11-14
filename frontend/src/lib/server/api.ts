import { INTERNAL_API_URL } from '$app/env/private';
import type { ApiErrorBody } from '#lib/types';

const base = INTERNAL_API_URL;

export class ServerApiError extends Error {
	status: number;
	constructor(status: number, message: string) {
		super(message);
		this.status = status;
	}
}

export async function serverGet<T>(path: string, clientAddress?: string): Promise<T> {
	const headers: Record<string, string> = {
		'X-Requested-With': 'azahar-web',
		Accept: 'application/json'
	};
	if (clientAddress) headers['X-Forwarded-For'] = clientAddress;

	const res = await fetch(`${base}${path}`, {
		headers,
		signal: AbortSignal.timeout(8000)
	});

	let data: unknown = null;
	try {
		data = await res.json();
	} catch {
		data = null;
	}

	if (!res.ok) {
		const body = (data ?? {}) as ApiErrorBody;
		throw new ServerApiError(
			res.status,
			body.error?.message ?? `The shop API returned ${res.status}.`
		);
	}

	return data as T;
}
