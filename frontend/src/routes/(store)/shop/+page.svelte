<script lang="ts">
	import { goto } from '$app/navigation';
	import ProductCard from '#lib/components/ProductCard.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	// svelte-ignore state_referenced_locally
	let query = $state(data.filters.q);
	// svelte-ignore state_referenced_locally
	let concern = $state(data.filters.concern);
	// svelte-ignore state_referenced_locally
	let sort = $state<string>(data.filters.sort);

	const filters = $derived(data.filters);
	const activeCategory = $derived(filters.category);

	const concernOptions = [
		{ value: '', label: 'All' },
		{ value: 'dry', label: 'Dry' },
		{ value: 'sensitive', label: 'Sensitive' },
		{ value: 'dull', label: 'Dull' },
		{ value: 'oily', label: 'Oily' },
		{ value: 'tired', label: 'Tired' }
	];

	const sortOptions = [
		{ value: 'featured', label: 'Featured' },
		{ value: 'price_asc', label: 'Price, low to high' },
		{ value: 'price_desc', label: 'Price, high to low' },
		{ value: 'new', label: 'Newest' }
	];

	$effect(() => {
		query = data.filters.q;
		concern = data.filters.concern;
		sort = data.filters.sort;
	});

	function changeConcern(value: string): void {
		concern = value;
		void goto(hrefFor({ concern: value }));
	}

	function changeSort(value: string): void {
		sort = value;
		void goto(hrefFor({ sort: value }));
	}
	const countLabel = $derived(
		`${data.products.length} ${data.products.length === 1 ? 'formula' : 'formulas'}`
	);

	type FilterKey = 'category' | 'concern' | 'q' | 'sort';

	function hrefFor(next: Partial<Record<FilterKey, string>>): string {
		const merged = { ...filters, q: query, concern, sort, ...next };
		const params = new URLSearchParams();
		if (merged.category) params.set('category', merged.category);
		if (merged.concern) params.set('concern', merged.concern);
		if (merged.q) params.set('q', merged.q);
		if (merged.sort && merged.sort !== 'featured') params.set('sort', merged.sort);
		const search = params.toString();
		return search ? `/shop?${search}` : '/shop';
	}

	function applyFilters(event: SubmitEvent): void {
		event.preventDefault();
		const form = event.currentTarget as HTMLFormElement;
		const values = new FormData(form);
		const params = new URLSearchParams();

		for (const key of ['category', 'concern', 'q', 'sort'] as const) {
			const value = values.get(key);
			if (typeof value !== 'string') continue;
			const trimmed = value.trim();
			if (!trimmed) continue;
			if (key === 'sort' && trimmed === 'featured') continue;
			params.set(key, trimmed);
		}

		const search = params.toString();
		void goto(search ? `/shop?${search}` : '/shop');
	}
</script>

