export type ProductImageChoice = {
	path: string;
	label: string;
};

export const PRODUCT_IMAGES: ProductImageChoice[] = [
	{ path: '/images/products/cleansing-oil-1200.webp', label: 'Cleansing oil' },
	{ path: '/images/products/serum-1200.webp', label: 'Face serum' },
	{ path: '/images/products/clay-mask-1200.webp', label: 'Clay mask' },
	{ path: '/images/products/body-balm-1200.webp', label: 'Body balm' },
	{ path: '/images/products/prickly-pear-1200.webp', label: 'Prickly pear oil' },
	{ path: '/images/products/hand-cream-1200.webp', label: 'Hand cream' },
	{ path: '/images/products/fig-oil-1200.webp', label: 'Body oil' },
	{ path: '/images/products/rose-soak-1200.webp', label: 'Bath soak' }
];

export function imageLabel(path: string): string {
	return PRODUCT_IMAGES.find((image) => image.path === path)?.label ?? 'Existing image';
}
