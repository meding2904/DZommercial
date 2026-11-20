<script lang="ts">
	import { onMount } from 'svelte';
	import ProductImage from '#lib/components/ProductImage.svelte';
	import QtyStepper from '#lib/components/QtyStepper.svelte';
	import {
		FREE_SHIPPING_OVER_CENTS,
		SHIPPING_CENTS,
		shippingFor
	} from '#lib/components/store/shipping';
	import { money } from '#lib/format';
	import { cart } from '#lib/stores/cart.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	let mounted = $state(false);
	let announcement = $state('');
	let notices = $state<Record<string, string>>({});

	onMount(() => {
		mounted = true;

		for (const line of cart.items) {
			if (line.stock < 1) {
				notices = {
					...notices,
					[line.key]: 'This product is sold out. Remove it to place the order.'
				};
				continue;
			}

			const limit = Math.min(line.stock, 10);
			if (line.quantity > limit) {
				const requested = line.quantity;
				cart.setQty(line.key, limit);
				notices = {
					...notices,
					[line.key]: `Only ${limit} can go in one bag. The quantity changed from ${requested} to ${limit}.`
				};
			}
		}
	});

	const shipping = $derived(shippingFor(cart.subtotal));
	const total = $derived(cart.subtotal + shipping);
	const hasStockIssue = $derived(
		cart.items.some((line) => line.stock < 1 || line.quantity > line.stock)
	);

	function changeQty(key: string, quantity: number): void {
		cart.setQty(key, quantity);
		const line = cart.items.find((item) => item.key === key);
		if (!line) return;

		if (notices[key]) {
			const next = { ...notices };
			delete next[key];
			notices = next;
		}

		announcement = `${line.name}, quantity ${line.quantity}. Bag subtotal ${money(cart.subtotal)}.`;
	}

	function remove(key: string): void {
		const line = cart.items.find((item) => item.key === key);
		cart.remove(key);

		if (notices[key]) {
			const next = { ...notices };
			delete next[key];
			notices = next;
		}

		announcement = line
			? `${line.name} removed from the bag. Bag subtotal ${money(cart.subtotal)}.`
			: 'Product removed from the bag.';
	}
</script>

<svelte:head>
	<title>Your bag | {data.theme.brand_name}</title>
	<meta
		name="description"
		content="The formulas in your bag, the shipping cost and the cash on delivery total."
	/>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="wrap page-head">
	<h1>Your bag</h1>
	<p class="lead">The formulas you have chosen, before they go to the courier.</p>
</div>

<p class="sr-only" aria-live="polite" aria-atomic="true">{announcement}</p>

<div class="wrap">
	{#if !mounted}
		<p class="loading">Loading your bag…</p>
	{:else if cart.items.length === 0}
		<div class="empty-state">
			<p class="meta">Empty bag</p>
			<h2>Your bag is empty.</h2>
			<p class="muted">The shelf is one page over.</p>
			<a class="btn btn-primary" href="/shop">Shop the shelf</a>
		</div>
	{:else}
		<div class="cart-layout">
			<div>
				<h2 class="sr-only">In your bag</h2>
				<div class="cart-lines">
					{#each cart.items as line (line.key)}
						<article class="cart-line">
							<ProductImage
								src={line.image}
								alt=""
								width={1200}
								height={1500}
								sizes="96px"
							/>

							<div>
								<h3>
									<a class="line-link" href={`/product/${line.slug}`}>{line.name}</a>
								</h3>
								<p class="meta">{line.size}</p>
								{#if line.options_label}
									<p class="meta">{line.options_label}</p>
								{/if}

								{#if notices[line.key]}
									<p class="form-error line-warning" role="status">
										{notices[line.key]}
									</p>
								{/if}

								<div class="cart-line-actions">
									{#if line.stock > 0}
										<QtyStepper
											value={line.quantity}
											max={Math.min(line.stock, 10)}
											label={`Quantity for ${line.name}`}
											onchange={(value) => changeQty(line.key, value)}
										/>
									{:else}
										<span class="stock-note">Sold out</span>
									{/if}

									<button
										class="link-button remove-btn"
										type="button"
										onclick={() => remove(line.key)}
										aria-label={`Remove ${line.name} from the bag`}
									>
										Remove
									</button>
								</div>
							</div>

							<p class="cart-line-total">
								<span class="sr-only">Line total </span>
								{money(line.price_cents * line.quantity)}
							</p>
						</article>
					{/each}
				</div>
			</div>

			<aside class="summary" aria-labelledby="summary-title">
				<h2 id="summary-title">Order summary</h2>

				<div class="summary-row">
					<span>Subtotal</span>
					<span>{money(cart.subtotal)}</span>
				</div>
				<div class="summary-row">
					<span>Shipping</span>
					<span>{shipping === 0 ? 'Free' : money(shipping)}</span>
				</div>

				{#if shipping === 0}
					<p class="meta">Shipping is free on this order.</p>
				{:else}
					<p class="meta">
						Shipping is {money(SHIPPING_CENTS)}. Free over {money(FREE_SHIPPING_OVER_CENTS)}.
					</p>
				{/if}

				<div class="summary-total">
					<span>Total</span>
					<span>{money(total)}</span>
				</div>

				<p class="muted cash-note">Cash on delivery. Pay the courier when the order arrives.</p>

				{#if hasStockIssue}
					<p class="form-error" role="status">
						Remove the sold-out formula before you continue.
					</p>
					<button class="btn btn-primary btn-block" type="button" disabled>
						Continue to checkout
					</button>
				{:else}
					<a class="btn btn-primary btn-block" href="/checkout">Continue to checkout</a>
				{/if}

				<a class="text-link continue-link" href="/shop">Add another formula</a>
			</aside>
		</div>
	{/if}
</div>

<style>
	.line-link {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.line-warning {
		margin-top: 10px;
		padding: 8px 10px;
		font-size: 13px;
	}

	.remove-btn {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
	}

	.cart-line :global(.qty button) {
		width: 44px;
	}

	.cash-note {
		font-size: 13px;
	}

	.continue-link {
		justify-content: center;
		min-height: 44px;
	}
</style>
