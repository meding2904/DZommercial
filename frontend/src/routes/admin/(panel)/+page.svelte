<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { adminApi } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import { formatRange } from '#lib/admin/format';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import RevenueChart from '#lib/components/admin/RevenueChart.svelte';
	import StatusPill from '#lib/components/admin/StatusPill.svelte';
	import { compactMoney, formatDate, formatDay, money, percent, titleCase } from '#lib/format';

	let days = $state(30);
	let shownRevenue = $state(0);
	let hasCounted = false;

	const metricsResource = createResource((signal) => adminApi.metrics(days, signal));
	const metrics = $derived(metricsResource.data);
	const loading = $derived(metricsResource.loading);
	const error = $derived(metricsResource.error);
	const load = metricsResource.load;

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

	const topUnits = $derived(
		metrics ? metrics.top_products.reduce((sum, product) => sum + product.units, 0) : 0
	);

	const topRevenue = $derived(
		metrics ? metrics.top_products.reduce((sum, product) => sum + product.revenue_cents, 0) : 0
	);

	const inPipeline = $derived.by(() => {
		if (!metrics) return 0;
		return metrics.status_split
			.filter((item) => item.status !== 'delivered' && item.status !== 'cancelled')
			.reduce((sum, item) => sum + item.count, 0);
	});

	const peakDay = $derived.by(() => {
		if (!metrics || metrics.revenue_series.length === 0) return null;
		return metrics.revenue_series.reduce((best, point) =>
			point.revenue_cents > best.revenue_cents ? point : best
		);
	});

	const noOrders = $derived(
		!!metrics &&
			metrics.orders_count === 0 &&
			metrics.status_split.every((item) => item.count === 0)
	);

	$effect(() => {
		const target = metrics?.revenue_cents ?? 0;
		if (!metrics) {
			shownRevenue = 0;
			return;
		}
		if (hasCounted || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
			hasCounted = true;
			shownRevenue = target;
			return;
		}

		hasCounted = true;
		const start = performance.now();
		let frame = 0;
		const step = (now: number) => {
			const progress = Math.min(1, (now - start) / 700);
			const eased = 1 - (1 - progress) ** 3;
			shownRevenue = Math.round(target * eased);
			if (progress < 1) frame = requestAnimationFrame(step);
		};
		frame = requestAnimationFrame(step);

		return () => cancelAnimationFrame(frame);
	});

	onMount(() => {
		const requested = Number(page.url.searchParams.get('days'));
		if (ranges.includes(requested) && requested !== 30) days = requested;

		void load();
	});

	function setRange(next: number) {
		if (next === days && metrics) return;
		days = next;

		const url = new URL(page.url.href);
		if (next === 30) url.searchParams.delete('days');
		else url.searchParams.set('days', String(next));
		void goto(url, { replace: true, reset: false, shallow: true }).catch(() => {});

		void load();
	}
</script>

