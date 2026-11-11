<script lang="ts">
	import { onMount } from 'svelte';
	import ProductImage from '#lib/components/ProductImage.svelte';
	import ProductCard from '#lib/components/ProductCard.svelte';
	import WordStatement from '#lib/components/WordStatement.svelte';
	import Chapters from '#lib/components/Chapters.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const order = [
		'orange-blossom-cleansing-oil',
		'rosehip-and-olive-face-serum',
		'rose-clay-mask',
		'fig-and-sesame-body-balm',
		'prickly-pear-face-oil',
		'cedar-and-myrrh-hand-cream',
		'green-fig-body-oil',
		'rose-petal-bath-soak'
	];

	const shelf = $derived(
		[...data.products]
			.sort((a, b) => {
				const ai = order.indexOf(a.slug);
				const bi = order.indexOf(b.slug);
				return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
			})
			.slice(0, 8)
	);

	const chapters = [
		{
			title: 'The grove',
			body: 'Oranges from the Mitidja, picked for the blossom and the oil. The scent of the city in spring, kept in a bottle.',
			image: '/images/moments/grove-1400.webp',
			alt: 'Oranges and green leaves in a grove'
		},
		{
			title: 'The press',
			body: 'Olives pressed cold, within a day of picking. The oil is green, peppery, and close to the fruit.',
			image: '/images/moments/olive-1200.webp',
			alt: 'Olive oil in a glass jug with olives and olive leaves'
		},
		{
			title: 'The jar',
			body: 'Blended in small batches and poured into amber glass. No filler, no fragrance to hide the materials.',
			image: '/images/moments/bottles-1200.webp',
			alt: 'Amber glass bottles with pumps and droppers on a plaster surface'
		}
	];

	const ingredients = [
		['Olive squalane', 'Pressed, light, close to skin'],
		['Orange blossom', 'The city in spring'],
		['Rose clay', 'Soft pink, drawn from the earth'],
		['Prickly pear', 'Seed oil from the dry south'],
		['Fig leaf', 'Green, milky, warm'],
		['Cedar and myrrh', 'Smoke without sweetness']
	];

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
	<title>Azahar, botanical skincare from the Mediterranean edge</title>
	<meta
		name="description"
		content="Oils, clays and blossoms, blended in small batches in Algiers. Eight formulas for the face, the body and the bath."
	/>
	<meta property="og:title" content="Azahar, botanical skincare" />
	<meta
		property="og:description"
		content="Oils, clays and blossoms from the Mediterranean edge. A demo storefront built with Go, Svelte and Turso."
	/>
	<meta property="og:type" content="website" />
</svelte:head>

<section class="hero">
	<div class="wrap">
		<div class="hero-head">
			<h1 data-enter>Botanical skincare from the Mediterranean edge.</h1>
			<div class="hero-aside" data-enter="2">
				<p class="lead">
					Oils, clays and blossoms, blended in small batches in Algiers. Eight formulas for the
					face, the body and the bath.
				</p>
				<div class="hero-actions">
					<a class="btn btn-primary" href="/shop">Shop the eight formulas</a>
					<a class="btn btn-secondary" href="/#ingredients">Read the ingredients</a>
				</div>
			</div>
		</div>
	</div>
	<div class="hero-stage">
		<ProductImage
			src="/images/hero-1600.webp"
			alt="Amber glass bottles and a linen cloth on a plaster shelf"
			width={1600}
			height={900}
			sizes="100vw"
			priority
			class="hero-image"
		/>
	</div>
	<div class="wrap">
		<div class="hero-footnote">
			<span>Eight formulas, made in Algiers.</span>
			<span>Cash on delivery across Algeria.</span>
		</div>
	</div>
</section>

<WordStatement
	text="We make fewer things, with more of the plant in them. The shelf is short on purpose."
	accent="plant"
/>

<section class="section section-rule" aria-labelledby="shelf-title">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">The shelf</p>
				<h2 id="shelf-title">Eight formulas for face, body and the bath.</h2>
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

<section class="section" aria-labelledby="chapters-title">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">The material</p>
				<h2 id="chapters-title">From the grove to the jar.</h2>
			</div>
			<p class="lead">Three materials carry the shelf. The rest is water, clay and time.</p>
		</div>
		<Chapters {chapters} />
	</div>
</section>

<section class="section" aria-labelledby="categories-title">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">Three shelves</p>
				<h2 id="categories-title">Start where the skin asks.</h2>
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

<section class="section section-rule" id="ingredients" aria-labelledby="ingredients-title">
	<div class="wrap editorial">
		<div class="editorial-media">
			<ProductImage
				src="/images/atelier-1200.webp"
				alt="Amber bottles, ceramic jars, linen and dried botanicals on a work table"
				width={1200}
				height={1500}
				sizes="(max-width: 900px) 100vw, 50vw"
			/>
		</div>
		<div class="editorial-copy">
			<p class="meta">The house</p>
			<h2 id="ingredients-title">A small shelf in Algiers.</h2>
			<p>
				Azahar began with the orange blossom that scents the city in spring. We press olives and
				figs, mix clays with rose, and pour everything into amber glass. The shelf is short on
				purpose.
			</p>
			<div class="ingredient-list">
				{#each ingredients as [name, note]}
					<div>
						<span>{name}</span>
						<span>{note}</span>
					</div>
				{/each}
			</div>
			<a class="text-link" href="/about">Read the house story</a>
		</div>
	</div>
</section>

<section class="section section-rule">
	<div class="wrap">
		<div class="section-head">
			<div>
				<p class="meta">The shelf is open</p>
				<h2>Take the whole shelf, or one jar.</h2>
			</div>
			<a class="btn btn-primary" href="/shop">Shop the shelf</a>
		</div>
	</div>
</section>
