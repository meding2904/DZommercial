<script lang="ts">
	import { onMount } from 'svelte';
	import Header from '#lib/components/Header.svelte';
	import Footer from '#lib/components/Footer.svelte';
	import Toast from '#lib/components/Toast.svelte';
	import { auth } from '#lib/stores/auth.svelte';
	import { themeCss } from '#lib/theme/storefront';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	const theme = $derived(data.theme);
	const pages = $derived(data.pages ?? []);

	onMount(() => {
		auth.refresh();
	});
</script>

<svelte:head>
	{@html `<style>${themeCss(theme)}</style>`}
	<meta name="theme-color" content={theme.light.ground} />
</svelte:head>

<a class="skip-link" href="#main">Skip to content</a>
<Header {theme} {pages} />
<main id="main">
	{@render children()}
</main>
<Footer {theme} />
<Toast />
