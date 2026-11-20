<script lang="ts">
	import QtyStepper from '#lib/components/QtyStepper.svelte';
	import ProductOptions from '#lib/components/store/ProductOptions.svelte';
	import { cart, cartLineKey } from '#lib/stores/cart.svelte';
	import { toast } from '#lib/stores/toast.svelte';
	import type { Product } from '#lib/types';

	let { product }: { product: Product } = $props();

	let quantity = $state(1);
	let selected = $state<Record<string, string>>({});

	const missing = $derived(
		product.option_schema.filter(
			(field) => field.type === 'select' && field.required && !selected[field.key]
		)
	);
	const variantStock = $derived.by(() => {
		const raw = product.plugin_data?.variants;
		if (!Array.isArray(raw)) return null;
		return raw.filter(
			(item): item is Record<string, string | number> & { in_stock: number } =>
				typeof item === 'object' && item !== null && typeof item.in_stock === 'number'
		);
	});
	const registered = $derived.by(() => {
		const value = product.plugin_data?.devices_in_stock;
		return typeof value === 'number' ? value : null;
	});
	const available = $derived.by(() => {
		if (variantStock && missing.length === 0) {
			const match = variantStock.find((variant) =>
				Object.entries(selected).every(([key, value]) => !value || variant[key] === value)
			);
			return Math.min(match?.in_stock ?? 0, product.stock);
		}
		return registered === null ? product.stock : Math.min(product.stock, registered);
	});
	const max = $derived(Math.min(available, 10));
	const outOfStock = $derived(available < 1);
	const lowStock = $derived(available > 0 && available <= 5);
	const blocked = $derived.by(() => {
		if (!variantStock || missing.length === 0) return false;
		return missing.some((field) => {
			const values = product.attributes[field.key] ?? [];
			return (
				values.length > 0 &&
				!values.some((value) =>
					variantStock.some(
						(variant) =>
							variant.in_stock > 0 &&
							variant[field.key] === value &&
							Object.entries(selected).every(
								([key, chosen]) => !chosen || variant[key] === chosen
							)
					)
				)
			);
		});
	});
	const ready = $derived(missing.length === 0 && !blocked);

	$effect(() => {
		if (max >= 1 && quantity > max) quantity = max;
	});

	function add(): void {
		if (blocked) {
			toast.show('That combination is sold out. Change another option.');
			return;
		}
		if (missing.length > 0) {
			toast.show(`Choose a ${missing[0].label.toLowerCase()} first.`);
			return;
		}

		const key = cartLineKey(product.id, selected);
		const before = cart.items.find((line) => line.key === key)?.quantity ?? 0;

		if (!cart.add(product, quantity, selected)) {
			toast.show('That one is out of stock.');
			return;
		}

		const after = cart.items.find((line) => line.key === key)?.quantity ?? 0;
		const label = cart.items.find((line) => line.key === key)?.options_label;
		if (after === before) {
			toast.show(`The bag already holds ${after} of this product.`, '/cart', 'View bag');
		} else {
			toast.show(
				`${product.name}${label ? `, ${label}` : ''} added to the bag.`,
				'/cart',
				'View bag'
			);
		}
		quantity = 1;
	}
</script>

<div class="buy">
	<ProductOptions {product} bind:selected />

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
			{outOfStock ? 'Sold out' : blocked ? 'Combination sold out' : ready ? 'Add to bag' : 'Choose options'}
		</button>
	</div>

	{#if blocked}
		<p class="option-note">No stock in this combination. Change another option.</p>
	{:else if !outOfStock && !ready}
		<p class="option-note">
			Pick {missing.map((field) => field.label).join(', ')} first.
		</p>
	{:else if outOfStock}
		<p class="stock-note">Sold out. This batch is gone.</p>
	{:else if lowStock}
		<p class="stock-note">Only {available} left.</p>
	{:else}
		<p class="stock-note">In stock.</p>
	{/if}
</div>

<style>
	.buy {
		display: grid;
		gap: 14px;
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

	.option-note {
		font-size: 13px;
		color: var(--muted);
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
