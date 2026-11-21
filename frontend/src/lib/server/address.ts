/**
 * A missing or misconfigured ADDRESS_HEADER makes adapter-node throw from
 * getClientAddress. A storefront read should still render, so treat the
 * address as optional and let the API rate-limit the web process itself.
 */
export function optionalClientAddress(read: () => string): string | undefined {
	try {
		const value = read();
		return value ? value : undefined;
	} catch {
		return undefined;
	}
}
