<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { ApiError, apiFetch } from '#lib/api';
	import { auth } from '#lib/stores/auth.svelte';
	import { errorMessage } from '#lib/admin/api';
	import { theme, syncThemeColor } from '#lib/admin/theme.svelte';
	import '#lib/admin/admin.css';

	let email = $state('');
	let password = $state('');
	let error = $state('');
	let brandName = $state('Azahar');

	onMount(() => {
		apiFetch<{ theme: { brand_name: string } }>('/storefront/theme')
			.then((data) => {
				if (data.theme?.brand_name) brandName = data.theme.brand_name;
			})
			.catch(() => {
				// The login page still works with the default wordmark.
			});
	});

	$effect(() => syncThemeColor(theme.preset, theme.scheme));

	async function signIn(event: SubmitEvent) {
		event.preventDefault();
		error = '';

		if (!email.trim() || !password) {
			error = 'Enter your email and password.';
			return;
		}

		try {
			const user = await auth.login(email.trim(), password);
			if (user.role === 'admin' || user.role === 'employee') {
				await goto('/admin');
			} else {
				error = 'This account does not have access.';
			}
		} catch (caught) {
			if (caught instanceof ApiError) {
				error = caught.fields.email ?? caught.fields.password ?? caught.message;
			} else {
				error = errorMessage(caught);
			}
		}
	}
</script>

<svelte:head>
	<title>Sign in · {brandName} admin</title>
</svelte:head>

<main class="admin-login" data-preset={theme.preset} data-scheme={theme.scheme}>
	<div class="login-wrap">
		<div class="login-top">
			<p class="login-brand">{brandName}<span>.</span></p>
			<button
				class="a-theme-toggle"
				type="button"
				onclick={(event) => theme.toggleScheme(event.currentTarget)}
				aria-label={theme.scheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
				title={theme.scheme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
				disabled={theme.busy}
			>
				<svg
					class="a-theme-icon a-theme-icon-moon"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="1.6"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7Z" />
				</svg>
				<svg
					class="a-theme-icon a-theme-icon-sun"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="1.6"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<circle cx="12" cy="12" r="4" />
					<path
						d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M18.7 5.3l-1.4 1.4M6.7 17.3l-1.4 1.4"
					/>
				</svg>
			</button>
		</div>

		<div class="login-panel">
			<header class="login-head">
				<h1>Sign in</h1>
				<p>Orders, stock and the month’s takings.</p>
			</header>

			<form class="login-form" onsubmit={signIn} novalidate>
				<div class="a-field">
					<label for="login-email">Email</label>
					<input
						id="login-email"
						type="email"
						autocomplete="username"
						bind:value={email}
						required
						aria-describedby={error ? 'login-error' : undefined}
					/>
				</div>

				<div class="a-field">
					<label for="login-password">Password</label>
					<input
						id="login-password"
						type="password"
						autocomplete="current-password"
						bind:value={password}
						required
						aria-describedby={error ? 'login-error' : undefined}
					/>
				</div>

				{#if error}
					<p class="a-form-error" id="login-error" role="alert">{error}</p>
				{/if}

				<button class="a-btn a-btn-primary a-btn-block" type="submit" disabled={auth.busy}>
					{auth.busy ? 'Signing in…' : 'Sign in'}
				</button>
			</form>

			{#if import.meta.env.DEV}
				<aside class="login-demo" aria-labelledby="demo-access-title">
					<p class="login-demo-title" id="demo-access-title">Demo access</p>
					<p>Admin <code>admin@azahar.example</code></p>
					<p>Password <code>azahar-dev-admin-2026</code></p>
					<p>Staff <code>staff@azahar.example</code> / <code>azahar-dev-staff-2026</code></p>
					<p class="a-muted-sm">Available when the seeded API is running.</p>
				</aside>
			{/if}
		</div>

		<footer class="login-foot">
			<span>{brandName} admin.</span>
			<a class="a-link" href="/">View store</a>
		</footer>
	</div>
</main>
