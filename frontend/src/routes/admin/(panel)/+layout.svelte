<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { auth } from '#lib/stores/auth.svelte';
	import { theme, syncThemeColor } from '#lib/admin/theme.svelte';
	import PresetSwitch from '#lib/components/admin/PresetSwitch.svelte';
	import '#lib/admin/admin.css';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	$effect(() => syncThemeColor(theme.preset, theme.scheme));

	let access = $state(false);
	let menuOpen = $state(false);
	let signingOut = $state(false);
	let menuButton = $state<HTMLButtonElement | null>(null);

	const navItems = [
		{ href: '/admin', label: 'Overview' },
		{ href: '/admin/orders', label: 'Orders' },
		{ href: '/admin/products', label: 'Products' },
		{ href: '/admin/customers', label: 'Customers' }
	];

	const activeIndex = $derived(
		Math.max(
			0,
			navItems.findIndex((item) =>
				item.href === '/admin'
					? page.url.pathname === '/admin'
					: page.url.pathname.startsWith(item.href)
			)
		)
	);

	onMount(() => {
		let live = true;

		(async () => {
			await auth.refresh();
			if (!live) return;

			if (!auth.isAdmin) {
				await goto('/admin/login', { replace: true });
				return;
			}

			await theme.loadRemote();
			if (!live) return;
			access = true;
		})();

		return () => {
			live = false;
		};
	});

	function closeMenu() {
		menuOpen = false;
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && menuOpen) {
			menuOpen = false;
			menuButton?.focus();
		}
	}

	async function signOut() {
		signingOut = true;
		try {
			await auth.logout();
		} catch {
			// The local session is cleared even when the request fails.
		}
		await goto('/admin/login', { replace: true });
	}
</script>

<svelte:window onkeydown={onKeydown} />

{#if !access}
	<main class="admin-gate" data-preset={theme.preset} data-scheme={theme.scheme}>
		<div class="gate-inner">
			<p class="gate-wordmark">Azahar<span>.</span></p>
			<p class="gate-status" role="status">
				<span class="a-spinner" aria-hidden="true"></span>
				Checking access…
			</p>
		</div>
	</main>
{:else}
	<div class="admin-shell" data-preset={theme.preset} data-scheme={theme.scheme}>
		<a class="a-skip" href="#admin-main">Skip to content</a>

		<aside class="admin-rail" aria-label="Admin">
			<header class="rail-head">
				<div class="rail-brand">
					<a class="rail-wordmark" href="/admin" onclick={closeMenu}>
						Azahar<span>.</span>
					</a>
					<p class="rail-role">Admin ledger</p>
				</div>
				<div class="rail-actions">
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
					<button
						bind:this={menuButton}
						class="rail-menu-btn"
						type="button"
						aria-expanded={menuOpen}
						aria-controls="admin-rail-panel"
						onclick={() => (menuOpen = !menuOpen)}
					>
						{menuOpen ? 'Close' : 'Menu'}
					</button>
				</div>
			</header>

			<div class="rail-panel" id="admin-rail-panel" data-open={menuOpen}>
				<nav class="rail-nav" aria-label="Admin sections" style={`--nav-index: ${activeIndex}`}>
					<span class="rail-marker" aria-hidden="true"></span>
					{#each navItems as item, index (item.href)}
						<a
							class="rail-link"
							href={item.href}
							aria-current={index === activeIndex ? 'page' : undefined}
							onclick={closeMenu}
						>
							{item.label}
						</a>
					{/each}
				</nav>

				<div class="rail-tools">
					<a class="rail-link" href="/" onclick={closeMenu}>View store</a>
					<button class="rail-signout" type="button" onclick={signOut} disabled={signingOut}>
						{signingOut ? 'Signing out…' : 'Sign out'}
					</button>
				</div>

				<footer class="rail-foot">
					<PresetSwitch />
				</footer>
			</div>
		</aside>

		<main class="admin-main" id="admin-main" tabindex="-1">
			{@render children()}
		</main>
	</div>
{/if}
