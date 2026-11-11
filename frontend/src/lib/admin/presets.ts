import type { ThemePreset } from '#lib/types';

export type PresetTokens = {
	bg: string;
	surface: string;
	ink: string;
	muted: string;
	accent: string;
	accentInk: string;
	chart: [string, string, string, string];
	status: {
		pending: string;
		confirmed: string;
		packed: string;
		shipped: string;
		delivered: string;
		cancelled: string;
	};
	dangerText: string;
};

export type PresetDefinition = {
	id: ThemePreset;
	name: string;
	scheme: 'light' | 'dark';
	tokens: PresetTokens;
};

// These mirror the custom properties in admin.css, which is the runtime source.
export const PRESETS: PresetDefinition[] = [
	{
		id: 'plaster',
		name: 'Plaster',
		scheme: 'light',
		tokens: {
			bg: '#f6f0eb',
			surface: '#ebe0d9',
			ink: '#2c2325',
			muted: '#6a5d5e',
			accent: '#9a3c4c',
			accentInk: '#ffffff',
			chart: ['#9a3c4c', '#476b5a', '#b07a4a', '#6a5d5e'],
			status: {
				pending: '#8a5a2b',
				confirmed: '#476b5a',
				packed: '#6a5d5e',
				shipped: '#7a4a2f',
				delivered: '#3f5f4b',
				cancelled: '#9a3c4c'
			},
			dangerText: '#852f3f'
		}
	},
	{
		id: 'cellar',
		name: 'Cellar',
		scheme: 'dark',
		tokens: {
			bg: '#151a16',
			surface: '#1d241e',
			ink: '#f1ece1',
			muted: '#a3ab9f',
			accent: '#d8a44c',
			accentInk: '#1a1408',
			chart: ['#d8a44c', '#8fae8b', '#c46a4f', '#d8d2c4'],
			status: {
				pending: '#d8a44c',
				confirmed: '#8fae8b',
				packed: '#a3ab9f',
				shipped: '#c46a4f',
				delivered: '#74b58a',
				cancelled: '#d98a8a'
			},
			dangerText: '#eda3a3'
		}
	},
	{
		id: 'terracotta',
		name: 'Terracotta',
		scheme: 'light',
		tokens: {
			bg: '#f2e5d8',
			surface: '#e5d1bf',
			ink: '#2b1d18',
			muted: '#6e554a',
			accent: '#a94e2b',
			accentInk: '#ffffff',
			chart: ['#a94e2b', '#4f6b57', '#c08a4e', '#6e554a'],
			status: {
				pending: '#8a5a29',
				confirmed: '#4f6b57',
				packed: '#6e554a',
				shipped: '#8f4a2f',
				delivered: '#3f5f4b',
				cancelled: '#a94e2b'
			},
			dangerText: '#8a341e'
		}
	},
	{
		id: 'lagoon',
		name: 'Lagoon',
		scheme: 'dark',
		tokens: {
			bg: '#0e1d23',
			surface: '#152b33',
			ink: '#e9f1ef',
			muted: '#9ab0b1',
			accent: '#5fb7b2',
			accentInk: '#06201f',
			chart: ['#5fb7b2', '#d8a44c', '#c46a4f', '#d8e3de'],
			status: {
				pending: '#d8a44c',
				confirmed: '#8fae8b',
				packed: '#9ab0b1',
				shipped: '#c46a4f',
				delivered: '#74b58a',
				cancelled: '#d98a8a'
			},
			dangerText: '#f0b0ae'
		}
	}
];

export const DEFAULT_PRESET: ThemePreset = 'cellar';
export const PRESET_STORAGE_KEY = 'azahar-admin-preset';

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
