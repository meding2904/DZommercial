<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminApi, errorMessage, isAbortError } from '#lib/admin/api';
	import ConfirmDialog from '#lib/components/admin/ConfirmDialog.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import ProductDialog from '#lib/components/admin/ProductDialog.svelte';
	import { money } from '#lib/format';
	import type { AdminProduct, Category } from '#lib/types';

	let products = $state<AdminProduct[]>([]);
	let categories = $state<Category[]>([]);
	let loading = $state(true);
	let error = $state('');
	let search = $state('');
	let lowOnly = $state(false);

	let formOpen = $state(false);
	let editProduct = $state<AdminProduct | null>(null);
	let archiveTarget = $state<AdminProduct | null>(null);
	let archiveBusy = $state(false);
	let archiveError = $state('');
	let notice = $state('');

	let controller: AbortController | null = null;
	let requestId = 0;
	let trigger: HTMLElement | null = null;

	const filtered = $derived.by(() => {
		const query = search.trim().toLowerCase();
		return products.filter((product) => {
			if (lowOnly && !(product.active && product.stock <= 5)) return false;
			if (!query) return true;
			return [product.name, product.slug, product.category.name].some((value) =>
				value.toLowerCase().includes(query)
			);
		});
	});

	const lowStockCount = $derived(
		products.filter((product) => product.active && product.stock <= 5).length
	);

	const countLabel = $derived.by(() => {
		if (error) return 'Products unavailable';
		if (loading) return 'Reading products...';
		if (filtered.length !== products.length) {
			return `${filtered.length} of ${products.length} products`;
		}
		return `${filtered.length} ${filtered.length === 1 ? 'product' : 'products'}`;
	});

	onMount(() => {
		const params = page.url.searchParams;
		search = params.get('q') ?? '';
		lowOnly = params.get('low') === '1';

		void load();

		return () => controller?.abort();
	});

	async function load() {
		const id = ++requestId;
		controller?.abort();
		controller = new AbortController();
		loading = true;
		error = '';
		products = [];

		try {
			const [productData, categoryData] = await Promise.all([
				adminApi.products(controller.signal),
				adminApi.categories(controller.signal).catch(() => null)
			]);
			if (id !== requestId) return;
			products = productData.products;
			categories = mergeCategories(categoryData?.categories ?? [], productData.products);
		} catch (caught) {
			if (id !== requestId || isAbortError(caught)) return;
			error = errorMessage(caught);
		} finally {
			if (id === requestId) loading = false;
		}
	}

	function mergeCategories(base: Category[], list: AdminProduct[]): Category[] {
		const merged = new Map(base.map((category) => [category.slug, category]));
		for (const product of list) {
			if (!merged.has(product.category.slug)) {
				merged.set(product.category.slug, {
					slug: product.category.slug,
					name: product.category.name,
					description: ''
				});
			}
		}
		return [...merged.values()];
	}

	function rememberTrigger(event: MouseEvent) {
		trigger = event.currentTarget as HTMLElement;
	}

	function returnFocus() {
		const returnTo = trigger;
		requestAnimationFrame(() => {
			if (returnTo?.isConnected) returnTo.focus();
			else document.getElementById('products-title')?.focus();
		});
	}

	function focusProduct(productId: string) {
		requestAnimationFrame(() => {
			const edit = [...document.querySelectorAll<HTMLElement>('[data-edit-product]')].find(
				(element) => element.dataset.editProduct === productId
			);
			if (edit) edit.focus();
			else document.getElementById('products-title')?.focus();
		});
	}

	function openCreate(event: MouseEvent) {
		rememberTrigger(event);
		editProduct = null;
		formOpen = true;
	}

	function openEdit(product: AdminProduct, event: MouseEvent) {
		rememberTrigger(event);
		editProduct = product;
		formOpen = true;
	}

	function closeForm() {
		formOpen = false;
		returnFocus();
	}

	function saved(product: AdminProduct, mode: 'create' | 'edit') {
		if (mode === 'create') products = [product, ...products];
		else products = products.map((item) => (item.id === product.id ? product : item));
		notice = mode === 'create' ? 'Product created.' : 'Product updated.';
		closeForm();
	}

	function openArchive(product: AdminProduct, event: MouseEvent) {
		rememberTrigger(event);
		archiveError = '';
		archiveTarget = product;
	}

	function closeArchive() {
		archiveTarget = null;
		archiveError = '';
		returnFocus();
	}

	async function archiveProduct() {
		if (!archiveTarget || archiveBusy) return;
		const target = archiveTarget;
		archiveBusy = true;
		archiveError = '';

		try {
			await adminApi.archiveProduct(target.id);
			products = products.map((product) =>
				product.id === target.id ? { ...product, active: false } : product
			);
			notice = 'Product archived.';
			archiveTarget = null;
			focusProduct(target.id);
		} catch (caught) {
			archiveError = errorMessage(caught);
		} finally {
			archiveBusy = false;
		}
	}

	function clearFilters() {
		search = '';
		lowOnly = false;
		void goto('/admin/products', { replace: true, reset: false }).catch(() => {});
	}
