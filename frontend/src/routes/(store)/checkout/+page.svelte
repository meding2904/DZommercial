<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { ApiError, apiFetch } from '#lib/api';
	import {
		FREE_SHIPPING_OVER_CENTS,
		SHIPPING_CENTS,
		shippingFor
	} from '#lib/components/store/shipping';
	import { money } from '#lib/format';
	import { cart } from '#lib/stores/cart.svelte';
	import type { Order } from '#lib/types';

	const FIELD_ORDER = ['name', 'email', 'phone', 'address', 'city'] as const;
	type FieldKey = (typeof FIELD_ORDER)[number];

	let mounted = $state(false);
	let busy = $state(false);
	let formError = $state('');
	let errors = $state<Record<string, string>>({});
	let errorBox = $state<HTMLDivElement | null>(null);

	let form = $state({
		name: '',
		email: '',
		phone: '',
		address: '',
		city: '',
		notes: ''
	});

	onMount(() => {
		mounted = true;
	});

	const shipping = $derived(shippingFor(cart.subtotal));
	const total = $derived(cart.subtotal + shipping);
	const hasStockIssue = $derived(
		cart.items.some((line) => line.stock < 1 || line.quantity > line.stock)
	);

	function validate(): FieldKey | null {
		const next: Record<string, string> = {};

		if (!form.name.trim()) next.name = 'Enter the name for the delivery.';
		if (!form.email.trim()) next.email = 'Enter an email for the order.';
		else if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(form.email.trim())) {
			next.email = 'Enter a valid email.';
		}
		if (!form.phone.trim()) next.phone = 'Enter a phone number the courier can call.';
		if (!form.address.trim()) next.address = 'Enter the delivery address.';
		if (!form.city.trim()) next.city = 'Enter the city.';

		errors = next;
		return FIELD_ORDER.find((key) => next[key]) ?? null;
	}

	async function placeOrder(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy || hasStockIssue || cart.items.length === 0) return;

		formError = '';

		const firstInvalid = validate();
		if (firstInvalid) {
			formError = 'Check the marked fields.';
			await tick();
			document.getElementById(firstInvalid)?.focus();
			return;
		}

		busy = true;

		try {
			const data = await apiFetch<{ order: Order }>('/checkout', {
				method: 'POST',
				body: {
					name: form.name.trim(),
					email: form.email.trim(),
					phone: form.phone.trim(),
					address: form.address.trim(),
					city: form.city.trim(),
					notes: form.notes.trim(),
					items: cart.items.map((line) => ({
						product_id: line.product_id,
						quantity: line.quantity
					}))
				}
			});

			if (!data.order.token) {
				cart.clear();
				formError = `The order ${data.order.number} was placed, but the confirmation link is missing. Write to hello@azahar.example with the order number.`;
				await tick();
				errorBox?.focus();
				return;
			}

			cart.clear();
			await goto(`/order/${encodeURIComponent(data.order.token)}`);
		} catch (err) {
			if (err instanceof ApiError) {
				formError = err.message;
				errors = Object.fromEntries(
					Object.entries(err.fields).filter(([key]) =>
						FIELD_ORDER.includes(key as FieldKey)
					)
				);
			} else {
				formError = 'The order could not be placed. Check your connection and try again.';
			}

			await tick();
			errorBox?.focus();
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head>
	<title>Checkout | Azahar</title>
	<meta
		name="description"
		content="Enter the delivery details, review the order and pay the courier in cash."
	/>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="wrap page-head">
	<h1>Checkout</h1>
	<p class="lead">Cash on delivery across Algeria. We call before the order leaves the house.</p>
</div>

<div class="wrap">
	{#if !mounted}
		<p class="loading">Loading your bag.</p>
	{:else if cart.items.length === 0}
		<div class="empty-state">
			<p class="meta">Empty bag</p>
			<h2>Your bag is empty.</h2>
			<p class="muted">There is nothing to send yet. Start on the shelf.</p>
			<a class="btn btn-primary" href="/shop">Shop the shelf</a>
		</div>
	{:else}
		<div class="checkout-layout">
			<form
				class="checkout-form"
				onsubmit={placeOrder}
				novalidate
				aria-busy={busy ? 'true' : undefined}
			>
				<h2 class="form-title">Delivery details</h2>

				{#if formError}
					<div class="form-error" role="alert" tabindex="-1" bind:this={errorBox}>
						{formError}
					</div>
				{/if}

				{#if hasStockIssue}
					<p class="form-error">
						One formula in your bag is sold out. <a href="/cart">Return to the bag</a> to remove it.
					</p>
				{/if}

				<div class="form-grid">
					<div class="field field-full">
						<label for="name">Full name</label>
						<input
							id="name"
							name="name"
							type="text"
							autocomplete="name"
							required
							bind:value={form.name}
							aria-invalid={errors.name ? 'true' : undefined}
							aria-describedby={errors.name ? 'name-error' : undefined}
						/>
						{#if errors.name}
							<p class="field-error" id="name-error">{errors.name}</p>
						{/if}
					</div>

					<div class="field">
						<label for="email">Email</label>
						<input
							id="email"
							name="email"
							type="email"
							inputmode="email"
							autocomplete="email"
							required
							bind:value={form.email}
							aria-invalid={errors.email ? 'true' : undefined}
							aria-describedby={errors.email ? 'email-error' : undefined}
						/>
						{#if errors.email}
							<p class="field-error" id="email-error">{errors.email}</p>
						{/if}
					</div>

					<div class="field">
						<label for="phone">Phone</label>
						<input
							id="phone"
							name="phone"
							type="tel"
							inputmode="tel"
							autocomplete="tel"
							required
							bind:value={form.phone}
							aria-invalid={errors.phone ? 'true' : undefined}
							aria-describedby={errors.phone ? 'phone-error' : undefined}
						/>
						{#if errors.phone}
							<p class="field-error" id="phone-error">{errors.phone}</p>
						{/if}
					</div>

					<div class="field field-full">
						<label for="address">Address</label>
						<input
							id="address"
							name="address"
							type="text"
							autocomplete="street-address"
							required
							bind:value={form.address}
							aria-invalid={errors.address ? 'true' : undefined}
							aria-describedby={errors.address ? 'address-error' : undefined}
						/>
						{#if errors.address}
							<p class="field-error" id="address-error">{errors.address}</p>
						{/if}
					</div>

					<div class="field">
						<label for="city">City</label>
						<input
							id="city"
							name="city"
							type="text"
							autocomplete="address-level2"
							required
							bind:value={form.city}
							aria-invalid={errors.city ? 'true' : undefined}
							aria-describedby={errors.city ? 'city-error' : undefined}
						/>
						{#if errors.city}
							<p class="field-error" id="city-error">{errors.city}</p>
						{/if}
					</div>

					<div class="field field-full">
						<label for="notes">Notes</label>
						<textarea
							id="notes"
							name="notes"
							rows="3"
							autocomplete="off"
							bind:value={form.notes}
							aria-describedby="notes-hint"
						></textarea>
						<p class="meta" id="notes-hint">
							Optional. Gate codes, floor, a time that suits you.
						</p>
					</div>
				</div>

				<fieldset class="payment">
					<legend>Payment</legend>
					<label class="pay-option">
						<input name="payment" type="radio" value="cod" checked disabled />
						<span>Cash on delivery</span>
					</label>
					<p class="meta">The courier collects the total in cash. No card is charged here.</p>
				</fieldset>

				<button
					class="btn btn-primary btn-block place-order"
					type="submit"
					disabled={busy || hasStockIssue}
				>
					{busy ? 'Placing the order...' : 'Place the order'}
				</button>
				<p class="meta place-note">No card is taken on this site.</p>
			</form>

			<aside class="summary checkout-summary" aria-labelledby="checkout-summary-title">
				<h2 id="checkout-summary-title">Your order</h2>

				<div class="summary-lines">
					{#each cart.items as line (line.product_id)}
						<div class="summary-line">
							<div>
								<span>{line.name}</span>
								<span class="meta">{line.quantity} × {line.size}</span>
							</div>
							<span class="mono">{money(line.price_cents * line.quantity)}</span>
						</div>
					{/each}
				</div>

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

				<p class="muted cash-note">Pay the courier when the order arrives.</p>
				<a class="text-link edit-link" href="/cart">Edit bag</a>
			</aside>
		</div>
	{/if}
</div>

<style>
	.checkout-form {
		display: grid;
		gap: 22px;
		align-content: start;
	}

	.form-title {
		font-size: clamp(28px, 3vw, 38px);
	}

	.form-error a {
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.payment {
		margin: 0;
		padding: 18px;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		display: grid;
		gap: 10px;
	}

	.payment legend {
		padding: 0 6px;
		font-size: 13px;
		color: var(--muted);
	}

	.pay-option {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 44px;
	}

	.pay-option input {
		width: 18px;
		height: 18px;
		accent-color: var(--accent);
	}

	.pay-option span {
		font-size: 15px;
	}

	.place-order {
		min-height: 50px;
	}

	.place-note {
		text-align: center;
	}

	.summary-lines {
		display: grid;
		gap: 12px;
		padding-bottom: 16px;
		border-bottom: 1px solid var(--line);
	}

	.summary-line {
		display: flex;
		justify-content: space-between;
		align-items: start;
		gap: 16px;
		font-size: 14px;
	}

	.summary-line > div {
		display: grid;
		gap: 2px;
	}

	.cash-note {
		font-size: 13px;
	}

	.edit-link {
		justify-content: center;
		min-height: 44px;
	}
</style>
