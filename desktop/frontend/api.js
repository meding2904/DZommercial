// Request helpers layered on the bridge. call() unwraps the API envelope into
// data or a RequestError; optionalCall() treats a 401 as "no session yet".

import { api } from './bridge.js';

export class RequestError extends Error {
	/**
	 * @param {{ status?: number, code?: string, message?: string, fields?: object } | null} payload
	 */
	constructor(payload) {
		super(payload && payload.message ? payload.message : 'The request failed.');
		this.name = 'RequestError';
		this.status = payload && payload.status ? payload.status : 0;
		this.code = payload && payload.code ? payload.code : 'request_failed';
		this.fields = payload && payload.fields ? payload.fields : {};
	}
}

/**
 * @param {'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'} method
 * @param {string} path
 * @param {unknown} [body]
 * @returns {Promise<any>}
 */
export function call(method, path, body) {
	return api.request(method, path, body).then((result) => {
		if (!result || !result.ok) throw new RequestError(result && result.error);
		return result.data;
	});
}

/**
 * @param {string} path
 * @returns {Promise<any>}
 */
export async function optionalCall(path) {
	try {
		return await call('GET', path);
	} catch (error) {
		if (error instanceof RequestError && error.status === 401) return null;
		throw error;
	}
}

/**
 * @returns {Promise<any>}
 */
export async function readHealth() {
	try {
		return await api.health();
	} catch (error) {
		return { ok: false, status: 0, message: error.message || 'The server is not answering.' };
	}
}
