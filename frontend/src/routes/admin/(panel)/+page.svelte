<script lang="ts">
	import { onMount } from 'svelte';
	import { adminApi, errorMessage, isAbortError } from '#lib/admin/api';
	import { formatRange } from '#lib/admin/format';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import RevenueChart from '#lib/components/admin/RevenueChart.svelte';
	import StatusPill from '#lib/components/admin/StatusPill.svelte';
	import { formatDate, compactMoney, money, percent, titleCase } from '#lib/format';
	import type { Metrics } from '#lib/types';

	let days = $state(30);
	let metrics = $state<Metrics | null>(null);
	let loading = $state(true);
	let error = $state('');

	let controller: AbortController | null = null;
	let requestId = 0;

	const ranges = [7, 30, 90];

	const toPack = $derived.by(() => {
		if (!metrics) return 0;
		return metrics.status_split
			.filter((item) => item.status === 'pending' || item.status === 'confirmed' || item.status === 'packed')
			.reduce((sum, item) => sum + item.count, 0);
	});

	const mixTotal = $derived(
		metrics ? metrics.status_split.reduce((sum, item) => sum + item.count, 0) : 0
	);

	const lowStock = $derived(metrics ? metrics.low_stock.slice(0, 5) : []);

	const maxUnits = $derived(
		Math.max(1, ...(metrics ? metrics.top_products.map((product) => product.units) : [0]))
	);

	const noOrders = $derived(
		!!metrics &&
			metrics.orders_count === 0 &&
			metrics.status_split.every((item) => item.count === 0)
	);

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
		metrics = null;

		try {
			const data = await adminApi.metrics(days, controller.signal);
			if (id !== requestId) return;
			metrics = data;
		} catch (caught) {
			if (id !== requestId || isAbortError(caught)) return;
			error = errorMessage(caught);
		} finally {
			if (id === requestId) loading = false;
		}
	}

	function setRange(next: number) {
		if (next === days && metrics) return;
		days = next;
		void load();
	}
</script>