<svelte:head>
	<title>The ledger · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div class="a-page-title">
			<h1>The ledger</h1>
			<p class="a-page-sub">
				{#if metrics}
					<span class="a-range-text">{formatRange(metrics.range.from, metrics.range.to)}</span>
					{#if metrics.demo}
						<span class="a-demo-chip">Demo dataset</span>
						<span>Orders are seeded sample data for this demo.</span>
					{/if}
				{:else}
					<span class="a-range-text">Last {days} days</span>
				{/if}
			</p>
		</div>

		<div class="a-period">
			<span class="a-period-label" id="period-label">Period</span>
			<div class="a-range" role="group" aria-labelledby="period-label">
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
		<LoadingState label="Reading the ledger…" />
	{:else if error}
		<ErrorState title="The ledger could not be read." message={error} onretry={load} />
	{:else if metrics}
		<p class="sr-only" role="status">
			Showing {formatRange(metrics.range.from, metrics.range.to)}.
			{money(metrics.revenue_cents)} in takings across {metrics.orders_count}
			{metrics.orders_count === 1 ? 'order' : 'orders'}.
		</p>

		{#if noOrders}
			<div class="a-notice">
				<p>
					<strong>No orders yet.</strong> The chart and the order queue will fill after the first
					checkout.
				</p>
				<a class="a-btn a-btn-secondary" href="/admin/products">Go to products</a>
			</div>
		{/if}

		<div class="overview-hero">
			<section class="a-panel revenue-panel" aria-labelledby="revenue-title">
				<div class="revenue-head">
					<div class="revenue-primary">
						<h2 id="revenue-title">Takings</h2>
						<p class="revenue-figure">{compactMoney(shownRevenue)}</p>
						<p class="revenue-exact">{money(metrics.revenue_cents)}</p>
						<p class="revenue-delta">
							{#if metrics.delta_pct === null}
								<span>No previous period to compare.</span>
							{:else}
								<strong>{percent(metrics.delta_pct)}</strong>
								<span>
									against {money(metrics.previous_revenue_cents)} in the previous
									{metrics.range.days} days
								</span>
							{/if}
						</p>
					</div>

					<dl class="revenue-summary">
						<div>
							<dt>Orders counted</dt>
							<dd>{metrics.orders_count}</dd>
						</div>
						<div>
							<dt>Units sold</dt>
							<dd>{metrics.units_sold}</dd>
						</div>
						<div>
							<dt>Average order</dt>
							<dd>{money(metrics.aov_cents)}</dd>
						</div>
					</dl>
				</div>

				<div class="revenue-chart">
					<RevenueChart
						series={metrics.revenue_series}
						from={metrics.range.from}
						to={metrics.range.to}
					/>
				</div>

				<footer class="revenue-foot">
					<p>Daily takings and order totals exclude cancelled orders.</p>
					{#if peakDay && peakDay.revenue_cents > 0}
						<p>
							Best day <span class="a-num">{formatDay(peakDay.date)}</span>, <span class="a-num"
								>{money(peakDay.revenue_cents)}</span
							>.
						</p>
					{/if}
				</footer>
			</section>

			<aside class="attention-rail" aria-labelledby="attention-title">
				<header class="attention-head">
					<h2 id="attention-title">Needs attention</h2>
				</header>

				<ol class="attention-list">
					<li class="attention-block" data-clear={toPack === 0}>
						<div class="attention-top">
							<span class="attention-index" aria-hidden="true">01</span>
							<h3 class="attention-label">Orders to pack</h3>
							<span class="attention-value">{toPack}</span>
						</div>
						<p class="attention-note">Pending, confirmed and packed in this period.</p>
						{#if toPack > 0}
							<a class="attention-link" href="/admin/orders">Open the order book</a>
						{:else}
							<p class="attention-clear">Nothing is waiting to be packed.</p>
						{/if}
					</li>

					<li class="attention-block" data-clear={metrics.low_stock.length === 0}>
						<div class="attention-top">
							<span class="attention-index" aria-hidden="true">02</span>
							<h3 class="attention-label">Low stock</h3>
							<span class="attention-value">{metrics.low_stock.length}</span>
						</div>
						{#if metrics.low_stock.length === 0}
							<p class="attention-note">Every active product is above 5 units.</p>
						{:else}
							<ul class="low-stock-list">
								{#each lowStock as item (item.id)}
									<li class="low-stock-item">
										<span>{item.name}</span>
										<span class="a-num" data-out={item.stock === 0}>
											{item.stock === 0 ? 'Out' : `${item.stock} left`}
										</span>
									</li>
								{/each}
							</ul>
							{#if metrics.low_stock.length > 5}
								<p class="attention-note">Showing 5 of {metrics.low_stock.length}.</p>
							{/if}
							<a class="attention-link" href="/admin/products?low=1">Review low stock</a>
						{/if}
					</li>

					<li class="attention-block" data-clear={metrics.new_customers === 0}>
						<div class="attention-top">
							<span class="attention-index" aria-hidden="true">03</span>
							<h3 class="attention-label">New customers</h3>
							<span class="attention-value">{metrics.new_customers}</span>
						</div>
						<p class="attention-note">Created in this period.</p>
						{#if metrics.new_customers > 0}
							<a class="attention-link" href="/admin/customers">View customers</a>
						{:else}
							<p class="attention-clear">No new customers in this period.</p>
						{/if}
					</li>
				</ol>
			</aside>
		</div>

		<section class="a-panel overview-band" aria-labelledby="top-products-title">
			<div class="a-panel-head">
				<h2 id="top-products-title">Top products</h2>
				<p>By units sold</p>
			</div>
			<div class="a-panel-body">
				{#if metrics.top_products.length === 0}
					<p class="a-muted">No sales in this period.</p>
				{:else}
					<ol class="top-list">
						{#each metrics.top_products as product, index (product.product_id)}
							<li class="top-item" data-lead={index === 0}>
								<div class="top-row">
									<span class="top-rank a-num" aria-hidden="true"
										>{String(index + 1).padStart(2, '0')}</span
									>
									<a
										class="top-name"
										href={`/admin/products?q=${encodeURIComponent(product.name)}`}
									>
										{product.name}
									</a>
									<span class="top-units a-num">
										{product.units} {product.units === 1 ? 'unit' : 'units'}
									</span>
									<span class="top-revenue a-num">{money(product.revenue_cents)}</span>
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
				{#if metrics.top_products.length > 0}
					<p class="panel-foot">
						<span>
							{topUnits} units across {metrics.top_products.length}
							{metrics.top_products.length === 1 ? 'product' : 'products'}
						</span>
						<span>{money(topRevenue)}</span>
					</p>
				{/if}
			</div>
		</section>

		<section class="a-panel overview-band" aria-labelledby="order-mix-title">
			<div class="a-panel-head">
				<h2 id="order-mix-title">Order mix</h2>
				<p>{mixTotal} {mixTotal === 1 ? 'order' : 'orders'}, all statuses</p>
			</div>
			<div class="a-panel-body">
				{#if mixTotal === 0}
					<p class="a-muted">No orders in this period.</p>
				{:else}
					<div class="mix-stack" aria-hidden="true">
						{#each metrics.status_split as item (item.status)}
							{#if item.count > 0}
								<span
									class="mix-segment"
									data-status={item.status}
									style={`flex-grow: ${item.count}`}
								></span>
							{/if}
						{/each}
					</div>
					<ul class="mix-legend">
						{#each metrics.status_split as item (item.status)}
							<li class="mix-legend-item">
								<span class="mix-dot" data-status={item.status} aria-hidden="true"></span>
								<span class="mix-label">{titleCase(item.status)}</span>
								<span class="mix-count a-num">{item.count}</span>
								<span class="mix-pct a-num"
									>{Math.round((item.count / mixTotal) * 100)}%</span
								>
							</li>
						{/each}
					</ul>
				{/if}
				{#if mixTotal > 0}
					<p class="panel-foot">
						<span>{inPipeline} of {mixTotal} orders still open</span>
					</p>
				{/if}
			</div>
		</section>

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
					<table class="a-table a-table--recent a-table--cards">
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
										<span class="a-cell-label">Order</span>
										<a
											class="a-order-number"
											href={`/admin/orders?q=${encodeURIComponent(order.number)}`}
										>
											{order.number}
										</a>
									</td>
									<td>
										<span class="a-cell-label">Customer</span>
										{order.name}
									</td>
									<td class="right a-num">
										<span class="a-cell-label">Total</span>
										{money(order.total_cents)}
									</td>
									<td>
										<span class="a-cell-label">Status</span>
										<StatusPill status={order.status} />
									</td>
									<td>
										<span class="a-cell-label">Date</span>
										{formatDate(order.created_at)}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</section>
	{/if}
</div>
