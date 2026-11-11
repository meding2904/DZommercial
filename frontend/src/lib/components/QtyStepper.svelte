<script lang="ts">
	let {
		value,
		min = 1,
		max = 10,
		label = 'Quantity',
		onchange
	}: {
		value: number;
		min?: number;
		max?: number;
		label?: string;
		onchange: (value: number) => void;
	} = $props();

	function set(next: number): void {
		onchange(Math.max(min, Math.min(max, next)));
	}
</script>

<div class="qty" role="group" aria-label={label}>
	<button type="button" onclick={() => set(value - 1)} disabled={value <= min} aria-label="Decrease quantity">
		<span aria-hidden="true">-</span>
	</button>
	<output aria-live="polite">{value}</output>
	<button type="button" onclick={() => set(value + 1)} disabled={value >= max} aria-label="Increase quantity">
		<span aria-hidden="true">+</span>
	</button>
</div>
