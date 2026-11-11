<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { ApiError, apiFetch } from '#lib/api';
	import StatusPill from '#lib/components/StatusPill.svelte';
	import { formatDate, money } from '#lib/format';
	import { auth } from '#lib/stores/auth.svelte';
	import type { OrderSummary } from '#lib/types';

	type Mode = 'signin' | 'register';
	type OrdersState = 'idle' | 'loading' | 'ready' | 'error';

	const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

	let mode = $state<Mode>('signin');
	let signinEmail = $state('');
	let signinPassword = $state('');
	let registerName = $state('');
	let registerEmail = $state('');
	let registerPassword = $state('');

	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let errorBox = $state<HTMLDivElement | null>(null);

	let sessionChecked = $state(false);
	let orders = $state<OrderSummary[]>([]);
	let ordersState = $state<OrdersState>('idle');
	let ordersError = $state('');

	const user = $derived(auth.user);

	onMount(() => {
		void boot();
	});

	async function boot(): Promise<void> {
		await auth.refresh();
		sessionChecked = true;

		if (auth.user) {
			await loadOrders();
		} else if (auth.error) {
			formError = auth.error;
		}
	}

	async function loadOrders(): Promise<void> {
		ordersState = 'loading';
		ordersError = '';

		try {
			const data = await apiFetch<{ orders: OrderSummary[] }>('/account/orders');
			orders = data.orders;
			ordersState = 'ready';
		} catch (err) {
			if (err instanceof ApiError && err.status === 401) {
				auth.user = null;
				orders = [];
				ordersState = 'idle';
				fieldErrors = {};
				formError = 'Your session ended. Sign in again.';
				return;
			}

			ordersError = err instanceof Error ? err.message : 'Your orders could not load.';
			ordersState = 'error';
		}
	}

	function setMode(next: Mode): void {
		if (next === mode) return;
		mode = next;
		fieldErrors = {};
		formError = '';
	}

	async function onTabKey(event: KeyboardEvent, current: Mode): Promise<void> {
		if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
		event.preventDefault();

		const next: Mode =
			event.key === 'Home'
				? 'signin'
				: event.key === 'End'
					? 'register'
					: current === 'signin'
						? 'register'
						: 'signin';

		setMode(next);
		await tick();
		document.getElementById(`${next}-tab`)?.focus();
	}

	async function submit(event: SubmitEvent, which: Mode): Promise<void> {
		event.preventDefault();
		if (auth.busy) return;

		fieldErrors = {};
		formError = '';

		const email = (which === 'signin' ? signinEmail : registerEmail).trim();
		const password = which === 'signin' ? signinPassword : registerPassword;
		const name = registerName.trim();
		const next: Record<string, string> = {};

		if (which === 'register' && !name) next.name = 'Enter your name.';
		if (!email) next.email = 'Enter your email.';
		else if (!EMAIL_PATTERN.test(email)) next.email = 'Enter a valid email.';
		if (!password) next.password = 'Enter your password.';
		else if (which === 'register' && password.length < 10) {
			next.password = 'Use at least 10 characters.';
		}

		fieldErrors = next;

		if (Object.keys(next).length > 0) {
			formError = 'Check the marked fields.';
			const first = next.name
				? 'register-name'
				: next.email
					? `${which}-email`
					: `${which}-password`;
			await tick();
			document.getElementById(first)?.focus();
			return;
		}

		try {
			if (which === 'signin') {
				await auth.login(email, password);
			} else {
				await auth.register(name, email, password);
			}

			signinPassword = '';
			registerPassword = '';
			await loadOrders();
		} catch (err) {
			if (err instanceof ApiError) {
				fieldErrors = err.fields;
				formError = err.message;
			} else {
				formError = 'The house could not be reached. Try again.';
			}

			await tick();
			errorBox?.focus();
		}
	}

	async function signOut(): Promise<void> {
		try {
			await auth.logout();
		} catch {
			auth.user = null;
		}

		orders = [];
		ordersState = 'idle';
		ordersError = '';
		setMode('signin');
	}
</script>

<svelte:head>
	<title>Your account | Azahar</title>
	<meta
		name="description"
		content="Sign in to see your order summaries, or check out as a guest and keep the confirmation link."
	/>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="wrap page-head">
	<h1>Your account</h1>
	<p class="lead">Sign in to keep your order summaries in one place, or check out as a guest.</p>
