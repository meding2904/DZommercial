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
	const currentPrice = $derived(product.effective_price_cents ?? product.price_cents);
	const wasPrice = $derived(
		product.on_sale
			? product.price_cents
			: product.compare_at_cents !== null && product.compare_at_cents > product.price_cents
				? product.compare_at_cents
				: null
	);
	const saleEnds = $derived(
		product.on_sale && product.sale_ends_at ? saleEndLabel(product.sale_ends_at) : ''
	);

	function saleEndLabel(value: string): string {
		const date = new Date(value);
		if (Number.isNaN(date.getTime())) return '';
		const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
		return `${date.getUTCDate()} ${months[date.getUTCMonth()]}`;
	}
	const extraImages = $derived(product.gallery.filter((src) => src !== product.image));
	const pluginDetails = $derived(
		product.option_schema
			.filter((field) => field.type !== 'select')
			.map((field) => ({ field, values: product.attributes[field.key] ?? [] }))
			.filter((entry) => entry.values.length > 0)
	);
</script>

<svelte:head>
	<title>{product.name} | {data.theme.brand_name}</title>
	<meta name="description" content={product.description || product.subtitle} />
	<meta property="og:title" content={`${product.name} | ${data.theme.brand_name}`} />
	<meta property="og:description" content={product.description || product.subtitle} />
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
				<span class="price">{money(currentPrice, product.currency)}</span>
				{#if wasPrice !== null}
					<span class="price-was">Was {money(wasPrice, product.currency)}</span>
				{/if}
				{#if saleEnds}
					<span class="sale-note">Sale ends {saleEnds}</span>
				{/if}
			</div>

			<p class="product-desc">{product.description}</p>

			{#if product.plugin_id !== 'core' && product.plugin_summary}
				<p class="plugin-note">{product.plugin_summary}</p>
			{/if}

			<AddToBag {product} />

			{#each pluginDetails as entry (entry.field.key)}
				{#if entry.field.type === 'textarea'}
					<section class="detail-section" aria-labelledby={`detail-${entry.field.key}`}>
						<h2 class="meta" id={`detail-${entry.field.key}`}>{entry.field.label}</h2>
						<p>{entry.values[0]}</p>
					</section>
				{:else if entry.field.type === 'list'}
					<section class="detail-section" aria-labelledby={`detail-${entry.field.key}`}>
						<h2 class="meta" id={`detail-${entry.field.key}`}>{entry.field.label}</h2>
						<ul class="ingredient-tags">
							{#each entry.values as value (value)}
								<li>{value}</li>
							{/each}
						</ul>
					</section>
				{/if}
			{/each}

			<dl class="spec-list">
				{#each pluginDetails as entry (entry.field.key)}
					{#if entry.field.type === 'text'}
						<div class="spec">
							<dt>{entry.field.label}</dt>
							<dd>{entry.values[0]}</dd>
						</div>
					{/if}
				{/each}
				<div class="spec">
					<dt>Category</dt>
					<dd>
						<a class="spec-link" href={`/shop?category=${product.category.slug}`}>
							{product.category.name}
						</a>
					</dd>
				</div>
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
					<h2 id="related-title">More from the shelf</h2>
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

	.plugin-note {
		max-width: 46ch;
		padding: 10px 12px;
		border-left: 2px solid var(--accent);
		background: var(--surface);
		font-size: 13px;
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
		white-space: pre-line;
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

	.sale-note {
		color: var(--accent);
		font-family: var(--font-mono);
		font-size: 12px;
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
