<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminApi, errorMessage, isAbortError } from '#lib/admin/api';
	import { ORDER_STATUSES } from '#lib/admin/format';
	import type { AdminOrder } from '#lib/admin/types';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import OrderDialog from '#lib/components/admin/OrderDialog.svelte';
	import StatusPill from '#lib/components/admin/StatusPill.svelte';
	import { formatDate, money, titleCase, courierLabel } from '#lib/format';
	import type { OrderStatus } from '#lib/types';

	const tabs: Array<{ value: '' | OrderStatus; label: string }> = [
		{ value: '', label: 'All' },
		...ORDER_STATUSES.map((status) => ({ value: status, label: titleCase(status) }))
	];

	let status = $state<'' | OrderStatus>('');
	let search = $state('');
	let searchInput = $state('');
	let orders = $state<AdminOrder[]>([]);
	let loading = $state(true);
	let error = $state('');
	let selected = $state<AdminOrder | null>(null);

	let controller: AbortController | null = null;
	let requestId = 0;
	let searchTimer: ReturnType<typeof setTimeout> | undefined;
	let trigger: HTMLElement | null = null;

	const isFiltered = $derived(status !== '' || search !== '');
	const resultLabel = $derived(
		loading
			? 'Reading orders...'
			: error
				? 'Orders unavailable'
				: `${orders.length} ${orders.length === 1 ? 'order' : 'orders'}`
	);

	onMount(() => {
		const params = page.url.searchParams;
		const requestedStatus = params.get('status') ?? '';
		if (ORDER_STATUSES.includes(requestedStatus as OrderStatus)) {
			status = requestedStatus as OrderStatus;
		}
		const requestedSearch = params.get('q') ?? '';
		searchInput = requestedSearch;
		search = requestedSearch.trim();

		void load();

		return () => {
			controller?.abort();
			clearTimeout(searchTimer);
		};
	});

	async function load() {
		const id = ++requestId;
		controller?.abort();
		controller = new AbortController();
		loading = true;
		error = '';
		orders = [];

		try {
			const data = await adminApi.orders(status, search, controller.signal);
			if (id !== requestId) return;
			orders = data.orders;
		} catch (caught) {
			if (id !== requestId || isAbortError(caught)) return;
			error = errorMessage(caught);
		} finally {
			if (id === requestId) loading = false;
		}
	}

	function syncUrl() {
		const url = new URL(page.url.href);
		if (status) url.searchParams.set('status', status);
		else url.searchParams.delete('status');
		if (search) url.searchParams.set('q', search);
		else url.searchParams.delete('q');
		void goto(url, { replace: true, reset: false }).catch(() => {});
	}

	function onSearchInput(value: string) {
		searchInput = value;
		clearTimeout(searchTimer);
		searchTimer = setTimeout(() => {
			search = value.trim();
			syncUrl();
			void load();
		}, 250);
	}

	function commitSearch() {
		clearTimeout(searchTimer);
		search = searchInput.trim();
		syncUrl();
		void load();
	}

	function setStatus(next: '' | OrderStatus) {
		if (next === status) return;
		clearTimeout(searchTimer);
		search = searchInput.trim();
		status = next;
		syncUrl();
		void load();
	}

	function clearFilters() {
		clearTimeout(searchTimer);
		status = '';
		search = '';
		searchInput = '';
		void goto('/admin/orders', { replace: true, reset: false }).catch(() => {});
		void load();
	}

	function openOrder(order: AdminOrder, event: MouseEvent) {
		trigger = event.currentTarget as HTMLElement;
		selected = order;
	}

	function closeOrder() {
		selected = null;
		const returnTo = trigger;
		requestAnimationFrame(() => {
			if (returnTo?.isConnected) returnTo.focus();
			else document.getElementById('orders-title')?.focus();
		});
	}

	function orderUpdated(updated: AdminOrder) {
		orders = orders.map((order) => (order.id === updated.id ? updated : order));
		if (status && updated.status !== status) void load();
	}

	function itemCount(order: AdminOrder): number {
		return order.items.reduce((sum, item) => sum + item.quantity, 0);
	}
