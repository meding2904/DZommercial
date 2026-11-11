<script lang="ts">
	import ProductImage from '#lib/components/ProductImage.svelte';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { courierStatusLabel, deliveryTypeLabel, formatDate, money } from '#lib/format';
	import type { OrderStatus } from '#lib/types';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const order = $derived(data.order);
	const placeLine = $derived.by(() => {
		const commune = order.commune_name || order.city;
		const wilaya = order.wilaya_name || (order.wilaya_code ? `Wilaya ${order.wilaya_code}` : '');
		return [commune, wilaya].filter(Boolean).join(', ');
	});

	const heading = $derived(
		order.status === 'cancelled'
			? 'The order was cancelled.'
			: order.status === 'delivered'
				? 'The order was delivered.'
				: order.status === 'shipped'
					? 'The order is on its way.'
					: 'The order is in.'
	);

	const statusLine = $derived.by(() => {
		const lines: Record<OrderStatus, string> = {
			pending: 'We will call to confirm before the courier comes.',
			confirmed: 'The order is confirmed.',
			packed: 'The order is packed.',
			shipped: 'The order is with the courier.',
			delivered: 'The order was delivered.',
			cancelled: 'Nothing is due for this order.'
		};
		return lines[order.status];
	});
</script>

<svelte:head>
	<title>Order {order.number} | Azahar</title>
	<meta
		name="description"
		content={`The confirmation for order ${order.number}, with the items, total and delivery address.`}
	/>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="wrap page-head">
	<p class="meta">Order</p>
	<h1>{heading}</h1>
	<p class="lead">{statusLine}</p>
</div>

<div class="wrap">
	<section class="order-card" aria-label={`Order ${order.number}`}>
		<div class="order-head">
			<div>
				<p class="order-number">{order.number}</p>
				<p class="meta">{formatDate(order.created_at)}</p>
			</div>
			<StatusPill status={order.status} />
		</div>

		<div class="order-items">
			{#each order.items as item (item.product_id)}
				<div class="order-item">
					<ProductImage
						src={item.image}
						alt=""
						width={1200}
						height={1500}
						sizes="56px"
					/>
					<div>
						<p>{item.name}</p>
						<p class="meta">{item.quantity} × {money(item.price_cents)}</p>
					</div>
					<p class="mono order-item-total">{money(item.price_cents * item.quantity)}</p>
				</div>
			{/each}
		</div>

		<div class="order-totals">
			<div class="summary-row">
				<span>Subtotal</span>
				<span>{money(order.subtotal_cents)}</span>
			</div>
			<div class="summary-row">
				<span>Shipping</span>
				<span>{order.shipping_cents === 0 ? 'Free' : money(order.shipping_cents)}</span>
			</div>
			<div class="summary-total">
				<span>Total</span>
				<span>{money(order.total_cents)}</span>
			</div>
		</div>

		<div class="order-delivery">
			<h2>Delivery</h2>
			<p class="meta">{deliveryTypeLabel(order.delivery_type)}</p>
			<address>
				{order.name}<br />
				{#if order.delivery_type === 'stopdesk'}
					{#if order.stop_desk_name}
						Stop desk: {order.stop_desk_name}<br />
					{/if}
					{#if order.address}
						{order.address}<br />
					{/if}
				{:else}
					{order.address}<br />
				{/if}
				{placeLine}<br />
				{order.phone}
			</address>
			<p class="meta">{order.email}</p>
			{#if order.notes}
				<p>Notes for the courier: {order.notes}</p>
			{/if}
			{#if order.tracking_number}
				<div class="order-tracking">
					<p class="meta">Courier tracking</p>
					<p class="mono">{order.tracking_number}</p>
					<p>{courierStatusLabel(order.courier_status)}</p>
				</div>
			{/if}
		</div>

		{#if ['pending', 'confirmed', 'packed', 'shipped'].includes(order.status)}
			<p class="muted cash-note">Pay the courier when the order arrives.</p>
		{/if}

		<div>
			<a class="btn btn-secondary" href="/shop">Back to the shelf</a>
		</div>
	</section>
</div>

<style>
	.order-card h2 {
		font-size: 20px;
		margin-bottom: 10px;
	}

	.order-totals {
		display: grid;
		gap: 10px;
	}

	.order-delivery {
		display: grid;
		gap: 4px;
		padding-top: 20px;
		border-top: 1px solid var(--line);
	}

	.order-delivery address {
		font-style: normal;
		line-height: 1.6;
	}

	.order-tracking {
		display: grid;
		gap: 2px;
		margin-top: 12px;
		padding-top: 12px;
		border-top: 1px solid var(--line);
	}

	.cash-note {
		font-size: 14px;
	}
</style>
