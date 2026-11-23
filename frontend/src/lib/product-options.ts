import type { OptionField, Product, VariantStock } from './types';

// The phone plugin's plugin_data keys. This module is the only core file that
// reads them; storefront surfaces go through the helpers below.
const VARIANTS = 'variants';
const DEVICES_IN_STOCK = 'devices_in_stock';

function variantStock(product: Product): VariantStock[] | null {
	const raw = product.plugin_data?.[VARIANTS];
	if (!Array.isArray(raw)) return null;
	return raw.filter(
		(item): item is VariantStock =>
			typeof item === 'object' && item !== null && typeof item.in_stock === 'number'
	);
}

function matches(
	variant: VariantStock,
	selection: Record<string, string>,
	skipKey?: string
): boolean {
	return Object.entries(selection).every(
		([key, value]) => key === skipKey || !value || variant[key] === value
	);
}

export function missingSelectFields(
	product: Product,
	selection: Record<string, string> = {}
): OptionField[] {
	return product.option_schema.filter(
		(field) => field.type === 'select' && field.required && !selection[field.key]
	);
}

export function resolveVariant(
	product: Product,
	selection: Record<string, string> = {}
): VariantStock | null {
	const variants = variantStock(product);
	if (!variants) return null;
	return variants.find((variant) => matches(variant, selection)) ?? null;
}

export function isValueAvailable(
	product: Product,
	key: string,
	value: string,
	selection: Record<string, string> = {}
): boolean {
	const variants = variantStock(product);
	if (!variants) return true;
	return variants.some(
		(variant) =>
			variant.in_stock > 0 && variant[key] === value && matches(variant, selection, key)
	);
}

// A variant answers only for a complete selection. Anything less falls back to
// the product's own stock, capped by the plugin's registered device count.
export function effectiveStock(product: Product, selection: Record<string, string> = {}): number {
	const variants = variantStock(product);
	if (variants && missingSelectFields(product, selection).length === 0) {
		return Math.min(resolveVariant(product, selection)?.in_stock ?? 0, product.stock);
	}
	const registered = product.plugin_data?.[DEVICES_IN_STOCK];
	return typeof registered === 'number' ? Math.min(product.stock, registered) : product.stock;
}

export function optionLabel(schema: OptionField[], selection: Record<string, string>): string {
	return schema
		.map((field) => {
			const value = selection[field.key];
			return value ? `${field.label}: ${value}` : '';
		})
		.filter(Boolean)
		.join(' · ');
}