<svelte:head>
	<title>The shelf | Azahar</title>
	<meta
		name="description"
		content="Browse eight botanical formulas for the face, the body and the bath. Filter by category, concern and price."
	/>
	<meta property="og:title" content="The shelf | Azahar" />
	<meta
		property="og:description"
		content="Eight formulas for the face, the body and the bath. Filter the shelf by category, concern and price."
	/>
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<h1>The shelf</h1>
	<p class="lead">Eight formulas for the face, the body and the bath.</p>
	{#if !data.apiDown}
		<p class="shelf-count meta">{countLabel}</p>
	{/if}
</div>

<div class="wrap">
	{#if !data.apiDown}
		<div class="filter-row">
			<div class="filter-group categories" role="group" aria-labelledby="category-label">
				<span class="filter-label" id="category-label">Category</span>
				<div class="chips">
					<a
						class="chip"
						href={hrefFor({ category: '' })}
						aria-current={!activeCategory ? 'page' : undefined}
					>
						All
					</a>
					{#each data.categories as category (category.slug)}
						<a
							class="chip"
							href={hrefFor({ category: category.slug })}
							aria-current={activeCategory === category.slug ? 'page' : undefined}
						>
							{category.name}
						</a>
					{/each}
				</div>
			</div>

			<form
				class="filter-form"
				method="GET"
				action="/shop"
				onsubmit={applyFilters}
			>
				{#if filters.category}
					<input type="hidden" name="category" value={filters.category} />
				{/if}
				<input type="hidden" name="concern" value={concern} />
				<input type="hidden" name="sort" value={sort} />
				<div class="filter-field filter-search">
					<label for="q">Search</label>
					<input
						class="filter-input"
						id="q"
						name="q"
						type="search"
						bind:value={query}
						placeholder="Fig, clay, oil"
						autocomplete="off"
					/>
				</div>

				<div class="filter-field">
					<span class="filter-label" id="concern-label">Concern</span>
					<Dropdown
						id="concern"
						label="Concern"
						labelId="concern-label"
						value={concern}
						options={concernOptions}
						onchange={changeConcern}
					/>
				</div>

				<div class="filter-field">
					<span class="filter-label" id="sort-label">Sort</span>
					<Dropdown
						id="sort"
						label="Sort"
						labelId="sort-label"
						value={sort}
						options={sortOptions}
						onchange={changeSort}
					/>
				</div>

				<button class="btn btn-secondary btn-small filter-submit" type="submit">
					Apply filters
				</button>
			</form>
		</div>
	{/if}

	{#if data.apiDown}
		<div class="empty-state shop-state">
			<p class="meta">Shelf offline</p>
			<h2>The shelf could not load.</h2>
			<p class="muted">The shop API is not answering. Nothing in your bag was lost.</p>
			<a class="btn btn-secondary" href="/shop">Reload the shelf</a>
		</div>
	{:else if data.products.length === 0}
		<div class="empty-state shop-state">
			<p class="meta">No match</p>
			<h2>No formulas match this shelf.</h2>
			<p class="muted">Clear the filters to see everything on the shelf.</p>
			<a class="btn btn-secondary" href="/shop">Show the whole shelf</a>
		</div>
	{:else}
		<h2 class="sr-only">Formulas</h2>
		<div class="product-grid shop-grid">
			{#each data.products as product (product.id)}
				<ProductCard {product} />
			{/each}
		</div>
	{/if}
</div>

<style>
	.shelf-count {
		margin-top: 18px;
	}

	.categories {
		align-items: center;
		gap: 12px;
	}

	.filter-label {
		font-family: var(--font-mono);
		font-size: 12px;
		color: var(--muted);
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}

	.chip {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
	}

	.chip[aria-current='page'] {
		background: var(--ink);
		border-color: var(--ink);
		color: var(--bg);
	}

	.filter-form {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		gap: 14px;
	}

	.filter-field {
		display: grid;
		gap: 4px;
		min-width: 160px;
	}

	.filter-search {
		min-width: 190px;
	}

	.filter-input {
		width: 100%;
		min-height: 44px;
		padding: 8px 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--radius);
		background: transparent;
		font-size: 13px;
	}

	.filter-input:focus {
		outline: none;
		border-color: var(--accent);
		box-shadow: 0 0 0 3px color-mix(in oklab, var(--accent) 22%, transparent);
	}

	.filter-label {
		font-size: 12px;
		color: var(--muted);
	}

	.filter-submit {
		min-height: 44px;
	}

	.shop-grid {
		padding-top: 44px;
	}

	.shop-grid :global(.product-add) {
		min-height: 44px;
	}

	.shop-state {
		margin-top: 0;
		border-top: 0;
	}

	@media (max-width: 720px) {
		.filter-form {
			width: 100%;
		}

		.filter-field {
			flex: 1 1 150px;
		}
	}

	@media (max-width: 560px) {
		.shop-grid {
			padding-top: 34px;
		}

		.filter-field {
			flex-basis: 100%;
		}
	}
</style>
