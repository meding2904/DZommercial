type ViewTransitionDocument = Document & {
	startViewTransition?: (callback: () => void) => {
		ready: Promise<void>;
		finished: Promise<void>;
	};
};

/**
 * Reveal a theme change from the toggle with the same circular motion the
 * storefront uses. The target background is the preset ground; the commit
 * applies the tokens. Browsers without the View Transitions API get a solid
 * ground wipe at the same origin and size.
 */
export function revealTheme(
	button: HTMLElement,
	background: string,
	commit: () => void,
	setBusy: (busy: boolean) => void
): void {
	if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
		commit();
		return;
	}

	const rect = button.getBoundingClientRect();
	const x = rect.left + rect.width / 2;
	const y = rect.top + rect.height / 2;
	const radius =
		Math.hypot(Math.max(x, window.innerWidth - x), Math.max(y, window.innerHeight - y)) + 2;
	const doc = document as ViewTransitionDocument;

	if (typeof doc.startViewTransition === 'function') {
		setBusy(true);

		let transition: ReturnType<NonNullable<ViewTransitionDocument['startViewTransition']>>;
		try {
			transition = doc.startViewTransition(commit);
		} catch {
			setBusy(false);
			solidWipe(background, x, y, radius, commit, setBusy);
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

		transition.finished.finally(() => setBusy(false));
		return;
	}

	solidWipe(background, x, y, radius, commit, setBusy);
}

function solidWipe(
	background: string,
	x: number,
	y: number,
	radius: number,
	commit: () => void,
	setBusy: (busy: boolean) => void
): void {
	const wipe = document.createElement('div');
	wipe.className = 'theme-wipe';
	wipe.setAttribute('aria-hidden', 'true');
	wipe.style.background = background;
	wipe.style.clipPath = `circle(0px at ${x}px ${y}px)`;
	document.body.appendChild(wipe);
	setBusy(true);

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
		commit();
		requestAnimationFrame(() => {
			wipe.remove();
			setBusy(false);
		});
	};

	animation.oncancel = () => {
		wipe.remove();
		setBusy(false);
	};
}