</div>

<div class="wrap">
	{#if !sessionChecked}
		<p class="loading">Checking your session.</p>
	{:else if user}
		<div class="account-layout">
			<section class="account-side" aria-labelledby="account-name">
				<p class="meta">Signed in</p>
				<h2 id="account-name">Hello, {user.name}.</h2>
				<p class="muted">{user.email}</p>
				<button
					class="btn btn-secondary signout"
					type="button"
					onclick={signOut}
					disabled={auth.busy}
				>
					{auth.busy ? 'Signing out...' : 'Sign out'}
				</button>
			</section>

			<section class="orders-panel" aria-labelledby="orders-title">
				<div class="orders-head">
					<h2 id="orders-title">Orders</h2>
					{#if ordersState === 'ready' && orders.length > 0}
						<p class="meta">
							{orders.length} {orders.length === 1 ? 'order' : 'orders'}
						</p>
					{/if}
				</div>

				{#if ordersState === 'loading'}
					<p class="loading">Loading your orders.</p>
				{:else if ordersState === 'error'}
					<div class="orders-error">
						<p class="form-error" role="alert">{ordersError}</p>
						<button class="btn btn-secondary" type="button" onclick={() => void loadOrders()}>
							Try again
						</button>
					</div>
				{:else if ordersState === 'ready' && orders.length === 0}
					<div class="empty-state orders-empty">
						<p class="meta">No orders</p>
						<h3>No orders yet.</h3>
						<p class="muted">When you place an order, the summary appears here.</p>
						<a class="btn btn-secondary" href="/shop">Shop the shelf</a>
					</div>
				{:else if ordersState === 'ready'}
					<div class="order-list">
						{#each orders as order (order.id)}
							<article class="order-row">
								<div>
									<h3>{order.number}</h3>
									<p class="meta">{formatDate(order.created_at)}, {money(order.total_cents)}</p>
								</div>
								<StatusPill status={order.status} />
							</article>
						{/each}
					</div>
				{/if}

				<p class="meta order-note">
					The confirmation link from checkout is the way to reopen an order. Your account
					keeps the summary, not the link.
				</p>
			</section>
		</div>
	{:else}
		<div class="account-layout">
			<section class="auth-panel" aria-labelledby="access-title">
				<h2 id="access-title">Access your orders</h2>

				{#if formError}
					<div class="form-error" role="alert" tabindex="-1" bind:this={errorBox}>
						{formError}
					</div>
				{/if}

				<div class="auth-switch" role="tablist" aria-label="Account access">
					<button
						id="signin-tab"
						type="button"
						role="tab"
						aria-selected={mode === 'signin'}
						aria-controls="signin-panel"
						tabindex={mode === 'signin' ? 0 : -1}
						onclick={() => setMode('signin')}
						onkeydown={(event) => void onTabKey(event, 'signin')}
					>
						Sign in
					</button>
					<button
						id="register-tab"
						type="button"
						role="tab"
						aria-selected={mode === 'register'}
						aria-controls="register-panel"
						tabindex={mode === 'register' ? 0 : -1}
						onclick={() => setMode('register')}
						onkeydown={(event) => void onTabKey(event, 'register')}
					>
						Create account
					</button>
				</div>

				<div
					id="signin-panel"
					role="tabpanel"
					aria-labelledby="signin-tab"
					hidden={mode !== 'signin'}
				>
					<form
						class="auth-form"
						onsubmit={(event) => void submit(event, 'signin')}
						novalidate
					>
						<div class="field">
							<label for="signin-email">Email</label>
							<input
								id="signin-email"
								name="email"
								type="email"
								inputmode="email"
								autocomplete="email"
								required
								bind:value={signinEmail}
								aria-invalid={fieldErrors.email ? 'true' : undefined}
								aria-describedby={fieldErrors.email ? 'signin-email-error' : undefined}
							/>
							{#if fieldErrors.email}
								<p class="field-error" id="signin-email-error">{fieldErrors.email}</p>
							{/if}
						</div>

						<div class="field">
							<label for="signin-password">Password</label>
							<input
								id="signin-password"
								name="password"
								type="password"
								autocomplete="current-password"
								required
								bind:value={signinPassword}
								aria-invalid={fieldErrors.password ? 'true' : undefined}
								aria-describedby={fieldErrors.password ? 'signin-password-error' : undefined}
							/>
							{#if fieldErrors.password}
								<p class="field-error" id="signin-password-error">{fieldErrors.password}</p>
							{/if}
						</div>

						<button
							class="btn btn-primary btn-block"
							type="submit"
							disabled={auth.busy}
						>
							{auth.busy ? 'Signing in...' : 'Sign in'}
						</button>
					</form>
				</div>

				<div
					id="register-panel"
					role="tabpanel"
					aria-labelledby="register-tab"
					hidden={mode !== 'register'}
				>
					<form
						class="auth-form"
						onsubmit={(event) => void submit(event, 'register')}
						novalidate
					>
						<div class="field">
							<label for="register-name">Name</label>
							<input
								id="register-name"
								name="name"
								type="text"
								autocomplete="name"
								required
								bind:value={registerName}
								aria-invalid={fieldErrors.name ? 'true' : undefined}
								aria-describedby={fieldErrors.name ? 'register-name-error' : undefined}
							/>
							{#if fieldErrors.name}
								<p class="field-error" id="register-name-error">{fieldErrors.name}</p>
							{/if}
						</div>

						<div class="field">
							<label for="register-email">Email</label>
							<input
								id="register-email"
								name="email"
								type="email"
								inputmode="email"
								autocomplete="email"
								required
								bind:value={registerEmail}
								aria-invalid={fieldErrors.email ? 'true' : undefined}
								aria-describedby={fieldErrors.email ? 'register-email-error' : undefined}
							/>
							{#if fieldErrors.email}
								<p class="field-error" id="register-email-error">{fieldErrors.email}</p>
							{/if}
						</div>

						<div class="field">
							<label for="register-password">Password</label>
							<input
								id="register-password"
								name="password"
								type="password"
								autocomplete="new-password"
								required
								bind:value={registerPassword}
								aria-invalid={fieldErrors.password ? 'true' : undefined}
								aria-describedby={fieldErrors.password
									? 'register-password-hint register-password-error'
									: 'register-password-hint'}
							/>
							{#if fieldErrors.password}
								<p class="field-error" id="register-password-error">{fieldErrors.password}</p>
							{/if}
							<p class="meta" id="register-password-hint">At least 10 characters.</p>
						</div>

						<button
							class="btn btn-primary btn-block"
							type="submit"
							disabled={auth.busy}
						>
							{auth.busy ? 'Creating the account...' : 'Create account'}
						</button>
					</form>
				</div>
			</section>

			<aside class="account-side" aria-labelledby="guest-title">
				<p class="meta">Guest checkout</p>
				<h2 id="guest-title">Orders without an account</h2>
				<p class="muted">
					You can check out as a guest. Keep the confirmation link from checkout; it is the
					way to reopen the order.
				</p>
				<a class="text-link guest-link" href="/shop">Shop the shelf</a>
			</aside>
		</div>
	{/if}
</div>

<style>
	.account-side {
		display: grid;
		gap: 10px;
		align-content: start;
		padding-top: 24px;
		border-top: 1px solid var(--line);
	}

	.account-side h2 {
		font-size: clamp(26px, 3vw, 34px);
	}

	.signout {
		justify-self: start;
		margin-top: 8px;
	}

	.auth-panel h2 {
		font-size: 24px;
	}

	.auth-form {
		display: grid;
		gap: 16px;
		padding-top: 18px;
	}

	[role='tabpanel'][hidden] {
		display: none;
	}

	.guest-link {
		justify-self: start;
		margin-top: 6px;
		min-height: 44px;
	}

	.orders-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 16px;
		margin-bottom: 14px;
	}

	.orders-head h2 {
		font-size: clamp(28px, 3.4vw, 40px);
	}

	.orders-error {
		display: grid;
		gap: 14px;
		justify-items: start;
	}

	.orders-empty {
		padding-block: 44px;
	}

	.orders-empty h3 {
		font-size: 30px;
	}

	.order-note {
		max-width: 54ch;
		margin-top: 22px;
	}

	@media (max-width: 900px) {
		.account-side {
			padding-top: 0;
			border-top: 0;
		}
	}
</style>
