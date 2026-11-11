<script lang="ts">
	import QtyStepper from '#lib/components/QtyStepper.svelte';
	import { cart } from '#lib/stores/cart.svelte';
	import { toast } from '#lib/stores/toast.svelte';
	import type { Product } from '#lib/types';

	let { product }: { product: Product } = $props();

	let quantity = $state(1);

	const max = $derived(Math.min(product.stock, 10));
	const outOfStock = $derived(product.stock < 1);
	const lowStock = $derived(product.stock > 0 && product.stock <= 5);
	const noun = $derived(product.name.trim().split(/\s+/).pop() ?? 'formula');

	function add(): void {
		const before = cart.items.find((line) => line.product_id === product.id)?.quantity ?? 0;

		if (!cart.add(product, quantity)) {
			toast.show('That one is out of stock.');
			return;
		}

		const after = cart.items.find((line) => line.product_id === product.id)?.quantity ?? 0;
		if (after === before) {
			toast.show(`The bag already holds ${after} of this formula.`, '/cart', 'View bag');
		} else {
			toast.show(`${product.name} added to the bag.`, '/cart', 'View bag');
		}
		quantity = 1;
	}
</script>

<div class="buy">
	<div class="buy-row">
		{#if !outOfStock}
			<QtyStepper
				value={quantity}
				max={max}
				label={`Quantity for ${product.name}`}
				onchange={(value) => (quantity = value)}
			/>
		{/if}
		<button
			class="btn btn-primary add-btn"
			type="button"
			onclick={add}
			disabled={outOfStock}
		>
			{outOfStock ? 'Sold out' : `Add the ${noun}`}
		</button>
	</div>

	{#if outOfStock}
		<p class="stock-note">Sold out. This batch is gone.</p>
	{:else if lowStock}
		<p class="stock-note">Only {product.stock} left.</p>
	{:else}
		<p class="stock-note">In stock.</p>
	{/if}
</div>

<style>
	.buy {
		display: grid;
		gap: 10px;
	}

	.buy-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 12px;
	}

	.add-btn {
		flex: 1 1 190px;
		min-height: 46px;
	}

	.buy-row :global(.qty button) {
		width: 44px;
	}

	@media (max-width: 560px) {
		.buy-row {
			display: grid;
		}

		.add-btn {
			width: 100%;
		}
	}
</style>
