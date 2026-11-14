<script lang="ts">
	import ProductImage from './ProductImage.svelte';
	import { money } from '#lib/format';
	import { cart } from '#lib/stores/cart.svelte';
	import { toast } from '#lib/stores/toast.svelte';
	import type { Product } from '#lib/types';

	let { product }: { product: Product } = $props();

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
				<p class="product-size">{product.size}</p>
			</div>
			<span class="product-price">{money(product.price_cents, product.currency)}</span>
		</div>
	</a>

	{#if product.stock > 0}
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
