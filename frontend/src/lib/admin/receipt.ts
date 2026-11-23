export { receiptHtml } from './domain/receipts.js';

export function printReceiptHtml(html: string): void {
	if (typeof document === 'undefined') return;
	const frame = document.createElement('iframe');
	frame.setAttribute('aria-hidden', 'true');
	frame.style.cssText = 'position:fixed;right:0;bottom:0;width:0;height:0;border:0;';
	frame.srcdoc = html;
	frame.onload = () => {
		const target = frame.contentWindow;
		if (!target) {
			frame.remove();
			return;
		}
		target.focus();
		target.print();
		setTimeout(() => frame.remove(), 1000);
	};
	document.body.appendChild(frame);
}
