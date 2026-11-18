import { browser } from '$app/env';
import { revealTheme } from '#lib/theme/reveal';

export type ThemeMode = 'light' | 'dark';

const STORAGE_KEY = 'azahar-store-theme';
const BACKGROUND: Record<ThemeMode, string> = {
	light: '#f6f0eb',
	dark: '#171311'
};

function readInitial(): ThemeMode {
	if (!browser) return 'light';

	const attribute = document.documentElement.getAttribute('data-store-theme');
	if (attribute === 'dark' || attribute === 'light') return attribute as ThemeMode;

	try {
		const saved = localStorage.getItem(STORAGE_KEY);
		if (saved === 'dark' || saved === 'light') return saved as ThemeMode;
	} catch {
		// Storage can be unavailable in private modes.
	}

	return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

class StoreTheme {
	current = $state<ThemeMode>('light');
	busy = $state(false);

	constructor() {
		if (!browser) return;
		this.current = readInitial();
		this.apply(this.current, false);
	}

	private apply(theme: ThemeMode, persist = true): void {
		document.documentElement.setAttribute('data-store-theme', theme);
		document
			.querySelector('meta[name="theme-color"]')
			?.setAttribute('content', BACKGROUND[theme]);

		if (persist) {
			try {
				localStorage.setItem(STORAGE_KEY, theme);
			} catch {
				// The visual state still changes if storage is unavailable.
			}
		}
	}

	private commit(theme: ThemeMode): void {
		this.current = theme;
		this.apply(theme);
	}

	toggle(button: HTMLElement): void {
		if (this.busy) return;

		const next: ThemeMode = this.current === 'dark' ? 'light' : 'dark';
		revealTheme(
			button,
			BACKGROUND[next],
			() => this.commit(next),
			(busy) => (this.busy = busy)
		);
	}
}

export const storeTheme = new StoreTheme();
