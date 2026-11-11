<script lang="ts">
	import { onMount } from 'svelte';
	import { adminApi, errorMessage, isAbortError } from '#lib/admin/api';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import { formatDate, money } from '#lib/format';
	import type { Customer } from '#lib/types';

	let customers = $state<Customer[]>([]);
	let loading = $state(true);
	let error = $state('');
	let search = $state('');

	let controller: AbortController | null = null;
	let requestId = 0;

	const filtered = $derived.by(() => {
		const query = search.trim().toLowerCase();
		if (!query) return customers;
		return customers.filter(
			(customer) =>
				customer.name.toLowerCase().includes(query) ||
				customer.email.toLowerCase().includes(query)
		);
	});

	const countLabel = $derived.by(() => {
		if (error) return 'Customers unavailable';
		if (loading) return 'Reading customers...';
		if (filtered.length !== customers.length) {
			return `${filtered.length} of ${customers.length} customers`;
		}
		return `${customers.length} ${customers.length === 1 ? 'customer' : 'customers'}`;
	});

	onMount(() => {
		void load();
		return () => controller?.abort();
	});

	async function load() {
		const id = ++requestId;
		controller?.abort();
		controller = new AbortController();
		loading = true;
		error = '';
		customers = [];

		try {
			const data = await adminApi.customers(controller.signal);
			if (id !== requestId) return;
			customers = data.customers;
		} catch (caught) {
			if (id !== requestId || isAbortError(caught)) return;
			error = errorMessage(caught);
		} finally {
			if (id === requestId) loading = false;
		}
	}
</script>

<svelte:head>
	<title>Customers · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div>
			<h1>Customers</h1>
			<p class="a-page-sub"><span>{countLabel}</span></p>
		</div>
	</header>

	<div class="a-toolbar">
		<form class="a-search" role="search" onsubmit={(event) => event.preventDefault()}>
			<label for="customer-search">Search customers</label>
			<input
				id="customer-search"
				type="search"
				bind:value={search}
				placeholder="Name or email"
				autocomplete="off"
			/>
		</form>
	</div>

	{#if loading}
		<LoadingState label="Reading customers..." />
	{:else if error}
		<ErrorState title="Customers could not be read." message={error} onretry={load} />
	{:else if customers.length === 0}
		<div class="a-panel">
			<div class="a-empty">
				<h2>No customers yet.</h2>
				<p>Customer accounts appear here after the first registration.</p>
			</div>
		</div>
	{:else if filtered.length === 0}
		<div class="a-panel">
			<div class="a-empty">
				<h2>No customers match that search.</h2>
				<p>Try a different name or email.</p>
				<button class="a-btn a-btn-secondary" type="button" onclick={() => (search = '')}>
					Clear search
				</button>
			</div>
		</div>
	{:else}
		<div class="a-panel">
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<div class="a-table-scroll" role="region" aria-label="Customers" tabindex="0">
				<table class="a-table">
					<thead>
						<tr>
							<th scope="col">Name</th>
							<th scope="col">Email</th>
							<th scope="col" class="right">Orders</th>
							<th scope="col" class="right">Spend</th>
							<th scope="col">Last order</th>
						</tr>
					</thead>
					<tbody>
						{#each filtered as customer (customer.id)}
							<tr>
								<td>{customer.name}</td>
								<td>{customer.email}</td>
								<td class="right a-num">{customer.orders_count}</td>
								<td class="right a-num">{money(customer.total_spent_cents)}</td>
								<td>{formatDate(customer.last_order_at)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	{/if}
</div>
