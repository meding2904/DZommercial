<script lang="ts">
	import { onMount } from 'svelte';
	import ProductImage from '#lib/components/ProductImage.svelte';
	import ProductCard from '#lib/components/ProductCard.svelte';
	import WordStatement from '#lib/components/WordStatement.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const theme = $derived(data.theme);
	const shelf = $derived(data.products.slice(0, 8));

	onMount(() => {
		const hash = window.location.hash.slice(1);
		if (!hash) return;
		const frame = requestAnimationFrame(() => {
			document.getElementById(hash)?.scrollIntoView({ block: 'start' });
		});
		return () => cancelAnimationFrame(frame);
	});
</script>

<svelte:head>
	<title>{theme.brand_name} | {theme.tagline}</title>
	<meta name="description" content={theme.description} />
	<meta property="og:title" content={`${theme.brand_name} | ${theme.tagline}`} />
	<meta property="og:description" content={theme.description} />
	<meta property="og:type" content="website" />
</svelte:head>

<section class="hero">
	<div class="wrap">
		<div class="hero-head">
			<h1 data-enter>{theme.hero_heading}</h1>
			<div class="hero-aside" data-enter="2">
				<p class="lead">{theme.hero_body}</p>
				<div class="hero-actions">
					<a class="btn btn-primary" href="/shop">{theme.hero_cta}</a>
					<a class="btn btn-secondary" href="/about">Read the story</a>
				</div>
			</div>
		</div>
	</div>
	<div class="hero-stage">
		<ProductImage
			src={theme.hero_image}
			alt={theme.hero_alt}
			width={1600}
			height={900}
			sizes="100vw"
			priority
			class="hero-image"
		/>
	</div>
	<div class="wrap">
		<div class="hero-footnote">
			<span>{theme.tagline}</span>
			{#if theme.announcement}
				<span>{theme.announcement}</span>
			{/if}
		</div>
	</div>
</section>

<WordStatement text={theme.statement} />

<section class="section section-rule" aria-labelledby="shelf-title">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">The shelf</p>
				<h2 id="shelf-title">{theme.shelf_heading}</h2>
			</div>
			<a class="text-link" href="/shop">See the whole shelf</a>
		</div>

		{#if data.apiDown}
			<div class="empty-state">
				<p class="meta">The shelf is offline</p>
				<h2>The shop could not load.</h2>
				<p class="muted">The API is not answering. Reload the page when the service is back.</p>
				<button class="btn btn-secondary" type="button" onclick={() => location.reload()}>
					Reload the shelf
				</button>
			</div>
		{:else if shelf.length === 0}
			<div class="empty-state">
				<p class="meta">Restocking</p>
				<h2>The shelf is being restocked.</h2>
				<p class="muted">The next batch is on its way. Come back soon.</p>
			</div>
		{:else}
			<div class="product-grid">
				{#each shelf as product (product.id)}
					<ProductCard {product} />
				{/each}
			</div>
		{/if}
	</div>
</section>

<section class="section" aria-labelledby="categories-title">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">Browse</p>
				<h2 id="categories-title">Start where the shelf asks.</h2>
			</div>
		</div>
	</div>
	<div class="category-band">
		{#each data.categories as category}
			<a href={`/shop?category=${category.slug}`}>
				<strong>{category.name}</strong>
				<span>{category.description}</span>
			</a>
		{/each}
	</div>
</section>

<section class="section section-rule" id="story" aria-labelledby="story-title">
	<div class="wrap editorial">
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
			<h2 id="story-title">{theme.about_heading}</h2>
			<p>{theme.about_body}</p>
			<a class="text-link" href="/about">Read the story</a>
		</div>
	</div>
</section>

<section class="section section-rule">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">The shelf is open</p>
				<h2>{theme.closing_heading}</h2>
			</div>
			<a class="btn btn-primary" href="/shop">{theme.hero_cta}</a>
		</div>
	</div>
</section>