<svelte:head>
	<title>Overview · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div>
			<h1>Overview</h1>
			<p class="a-page-sub">
				{#if metrics}
					<span>{formatRange(metrics.range.from, metrics.range.to)}</span>
					{#if metrics.demo}
						<span class="a-demo-chip">Demo dataset</span>
						<span>Orders are seeded sample data for this demo.</span>
					{/if}
				{:else}
					<span>Last {days} days</span>
				{/if}
			</p>
		</div>

		<div class="a-page-actions">
			<div class="a-range" role="group" aria-label="Reporting range">
				{#each ranges as range (range)}
					<button
						type="button"
						aria-pressed={days === range}
						onclick={() => setRange(range)}
					>
						{range} days
					</button>
				{/each}
			</div>
		</div>
	</header>

	{#if loading}
		<LoadingState label="Reading the ledger..." />
	{:else if error}
		<ErrorState title="The ledger could not be read." message={error} onretry={load} />
	{:else if metrics}
		{#if noOrders}
			<div class="a-notice">
				<p>
					<strong>No orders yet.</strong> The chart and the order mix will fill after the first
					checkout.
				</p>
				<a class="a-btn a-btn-secondary" href="/admin/products">Go to products</a>
			</div>
		{/if}

		<div class="overview-hero">
			<section class="a-panel revenue-panel" aria-labelledby="revenue-title">
				<div class="revenue-top">
					<div>
						<h2 id="revenue-title">Revenue</h2>
						<p class="revenue-figure">{compactMoney(metrics.revenue_cents)}</p>
						<p class="revenue-exact">{money(metrics.revenue_cents)}</p>
						<p class="revenue-delta">
							{#if metrics.delta_pct === null}
								<span>No previous period</span>
							{:else}
								<strong>{percent(metrics.delta_pct)}</strong>
								<span>vs the previous {metrics.range.days} days</span>
							{/if}
						</p>
					</div>

					<dl class="revenue-summary">
						<div>
							<dt>Orders</dt>
							<dd>{metrics.orders_count}</dd>
						</div>
						<div>
							<dt>Units</dt>
							<dd>{metrics.units_sold}</dd>
						</div>
						<div>
							<dt>Average order</dt>
							<dd>{money(metrics.aov_cents)}</dd>
						</div>
					</dl>
				</div>

				<p class="revenue-caption">Daily revenue and order totals exclude cancelled orders.</p>

				<RevenueChart
					series={metrics.revenue_series}
					from={metrics.range.from}
					to={metrics.range.to}
				/>
			</section>

			<aside class="attention-rail" aria-labelledby="attention-title">
				<h2 id="attention-title" class="sr-only">Needs attention</h2>

				<div class="attention-block">
					<div class="attention-top">
						<span class="attention-label">Orders to pack</span>
						<span class="attention-value">{toPack}</span>
					</div>
					<p class="attention-note">Pending, confirmed and packed in this range.</p>
					<a class="attention-link" href="/admin/orders?status=pending">View pending orders</a>
				</div>

				<div class="attention-block">
					<div class="attention-top">
						<span class="attention-label">Low stock</span>
						<span class="attention-value">{metrics.low_stock.length}</span>
					</div>
					{#if metrics.low_stock.length === 0}
						<p class="attention-note">No low stock. Every active product is above 5.</p>
					{:else}
						<ul class="low-stock-list">
							{#each lowStock as item (item.id)}
								<li class="low-stock-item">
									<span>{item.name}</span>
									<span>{item.stock} left</span>
								</li>
							{/each}
						</ul>
						{#if metrics.low_stock.length > 5}
							<p class="attention-note">Showing 5 of {metrics.low_stock.length}.</p>
						{/if}
						<a class="attention-link" href="/admin/products?low=1">Review low stock</a>
					{/if}
				</div>

				<div class="attention-block">
					<div class="attention-top">
						<span class="attention-label">New customers</span>
						<span class="attention-value">{metrics.new_customers}</span>
					</div>
					<p class="attention-note">Created in this range.</p>
					<a class="attention-link" href="/admin/customers">View customers</a>
				</div>
			</aside>
		</div>

		<div class="overview-grid">
			<section class="a-panel" aria-labelledby="top-products-title">
				<div class="a-panel-head">
					<h2 id="top-products-title">Top products</h2>
					<p>By units sold</p>
				</div>
				<div class="a-panel-body">
					{#if metrics.top_products.length === 0}
						<p class="a-muted">No sales in this range.</p>
					{:else}
						<ol class="top-list">
							{#each metrics.top_products as product (product.product_id)}
								<li class="top-item">
									<div class="top-row">
										<a
											class="top-name"
											href={`/admin/products?q=${encodeURIComponent(product.name)}`}
										>
											{product.name}
										</a>
										<span class="top-units">
											{product.units} {product.units === 1 ? 'unit' : 'units'}
										</span>
										<span class="top-revenue">{money(product.revenue_cents)}</span>
									</div>
									<div class="top-track" aria-hidden="true">
										<div
											class="top-bar"
											style={`width: ${(product.units / maxUnits) * 100}%`}
										></div>
									</div>
								</li>
							{/each}
						</ol>
					{/if}
				</div>
			</section>

			<section class="a-panel" aria-labelledby="order-mix-title">
				<div class="a-panel-head">
					<h2 id="order-mix-title">Order mix</h2>
					<p>{mixTotal} {mixTotal === 1 ? 'order' : 'orders'}, all statuses</p>
				</div>
				<div class="a-panel-body">
					{#if mixTotal === 0}
						<p class="a-muted">No orders in this range.</p>
					{:else}
						<div class="mix-list">
							{#each metrics.status_split as item (item.status)}
								<div class="mix-row">
									<span class="mix-label">{titleCase(item.status)}</span>
									<div class="mix-track" aria-hidden="true">
										<div
											class="mix-bar"
											data-status={item.status}
											style={`width: ${(item.count / mixTotal) * 100}%`}
										></div>
									</div>
									<span class="mix-count">{item.count}</span>
									<span class="mix-pct">{Math.round((item.count / mixTotal) * 100)}%</span>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			</section>
		</div>

		<section class="a-panel overview-orders" aria-labelledby="recent-orders-title">
			<div class="a-panel-head">
				<h2 id="recent-orders-title">Recent orders</h2>
				<a class="a-link" href="/admin/orders">All orders</a>
			</div>

			{#if metrics.recent_orders.length === 0}
				<div class="a-empty">
					<h3>No orders yet.</h3>
					<p>Orders will appear here after the first checkout.</p>
					<a class="a-btn a-btn-secondary" href="/admin/products">Go to products</a>
				</div>
			{:else}
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div class="a-table-scroll" role="region" aria-label="Recent orders" tabindex="0">
					<table class="a-table">
						<thead>
							<tr>
								<th scope="col">Order</th>
								<th scope="col">Customer</th>
								<th scope="col" class="right">Total</th>
								<th scope="col">Status</th>
								<th scope="col">Date</th>
							</tr>
						</thead>
						<tbody>
							{#each metrics.recent_orders as order (order.id)}
								<tr>
									<td>
										<a
											class="a-order-number"
											href={`/admin/orders?q=${encodeURIComponent(order.number)}`}
										>
											{order.number}
										</a>
									</td>
									<td>{order.name}</td>
									<td class="right a-num">{money(order.total_cents)}</td>
									<td><StatusPill status={order.status} /></td>
									<td>{formatDate(order.created_at)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</section>
	{/if}
</div>
