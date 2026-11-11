<script lang="ts">
	import { titleCase } from '#lib/format';
	import type { OrderStatus } from '#lib/types';

	let { status }: { status: OrderStatus } = $props();
</script>

<span class="status-pill" data-status={status}>
	<span class="status-dot" aria-hidden="true"></span>
	{titleCase(status)}
</span>

<style>
	.status-pill {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		padding: 4px 8px;
		border: 1px solid var(--line-strong);
		border-radius: var(--radius-sm);
		font-family: var(--font-mono);
		font-size: 11px;
		line-height: 1.5;
		white-space: nowrap;
	}

	.status-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--muted);
	}

	[data-status='pending'] .status-dot {
		background: #8a5a33;
	}

	[data-status='confirmed'] .status-dot {
		background: #476b5a;
	}

	[data-status='packed'] .status-dot {
		background: #6a5d5e;
	}

	[data-status='shipped'] .status-dot {
		background: #2456d6;
	}

	[data-status='delivered'] .status-dot {
		background: #2e5b44;
	}

	[data-status='cancelled'] .status-dot {
		background: #9a3c4c;
	}
</style>
