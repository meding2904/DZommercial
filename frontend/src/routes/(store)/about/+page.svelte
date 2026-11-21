<script lang="ts">
	import ProductImage from '#lib/components/ProductImage.svelte';
	import type { StorefrontTheme } from '#lib/theme/storefront';
	import type { PageData } from './$types';

	let { data }: { data: PageData & { theme: StorefrontTheme } } = $props();
	const theme = $derived(data.theme);
</script>

<svelte:head>
	<title>{theme.house_label} | {theme.brand_name}</title>
	<meta name="description" content={theme.description} />
	<meta property="og:title" content={`${theme.house_label} | ${theme.brand_name}`} />
	<meta property="og:description" content={theme.description} />
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<h1>{theme.about_heading}</h1>
	<p class="lead">{theme.tagline}</p>
</div>

<div class="wrap editorial about-editorial">
	<div class="editorial-media">
		<ProductImage
			src={theme.about_image}
			alt={theme.about_alt}
			width={1200}
			height={1500}
			sizes="(max-width: 900px) 100vw, 50vw"
		/>
	</div>

	<div class="editorial-copy">
		<p class="meta">{theme.house_label}</p>
		<h2>{theme.statement}</h2>
		<p class="story-body">{theme.about_body}</p>
	</div>
</div>

<section class="section section-rule about-materials" aria-labelledby="materials-title">
	<div class="wrap prose">
		<h2 id="materials-title">The shelf</h2>
		<p>{theme.shelf_heading}</p>
		{#if theme.announcement}
			<p>{theme.announcement}</p>
		{/if}
		<a class="btn btn-primary about-link" href="/shop">{theme.hero_cta}</a>
	</div>
</section>

<style>
	.about-editorial {
		padding-bottom: 24px;
	}

	.about-materials {
		padding-top: 72px;
	}

	.about-link {
		justify-self: start;
		margin-top: 12px;
	}

	.story-body {
		white-space: pre-line;
	}
</style>
