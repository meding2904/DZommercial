<script lang="ts">
	import { page } from '$app/state';
	import { auth } from '#lib/stores/auth.svelte';
	import { cart } from '#lib/stores/cart.svelte';
	import { storeTheme } from '#lib/stores/storeTheme.svelte';

	let menu = $state<HTMLDialogElement | null>(null);
	let menuOpen = $state(false);
	let ingredientsActive = $state(false);

	const links = [
		{ href: '/shop', label: 'Shop' },
		{ href: '/about', label: 'The house' },
		{ href: '/#ingredients', label: 'Ingredients' },
		{ href: '/contact', label: 'Contact' }
	];

	const bagLabel = $derived(`Bag, ${cart.count} ${cart.count === 1 ? 'item' : 'items'}`);

	function isCurrent(href: string): boolean {
		if (href.startsWith('/#')) {
			return (
				page.url.pathname === '/' &&
				(page.url.hash === href.slice(1) || (href === '/#ingredients' && ingredientsActive))
			);
		}
		return page.url.pathname === href || page.url.pathname.startsWith(`${href}/`);
	}

	$effect(() => {
		if (page.url.pathname !== '/') ingredientsActive = false;
	});

	function goToIngredients(event: MouseEvent): void {
		if (page.url.pathname !== '/') return;
		event.preventDefault();
		ingredientsActive = true;
		history.replaceState(null, '', '#ingredients');
		const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		document
			.getElementById('ingredients')
			?.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'start' });
	}

	function openMenu(): void {
		menu?.showModal();
		menuOpen = true;
		document.body.classList.add('menu-open');
	}

	function closeMenu(): void {
		menu?.close();
		menuOpen = false;
		document.body.classList.remove('menu-open');
	}
</script>

<header class="site-header">
	<div class="wrap header-inner">
		<a class="wordmark" href="/" aria-label="Azahar home">Azahar<span>.</span></a>

		<nav class="site-nav" aria-label="Main">
			{#each links as link}
				<a
					href={link.href}
					aria-current={isCurrent(link.href) ? 'page' : undefined}
					onclick={link.href === '/#ingredients' ? goToIngredients : undefined}
				>
					{link.label}
				</a>
			{/each}
		</nav>

		<div class="header-tools">
			<button
				class="theme-toggle"
				type="button"
				onclick={(event) => storeTheme.toggle(event.currentTarget)}
				aria-label={storeTheme.current === 'dark'
					? 'Switch to light mode'
					: 'Switch to dark mode'}
				title={storeTheme.current === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
				disabled={storeTheme.busy}
			>
				<svg
					class="theme-icon theme-icon-moon"
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
					class="theme-icon theme-icon-sun"
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
			<a class="text-link" href="/account">{auth.user ? auth.user.name.split(' ')[0] : 'Account'}</a>
			<a class="bag-link" href="/cart" aria-label={bagLabel}>
				<span>Bag</span>
				{#key cart.count}
					<span class="bag-count" data-pop="true" aria-hidden="true">{cart.count}</span>
				{/key}
			</a>
			<button
				class="menu-btn"
				type="button"
				onclick={openMenu}
				aria-haspopup="dialog"
				aria-expanded={menuOpen}
				aria-controls="site-menu"
			>
				Menu
			</button>
		</div>
	</div>
</header>

<dialog
	id="site-menu"
	class="menu-dialog"
	aria-label="Menu"
	bind:this={menu}
	onclose={() => {
		menuOpen = false;
		document.body.classList.remove('menu-open');
	}}
>
	<div class="menu-panel">
		<div class="menu-top">
			<a class="wordmark" href="/" onclick={closeMenu} aria-label="Azahar home">Azahar<span>.</span></a>
			<button class="menu-close" type="button" onclick={closeMenu}>Close</button>
		</div>
		<nav class="menu-links" aria-label="Mobile">
			{#each links as link}
				<a
					href={link.href}
					aria-current={isCurrent(link.href) ? 'page' : undefined}
					onclick={(event) => {
						if (link.href === '/#ingredients' && page.url.pathname === '/') goToIngredients(event);
						closeMenu();
					}}>{link.label}</a
				>
			{/each}
		</nav>
		<div class="menu-foot">
			<a href="/account" onclick={closeMenu}>{auth.user ? 'Your account' : 'Sign in'}</a>
			<a href="/cart" onclick={closeMenu}>Bag, {cart.count} {cart.count === 1 ? 'item' : 'items'}</a>
		</div>
	</div>
</dialog>
