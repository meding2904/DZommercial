export type Palette = {
	ground: string;
	surface: string;
	ink: string;
	muted: string;
	accent: string;
	accent_ink: string;
};

export type StorefrontTheme = {
	brand_name: string;
	tagline: string;
	description: string;
	hero_heading: string;
	hero_body: string;
	hero_alt: string;
	hero_cta: string;
	hero_image: string;
	statement: string;
	shelf_heading: string;
	shop_title: string;
	house_label: string;
	about_heading: string;
	about_body: string;
	about_image: string;
	about_alt: string;
	closing_heading: string;
	footer_note: string;
	footer_credit: string;
	contact_email: string;
	announcement: string;
	concerns_enabled: boolean;
	font_display: FontId;
	font_body: FontId;
	font_scale: number;
	radius: number;
	light: Palette;
	dark: Palette;
	updated_at?: string;
};

export type FontId = 'schibsted' | 'source_serif' | 'fraunces' | 'inter' | 'system';
export type FontKind = 'display' | 'body';

export type StorefrontStyle = {
	id: string;
	name: string;
	description: string;
	font_display: FontId;
	font_body: FontId;
	radius: number;
	light: Palette;
	dark: Palette;
};

export type FontChoice = {
	id: FontId;
	label: string;
	stack: string;
	kinds: FontKind[];
};

export const FONT_CHOICES: FontChoice[] = [
	{
		id: 'schibsted',
		label: 'Schibsted Grotesk',
		stack: "'Schibsted Grotesk Variable', 'Schibsted Grotesk', system-ui, sans-serif",
		kinds: ['display', 'body']
	},
	{
		id: 'source_serif',
		label: 'Source Serif 4',
		stack: "'Source Serif 4 Variable', 'Source Serif 4', Georgia, serif",
		kinds: ['display', 'body']
	},
	{
		id: 'fraunces',
		label: 'Fraunces',
		stack: "'Fraunces Variable', 'Fraunces', Georgia, serif",
		kinds: ['display']
	},
	{
		id: 'inter',
		label: 'Inter',
		stack: "'Inter Variable', 'Inter', system-ui, sans-serif",
		kinds: ['body']
	},
	{
		id: 'system',
		label: 'System sans',
		stack: "system-ui, -apple-system, 'Segoe UI', sans-serif",
		kinds: ['display', 'body']
	}
];

export const DEFAULT_THEME: StorefrontTheme = {
	brand_name: 'Azahar',
	tagline: 'Botanical skincare from the Mediterranean edge.',
	description: 'Oils, clays and blossoms, blended in small batches in Algiers.',
	hero_heading: 'Botanical skincare from the Mediterranean edge.',
	hero_body:
		'Oils, clays and blossoms, blended in small batches in Algiers. Eight formulas for the face, the body and the bath.',
	hero_alt: 'Amber glass bottles and a linen cloth on a plaster shelf',
	hero_cta: 'Shop the shelf',
	hero_image: '/images/hero-1600.webp',
	statement:
		'We make fewer things, with more of the plant in them. The shelf is short on purpose.',
	shelf_heading: 'Eight formulas for face, body and the bath.',
	shop_title: 'The shelf',
	house_label: 'The house',
	about_heading: 'A small shelf in Algiers.',
	about_body:
		'Azahar began with the orange blossom that scents the city in spring. We press olives and figs, mix clays with rose, and pour everything into amber glass. The shelf is short on purpose.',
	about_image: '/images/atelier-1200.webp',
	about_alt: 'Amber bottles, ceramic jars, linen and dried botanicals on a work table',
	closing_heading: 'Take the whole shelf, or one jar.',
	footer_note:
		'Botanical skincare from the Mediterranean edge. Oils, clays and blossoms, blended in small batches in Algiers.',
	footer_credit: 'Photography from Pexels. Products, prices and story are sample data.',
	contact_email: 'hello@azahar.example',
	announcement: 'Cash on delivery across Algeria.',
	concerns_enabled: true,
	font_display: 'schibsted',
	font_body: 'schibsted',
	font_scale: 1,
	radius: 4,
	light: {
		ground: '#f6f0eb',
		surface: '#ebe0d9',
		ink: '#2c2325',
		muted: '#6a5d5e',
		accent: '#9a3c4c',
		accent_ink: '#ffffff'
	},
	dark: {
		ground: '#171311',
		surface: '#221b18',
		ink: '#f4ece5',
		muted: '#b9aca4',
		accent: '#e0a0a8',
		accent_ink: '#241316'
	}
};

