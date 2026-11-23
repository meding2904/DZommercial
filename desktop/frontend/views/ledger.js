// The ledger: takings for a period, what moved, the order mix, low stock and
// the last five orders. Pure: state in, HTML out.

import { compactMoney, escapeHtml, formatDate, formatDayShort, money, percent } from '../format.js';
import { titleCase } from '../domain/labels.js';
import { pageHead, statusTag } from './common.js';

/**
 * @param {{ metrics: any, metricsDays: number }} state
 * @returns {string}
 */
export function ledgerView(state) {
	const metrics = state.metrics;
	const days = state.metricsDays;
	const toPack = (metrics.status_split || [])
		.filter((item) => ['pending', 'confirmed', 'packed'].includes(item.status))
		.reduce((sum, item) => sum + item.count, 0);
	const lowStock = metrics.low_stock || [];
	const top = metrics.top_products || [];
	const maxUnits = Math.max(1, ...top.map((item) => item.units));
	const topUnits = top.reduce((sum, item) => sum + item.units, 0);
	const topRevenue = top.reduce((sum, item) => sum + item.revenue_cents, 0);
	const mixTotal = (metrics.status_split || []).reduce((sum, item) => sum + item.count, 0);
	const chart = revenueChart(metrics.revenue_series || []);
	const peak = (metrics.revenue_series || []).reduce(
		(best, point) => (point.revenue_cents > best.revenue_cents ? point : best),
		{ date: '', revenue_cents: 0, orders: 0 }
	);
	const period = `<div class="period"><span class="period-label" id="period-label">Period</span><div class="segmented" role="group" aria-labelledby="period-label">${[7, 30, 90]
		.map(
			(value) =>
				`<button type="button" data-days="${value}" aria-pressed="${days === value}">${value} days</button>`
		)
		.join('')}</div></div>`;
	const sub = `${formatDate(metrics.range.from)} to ${formatDate(metrics.range.to)}${
		metrics.demo ? ' · <span class="demo-label">Demo dataset</span>' : ''
	}`;

	const mix = (metrics.status_split || [])
		.filter((item) => item.count > 0)
		.map((item) => {
			const width = mixTotal ? (item.count / mixTotal) * 100 : 0;
			return `<span style="width:${width.toFixed(2)}%;background:var(--a-status-${item.status})" title="${escapeHtml(titleCase(item.status))}: ${item.count}"></span>`;
		})
		.join('');
	const legend = (metrics.status_split || [])
		.map(
			(item) =>
				`<span><i style="background:var(--a-status-${item.status})"></i>${escapeHtml(titleCase(item.status))} ${item.count}</span>`
		)
		.join('');

	const ranked = top.length
		? top
				.map(
					(product, index) => `<div class="rank-row">
						<span class="rank-number">${String(index + 1).padStart(2, '0')}</span>
						<span class="rank-name">${escapeHtml(product.name)}</span>
						<span class="rank-units">${product.units} units</span>
						<span class="rank-money">${money(product.revenue_cents)}</span>
						<span class="rank-share"><span style="width:${((product.units / maxUnits) * 100).toFixed(1)}%"></span></span>
					</div>`
				)
				.join('')
		: '<p class="meta">No products have sold in this period.</p>';

	const recent = (metrics.recent_orders || []).length
		? `<div class="table-wrap"><table><thead><tr><th>Order</th><th>Customer</th><th class="numeric">Total</th><th>Status</th><th>Date</th></tr></thead><tbody>${metrics.recent_orders
				.map(
					(order) => `<tr data-open-order="${escapeHtml(order.id)}" data-clickable="true">
						<td class="mono">${escapeHtml(order.number)}</td>
						<td>${escapeHtml(order.name)}</td>
						<td class="numeric">${money(order.total_cents)}</td>
						<td>${statusTag(order.status)}</td>
						<td class="meta">${formatDate(order.created_at)}</td>
					</tr>`
				)
				.join('')}</tbody></table></div>`
		: '<p class="meta">No orders yet. The first checkout fills this table.</p>';

	return `<div>
		${pageHead('The ledger', sub, period)}
		<div class="ledger-top">
			<section class="panel revenue-panel" aria-labelledby="revenue-title">
				<div class="revenue-head">
					<div>
						<p class="revenue-title" id="revenue-title">Takings</p>
						<p class="revenue-figure">${compactMoney(metrics.revenue_cents)}</p>
						<p class="revenue-exact">${money(metrics.revenue_cents)}</p>
						<p class="revenue-delta" data-down="${Number(metrics.delta_pct) < 0}">${
							metrics.delta_pct === null
								? '<span>No previous period to compare.</span>'
								: `<strong>${percent(metrics.delta_pct)}</strong> <span>against ${money(metrics.previous_revenue_cents)} in the previous ${days} days</span>`
						}</p>
					</div>
					<dl class="revenue-summary">
						<div><dt>Orders</dt><dd>${metrics.orders_count}</dd></div>
						<div><dt>Units</dt><dd>${metrics.units_sold}</dd></div>
						<div><dt>Average order</dt><dd>${money(metrics.aov_cents)}</dd></div>
					</dl>
				</div>
				<div class="chart-wrap">${chart}<div class="chart-foot"><span>Best day ${formatDayShort(peak.date)} · ${money(peak.revenue_cents)}</span><span>${peak.orders} orders that day</span></div></div>
			</section>
			<section class="panel attention" aria-labelledby="attention-title">
				<div class="attention-head"><h2 id="attention-title">Needs attention</h2><p class="meta">The work before the numbers.</p></div>
				<div class="attention-cell">
					<span class="attention-number">01</span>
					<div class="attention-copy"><strong>${toPack} ${toPack === 1 ? 'order' : 'orders'} to pack</strong><span>Paid or confirmed, still on the shelf.</span></div>
					<button class="button button-secondary button-small" type="button" data-goto-orders="pending">Pack</button>
				</div>
				<div class="attention-cell">
					<span class="attention-number">02</span>
					<div class="attention-copy"><strong>${lowStock.length} ${lowStock.length === 1 ? 'line' : 'lines'} low on stock</strong><span>${lowStock.length ? escapeHtml(lowStock.slice(0, 2).map((item) => item.name).join(', ')) : 'Every line is above the restock mark.'}</span></div>
					<button class="button button-secondary button-small" type="button" data-goto-low-stock>Review</button>
				</div>
				<div class="attention-cell">
					<span class="attention-number">03</span>
					<div class="attention-copy"><strong>${metrics.new_customers} new ${metrics.new_customers === 1 ? 'customer' : 'customers'}</strong><span>Accounts opened in this period.</span></div>
					<button class="button button-secondary button-small" type="button" data-goto-customers>See</button>
				</div>
			</section>
		</div>

		<section class="band" aria-labelledby="ranked-title">
			<div class="band-head"><div><h2 id="ranked-title">What moved</h2><p>Ranked by units sold in this period.</p></div><span class="meta">${topRevenue ? `${topUnits} units total` : ''}</span></div>
			<div class="ranked">${ranked}</div>
			<div class="rank-foot"><span>${top.length} products with a sale</span><span>${money(topRevenue)}</span></div>
		</section>

		<div class="two-band">
			<section class="band" aria-labelledby="mix-title">
				<div class="band-head"><div><h2 id="mix-title">Order mix</h2><p>The pipeline at a glance.</p></div></div>
				<div class="mix"><div class="mix-rule" role="img" aria-label="Order status mix">${mix}</div><div class="mix-legend">${legend}</div></div>
				<p class="mix-foot">${(metrics.status_split || []).reduce((sum, item) => sum + (['delivered', 'cancelled'].includes(item.status) ? 0 : item.count), 0)} orders still open.</p>
			</section>
			<section class="band" aria-labelledby="low-title">
				<div class="band-head"><div><h2 id="low-title">Low stock</h2><p>Lines the shelf cannot cover for long.</p></div></div>
				${
					lowStock.length
						? `<div class="table-wrap"><table><tbody>${lowStock
								.slice(0, 6)
								.map(
									(item) => `<tr><td class="cell-strong">${escapeHtml(item.name)}</td><td class="numeric ${item.stock === 0 ? 'stock-out' : 'stock-low'}">${item.stock === 0 ? 'Out' : `${item.stock} left`}</td><td class="numeric"><button class="button button-secondary button-small" type="button" data-restock="${escapeHtml(item.id)}">Restock</button></td></tr>`
								)
								.join('')}</tbody></table></div>`
						: '<p class="meta">Nothing sits below the restock mark.</p>'
				}
			</section>
		</div>

		<section class="band" aria-labelledby="recent-title">
			<div class="band-head"><div><h2 id="recent-title">Recent orders</h2><p>The last five, newest first.</p></div><a class="button button-secondary button-small" href="#/orders">All orders</a></div>
			${recent}
		</section>
	</div>`;
}

