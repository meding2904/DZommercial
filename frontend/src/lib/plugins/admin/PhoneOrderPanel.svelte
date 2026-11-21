<script lang="ts">
	import { onMount } from 'svelte';
	import { apiFetch } from '#lib/api';
	import { errorMessage, isAbortError } from '#lib/admin/api';
	import type { AdminOrder } from '#lib/admin/types';

	type Handset = {
		id: string;
		imei: string;
		variant_label: string;
		status: string;
	};

	let { order }: { order: AdminOrder } = $props();

	let handsets = $state<Handset[]>([]);
	let loading = $state(true);
	let error = $state('');
	let controller: AbortController | null = null;

	onMount(() => {
		void load();
		return () => controller?.abort();
	});

	async function load(): Promise<void> {
		controller?.abort();
		const current = new AbortController();
		controller = current;
		loading = true;
		error = '';
		try {
			const data = await apiFetch<{ devices: Handset[] }>(
				`/plugins/phone/admin/orders/${encodeURIComponent(order.id)}/devices`,
				{ signal: current.signal }
			);
			if (controller !== current) return;
			handsets = data.devices;
		} catch (caught) {
			if (controller !== current || isAbortError(caught)) return;
			error = errorMessage(caught);
		} finally {
			if (controller === current) loading = false;
		}
	}
</script>

<section class="order-section handsets" aria-labelledby="order-handsets-title">
	<h3 id="order-handsets-title">Registered handsets</h3>

	{#if loading}
		<p class="a-muted-sm" role="status">Reading the IMEIs reserved for this order…</p>
	{:else if error}
		<p class="a-form-error" role="alert">{error}</p>
		<button class="a-btn a-btn-secondary a-btn-table" type="button" onclick={load}>
			Try again
		</button>
	{:else if handsets.length === 0}
		<p class="a-muted-sm">No handset is reserved for this order.</p>
	{:else}
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<div class="a-table-scroll" role="region" aria-label="Reserved handsets" tabindex="0">
			<table class="a-table">
				<thead>
					<tr>
						<th scope="col">IMEI</th>
						<th scope="col">Configuration</th>
						<th scope="col">State</th>
					</tr>
				</thead>
				<tbody>
					{#each handsets as handset (handset.id)}
						<tr>
							<td class="a-num handset-imei">{handset.imei}</td>
							<td>{handset.variant_label}</td>
							<td>{handset.status === 'sold' ? 'Sold' : 'Reserved'}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</section>

<style>
	.handsets {
		gap: 10px;
	}

	.handset-imei {
		font-family: var(--a-font-mono);
		font-size: 12px;
		white-space: nowrap;
	}
</style>
