<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { auth } from '#lib/stores/auth.svelte';
	import { theme } from '#lib/admin/theme.svelte';
	import PresetSwitch from '#lib/components/admin/PresetSwitch.svelte';
	import '#lib/admin/admin.css';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

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

			access = true;
			void theme.loadRemote();
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
	<main class="admin-gate" data-preset={theme.preset}>
		<div class="gate-inner">
			<p class="gate-wordmark">Azahar<span>.</span></p>
			<p class="gate-status" role="status">
				<span class="a-spinner" aria-hidden="true"></span>
				Checking access...
			</p>
		</div>
	</main>
{:else}
	<div class="admin-shell" data-preset={theme.preset}>
		<a class="a-skip" href="#admin-main">Skip to content</a>

		<aside class="admin-rail" aria-label="Admin">
			<header class="rail-head">
				<a class="rail-wordmark" href="/admin" onclick={closeMenu}>
					Azahar<span>.</span>
				</a>
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
						{signingOut ? 'Signing out...' : 'Sign out'}
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
