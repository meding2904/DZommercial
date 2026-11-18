import type { ThemePreset } from '#lib/types';

export type AdminScheme = 'light' | 'dark';

export type PresetDefinition = {
	id: ThemePreset;
	name: string;
	/** The scheme shown when the device has no saved choice. */
	defaultScheme: AdminScheme;
	/** Ground colours for the browser chrome and the reveal. The CSS carries every other token. */
	background: Record<AdminScheme, string>;
};

// Four colour styles. Each style has its own light and dark token set in
// admin.css; this list only names the styles, their default scheme and the
// ground colour used by theme-color and the reveal.
export const PRESETS: PresetDefinition[] = [
	{
		id: 'plaster',
		name: 'Plaster',
		defaultScheme: 'light',
		background: { light: '#f6f0eb', dark: '#171311' }
	},
	{
		id: 'cellar',
		name: 'Cellar',
		defaultScheme: 'dark',
		background: { light: '#f5f0e2', dark: '#151a16' }
	},
	{
		id: 'terracotta',
		name: 'Terracotta',
		defaultScheme: 'light',
		background: { light: '#f2e5d8', dark: '#1b1210' }
	},
	{
		id: 'lagoon',
		name: 'Lagoon',
		defaultScheme: 'dark',
		background: { light: '#eef5f4', dark: '#0e1d23' }
	}
];

export const DEFAULT_PRESET: ThemePreset = 'cellar';
export const PRESET_STORAGE_KEY = 'azahar-admin-preset';
export const SCHEME_STORAGE_KEY = 'azahar-admin-scheme';

export function isPreset(value: unknown): value is ThemePreset {
	return typeof value === 'string' && PRESETS.some((preset) => preset.id === value);
}

export function readStoredPreset(): ThemePreset {
	try {
		if (typeof localStorage === 'undefined') return DEFAULT_PRESET;
		const stored = localStorage.getItem(PRESET_STORAGE_KEY);
		return isPreset(stored) ? stored : DEFAULT_PRESET;
	} catch {
		return DEFAULT_PRESET;
	}
}

export function writeStoredPreset(preset: ThemePreset): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(PRESET_STORAGE_KEY, preset);
	} catch {
		// Storage can be unavailable in private browsing. The in-memory choice still applies.
	}
}

/** The saved scheme, or null when the style's default scheme should apply. */
export function readStoredScheme(): AdminScheme | null {
	try {
		if (typeof localStorage === 'undefined') return null;
		const stored = localStorage.getItem(SCHEME_STORAGE_KEY);
		return stored === 'light' || stored === 'dark' ? stored : null;
	} catch {
		return null;
	}
}

export function writeStoredScheme(scheme: AdminScheme): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(SCHEME_STORAGE_KEY, scheme);
	} catch {
		// Storage can be unavailable in private browsing. The in-memory choice still applies.
	}
}
