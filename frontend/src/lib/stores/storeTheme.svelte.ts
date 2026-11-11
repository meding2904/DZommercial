import { browser } from '$app/env';

export type ThemeMode = 'light' | 'dark';

const STORAGE_KEY = 'azahar-store-theme';
const BACKGROUND: Record<ThemeMode, string> = {
	light: '#f6f0eb',
	dark: '#171311'
};

type ViewTransitionDocument = Document & {
	startViewTransition?: (callback: () => void) => {
		ready: Promise<void>;
		finished: Promise<void>;
	};
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
		const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

		if (reduce) {
			this.commit(next);
			return;
		}

		const rect = button.getBoundingClientRect();
		const x = rect.left + rect.width / 2;
		const y = rect.top + rect.height / 2;
		const radius =
			Math.hypot(
				Math.max(x, window.innerWidth - x),
				Math.max(y, window.innerHeight - y)
			) + 2;

		const doc = document as ViewTransitionDocument;

		if (typeof doc.startViewTransition === 'function') {
			this.busy = true;

			let transition: ReturnType<NonNullable<ViewTransitionDocument['startViewTransition']>>;
			try {
				transition = doc.startViewTransition(() => this.commit(next));
			} catch {
				this.busy = false;
				this.solidWipe(next, x, y, radius);
				return;
			}

			transition.ready
				.then(() => {
					document.documentElement.animate(
						[
							{ clipPath: `circle(0px at ${x}px ${y}px)` },
							{ clipPath: `circle(${radius}px at ${x}px ${y}px)` }
						],
						{
							duration: 640,
							easing: 'cubic-bezier(0.2, 0.7, 0.2, 1)',
							pseudoElement: '::view-transition-new(root)'
						} as KeyframeAnimationOptions
					);
				})
				.catch(() => {
					// The browser can skip a transition when the document is hidden.
				});

			transition.finished.finally(() => {
				this.busy = false;
			});

			return;
		}

		this.solidWipe(next, x, y, radius);
	}

	private solidWipe(next: ThemeMode, x: number, y: number, radius: number): void {
		const wipe = document.createElement('div');
		wipe.className = 'theme-wipe';
		wipe.setAttribute('aria-hidden', 'true');
		wipe.style.background = BACKGROUND[next];
		wipe.style.clipPath = `circle(0px at ${x}px ${y}px)`;
		document.body.appendChild(wipe);

		this.busy = true;

		const animation = wipe.animate(
			[
				{ clipPath: `circle(0px at ${x}px ${y}px)` },
				{ clipPath: `circle(${radius}px at ${x}px ${y}px)` }
			],
			{
				duration: 640,
				easing: 'cubic-bezier(0.2, 0.7, 0.2, 1)',
				fill: 'forwards'
			}
		);

		animation.onfinish = () => {
			this.commit(next);
			requestAnimationFrame(() => {
				wipe.remove();
				this.busy = false;
			});
		};

		animation.oncancel = () => {
			wipe.remove();
			this.busy = false;
		};
	}
}

export const storeTheme = new StoreTheme();
