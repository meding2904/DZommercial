export const SHIPPING_CENTS = 60000;
export const FREE_SHIPPING_OVER_CENTS = 800000;

export function shippingFor(subtotalCents: number): number {
	if (subtotalCents <= 0) return 0;
	return subtotalCents > FREE_SHIPPING_OVER_CENTS ? 0 : SHIPPING_CENTS;
}
