<script lang="ts">
	import { deliveryTypeLabel, formatDate, money, titleCase } from '#lib/format';
	import { themeCss } from '#lib/theme/storefront';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const order = $derived(data.order);
	const theme = $derived(data.theme);
	const paid = $derived(order.delivery_type === 'pickup' || order.status === 'delivered');
	const placeLine = $derived.by(() => {
		const commune = order.commune_name || order.city;
		const wilaya = order.wilaya_name || (order.wilaya_code ? `Wilaya ${order.wilaya_code}` : '');
		return [commune, wilaya].filter(Boolean).join(', ');
	});
</script>

<svelte:head>
	<title>Receipt {order.number} | {theme.brand_name}</title>
	<meta
		name="description"
		content={`The receipt for order ${order.number}, with the items, total and delivery details.`}
	/>
	<meta name="robots" content="noindex" />
	{@html `<style>${themeCss(theme)}</style>`}
</svelte:head>

<main class="receipt-page">
	<article class="receipt" aria-labelledby="receipt-title">
		<header class="receipt-head">
			<div>
				<p class="wordmark">{theme.brand_name}<span>.</span></p>
				<p class="receipt-muted">{theme.tagline}</p>
			</div>
			<div class="receipt-head-right">
				<p class="receipt-label" id="receipt-title">Receipt</p>
				<p class="receipt-number">{order.number}</p>
				<p class="receipt-muted">{formatDate(order.created_at)}</p>
			</div>
		</header>

		<div class="receipt-parties">
			<section>
				<p class="receipt-label">Customer</p>
				<p>{order.name}</p>
				<p class="receipt-muted">{order.email}</p>
				<p class="receipt-muted">{order.phone}</p>
			</section>
			<section>
				<p class="receipt-label">Delivery</p>
				<p>{deliveryTypeLabel(order.delivery_type)}</p>
				{#if order.delivery_type === 'stopdesk' && order.stop_desk_name}
					<p class="receipt-muted">{order.stop_desk_name}</p>
				{/if}
				{#if order.address}
					<p class="receipt-muted">{order.address}</p>
				{/if}
				{#if placeLine}
					<p class="receipt-muted">{placeLine}</p>
				{/if}
			</section>
			<section>
				<p class="receipt-label">Status</p>
				<p>{titleCase(order.status)}</p>
				<p class="receipt-muted">Placed {formatDate(order.created_at)}</p>
			</section>
		</div>

		<table class="receipt-table">
			<thead>
				<tr>
					<th scope="col">Item</th>
					<th scope="col" class="right">Qty</th>
					<th scope="col" class="right">Unit</th>
					<th scope="col" class="right">Line total</th>
				</tr>
			</thead>
			<tbody>
				{#each order.items as item (item.id)}
					<tr>
						<td>
							{item.name}
							{#if item.options_label}
								<span class="receipt-muted receipt-options">{item.options_label}</span>
							{/if}
						</td>
						<td class="right">{item.quantity}</td>
						<td class="right mono">{money(item.price_cents)}</td>
						<td class="right mono">{money(item.price_cents * item.quantity)}</td>
					</tr>
				{/each}
			</tbody>
		</table>

		<div class="receipt-totals">
			<div><span>Subtotal</span><span>{money(order.subtotal_cents)}</span></div>
			<div><span>Shipping</span><span>{order.shipping_cents === 0 ? 'Free' : money(order.shipping_cents)}</span></div>
			<div class="receipt-total"><span>Total</span><span>{money(order.total_cents)}</span></div>
		</div>

		<p class="receipt-payment">
			{paid ? 'Paid in cash.' : 'Cash on delivery. The courier collects the total.'}
		</p>

		{#if order.notes}
			<p class="receipt-muted receipt-note">Note: {order.notes}</p>
		{/if}

		<footer class="receipt-foot">
			<p>{theme.footer_note}</p>
			<p class="receipt-muted">{theme.contact_email}</p>
			<p class="receipt-muted">
				This is a demonstration shop. No binding sale is created.
			</p>
		</footer>
	</article>

	<div class="receipt-actions">
		<button class="btn btn-primary" type="button" onclick={() => window.print()}>
			Print receipt
		</button>
		<a class="btn btn-secondary" href={data.orderPath}>Back to the order</a>
	</div>
</main>

<style>
	.receipt-page {
		min-height: 100vh;
		padding: 32px 20px 64px;
		background: var(--bg);
		color: var(--ink);
		font-family: var(--font-text);
	}

	.receipt {
		width: min(720px, 100%);
		margin: 0 auto;
		padding: 40px;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
	}

	.receipt-head {
		display: flex;
		flex-wrap: wrap;
		align-items: start;
		justify-content: space-between;
		gap: 20px;
		padding-bottom: 22px;
		border-bottom: 2px solid var(--line-strong);
	}

	.wordmark {
		font-family: var(--font-display);
		font-size: 30px;
		font-weight: 600;
		letter-spacing: -0.04em;
		line-height: 1;
	}

	.wordmark span {
		color: var(--accent);
	}

	.receipt-head-right {
		text-align: right;
	}

	.receipt-label {
		margin: 0;
		color: var(--muted);
		font-family: var(--font-mono);
		font-size: 10px;
		letter-spacing: 0.04em;
		text-transform: uppercase;
	}

	.receipt-number {
		margin: 4px 0 2px;
		font-family: var(--font-mono);
		font-size: 18px;
	}

	.receipt-muted {
		color: var(--muted);
		font-size: 13px;
		line-height: 1.5;
	}

	.receipt-parties {
		display: grid;
		grid-template-columns: 1fr 1fr 1fr;
		gap: 20px;
		padding: 22px 0;
		border-bottom: 1px solid var(--line);
	}

	.receipt-parties p {
		margin: 0;
	}

	.receipt-parties section {
		display: grid;
		gap: 3px;
		align-content: start;
	}

	.receipt-table {
		width: 100%;
		margin-top: 6px;
		border-collapse: collapse;
	}

	.receipt-table th {
		padding: 16px 8px 8px;
		border-bottom: 1px solid var(--line-strong);
		color: var(--muted);
		font-family: var(--font-mono);
		font-size: 10px;
		font-weight: 400;
		text-align: left;
	}

	.receipt-table td {
		padding: 10px 8px;
		border-bottom: 1px solid var(--line);
		font-size: 14px;
		vertical-align: top;
	}

	.receipt-table th:first-child,
	.receipt-table td:first-child {
		padding-left: 0;
	}

	.receipt-table th:last-child,
	.receipt-table td:last-child {
		padding-right: 0;
	}

	.right {
		text-align: right;
	}

	.mono {
		font-family: var(--font-mono);
		font-variant-numeric: tabular-nums;
		font-size: 13px;
	}

	.receipt-options {
		display: block;
		margin-top: 2px;
		font-size: 11px;
	}

	.receipt-totals {
		display: grid;
		gap: 8px;
		margin-top: 16px;
		margin-left: auto;
		width: min(280px, 100%);
	}

	.receipt-totals div {
		display: flex;
		justify-content: space-between;
		gap: 16px;
		color: var(--muted);
		font-size: 13px;
	}

	.receipt-totals .receipt-total {
		padding-top: 10px;
		border-top: 1px solid var(--line-strong);
		color: var(--ink);
		font-family: var(--font-mono);
		font-size: 17px;
	}

	.receipt-payment {
		margin-top: 22px;
		padding-top: 14px;
		border-top: 1px dashed var(--line-strong);
		font-size: 14px;
	}

	.receipt-note {
		margin-top: 12px;
	}

	.receipt-foot {
		display: grid;
		gap: 3px;
		margin-top: 28px;
		padding-top: 18px;
		border-top: 1px solid var(--line);
		font-size: 12px;
	}

	.receipt-foot p {
		margin: 0;
	}

	.receipt-actions {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: 10px;
		margin-top: 20px;
	}

	@media (max-width: 640px) {
		.receipt {
			padding: 22px;
		}

		.receipt-parties {
			grid-template-columns: 1fr;
			gap: 14px;
		}

		.receipt-table th:nth-child(3),
		.receipt-table td:nth-child(3) {
			display: none;
		}
	}

	@media print {
		:global(body) {
			background: #fff;
			color: #000;
		}

		.receipt-page {
			min-height: 0;
			padding: 0;
			background: #fff;
			color: #000;
		}

		.receipt {
			width: 100%;
			padding: 0;
			border: 0;
			border-radius: 0;
			background: #fff;
			box-shadow: none;
		}

		.receipt-head,
		.receipt-parties,
		.receipt-table th,
		.receipt-table td,
		.receipt-foot {
			border-color: #999;
		}

		.receipt-label,
		.receipt-muted,
		.receipt-table th,
		.receipt-totals div {
			color: #555;
		}

		.wordmark span {
			color: #000;
		}

		.receipt-actions {
			display: none;
		}
	}

	@page {
		margin: 14mm;
	}
</style>
