import { deliveryTypeLabel, money, titleCase } from '../format.ts';
import type { AdminOrder } from './types';

function escapeHtml(value: unknown): string {
	return String(value ?? '').replace(/[&<>"']/g, (character) => {
		return {
			'&': '&amp;',
			'<': '&lt;',
			'>': '&gt;',
			'"': '&quot;',
			"'": '&#39;'
		}[character]!;
	});
}

function formatDateTime(value: string | null | undefined): string {
	if (!value) return 'No date';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return value;
	return new Intl.DateTimeFormat('en-GB', {
		day: '2-digit',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit'
	}).format(date);
}

function placeLine(order: AdminOrder): string {
	return [order.commune_name || order.city, order.wilaya_name].filter(Boolean).join(', ');
}

export function receiptHtml(brandName: string, order: AdminOrder): string {
	const items = order.items
		.map(
			(item) => `<tr>
				<td>${escapeHtml(item.name)}${
					item.options_label
						? `<span class="muted">${escapeHtml(item.options_label)}</span>`
						: ''
				}</td>
				<td class="right">${item.quantity}</td>
				<td class="right mono">${escapeHtml(money(item.unit_price_cents, order.currency))}</td>
				<td class="right mono">${escapeHtml(money(item.line_total_cents, order.currency))}</td>
			</tr>`
		)
		.join('');
	const paid = order.delivery_type === 'pickup' || order.status === 'delivered';
	const place = placeLine(order);
	return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<title>Receipt ${escapeHtml(order.number)}</title>
<style>
  * { box-sizing: border-box; }
  body { margin: 0; padding: 28px; color: #171311; background: #fff; font: 13px/1.45 ui-sans-serif, system-ui, sans-serif; }
  .receipt { width: 100%; max-width: 700px; margin: 0 auto; }
  .head { display: flex; justify-content: space-between; gap: 24px; padding-bottom: 16px; border-bottom: 2px solid #171311; }
  .brand { font-size: 26px; font-weight: 650; letter-spacing: -0.04em; line-height: 1; }
  .brand span { color: #9a3c4c; }
  .label { margin: 0; color: #6a5d5e; font: 10px/1.2 ui-monospace, monospace; letter-spacing: 0.04em; text-transform: uppercase; }
  .number { margin: 4px 0 2px; font: 17px/1.2 ui-monospace, monospace; }
  .muted { color: #6a5d5e; }
  .parties { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 18px; padding: 16px 0; border-bottom: 1px solid #cfc6bd; }
  .parties p { margin: 0; }
  .parties section { display: grid; gap: 2px; align-content: start; }
  table { width: 100%; border-collapse: collapse; }
  th { padding: 14px 6px 6px; border-bottom: 1px solid #171311; color: #6a5d5e; font: 10px/1.2 ui-monospace, monospace; font-weight: 400; text-align: left; }
  td { padding: 7px 6px; border-bottom: 1px solid #e2dad2; vertical-align: top; }
  th:first-child, td:first-child { padding-left: 0; }
  th:last-child, td:last-child { padding-right: 0; }
  .right { text-align: right; }
  .mono { font-family: ui-monospace, monospace; font-variant-numeric: tabular-nums; }
  .totals { display: grid; gap: 5px; width: 250px; max-width: 100%; margin: 12px 0 0 auto; }
  .totals div { display: flex; justify-content: space-between; gap: 14px; color: #6a5d5e; }
  .totals .final { padding-top: 7px; border-top: 1px solid #171311; color: #171311; font: 16px/1.2 ui-monospace, monospace; }
  .payment { margin-top: 16px; padding-top: 10px; border-top: 1px dashed #9d9186; }
  .foot { margin-top: 22px; padding-top: 12px; border-top: 1px solid #cfc6bd; color: #6a5d5e; font-size: 11px; }
  .foot p { margin: 0 0 3px; }
  @media print { @page { margin: 12mm; } body { padding: 0; } }
</style>
</head>
<body>
<div class="receipt">
  <div class="head">
    <div><p class="brand">${escapeHtml(brandName)}<span>.</span></p><p class="muted">${escapeHtml(deliveryTypeLabel(order.delivery_type))}</p></div>
    <div style="text-align:right"><p class="label">Receipt</p><p class="number">${escapeHtml(order.number)}</p><p class="muted">${escapeHtml(formatDateTime(order.created_at))}</p></div>
  </div>
  <div class="parties">
    <section><p class="label">Customer</p><p>${escapeHtml(order.name)}</p><p class="muted">${escapeHtml(order.email || 'No email')}</p><p class="muted">${escapeHtml(order.phone || 'No phone')}</p></section>
    <section><p class="label">Delivery</p><p>${escapeHtml(deliveryTypeLabel(order.delivery_type))}</p>${
			order.stop_desk_name ? `<p class="muted">${escapeHtml(order.stop_desk_name)}</p>` : ''
		}${order.address ? `<p class="muted">${escapeHtml(order.address)}</p>` : ''}${
			place ? `<p class="muted">${escapeHtml(place)}</p>` : ''
		}</section>
    <section><p class="label">Status</p><p>${escapeHtml(titleCase(order.status))}</p><p class="muted">${escapeHtml(formatDateTime(order.created_at))}</p></section>
  </div>
  <table>
    <thead><tr><th>Item</th><th class="right">Qty</th><th class="right">Unit</th><th class="right">Line total</th></tr></thead>
    <tbody>${items}</tbody>
  </table>
  <div class="totals">
    <div><span>Subtotal</span><span>${escapeHtml(money(order.subtotal_cents, order.currency))}</span></div>
    <div><span>Shipping</span><span>${escapeHtml(order.shipping_cents === 0 ? 'Free' : money(order.shipping_cents, order.currency))}</span></div>
    <div class="final"><span>Total</span><span>${escapeHtml(money(order.total_cents, order.currency))}</span></div>
  </div>
  <p class="payment">${paid ? 'Paid in cash.' : 'Cash on delivery. The courier collects the total.'}</p>
  ${order.notes ? `<p class="muted">Note: ${escapeHtml(order.notes)}</p>` : ''}
  <div class="foot"><p>${escapeHtml(brandName)} receipt.</p><p>Keep this receipt for the order and any courier claim.</p><p>This is a demonstration shop. No binding sale is created.</p></div>
</div>
</body>
</html>`;
}

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
