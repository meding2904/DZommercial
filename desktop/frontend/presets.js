// Theme palettes and their storage keys. The admin picks one of these presets
// in the rail; applyTheme in theme.js maps the tokens onto CSS variables.

export const PRESET_TOKENS = {
	plaster: {
		light: {
			bg: '#f6f0eb', surface: '#ebe0d9', ink: '#2c2325', muted: '#6a5d5e',
			accent: '#9a3c4c', accentInk: '#ffffff', chart1: '#9a3c4c', chart2: '#476b5a',
			chart3: '#8a5a33', chart4: '#6a5d5e', pending: '#8a5a2b', confirmed: '#476b5a',
			packed: '#6a5d5e', shipped: '#7a4a2f', delivered: '#3f5f4b', cancelled: '#9a3c4c',
			danger: '#852f3f'
		},
		dark: {
			bg: '#171311', surface: '#221b18', ink: '#f4ece5', muted: '#b9aca4',
			accent: '#e0a0a8', accentInk: '#241316', chart1: '#e0a0a8', chart2: '#8fae8b',
			chart3: '#d8a44c', chart4: '#b9aca4', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#b9aca4', shipped: '#c46a4f', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#f0b0ae'
		}
	},
	cellar: {
		light: {
			bg: '#f5f0e2', surface: '#e9e1cd', ink: '#26271d', muted: '#5f6353',
			accent: '#8a5a12', accentInk: '#ffffff', chart1: '#8a5a12', chart2: '#4f6b57',
			chart3: '#a45a3a', chart4: '#6e6a5e', pending: '#8a5a12', confirmed: '#4f6b57',
			packed: '#6e6a5e', shipped: '#9a4a2f', delivered: '#3f5f4b', cancelled: '#9a3c4c',
			danger: '#7f3415'
		},
		dark: {
			bg: '#151a16', surface: '#1d241e', ink: '#f1ece1', muted: '#a3ab9f',
			accent: '#d8a44c', accentInk: '#1a1408', chart1: '#d8a44c', chart2: '#8fae8b',
			chart3: '#c46a4f', chart4: '#d8d2c4', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#a3ab9f', shipped: '#c46a4f', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#eda3a3'
		}
	},
	terracotta: {
		light: {
			bg: '#f2e5d8', surface: '#e5d1bf', ink: '#2b1d18', muted: '#6e554a',
			accent: '#a94e2b', accentInk: '#ffffff', chart1: '#a94e2b', chart2: '#4f6b57',
			chart3: '#8a5a33', chart4: '#6e554a', pending: '#8a5a29', confirmed: '#4f6b57',
			packed: '#6e554a', shipped: '#8f4a2f', delivered: '#3f5f4b', cancelled: '#a94e2b',
			danger: '#8a341e'
		},
		dark: {
			bg: '#1b1210', surface: '#271a16', ink: '#f3e7dd', muted: '#b9a396',
			accent: '#dc8a63', accentInk: '#2a120b', chart1: '#dc8a63', chart2: '#8fae8b',
			chart3: '#d8a44c', chart4: '#b9a396', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#b9a396', shipped: '#dc8a63', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#f0b0ae'
		}
	},
	lagoon: {
		light: {
			bg: '#eef5f4', surface: '#deebe9', ink: '#112528', muted: '#526b6c',
			accent: '#1d6f6b', accentInk: '#ffffff', chart1: '#1d6f6b', chart2: '#8a5a12',
			chart3: '#a45a3a', chart4: '#526b6c', pending: '#8a5a12', confirmed: '#3f5f4b',
			packed: '#526b6c', shipped: '#9a4a2f', delivered: '#2f6b4f', cancelled: '#9a3c4c',
			danger: '#7f3415'
		},
		dark: {
			bg: '#0e1d23', surface: '#152b33', ink: '#e9f1ef', muted: '#9ab0b1',
			accent: '#5fb7b2', accentInk: '#06201f', chart1: '#5fb7b2', chart2: '#d8a44c',
			chart3: '#c46a4f', chart4: '#d8e3de', pending: '#d8a44c', confirmed: '#8fae8b',
			packed: '#9ab0b1', shipped: '#c46a4f', delivered: '#74b58a', cancelled: '#d98a8a',
			danger: '#f0b0ae'
		}
	}
};

export const DEFAULT_PRESET = 'cellar';
export const PRESET_KEY = 'azahar.admin.preset';
export const SCHEME_KEY = 'azahar.admin.scheme';
