class Toast {
	message = $state('');
	href = $state('');
	hrefLabel = $state('View bag');
	private timer: ReturnType<typeof setTimeout> | null = null;

	show(message: string, href = '', hrefLabel = 'View bag'): void {
		this.message = message;
		this.href = href;
		this.hrefLabel = hrefLabel;
		if (this.timer) clearTimeout(this.timer);
		this.timer = setTimeout(() => this.hide(), 4500);
	}

	hide(): void {
		this.message = '';
		this.href = '';
		if (this.timer) {
			clearTimeout(this.timer);
			this.timer = null;
		}
	}
}

export const toast = new Toast();
