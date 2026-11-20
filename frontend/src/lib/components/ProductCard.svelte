<script lang="ts">
	import ProductImage from './ProductImage.svelte';
	import { money } from '#lib/format';
	import { cart } from '#lib/stores/cart.svelte';
	import { toast } from '#lib/stores/toast.svelte';
	import type { Product } from '#lib/types';

	let { product }: { product: Product } = $props();

	const detail = $derived(product.attributes?.size?.[0] ?? product.size);
	const price = $derived(product.effective_price_cents ?? product.price_cents);
	const sale = $derived(product.on_sale === true);

	function add(): void {
		if (!cart.add(product)) {
			toast.show('That one is out of stock.');
			return;
		}
		toast.show(`${product.name} added to the bag.`, '/cart');
	}
</script>

<article class="product-card">
	<a class="product-link" href={`/product/${product.slug}`}>
		<div class="product-media">
			<ProductImage
				src={product.image}
				alt=""
				width={1200}
				height={1500}
				sizes="(max-width: 560px) 100vw, (max-width: 1080px) 50vw, 25vw"
			/>
		</div>
		<div class="product-copy">
			<div>
				<h3 class="product-name">{product.name}</h3>
				<p class="product-size">{detail}</p>
			</div>
			<span class="product-price">
				{money(price, product.currency)}
				{#if sale}
					<span class="product-was">{money(product.price_cents, product.currency)}</span>
					<span class="product-sale">Sale</span>
				{/if}
			</span>
		</div>
	</a>

	{#if product.option_schema.some((field) => field.type === 'select')}
		<a
			class="btn btn-secondary btn-small product-add"
			href={`/product/${product.slug}`}
			aria-label={product.stock > 0 ? `Choose options for ${product.name}` : `Sold out: ${product.name}`}
		>
			{product.stock > 0 ? 'Choose options' : 'Sold out'}
		</a>
	{:else if product.stock > 0}
		<button
			class="btn btn-secondary btn-small product-add"
			type="button"
			aria-label={`Add to bag: ${product.name}`}
			onclick={add}
		>
			Add to bag
		</button>
	{:else}
		<button
			class="btn btn-secondary btn-small product-add"
			type="button"
			aria-label={`Sold out: ${product.name}`}
			disabled
		>
			Sold out
		</button>
	{/if}
</article>

<style>
	.product-sale {
		margin-left: 6px;
		color: var(--accent);
		font-family: var(--font-mono);
		font-size: 10px;
		letter-spacing: 0.08em;
		text-transform: uppercase;
	}

	.product-was {
		margin-left: 6px;
		color: var(--muted);
		font-family: var(--font-mono);
		font-size: 11px;
		text-decoration: line-through;
	}
</style>
