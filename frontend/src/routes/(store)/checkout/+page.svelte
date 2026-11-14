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
	import { TERMS_VERSION } from '#lib/legal';
	import { cart } from '#lib/stores/cart.svelte';
	import type { Commune, Order, StopDesk, Wilaya } from '#lib/types';

	const FIELD_ORDER = [
		'name',
		'email',
		'phone',
		'wilaya_code',
		'commune_name',
		'delivery_type',
		'stop_desk_id',
		'address',
		'captcha',
		'terms'
	] as const;
	type FieldKey = (typeof FIELD_ORDER)[number];

	type CaptchaChallenge = {
		token: string;
		image: string;
		expires_in: number;
	};

	let mounted = $state(false);
	let busy = $state(false);
	let formError = $state('');
	let errors = $state<Record<string, string>>({});
	let errorBox = $state<HTMLDivElement | null>(null);

	let captcha = $state<CaptchaChallenge | null>(null);
	let captchaState = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
	let captchaAnswer = $state('');
	let captchaError = $state('');
	let termsAccepted = $state(false);
	let website = $state('');

	let form = $state({
		name: '',
		email: '',
		phone: '',
		wilayaCode: '',
		communeName: '',
		deliveryType: 'home' as 'home' | 'stopdesk',
		stopDeskId: '',
		address: '',
		notes: ''
	});

	let wilayas = $state<Wilaya[]>([]);
	let communes = $state<Commune[]>([]);
	let desks = $state<StopDesk[]>([]);
	let wilayaState = $state<'loading' | 'ready' | 'error'>('loading');
	let communeState = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
	let deskState = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
	let mapError = $state('');
	let communeError = $state('');
	let deskError = $state('');
	let communeRequest = 0;
	let deskRequest = 0;

	const selectedDesk = $derived(desks.find((desk) => desk.id === form.stopDeskId));
	const communeLocked = $derived(
		form.deliveryType === 'stopdesk' && Boolean(selectedDesk?.communeName)
	);
	const shipping = $derived(shippingFor(cart.subtotal));
	const total = $derived(cart.subtotal + shipping);
	const hasStockIssue = $derived(
		cart.items.some((line) => line.stock < 1 || line.quantity > line.stock)
	);

	onMount(() => {
		mounted = true;
		if (cart.items.length > 0) {
			void loadWilayas();
			void loadCaptcha();
		}
	});

	async function loadCaptcha(): Promise<void> {
		captchaState = 'loading';
		captchaError = '';
		captcha = null;
		captchaAnswer = '';
		try {
			const data = await apiFetch<CaptchaChallenge>('/checkout/challenge');
			captcha = data;
			captchaState = 'ready';
		} catch {
			captchaState = 'error';
			captchaError = 'The security check could not load. Check your connection and try again.';
		}
	}

	async function loadWilayas(): Promise<void> {
		wilayaState = 'loading';
		mapError = '';
		try {
			const data = await apiFetch<{ wilayas: Wilaya[] }>('/shipping/wilayas');
			wilayas = data.wilayas;
			wilayaState = 'ready';
			if (form.wilayaCode) void loadCommunes(Number(form.wilayaCode));
		} catch {
			wilayaState = 'error';
			mapError = 'The delivery map could not be read. Check your connection and try again.';
		}
	}

	async function loadCommunes(wilayaCode: number): Promise<void> {
		const request = ++communeRequest;
		communeState = 'loading';
		communeError = '';
		communes = [];
		try {
			const data = await apiFetch<{ communes: Commune[] }>(
				`/shipping/communes?wilaya=${wilayaCode}`
			);
			if (request !== communeRequest) return;
			communes = data.communes;
			communeState = 'ready';
		} catch {
			if (request !== communeRequest) return;
			communeState = 'error';
			communeError = 'The communes could not be read. Try again.';
		}
	}

	async function loadDesks(wilayaCode: number): Promise<void> {
		const request = ++deskRequest;
		deskState = 'loading';
		deskError = '';
		desks = [];
		try {
			const data = await apiFetch<{ desks: StopDesk[] }>(`/shipping/desks?wilaya=${wilayaCode}`);
			if (request !== deskRequest) return;
			desks = data.desks;
			deskState = 'ready';
		} catch {
			if (request !== deskRequest) return;
			deskState = 'error';
			deskError = 'The stop desks could not be read. Try again, or choose home delivery.';
		}
	}

	function onWilayaChange(value: string): void {
		form.wilayaCode = value;
		form.communeName = '';
		form.stopDeskId = '';
		communes = [];
		desks = [];
		communeState = 'idle';
		deskState = 'idle';
		errors = {};

		const code = Number(value);
		if (!code) return;
		void loadCommunes(code);
		if (form.deliveryType === 'stopdesk') void loadDesks(code);
	}

	function onDeliveryTypeChange(next: 'home' | 'stopdesk'): void {
		form.deliveryType = next;
		form.stopDeskId = '';
		desks = [];
		deskState = 'idle';
		errors = {};

		if (next === 'stopdesk' && form.wilayaCode) {
			void loadDesks(Number(form.wilayaCode));
		}
	}

	function onDeskChange(value: string): void {
		form.stopDeskId = value;
		const desk = desks.find((item) => item.id === value);
		if (desk?.communeName) form.communeName = desk.communeName;
	}

	function validMobile(value: string): boolean {
		const digits = value.replace(/[\s()+-]/g, '').replace(/^213/, '0');
		return /^0[567]\d{8}$/.test(digits);
	}

	function validate(): FieldKey | null {
		const next: Record<string, string> = {};

		if (!form.name.trim()) next.name = 'Enter the name for the delivery.';
		if (!form.email.trim()) next.email = 'Enter an email for the order.';
		else if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(form.email.trim())) {
			next.email = 'Enter a valid email.';
		}
		if (!form.phone.trim()) next.phone = 'Enter a phone number the courier can call.';
		else if (!validMobile(form.phone)) {
			next.phone = 'Use an Algerian mobile number, for example 0550 12 34 56.';
		}
		if (!form.wilayaCode) next.wilaya_code = 'Choose a wilaya.';
		if (!form.communeName) next.commune_name = 'Choose a commune.';
		if (form.deliveryType === 'stopdesk') {
			if (!form.stopDeskId) next.stop_desk_id = 'Choose a stop desk.';
		} else if (!form.address.trim()) {
			next.address = 'Enter the delivery address.';
		}
		if (!captcha?.token) {
			next.captcha = 'The security check is not ready. Request a new code.';
		} else if (!captchaAnswer.trim()) {
			next.captcha = 'Enter the code from the security check.';
		}
		if (!termsAccepted) {
			next.terms = 'Accept the terms of service before placing the order.';
		}

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
			const focusId =
				firstInvalid === 'delivery_type'
					? 'delivery-home'
					: firstInvalid === 'captcha'
						? 'captcha-answer'
						: firstInvalid === 'terms'
							? 'terms'
							: firstInvalid;
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
				body: {
					name: form.name.trim(),
					email: form.email.trim(),
					phone: form.phone.trim(),
					address: form.deliveryType === 'home' ? form.address.trim() : '',
					wilaya_code: Number(form.wilayaCode),
					commune_name: form.communeName,
					delivery_type: form.deliveryType,
					stop_desk_id: form.deliveryType === 'stopdesk' ? form.stopDeskId : '',
					notes: form.notes.trim(),
					items: cart.items.map((line) => ({
						product_id: line.product_id,
						quantity: line.quantity
					})),
					website,
					captcha_token: captcha?.token ?? '',
					captcha_answer: captchaAnswer.trim(),
					terms_accepted: termsAccepted,
					terms_version: TERMS_VERSION
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
			let refreshCaptcha = false;
			if (err instanceof ApiError) {
				formError = err.message;
				errors = Object.fromEntries(
					Object.entries(err.fields).filter(([key]) =>
						FIELD_ORDER.includes(key as FieldKey)
					)
				);
				refreshCaptcha = Boolean(err.fields.captcha) || err.status === 409;
			} else {
				formError = 'The order could not be placed. Check your connection and try again.';
			}

			if (refreshCaptcha) await loadCaptcha();
			await tick();
			if (refreshCaptcha) {
				document.getElementById('captcha-answer')?.focus();
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
		<p class="loading" aria-live="polite">Loading your bag.</p>
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
						<label for="wilaya_code">Wilaya</label>
						<select
							id="wilaya_code"
							name="wilaya_code"
							required
							bind:value={form.wilayaCode}
							onchange={(event) => onWilayaChange(event.currentTarget.value)}
							disabled={wilayaState !== 'ready'}
							aria-invalid={errors.wilaya_code ? 'true' : undefined}
							aria-describedby={errors.wilaya_code ? 'wilaya-error' : undefined}
						>
							{#if wilayaState === 'loading'}
								<option value="">Reading wilayas...</option>
							{:else if wilayaState === 'error'}
								<option value="">Wilayas unavailable</option>
							{:else}
								<option value="" disabled>Choose a wilaya</option>
								{#each wilayas as wilaya (wilaya.code)}
									<option value={String(wilaya.code)}>{wilaya.nameFr}</option>
								{/each}
							{/if}
						</select>
						{#if errors.wilaya_code}
							<p class="field-error" id="wilaya-error">{errors.wilaya_code}</p>
						{/if}
					</div>

					<div class="field">
						<label for="commune_name">Commune</label>
						<select
							id="commune_name"
							name="commune_name"
							required
							bind:value={form.communeName}
							disabled={!form.wilayaCode || communeState !== 'ready' || communeLocked}
							aria-invalid={errors.commune_name ? 'true' : undefined}
							aria-describedby={errors.commune_name
								? 'commune-error'
								: communeLocked
									? 'commune-hint'
									: undefined}
						>
							{#if !form.wilayaCode}
								<option value="">Choose a wilaya first</option>
							{:else if communeState === 'loading'}
								<option value="">Reading communes...</option>
							{:else if communeState === 'error'}
								<option value="">Communes unavailable</option>
							{:else}
								<option value="" disabled>Choose a commune</option>
								{#each communes as commune (commune.nameFr)}
									<option value={commune.nameFr}>{commune.nameFr}</option>
								{/each}
							{/if}
						</select>
						{#if errors.commune_name}
							<p class="field-error" id="commune-error">{errors.commune_name}</p>
						{:else if communeLocked}
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
								checked={form.deliveryType === 'home'}
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
								checked={form.deliveryType === 'stopdesk'}
								onchange={() => onDeliveryTypeChange('stopdesk')}
							/>
							<span>
								<span>Stop desk</span>
								<span class="meta">Collect from a courier desk.</span>
							</span>
						</label>
					</fieldset>

					{#if form.deliveryType === 'home'}
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
							<label for="stop_desk_id">Stop desk</label>
							<select
								id="stop_desk_id"
								name="stop_desk_id"
								required
								bind:value={form.stopDeskId}
								onchange={(event) => onDeskChange(event.currentTarget.value)}
								disabled={deskState !== 'ready' || desks.length === 0}
								aria-invalid={errors.stop_desk_id ? 'true' : undefined}
								aria-describedby={errors.stop_desk_id ? 'stop-desk-error' : 'stop-desk-hint'}
							>
								{#if !form.wilayaCode}
									<option value="">Choose a wilaya first</option>
								{:else if deskState === 'loading'}
									<option value="">Reading stop desks...</option>
								{:else if deskState === 'error'}
									<option value="">Stop desks unavailable</option>
								{:else if desks.length === 0}
									<option value="">No stop desks in this wilaya</option>
								{:else}
									<option value="" disabled>Choose a stop desk</option>
									{#each desks as desk (desk.id)}
										<option value={desk.id}>
											{desk.name}{desk.communeName ? `, ${desk.communeName}` : ''}
										</option>
									{/each}
								{/if}
							</select>
							{#if errors.stop_desk_id}
								<p class="field-error" id="stop-desk-error">{errors.stop_desk_id}</p>
							{:else if deskState === 'ready' && desks.length === 0}
								<p class="meta" id="stop-desk-hint">No stop desks here. Choose home delivery.</p>
							{:else if selectedDesk?.address}
								<p class="meta" id="stop-desk-hint">{selectedDesk.address}</p>
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
							bind:value={form.notes}
							aria-describedby="notes-hint"
						></textarea>
						<p class="meta" id="notes-hint">
							Optional. Gate codes, floor, a time that suits you.
						</p>
					</div>
				</div>

				{#if wilayaState === 'error'}
					<div class="form-error" role="alert">
						<p>{mapError}</p>
						<button class="inline-action" type="button" onclick={loadWilayas}>Try again</button>
					</div>
				{:else if communeState === 'error'}
					<div class="form-error" role="alert">
						<p>{communeError}</p>
						<button
							class="inline-action"
							type="button"
							onclick={() => loadCommunes(Number(form.wilayaCode))}
						>
							Try again
						</button>
					</div>
				{:else if form.deliveryType === 'stopdesk' && deskState === 'error'}
					<div class="form-error" role="alert">
						<p>{deskError}</p>
						<button
							class="inline-action"
							type="button"
							onclick={() => loadDesks(Number(form.wilayaCode))}
						>
							Try again
						</button>
					</div>
				{:else if wilayaState === 'loading'}
					<p class="meta" aria-live="polite">Reading the delivery map.</p>
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

				<fieldset class="security-check">
					<legend>Security check</legend>
					<p class="meta">Type the five characters. This keeps automated orders off the ledger.</p>

					<div class="captcha-row">
						<div class="captcha-frame">
							{#if captchaState === 'ready' && captcha}
								<img
									src={captcha.image}
									alt="Verification code. Type the five characters shown."
									width="220"
									height="64"
								/>
							{:else if captchaState === 'loading'}
								<span class="loading">Loading code.</span>
							{:else}
								<span class="captcha-error" role="alert">{captchaError}</span>
							{/if}
						</div>
						<button
							class="btn btn-secondary btn-small"
							type="button"
							onclick={loadCaptcha}
							disabled={captchaState === 'loading'}
						>
							{captchaState === 'error' ? 'Try again' : 'New code'}
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
							bind:value={captchaAnswer}
							aria-invalid={errors.captcha ? 'true' : undefined}
							aria-describedby={errors.captcha ? 'captcha-error' : 'captcha-hint'}
						/>
						{#if errors.captcha}
							<p class="field-error" id="captcha-error">{errors.captcha}</p>
						{:else}
							<p class="meta" id="captcha-hint">
								Five characters, not case-sensitive. If the code is hard to read, request a new
								one or write to hello@azahar.example.
							</p>
						{/if}
					</div>
				</fieldset>

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