</script>

<svelte:head>
	<title>Products · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div>
			<h1 id="products-title" tabindex="-1">Products</h1>
			<p class="a-page-sub">
				<span>{countLabel}</span>
				{#if !loading && lowStockCount > 0}
					<span>{lowStockCount} low on stock</span>
				{/if}
			</p>
		</div>

		<div class="a-page-actions">
			<button class="a-btn a-btn-primary" type="button" onclick={openCreate}>
				New product
			</button>
		</div>
	</header>

	{#if notice}
		<div class="a-notice a-notice-success" role="status">
			<p>{notice}</p>
			<button class="a-btn a-btn-quiet" type="button" onclick={() => (notice = '')}>
				Dismiss
			</button>
		</div>
	{/if}

	<div class="a-toolbar">
		<form
			class="a-search"
			role="search"
			onsubmit={(event) => event.preventDefault()}
		>
			<label for="product-search">Search products</label>
			<input
				id="product-search"
				type="search"
				bind:value={search}
				placeholder="Name, slug or category"
				autocomplete="off"
			/>
		</form>

		{#if lowOnly}
			<div class="a-filter-line">
				<span>Showing low stock only, 5 units or fewer.</span>
				<button class="a-btn a-btn-secondary a-btn-table" type="button" onclick={clearFilters}>
					Clear filters
				</button>
			</div>
		{/if}
	</div>

	{#if loading}
		<LoadingState label="Reading the catalogue..." />
	{:else if error}
		<ErrorState title="Products could not be read." message={error} onretry={load} />
	{:else if products.length === 0}
		<div class="a-panel">
			<div class="a-empty">
				<h2>No products yet.</h2>
				<p>Add the first formula so the storefront has something to sell.</p>
				<button class="a-btn a-btn-primary" type="button" onclick={openCreate}>
					New product
				</button>
			</div>
		</div>
	{:else if filtered.length === 0}
		<div class="a-panel">
			<div class="a-empty">
				{#if lowOnly && !search}
					<h2>No products are low on stock.</h2>
					<p>Every active product has more than 5 units.</p>
				{:else}
					<h2>No products match that search.</h2>
					<p>Try a product name, slug or category.</p>
				{/if}
				<button class="a-btn a-btn-secondary" type="button" onclick={clearFilters}>
					Clear filters
				</button>
			</div>
		</div>
	{:else}
		<div class="a-panel">
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<div class="a-table-scroll" role="region" aria-label="Products" tabindex="0">
				<table class="a-table a-products-table">
					<thead>
						<tr>
							<th scope="col">Product</th>
							<th scope="col">Category</th>
							<th scope="col" class="right">Price</th>
							<th scope="col" class="right">Stock</th>
							<th scope="col">State</th>
							<th scope="col">Featured</th>
							<th scope="col" class="right">Actions</th>
						</tr>
					</thead>
					<tbody>
						{#each filtered as product (product.id)}
							<tr class:archived-row={!product.active}>
								<td>
									<div class="product-cell">
										<img
											class="product-thumb"
											src={product.image}
											alt={product.name}
											width="40"
											height="48"
											loading="lazy"
										/>
										<div class="product-info">
											<strong>{product.name}</strong>
											<span>{product.size}</span>
										</div>
									</div>
								</td>
								<td>{product.category.name}</td>
								<td class="right a-num">{money(product.price_cents, product.currency)}</td>
								<td class="right">
									<span class="a-num">{product.stock}</span>
									{#if product.stock <= 5}
										<span class="a-low">Low</span>
									{/if}
								</td>
								<td>
									<span class="a-state-label" data-state={product.active ? 'active' : 'archived'}>
										{product.active ? 'Active' : 'Archived'}
									</span>
								</td>
								<td>{product.featured ? 'Yes' : 'No'}</td>
								<td>
									<div class="cell-actions">
										<button
											class="a-btn a-btn-secondary a-btn-table"
											type="button"
											data-edit-product={product.id}
											aria-label={`Edit ${product.name}`}
											onclick={(event) => openEdit(product, event)}
										>
											Edit
										</button>
										{#if product.active}
											<button
												class="a-btn a-btn-quiet a-btn-table"
												type="button"
												aria-label={`Archive ${product.name}`}
												onclick={(event) => openArchive(product, event)}
											>
												Archive
											</button>
										{/if}
									</div>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	{/if}
</div>

{#if formOpen}
	<ProductDialog
		product={editProduct}
		{categories}
		onclose={closeForm}
		onsaved={saved}
	/>
{/if}

{#if archiveTarget}
	<ConfirmDialog
		title="Archive this product?"
		message={`Archive "${archiveTarget.name}"? It will be hidden from the storefront and kept in past orders.`}
		confirmLabel="Archive product"
		cancelLabel="Keep product"
		busy={archiveBusy}
		error={archiveError}
		onconfirm={archiveProduct}
		ondismiss={closeArchive}
	/>
{/if}
