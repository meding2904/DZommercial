import type { ThemePreset } from '#lib/types';
import { adminApi } from './api';
import { isPreset, readStoredPreset, writeStoredPreset } from './presets';

class ThemeStore {
	preset = $state<ThemePreset>(readStoredPreset());
	saveError = $state('');
	private version = 0;

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