export const STOREFRONT_STYLES: StorefrontStyle[] = [
	{
		id: 'plaster',
		name: 'Plaster',
		description: 'Warm plaster, deep wine and a quiet shelf.',
		font_display: DEFAULT_THEME.font_display,
		font_body: DEFAULT_THEME.font_body,
		radius: DEFAULT_THEME.radius,
		light: { ...DEFAULT_THEME.light },
		dark: { ...DEFAULT_THEME.dark }
	},
	{
		id: 'noir',
		name: 'Noir',
		description: 'Editorial ink, bone paper and a wine accent.',
		font_display: 'fraunces',
		font_body: 'inter',
		radius: 2,
		light: {
			ground: '#f4f1ea',
			surface: '#e7e0d5',
			ink: '#1d1a17',
			muted: '#6b6259',
			accent: '#7a2f35',
			accent_ink: '#ffffff'
		},
		dark: {
			ground: '#141210',
			surface: '#211d1a',
			ink: '#f2ece4',
			muted: '#b9ada1',
			accent: '#d8a7a0',
			accent_ink: '#1f1414'
		}
	},
	{
		id: 'olive',
		name: 'Olive',
		description: 'Green-gray paper and a botanical olive accent.',
		font_display: 'source_serif',
		font_body: 'schibsted',
		radius: 6,
		light: {
			ground: '#eef0e4',
			surface: '#e0e4d2',
			ink: '#2a2e22',
			muted: '#62684f',
			accent: '#56633a',
			accent_ink: '#ffffff'
		},
		dark: {
			ground: '#151810',
			surface: '#20251a',
			ink: '#edf0e2',
			muted: '#aeb59e',
			accent: '#b7c68a',
			accent_ink: '#1b2113'
		}
	},
	{
		id: 'tidal',
		name: 'Tidal',
		description: 'Cool mineral paper and a deep sea accent.',
		font_display: 'schibsted',
		font_body: 'inter',
		radius: 8,
		light: {
			ground: '#edf3f2',
			surface: '#dbe7e5',
			ink: '#17302e',
			muted: '#5a6f6c',
			accent: '#1f6a63',
			accent_ink: '#ffffff'
		},
		dark: {
			ground: '#0c1a19',
			surface: '#142827',
			ink: '#e7f1ef',
			muted: '#a4bcb8',
			accent: '#77c9c0',
			accent_ink: '#08201d'
		}
	},
	{
		id: 'clay',
		name: 'Clay',
		description: 'Terracotta warmth on a soft linen ground.',
		font_display: 'fraunces',
		font_body: 'source_serif',
		radius: 10,
		light: {
			ground: '#f6efe7',
			surface: '#eadccd',
			ink: '#33261e',
			muted: '#746153',
			accent: '#8a4c2f',
			accent_ink: '#ffffff'
		},
		dark: {
			ground: '#1a1210',
			surface: '#271b17',
			ink: '#f2e8df',
			muted: '#bda99a',
			accent: '#d99a76',
			accent_ink: '#24120b'
		}
	}
];

export function fontStack(id: FontId, kind: FontKind): string {
	const choice = FONT_CHOICES.find((item) => item.id === id && item.kinds.includes(kind));
	if (choice) return choice.stack;
	const fallback = FONT_CHOICES.find((item) => item.id === 'schibsted');
	return fallback?.stack ?? 'system-ui, sans-serif';
}

export function fontsFor(kind: FontKind): FontChoice[] {
	return FONT_CHOICES.filter((item) => item.kinds.includes(kind));
}

export function themeCss(theme: StorefrontTheme): string {
	const scale = clampNumber(theme.font_scale, 0.85, 1.15);
	const radius = Math.round(clampNumber(theme.radius, 0, 12));
	const color = (value: string, fallback: string): string =>
		/^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : fallback;
	const light = {
		ground: color(theme.light.ground, DEFAULT_THEME.light.ground),
		surface: color(theme.light.surface, DEFAULT_THEME.light.surface),
		ink: color(theme.light.ink, DEFAULT_THEME.light.ink),
		muted: color(theme.light.muted, DEFAULT_THEME.light.muted),
		accent: color(theme.light.accent, DEFAULT_THEME.light.accent),
		accent_ink: color(theme.light.accent_ink, DEFAULT_THEME.light.accent_ink)
	};
	const dark = {
		ground: color(theme.dark.ground, DEFAULT_THEME.dark.ground),
		surface: color(theme.dark.surface, DEFAULT_THEME.dark.surface),
		ink: color(theme.dark.ink, DEFAULT_THEME.dark.ink),
		muted: color(theme.dark.muted, DEFAULT_THEME.dark.muted),
		accent: color(theme.dark.accent, DEFAULT_THEME.dark.accent),
		accent_ink: color(theme.dark.accent_ink, DEFAULT_THEME.dark.accent_ink)
	};
	const display = fontStack(theme.font_display, 'display');
	const body = fontStack(theme.font_body, 'body');

	// This string is injected into the document head as a style element. Every
	// value above is allowlisted or numeric, so no theme text reaches the CSS.
	return `
:root:root {
  color-scheme: light;
  --p-bg: ${light.ground};
  --p-surface: ${light.surface};
  --p-ink: ${light.ink};
  --p-muted: ${light.muted};
  --p-accent: ${light.accent};
  --p-accent-ink: ${light.accent_ink};
  --theme-light-bg: ${light.ground};
  --theme-dark-bg: ${dark.ground};
  --font-display: ${display};
  --font-text: ${body};
  --text-base: ${(16 * scale).toFixed(2)}px;
  --radius-sm: ${Math.max(0, radius - 2)}px;
  --radius: ${radius}px;
  --radius-lg: ${Math.min(12, radius + 2)}px;
}
:root[data-store-theme='dark'][data-store-theme='dark'] {
  color-scheme: dark;
  --p-bg: ${dark.ground};
  --p-surface: ${dark.surface};
  --p-ink: ${dark.ink};
  --p-muted: ${dark.muted};
  --p-accent: ${dark.accent};
  --p-accent-ink: ${dark.accent_ink};
}
`.trim();
}

export function contrastRatio(foreground: string, background: string): number {
	const fg = luminance(foreground);
	const bg = luminance(background);
	if (fg === null || bg === null) return 0;
	const lighter = Math.max(fg, bg);
	const darker = Math.min(fg, bg);
	return (lighter + 0.05) / (darker + 0.05);
}

function luminance(value: string): number | null {
	if (!/^#[0-9a-f]{6}$/i.test(value)) return null;
	const number = Number.parseInt(value.slice(1), 16);
	const channel = (shift: number): number => {
		const raw = ((number >> shift) & 0xff) / 255;
		return raw <= 0.04045 ? raw / 12.92 : ((raw + 0.055) / 1.055) ** 2.4;
	};
	return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0);
}

export function clampNumber(value: number, min: number, max: number): number {
	if (!Number.isFinite(value)) return min;
	return Math.min(max, Math.max(min, value));
}
