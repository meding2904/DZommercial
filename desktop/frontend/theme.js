// Theme application and the rail controls. The palettes live in presets.js; the
// saved preference is mirrored to the server once the admin is signed in.

import { PRESET_TOKENS, DEFAULT_PRESET, PRESET_KEY, SCHEME_KEY } from './presets.js';
import { state } from './state.js';
import { call } from './api.js';
import { toast } from './toast.js';

export function applyTheme() {
	const preset = PRESET_TOKENS[state.preset] ? state.preset : DEFAULT_PRESET;
	const tokens = PRESET_TOKENS[preset][state.scheme === 'light' ? 'light' : 'dark'];
	const root = document.documentElement;
	root.dataset.preset = preset;
	root.dataset.scheme = state.scheme;
	const map = {
		bg: '--a-bg',
		surface: '--a-surface',
		ink: '--a-ink',
		muted: '--a-muted',
		accent: '--a-accent',
		accentInk: '--a-accent-ink',
		chart1: '--a-chart-1',
		chart2: '--a-chart-2',
		chart3: '--a-chart-3',
		chart4: '--a-chart-4',
		pending: '--a-status-pending',
		confirmed: '--a-status-confirmed',
		packed: '--a-status-packed',
		shipped: '--a-status-shipped',
		delivered: '--a-status-delivered',
		cancelled: '--a-status-cancelled',
		danger: '--a-danger-text'
	};
	for (const [key, cssVar] of Object.entries(map)) root.style.setProperty(cssVar, tokens[key]);
	const select = document.getElementById('preset-select');
	if (select) select.value = preset;
	const label = document.getElementById('scheme-label');
	if (label) label.textContent = state.scheme === 'dark' ? 'Dark' : 'Light';
	const toggle = document.getElementById('scheme-toggle');
	if (toggle) {
		toggle.setAttribute('aria-label', `Switch to ${state.scheme === 'dark' ? 'light' : 'dark'} mode`);
	}
}

/**
 * @param {string} preset
 */
export async function savePreset(preset) {
	state.preset = preset;
	localStorage.setItem(PRESET_KEY, preset);
	applyTheme();
	if (!state.user) return;
	try {
		await call('PUT', '/api/admin/preferences', { theme_preset: preset });
	} catch (error) {
		toast(error.message || 'The style could not be saved.', 'error');
	}
}

export async function loadPreferences() {
	try {
		const data = await call('GET', '/api/admin/preferences');
		if (data && PRESET_TOKENS[data.theme_preset]) {
			state.preset = data.theme_preset;
			localStorage.setItem(PRESET_KEY, state.preset);
		}
	} catch {
		/* the saved local style is a good enough fallback */
	}
}

export function toggleScheme() {
	state.scheme = state.scheme === 'dark' ? 'light' : 'dark';
	localStorage.setItem(SCHEME_KEY, state.scheme);
	applyTheme();
}

export function initThemeControls() {
	document.getElementById('scheme-toggle').addEventListener('click', toggleScheme);
	document.getElementById('preset-select').addEventListener('change', (event) => {
		void savePreset(event.target.value);
	});
}
