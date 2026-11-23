<script lang="ts">
	import { onDestroy, onMount, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { PUBLIC_TURNSTILE_SITE_KEY } from '$app/env/public';
	import { ApiError, apiFetch } from '#lib/api';
	import { createDeliveryMap, createDeliveryMapState } from '#lib/checkout/delivery-map';
	import { createHumanCheck, createHumanCheckState } from '#lib/checkout/human-check';
	import {
		INVALID_FIELDS_MESSAGE,
		orderBody,
		pickFieldErrors,
		validateOrder,
		type DeliveryType
	} from '#lib/checkout/order-validation';
	import {
		FREE_SHIPPING_OVER_CENTS,
		SHIPPING_CENTS,
		shippingFor
	} from '#lib/components/store/shipping';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import { money } from '#lib/format';
	import { TERMS_VERSION } from '#lib/legal';
	import { cart } from '#lib/stores/cart.svelte';
	import type { StorefrontTheme } from '#lib/theme/storefront';
	import type { Order } from '#lib/types';
	import type { PageData } from './$types';

	let { data }: { data: PageData & { theme: StorefrontTheme } } = $props();
	const contactEmail = $derived(data.theme.contact_email);

	let mounted = $state(false);
	let busy = $state(false);
	let formError = $state('');
	let errors = $state<Record<string, string>>({});
	let errorBox = $state<HTMLDivElement | null>(null);
	let termsAccepted = $state(false);
	let website = $state('');
	let turnstileContainer = $state<HTMLDivElement | null>(null);

	// The page keeps the contact and address details; the delivery map owns the
	// wilaya, commune and stop desk, and the human check owns its own state.
	let form = $state({
		name: '',
		email: '',
		phone: '',
		address: '',
		notes: ''
	});

	const mapState = $state(createDeliveryMapState());
	const checkState = $state(createHumanCheckState(PUBLIC_TURNSTILE_SITE_KEY));

	const check = createHumanCheck(checkState, {
		request: (path, signal) => apiFetch(path, { signal }),
		container: () => turnstileContainer
	});

	const map = createDeliveryMap(mapState, {
		request: (path, signal) => apiFetch(path, { signal })
	});

	const shipping = $derived(shippingFor(cart.subtotal));
	const total = $derived(cart.subtotal + shipping);
	const hasStockIssue = $derived(
		cart.items.some((line) => line.stock < 1 || line.quantity > line.stock)
	);

	onMount(() => {
		mounted = true;
		if (cart.items.length === 0) return;
		void map.load();
		// The form appears only once `mounted` is true, so the check's container
		// is in the DOM one tick later.
		void tick().then(() => check.load());
	});

	onDestroy(() => {
		check.destroy();
		map.destroy();
	});

	// A new wilaya or delivery type clears the messages that pointed at the
	// previous choice.
	function onWilayaChange(value: string): void {
		map.chooseWilaya(value);
		errors = {};
	}

	function onDeliveryTypeChange(next: DeliveryType): void {
		map.chooseDeliveryType(next);
		errors = {};
	}

	async function placeOrder(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy || hasStockIssue || cart.items.length === 0) return;

		formError = '';

		const validation = validateOrder({
			form: { ...form, ...map.selection() },
			termsAccepted,
			checkError: check.validationError()
		});
		// A passing check clears every message the previous submit left behind,
		// matching the old page: stale field errors must not survive a fix.
		errors = validation.errors;
		if (validation.field) {
			formError = INVALID_FIELDS_MESSAGE;
			await tick();
			const focusId =
				validation.field === 'delivery_type'
					? 'delivery-home'
					: validation.field === 'captcha'
						? check.focusId()
						: validation.field === 'terms'
							? 'terms'
							: validation.field;
			const target = document.getElementById(focusId);
			if (target instanceof HTMLElement && !target.matches(':disabled')) {
				target.focus();
			} else {
				errorBox?.focus();
			}
			return;
		}

		busy = true;

		try {
			const data = await apiFetch<{ order: Order }>('/checkout', {
				method: 'POST',
				body: orderBody({
					form: { ...form, ...map.selection() },
					bag: cart.items,
					check: check.body(),
					website,
					termsAccepted,
					termsVersion: TERMS_VERSION
				})
			});

			if (!data.order.token) {
				cart.clear();
				formError = `The order ${data.order.number} was placed, but the confirmation link is missing. Write to ${contactEmail} with the order number.`;
				await tick();
				errorBox?.focus();
				return;
			}

			cart.clear();
			await goto(`/order/${encodeURIComponent(data.order.token)}`);
		} catch (err) {
			let refreshed = false;
			if (err instanceof ApiError) {
				if (err.code === 'price_changed') {
					await cart.refreshPrices();
					formError =
						'Prices changed while the bag was open. The bag has been updated; review the total and try again.';
				} else {
					formError = err.fields.items ?? err.message;
				}
				errors = pickFieldErrors(err.fields);
				// The check knows whether a refusal spent it: a 422 that names the
				// check, or any 409, and it replaces itself.
				refreshed = await check.recover(err);
			} else {
				formError = 'The order could not be placed. Check your connection and try again.';
			}

			await tick();
			if (refreshed) {
				document.getElementById(check.focusId())?.focus();
			} else if (errors.terms) {
				document.getElementById('terms')?.focus();
			} else {
				errorBox?.focus();
			}
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head>
	<title>Checkout | {data.theme.brand_name}</title>
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
		<p class="loading" aria-live="polite">Loading your bag…</p>
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
							aria-describedby={errors.phone ? 'phone-error' : 'phone-hint'}
						/>
						{#if errors.phone}
							<p class="field-error" id="phone-error">{errors.phone}</p>
						{:else}
							<p class="meta" id="phone-hint">Algerian mobile, for example 0550 12 34 56.</p>
						{/if}
					</div>

					<div class="field">
						<span class="field-label" id="wilaya-label">Wilaya</span>
						<Dropdown
							id="wilaya_code"
							class="checkout-dropdown"
							label="Wilaya"
							labelId="wilaya-label"
							value={map.state.wilayaCode}
							options={map.wilayaOptions}
							onchange={onWilayaChange}
							disabled={map.state.wilayaStatus !== 'ready'}
							invalid={Boolean(errors.wilaya_code)}
							describedBy={errors.wilaya_code ? 'wilaya-error' : undefined}
						/>
						{#if errors.wilaya_code}
							<p class="field-error" id="wilaya-error">{errors.wilaya_code}</p>
						{/if}
					</div>

					<div class="field">
						<span class="field-label" id="commune-label">Commune</span>
						<Dropdown
							id="commune_name"
							class="checkout-dropdown"
							label="Commune"
							labelId="commune-label"
							value={map.state.communeName}
							options={map.communeOptions}
							onchange={map.chooseCommune}
							disabled={!map.state.wilayaCode || map.state.communeStatus !== 'ready' || map.communeLocked}
							invalid={Boolean(errors.commune_name)}
							describedBy={errors.commune_name
								? 'commune-error'
								: map.communeLocked
									? 'commune-hint'
									: undefined}
						/>
						{#if errors.commune_name}
							<p class="field-error" id="commune-error">{errors.commune_name}</p>
						{:else if map.communeLocked}
							<p class="meta" id="commune-hint">The stop desk sets the commune.</p>
						{/if}
					</div>

					<fieldset class="field field-full delivery-choice">
						<legend>Delivery</legend>
						<label class="delivery-option">
							<input
								type="radio"
								id="delivery-home"
								name="delivery_type"
								value="home"
								checked={map.state.deliveryType === 'home'}
								onchange={() => onDeliveryTypeChange('home')}
							/>
							<span>
								<span>Home delivery</span>
								<span class="meta">To your door.</span>
							</span>
						</label>
						<label class="delivery-option">
							<input
								type="radio"
								id="delivery-stopdesk"
								name="delivery_type"
								value="stopdesk"
								checked={map.state.deliveryType === 'stopdesk'}
								onchange={() => onDeliveryTypeChange('stopdesk')}
							/>
							<span>
								<span>Stop desk</span>
								<span class="meta">Collect from a courier desk.</span>
							</span>
						</label>
					</fieldset>

					{#if map.state.deliveryType === 'home'}
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
								aria-describedby={errors.address ? 'address-error' : 'address-hint'}
							/>
							{#if errors.address}
								<p class="field-error" id="address-error">{errors.address}</p>
							{:else}
								<p class="meta" id="address-hint">Street, building, floor.</p>
							{/if}
						</div>
					{:else}
						<div class="field field-full">
							<span class="field-label" id="stop-desk-label">Stop desk</span>
							<Dropdown
								id="stop_desk_id"
								class="checkout-dropdown"
								label="Stop desk"
								labelId="stop-desk-label"
								value={map.state.stopDeskId}
								options={map.deskOptions}
								onchange={map.chooseDesk}
								disabled={map.state.deskStatus !== 'ready' || map.state.desks.length === 0}
								invalid={Boolean(errors.stop_desk_id)}
								describedBy={errors.stop_desk_id
									? 'stop-desk-error'
									: 'stop-desk-hint'}
							/>
							{#if errors.stop_desk_id}
								<p class="field-error" id="stop-desk-error">{errors.stop_desk_id}</p>
							{:else if map.state.deskStatus === 'ready' && map.state.desks.length === 0}
								<p class="meta" id="stop-desk-hint">No stop desks here. Choose home delivery.</p>
							{:else if map.selectedDesk?.address}
								<p class="meta" id="stop-desk-hint">{map.selectedDesk?.address}</p>
							{:else}
								<p class="meta" id="stop-desk-hint">The desk that suits you.</p>
							{/if}
						</div>
					{/if}

					<div class="field field-full">
						<label for="notes">Notes</label>
						<textarea
							id="notes"
							name="notes"
							rows="3"
							autocomplete="off"
							maxlength="500"
							bind:value={form.notes}
							aria-invalid={errors.notes ? 'true' : undefined}
							aria-describedby={errors.notes ? 'notes-error' : 'notes-hint'}
						></textarea>
						{#if errors.notes}
							<p class="field-error" id="notes-error">{errors.notes}</p>
						{:else}
							<p class="meta" id="notes-hint">
								Optional. Gate codes, floor, a time that suits you.
							</p>
						{/if}
					</div>
				</div>

				{#if map.state.wilayaStatus === 'error'}
					<div class="form-error" role="alert">
						<p>{map.state.wilayaError}</p>
						<button class="inline-action" type="button" onclick={() => void map.load()}>
							Try again
						</button>
					</div>
				{:else if map.state.communeStatus === 'error'}
					<div class="form-error" role="alert">
						<p>{map.state.communeError}</p>
						<button
							class="inline-action"
							type="button"
							onclick={map.retryCommunes}
						>
							Try again
						</button>
					</div>
				{:else if map.state.deliveryType === 'stopdesk' && map.state.deskStatus === 'error'}
					<div class="form-error" role="alert">
						<p>{map.state.deskError}</p>
						<button
							class="inline-action"
							type="button"
							onclick={map.retryDesks}
						>
							Try again
						</button>
					</div>
				{:else if map.state.wilayaStatus === 'loading'}
					<p class="meta" aria-live="polite">Reading the delivery map…</p>
				{/if}

				<fieldset class="payment">
					<legend>Payment</legend>
					<label class="pay-option">
						<input name="payment" type="radio" value="cod" checked disabled />
						<span>Cash on delivery</span>
					</label>
					<p class="meta">The courier collects the total in cash. No card is charged here.</p>
				</fieldset>

				<div class="hp-field" aria-hidden="true">
					<label for="azahar-extra">Leave this field empty</label>
					<input
						id="azahar-extra"
						name="azahar_extra"
						type="text"
						tabindex="-1"
						autocomplete="off"
						bind:value={website}
					/>
				</div>

				{#if check.kind === 'turnstile'}
					<fieldset class="security-check">
						<legend>Security check</legend>
						<p class="meta">
							This check works with a keyboard and screen readers. It keeps automated orders off
							the ledger.
						</p>

						<div
							id="turnstile-check"
							class="turnstile-check"
							tabindex="-1"
							role="group"
							bind:this={turnstileContainer}
							aria-label="Cloudflare security check"
							aria-describedby={errors.captcha ? 'captcha-error' : 'turnstile-status'}
						></div>

						{#if errors.captcha}
							<p class="field-error" id="captcha-error">{errors.captcha}</p>
						{/if}

						{#if check.state.status === 'error'}
							<div class="form-error" id="turnstile-status" role="alert">
								<p>{check.state.error}</p>
								<button class="inline-action" type="button" onclick={() => void check.load()}>
									Try again
								</button>
							</div>
						{:else}
							<p class="meta" id="turnstile-status" role="status" aria-live="polite">
								{#if check.state.status === 'loading'}
									Loading the security check…
								{:else if check.state.error}
									{check.state.error}
								{:else if check.state.token}
									Security check complete.
								{:else}
									Complete the security check to place the order.
								{/if}
							</p>
						{/if}

						<p class="meta">
							If the check keeps failing, write to {contactEmail}.
						</p>
					</fieldset>
				{:else}
					<fieldset class="security-check">
						<legend>Security check</legend>
						<p class="meta">Type the five characters. This keeps automated orders off the ledger.</p>

						<div class="captcha-row">
							<div class="captcha-frame">
								{#if check.state.status === 'ready' && check.state.challenge}
									<img
										src={check.state.challenge.image}
										alt="Verification code. Type the five characters shown."
										width="220"
										height="64"
									/>
								{:else if check.state.status === 'loading'}
									<span class="loading">Loading code…</span>
								{:else}
									<span class="captcha-error" role="alert">{check.state.error}</span>
								{/if}
							</div>
							<button
								class="btn btn-secondary btn-small"
								type="button"
								onclick={() => void check.load()}
								disabled={check.state.status === 'loading'}
							>
								{check.state.status === 'error' ? 'Try again' : 'New code'}
							</button>
						</div>

						<div class="field">
							<label for="captcha-answer">Code</label>
							<input
								id="captcha-answer"
								name="captcha_answer"
								type="text"
								inputmode="text"
								autocomplete="off"
								autocapitalize="characters"
								spellcheck="false"
								maxlength="5"
								value={check.state.answer}
								oninput={(event) => check.setAnswer(event.currentTarget.value)}
								aria-invalid={errors.captcha ? 'true' : undefined}
								aria-describedby={errors.captcha ? 'captcha-error' : 'captcha-hint'}
							/>
							{#if errors.captcha}
								<p class="field-error" id="captcha-error">{errors.captcha}</p>
							{:else}
								<p class="meta" id="captcha-hint">
									Five characters, not case-sensitive. If the code is hard to read, request a
									new one or write to hello@azahar.example.
								</p>
							{/if}
						</div>
					</fieldset>
				{/if}

				<div class="terms">
					<div class="terms-row">
						<input
							id="terms"
							name="terms"
							type="checkbox"
							bind:checked={termsAccepted}
							aria-invalid={errors.terms ? 'true' : undefined}
							aria-describedby={errors.terms ? 'terms-error' : 'terms-detail'}
						/>
						<div>
							<label for="terms">I accept the terms of service.</label>
							<p class="meta" id="terms-detail">
								Read the <a href="/policies/terms" target="_blank" rel="noopener"
									>terms of service</a
								>
								(version {TERMS_VERSION}) and the <a
									href="/policies/privacy"
									target="_blank"
									rel="noopener">privacy policy</a
								>. I understand this is a demo shop and no binding sale is created.
							</p>
						</div>
					</div>
					{#if errors.terms}
						<p class="field-error" id="terms-error">{errors.terms}</p>
					{/if}
				</div>

				<div class="place-total">
					<span>Total, cash on delivery</span>
					<span class="mono">{money(total)}</span>
				</div>

				<button
					class="btn btn-primary btn-block place-order"
					type="submit"
					disabled={busy || hasStockIssue}
				>
					{busy ? 'Placing the order…' : 'Place the order'}
				</button>
				<p class="meta place-note">No card is taken on this site.</p>
			</form>

			<aside class="summary checkout-summary" aria-labelledby="checkout-summary-title">
				<h2 id="checkout-summary-title">Your order</h2>

				<div class="summary-lines">
					{#each cart.items as line (line.key)}
						<div class="summary-line">
							<div>
								<span>{line.name}</span>
								<span class="meta">{line.quantity} × {line.size}</span>
								{#if line.options_label}
									<span class="meta">{line.options_label}</span>
								{/if}
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

	.field-label {
		font-size: 13px;
		color: var(--ink);
	}

	:global(.checkout-dropdown .dropdown-trigger) {
		min-height: 48px;
		padding: 11px 13px;
		border-radius: var(--radius-sm);
		font-size: 15px;
	}

	:global(.checkout-dropdown .dropdown-option) {
		font-size: 15px;
	}

	:global(.checkout-dropdown .dropdown-menu) {
		border-radius: var(--radius-sm);
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

	.delivery-choice {
		margin: 0;
		padding: 0;
		border: 0;
	}

	.delivery-choice legend {
		padding: 0;
		font-size: 13px;
		color: var(--ink);
	}

	.delivery-option {
		display: flex;
		align-items: start;
		gap: 10px;
		min-height: 44px;
		padding: 10px 0;
		cursor: pointer;
	}

	.delivery-option input {
		width: 18px;
		height: 18px;
		margin-top: 2px;
		accent-color: var(--accent);
	}

	.delivery-option > span {
		display: grid;
		gap: 2px;
	}

	.delivery-option > span > span:first-child {
		font-size: 15px;
	}

	.inline-action {
		justify-self: start;
		min-height: 44px;
		padding: 0;
		border: 0;
		background: none;
		color: var(--accent);
		font: inherit;
		text-decoration: underline;
		text-underline-offset: 3px;
		cursor: pointer;
	}

	.inline-action:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 3px;
	}

	.place-order {
		min-height: 50px;
	}

	.hp-field {
		position: absolute;
		left: -9999px;
		width: 1px;
		height: 1px;
		overflow: hidden;
	}

	.security-check {
		display: grid;
		gap: 10px;
		margin: 0;
		padding: 0;
		border: 0;
	}

	.security-check legend {
		padding: 0;
		font-size: 13px;
		color: var(--ink);
	}

	.turnstile-check {
		min-height: 65px;
	}

	.captcha-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 12px;
	}

	.captcha-frame {
		display: grid;
		place-items: center;
		width: 220px;
		height: 64px;
		overflow: hidden;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
	}

	.captcha-frame img {
		width: 220px;
		height: 64px;
	}

	.captcha-error {
		padding: 6px;
		color: var(--accent);
		font-size: 12px;
		line-height: 1.35;
		text-align: center;
	}

	.terms {
		display: grid;
		gap: 6px;
	}

	.terms-row {
		display: flex;
		align-items: flex-start;
		gap: 10px;
	}

	.terms-row input {
		flex: 0 0 auto;
		width: 18px;
		height: 18px;
		margin-top: 3px;
		accent-color: var(--accent);
	}

	.terms-row label {
		font-size: 14px;
		font-weight: 500;
	}

	.terms-row .meta {
		max-width: 52ch;
		margin-top: 4px;
	}

	.terms-row a {
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.place-total {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px 16px;
		padding-top: 18px;
		border-top: 1px solid var(--line);
		font-size: 14px;
	}

	.place-total .mono {
		font-size: 17px;
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
