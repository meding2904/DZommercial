<script lang="ts">
	import { isValueAvailable } from '#lib/product-options';
	import type { Product } from '#lib/types';

	let {
		product,
		selected = $bindable<Record<string, string>>({})
	}: {
		product: Product;
		selected?: Record<string, string>;
	} = $props();

	const selectFields = $derived(
		product.option_schema.filter((field) => field.type === 'select')
	);

	function choose(key: string, value: string): void {
		const next: Record<string, string> = { ...selected, [key]: value };
		for (const field of selectFields) {
			if (
				field.key !== key &&
				next[field.key] &&
				!isValueAvailable(product, field.key, next[field.key], next)
			) {
				delete next[field.key];
			}
		}
		selected = next;
	}
</script>

{#if selectFields.length > 0}
	<div class="product-options">
		{#each selectFields as field (field.key)}
			<fieldset
				class="option-group"
				aria-describedby={field.help ? `${product.id}-${field.key}-help` : undefined}
			>
				<legend>
					{field.label}
					{#if field.required}<span class="required">Required</span>{/if}
				</legend>
				<div class="option-values">
					{#each product.attributes[field.key] ?? [] as value (value)}
						{@const isAvailable = isValueAvailable(product, field.key, value, selected)}
						<label
							class="option"
							class:is-selected={selected[field.key] === value}
							class:is-unavailable={!isAvailable}
						>
							<input
								type="radio"
								name={`${product.id}-${field.key}`}
								value={value}
								required={field.required}
								disabled={!isAvailable}
								checked={selected[field.key] === value}
								onchange={() => choose(field.key, value)}
							/>
							<span>{value}{#if !isAvailable}<span class="sr-only">, sold out</span>{/if}</span>
						</label>
					{/each}
				</div>
				{#if field.help}
					<p class="option-help" id={`${product.id}-${field.key}-help`}>{field.help}</p>
				{/if}
			</fieldset>
		{/each}
	</div>
{/if}

<style>
	.product-options {
		display: grid;
		gap: 16px;
		padding-top: 4px;
	}

	.option-group {
		margin: 0;
		padding: 0;
		border: 0;
	}

	.option-group legend {
		display: flex;
		align-items: baseline;
		gap: 8px;
		margin-bottom: 8px;
		padding: 0;
		font-family: var(--font-mono);
		font-size: 12px;
		color: var(--muted);
	}

	.required {
		font-size: 10px;
		color: var(--accent);
	}

	.option-values {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}

	.option {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-height: 44px;
		padding: 0 14px;
		border: 1px solid var(--line-strong);
		border-radius: var(--radius);
		font-size: 14px;
		cursor: pointer;
		transition:
			border-color 160ms var(--ease),
			background 160ms var(--ease);
	}

	.option:hover {
		border-color: var(--accent);
	}

	.option.is-selected {
		border-color: var(--accent);
		background: var(--accent);
		color: var(--accent-ink);
	}

	.option.is-unavailable {
		cursor: not-allowed;
		opacity: 0.45;
	}

	.option.is-unavailable:hover {
		border-color: var(--line-strong);
	}

	.option input {
		position: absolute;
		width: 1px;
		height: 1px;
		margin: -1px;
		opacity: 0;
	}

	.option:has(input:focus-visible) {
		outline: 2px solid var(--accent);
		outline-offset: 3px;
	}

	.option-help {
		margin-top: 6px;
		font-size: 12px;
		color: var(--muted);
	}
</style>
