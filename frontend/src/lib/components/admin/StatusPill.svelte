<script lang="ts">
	import { titleCase } from '#lib/format';
	import type { OrderStatus } from '#lib/types';

	let { status }: { status: OrderStatus } = $props();
</script>

<span class="a-status" data-status={status}>{titleCase(status)}</span>
