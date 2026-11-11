/**
 * The gate is only usable when it is properly configured. A missing token, a
 * short token or the documented development token must disable the gate
 * everywhere, including the proxied admin API. Returning false here is what
 * makes a misconfigured production deploy fail closed.
 */
export function gateConfigured(appEnv: string, token: string): boolean {
	const env = appEnv.trim().toLowerCase();
	return env !== 'production' || (token.length >= 32 && token !== 'devtoken');
}

/**
 * A cookie opens the gate only when the gate is configured and the value
 * matches the token. The configured check is deliberate: `hasGate` must not
 * accept `devtoken` in a production process that failed to set a token.
 */
export function gateMatches(
	value: string | undefined,
	token: string,
	configured: boolean
): boolean {
	return configured && typeof value === 'string' && value.length > 0 && value === token;
}
