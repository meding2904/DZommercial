<script lang="ts">
	import { compactMoney, formatDay, money } from '#lib/format';
	import type { MetricPoint } from '#lib/types';

	let {
		series,
		from,
		to
	}: {
		series: MetricPoint[];
		from: string;
		to: string;
	} = $props();

	let measuredWidth = $state(0);
	let figureEl = $state<HTMLElement | null>(null);
	let frame = 0;

	$effect(() => {
		if (!figureEl) return;

		const observer = new ResizeObserver((entries) => {
			const entry = entries[0];
			const next = entry ? Math.round(entry.contentRect.width) : 0;
			if (!next) return;
			cancelAnimationFrame(frame);
			frame = requestAnimationFrame(() => {
				if (next !== measuredWidth) measuredWidth = next;
			});
		});

		observer.observe(figureEl);

		return () => {
			observer.disconnect();
			cancelAnimationFrame(frame);
		};
	});

	const pad = { top: 20, right: 16, bottom: 36, left: 64 };

	const width = $derived(Math.max(320, measuredWidth || 640));
	const height = $derived(width < 520 ? 230 : 300);
	const plotWidth = $derived(Math.max(1, width - pad.left - pad.right));
	const plotHeight = $derived(height - pad.top - pad.bottom);
	const maxValue = $derived(Math.max(1, ...series.map((point) => point.revenue_cents)));

	const points = $derived(
		series.map((point, index) => {
			const x =
				series.length <= 1
					? pad.left + plotWidth / 2
					: pad.left + (index / (series.length - 1)) * plotWidth;
			const y = pad.top + plotHeight - (point.revenue_cents / maxValue) * plotHeight;
			return { ...point, x, y };
		})
	);

	const linePath = $derived(
		points
			.map((point, index) => `${index === 0 ? 'M' : 'L'}${point.x.toFixed(2)},${point.y.toFixed(2)}`)
			.join(' ')
	);

	const areaPath = $derived(
		points.length
			? `${linePath} L${points[points.length - 1].x.toFixed(2)},${(pad.top + plotHeight).toFixed(2)} L${points[0].x.toFixed(2)},${(pad.top + plotHeight).toFixed(2)} Z`
			: ''
	);

	const peak = $derived(
		points.length
			? points.reduce((best, point) => (point.revenue_cents > best.revenue_cents ? point : best))
			: null
	);

	const yTicks = $derived(
		[0, 0.5, 1].map((fraction) => ({
			value: maxValue * fraction,
			y: pad.top + plotHeight - fraction * plotHeight
		}))
	);

	const labelIndexes = $derived.by(() => {
		if (series.length === 0) return [];
		const count = width < 520 ? 3 : series.length <= 7 ? 4 : 5;
		const indexes = new Set<number>();
		for (let index = 0; index < count; index += 1) {
			indexes.add(Math.round((index / (count - 1)) * (series.length - 1)));
		}
		return [...indexes].sort((a, b) => a - b);
	});

	const chartKey = $derived(series.map((point) => `${point.date}:${point.revenue_cents}`).join('|'));

	function axisLabel(cents: number): string {
		return compactMoney(cents).replace(/\s*DZD$/, '');
	}
</script>

<figure class="chart" bind:this={figureEl}>
	<figcaption class="chart-caption">
		<span>Daily takings</span>
		<span>{formatDay(from)} to {formatDay(to)}</span>
	</figcaption>

	{#if series.length === 0}
		<p class="a-muted">No takings data for this period.</p>
	{:else}
		<svg
			class="chart-svg"
			viewBox={`0 0 ${width} ${height}`}
			width={width}
			height={height}
			aria-hidden="true"
			focusable="false"
		>
			{#each yTicks as tick}
				<line
					class="chart-grid"
					x1={pad.left}
					x2={width - pad.right}
					y1={tick.y}
					y2={tick.y}
				></line>
				<text class="chart-axis" x={pad.left - 10} y={tick.y + 4} text-anchor="end">
					{axisLabel(tick.value)}
				</text>
			{/each}

			<text class="chart-unit" x={pad.left} y={pad.top - 8} text-anchor="start">DZD</text>

			{#each labelIndexes as index}
				<text
					class="chart-axis"
					x={points[index].x}
					y={height - 12}
					text-anchor={index === 0 ? 'start' : index === series.length - 1 ? 'end' : 'middle'}
				>
					{formatDay(series[index].date)}
				</text>
			{/each}

			<line
				class="chart-baseline"
				x1={pad.left}
				x2={width - pad.right}
				y1={pad.top + plotHeight}
				y2={pad.top + plotHeight}
			></line>

			{#if areaPath}
				<path class="chart-area" d={areaPath}></path>
			{/if}

			{#key chartKey}
				<path class="chart-line" pathLength="1" d={linePath}></path>
			{/key}

			{#if peak && peak.revenue_cents > 0}
				<circle class="chart-peak" cx={peak.x} cy={peak.y} r="3.5"></circle>
			{/if}
		</svg>

		<table class="sr-only">
			<caption>Daily takings, {formatDay(from)} to {formatDay(to)}</caption>
			<thead>
				<tr>
					<th scope="col">Day</th>
					<th scope="col">Takings</th>
					<th scope="col">Orders</th>
				</tr>
			</thead>
			<tbody>
				{#each series as point (point.date)}
					<tr>
						<th scope="row">{formatDay(point.date)}</th>
						<td>{money(point.revenue_cents)}</td>
						<td>{point.orders}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</figure>
