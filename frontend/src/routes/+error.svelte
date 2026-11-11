<script lang="ts">
	import { onMount } from 'svelte';
	import Header from '#lib/components/Header.svelte';
	import Footer from '#lib/components/Footer.svelte';
	import Toast from '#lib/components/Toast.svelte';
	import NotFound from '#lib/components/NotFound.svelte';
	import { auth } from '#lib/stores/auth.svelte';

	onMount(() => {
		auth.refresh();
	});
</script>

<a class="skip-link" href="#main">Skip to content</a>
<Header />
<main id="main">
	<NotFound />
</main>
<Footer />
<Toast />