/**
 * @param {{ date: string, revenue_cents: number, orders: number }[]} series
 * @returns {string}
 */
function revenueChart(series) {
	if (!series.length) return '<p class="meta">No revenue yet.</p>';
	const width = 760;
	const height = 190;
	const bottom = 160;
	const top = 10;
	const max = Math.max(1, ...series.map((point) => point.revenue_cents));
	const gap = series.length > 1 ? width / series.length : width;
	const barWidth = Math.max(2, gap * 0.62);
	const peakIndex = series.reduce(
		(best, point, index) => (point.revenue_cents > series[best].revenue_cents ? index : best),
		0
	);
	const bars = series
		.map((point, index) => {
			const value = point.revenue_cents;
			const barHeight = value > 0 ? Math.max(2, ((bottom - top) * value) / max) : 0;
			const x = index * gap + (gap - barWidth) / 2;
			return `<rect class="bar" data-peak="${index === peakIndex}" x="${x.toFixed(1)}" y="${(bottom - barHeight).toFixed(1)}" width="${barWidth.toFixed(1)}" height="${barHeight.toFixed(1)}"><title>${escapeHtml(formatDayShort(point.date))}: ${money(value)}, ${point.orders} orders</title></rect>`;
		})
		.join('');
	const labelEvery = Math.max(1, Math.ceil(series.length / 6));
	const labels = series
		.map((point, index) => {
			if (index % labelEvery !== 0 && index !== series.length - 1) return '';
			const x = index * gap + gap / 2;
			return `<text class="axis-label" x="${x.toFixed(1)}" y="182" text-anchor="middle">${escapeHtml(formatDayShort(point.date))}</text>`;
		})
		.join('');
	const grid = [0, 0.5, 1]
		.map((fraction) => {
			const y = bottom - (bottom - top) * fraction;
			return `<line class="grid" x1="0" x2="${width}" y1="${y}" y2="${y}" />`;
		})
		.join('');
	return `<svg class="chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="Revenue by day">${grid}${bars}${labels}</svg>`;
}
