<script lang="ts">
	import { page } from '$app/state';
	import ProductCard from '#lib/components/ProductCard.svelte';
	import ProductImage from '#lib/components/ProductImage.svelte';
	import AddToBag from '#lib/components/store/AddToBag.svelte';
	import { money } from '#lib/format';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const product = $derived(data.product);
	const related = $derived(data.related);
	const onSale = $derived(
		product.compare_at_cents !== null && product.compare_at_cents > product.price_cents
	);
	const extraImages = $derived(product.gallery.filter((src) => src !== product.image));
</script>

<svelte:head>
	<title>{product.name} | Azahar</title>
	<meta name="description" content={product.description} />
	<meta property="og:title" content={`${product.name} | Azahar`} />
	<meta property="og:description" content={product.description} />
	<meta property="og:type" content="product" />
	<meta property="og:image" content={`${page.url.origin}${product.image}`} />
</svelte:head>

<div class="wrap">
	<nav class="crumbs" aria-label="Breadcrumb">
		<a href="/">Home</a>
		<span aria-hidden="true">/</span>
		<a href="/shop">Shop</a>
		<span aria-hidden="true">/</span>
		<a href={`/shop?category=${product.category.slug}`}>{product.category.name}</a>
		<span aria-hidden="true">/</span>
		<span aria-current="page">{product.name}</span>
	</nav>

	<div class="product-detail">
		<div class="product-gallery">
			<ProductImage
				src={product.image}
				alt={product.name}
				width={1200}
				height={1500}
				sizes="(max-width: 900px) 100vw, 58vw"
				priority
			/>
			{#if extraImages.length > 0}
				<div class="gallery-extra">
					{#each extraImages as src, index (src)}
						<ProductImage
							{src}
							alt={`${product.name}, view ${index + 2}`}
							width={1200}
							height={1500}
							sizes="(max-width: 900px) 50vw, 28vw"
						/>
					{/each}
				</div>
			{/if}
		</div>

		<div class="product-info">
			<div>
				<h1>{product.name}</h1>
				<p class="product-subtitle">{product.subtitle}</p>
			</div>

			<div class="price-row">
				<span class="price">{money(product.price_cents, product.currency)}</span>
				{#if onSale}
					<span class="price-was">Was {money(product.compare_at_cents ?? 0, product.currency)}</span>
				{/if}
			</div>

			<p class="product-desc">{product.description}</p>

			<AddToBag {product} />

			<section class="detail-section" aria-labelledby="ingredients-title">
				<h2 class="meta" id="ingredients-title">Ingredients</h2>
				<ul class="ingredient-tags">
					{#each product.ingredients as ingredient}
						<li>{ingredient}</li>
					{/each}
				</ul>
			</section>

			<section class="detail-section" aria-labelledby="how-to-title">
				<h2 class="meta" id="how-to-title">How to use</h2>
				<p>{product.how_to}</p>
			</section>

			<dl class="spec-list">
				<div class="spec">
					<dt>Size</dt>
					<dd>{product.size}</dd>
				</div>
				<div class="spec">
					<dt>Category</dt>
					<dd>
						<a class="spec-link" href={`/shop?category=${product.category.slug}`}>
							{product.category.name}
						</a>
					</dd>
				</div>
				{#if product.concerns.length > 0}
					<div class="spec">
						<dt>Concerns</dt>
						<dd>{product.concerns.join(', ')}</dd>
					</div>
				{/if}
			</dl>
		</div>
	</div>
</div>

{#if related.length > 0}
	<section class="related" aria-labelledby="related-title">
		<div class="wrap">
			<div class="section-head">
				<div>
					<p class="meta">The rest of the shelf</p>
					<h2 id="related-title">More formulas</h2>
				</div>
				<a class="text-link" href="/shop">See the whole shelf</a>
			</div>

			<div class="product-grid related-grid">
				{#each related as item (item.id)}
					<ProductCard product={item} />
				{/each}
			</div>
		</div>
	</section>
{/if}

<style>
	.crumbs {
		align-items: center;
	}

	.crumbs a {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
	}

	.product-detail {
		padding-top: 12px;
	}

	.product-info h1 {
		max-width: 18ch;
	}

	.product-subtitle {
		margin-top: 10px;
		font-size: 18px;
		color: var(--muted);
	}

	.product-desc {
		max-width: 46ch;
		color: var(--muted);
	}

	.gallery-extra {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 16px;
	}

	.detail-section {
		display: grid;
		gap: 10px;
		padding-top: 20px;
		border-top: 1px solid var(--line);
	}

	.detail-section p {
		max-width: 46ch;
	}

	.ingredient-tags {
		display: grid;
		gap: 0;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.ingredient-tags li {
		padding: 8px 0;
		border-bottom: 1px solid var(--line);
		font-size: 14px;
	}

	.ingredient-tags li:last-child {
		border-bottom: 0;
	}

	.spec-list {
		margin: 0;
	}

	.spec-link {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.related {
		padding-bottom: 72px;
	}

	.related :global(.text-link) {
		min-height: 44px;
	}

	.related-grid :global(.product-add) {
		min-height: 44px;
	}

	.related-grid {
		grid-template-columns: repeat(3, minmax(0, 1fr));
	}

	@media (max-width: 1080px) {
		.related-grid {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}

	@media (max-width: 560px) {
		.related-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