</script>

<svelte:head>
	<title>Orders · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div>
			<h1 id="orders-title" tabindex="-1">Orders</h1>
			<p class="a-page-sub">
				<span>{resultLabel}</span>
				{#if isFiltered}
					<span>
						{status ? titleCase(status) : 'All statuses'}{search ? `, search "${search}"` : ''}
					</span>
				{/if}
			</p>
		</div>
	</header>

	<div class="a-toolbar">
		<div class="a-tabs" role="group" aria-label="Filter orders by status">
			{#each tabs as tab (tab.value)}
				<button
					class="a-tab"
					type="button"
					aria-pressed={status === tab.value}
					onclick={() => setStatus(tab.value)}
				>
					{tab.label}
				</button>
			{/each}
		</div>

		<form
			class="a-search"
			role="search"
			onsubmit={(event) => {
				event.preventDefault();
				commitSearch();
			}}
		>
			<label for="order-search">Search orders</label>
			<input
				id="order-search"
				type="search"
				value={searchInput}
				oninput={(event) => onSearchInput(event.currentTarget.value)}
				placeholder="Number, name, email, phone or tracking"
				autocomplete="off"
			/>
		</form>
	</div>

	{#if loading}
		<LoadingState label="Reading the order book..." />
	{:else if error}
		<ErrorState title="Orders could not be read." message={error} onretry={load} />
	{:else if orders.length === 0}
		<div class="a-panel">
			<div class="a-empty">
				{#if search}
					<h2>No orders match that search.</h2>
					<p>Try a different order number, name, email or phone.</p>
					<a
						class="a-btn a-btn-secondary"
						href="/admin/orders"
						onclick={(event) => {
							event.preventDefault();
							clearFilters();
						}}
					>
						Clear filters
					</a>
				{:else if status}
					<h2>No orders in this state.</h2>
					<p>No orders are currently {status}.</p>
					<a
						class="a-btn a-btn-secondary"
						href="/admin/orders"
						onclick={(event) => {
							event.preventDefault();
							clearFilters();
						}}
					>
						Show all orders
					</a>
				{:else}
					<h2>No orders yet.</h2>
					<p>Orders will appear here after the first checkout.</p>
					<a class="a-btn a-btn-secondary" href="/admin/products">Go to products</a>
				{/if}
			</div>
		</div>
	{:else}
		<div class="a-panel">
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<div class="a-table-scroll" role="region" aria-label="Orders" tabindex="0">
				<table class="a-table">
					<thead>
						<tr>
							<th scope="col">Order</th>
							<th scope="col">Customer</th>
							<th scope="col" class="right">Items</th>
							<th scope="col" class="right">Total</th>
							<th scope="col">Status</th>
							<th scope="col">Courier</th>
							<th scope="col">Date</th>
						</tr>
					</thead>
					<tbody>
						{#each orders as order (order.id)}
							<tr>
								<td>
									<div class="order-cell">
										<span class="a-order-number">{order.number}</span>
										<button
											class="a-open"
											type="button"
											aria-label={`Open order ${order.number}`}
											onclick={(event) => openOrder(order, event)}
										>
											Open
										</button>
									</div>
								</td>
								<td>
									<div class="customer-cell">
										<span>{order.name}</span>
										<span>{order.email}</span>
									</div>
								</td>
								<td class="right a-num">{itemCount(order)}</td>
								<td class="right a-num">{money(order.total_cents, order.currency)}</td>
								<td><StatusPill status={order.status} /></td>
								<td>
									{#if order.tracking_number}
										<div class="customer-cell">
											<span>{courierLabel(order.courier_key)}</span>
											<span>{order.tracking_number}</span>
										</div>
									{:else}
										<span class="a-muted-sm">Not sent</span>
									{/if}
								</td>
								<td>{formatDate(order.created_at)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	{/if}
</div>

{#if selected}
	<OrderDialog order={selected} onclose={closeOrder} onupdated={orderUpdated} />
{/if}
