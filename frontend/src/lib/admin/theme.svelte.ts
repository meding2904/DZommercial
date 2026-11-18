import type { ThemePreset } from '#lib/types';
import { revealTheme } from '#lib/theme/reveal';
import { adminApi } from './api';
import {
	PRESETS,
	isPreset,
	readStoredPreset,
	readStoredScheme,
	writeStoredPreset,
	writeStoredScheme,
	type AdminScheme
} from './presets';

const initialPreset = readStoredPreset();
const initialScheme =
	readStoredScheme() ?? PRESETS.find((item) => item.id === initialPreset)?.defaultScheme ?? 'light';
// Persist the first scheme so it stays stable when the style changes later.
writeStoredScheme(initialScheme);

class ThemeStore {
	preset = $state<ThemePreset>(initialPreset);
	scheme = $state<AdminScheme>(initialScheme);
	saveError = $state('');
	busy = $state(false);
	private version = 0;

	toggleScheme(button: HTMLElement): void {
		if (this.busy) return;

		const definition = PRESETS.find((item) => item.id === this.preset);
		if (!definition) return;

		const next: AdminScheme = this.scheme === 'dark' ? 'light' : 'dark';
		revealTheme(
			button,
			definition.background[next],
			() => {
				this.scheme = next;
				writeStoredScheme(next);
			},
			(busy) => (this.busy = busy)
		);
	}

	async loadRemote(): Promise<void> {
		const version = this.version;
		try {
			const data = await adminApi.preferences();
			if (this.version !== version) return;
			if (isPreset(data.theme_preset)) {
				this.preset = data.theme_preset;
				writeStoredPreset(data.theme_preset);
			}
		} catch {
			// Keep the local choice when the preference cannot be read.
		}
	}

	async choose(next: ThemePreset): Promise<void> {
		if (!isPreset(next)) return;
		const version = ++this.version;
		this.preset = next;
		this.saveError = '';
		writeStoredPreset(next);
		try {
			await adminApi.savePreferences(next);
			if (version === this.version) this.saveError = '';
		} catch {
			if (version === this.version) {
				this.saveError = 'Saved on this device. The server did not take the change. Try again to sync it.';
			}
		}
	}
}

export const theme = new ThemeStore();

/** Match the browser chrome to the current style and scheme. */
export function syncThemeColor(preset: ThemePreset, scheme: AdminScheme): void {
	if (typeof document === 'undefined') return;
	const definition = PRESETS.find((item) => item.id === preset);
	if (!definition) return;
	document
		.querySelector('meta[name="theme-color"]')
		?.setAttribute('content', definition.background[scheme]);
}
