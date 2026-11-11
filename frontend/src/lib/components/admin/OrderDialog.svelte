<script lang="ts">
	import { untrack } from 'svelte';
	import { ApiError } from '#lib/api';
	import { errorMessage, adminApi } from '#lib/admin/api';
	import { formatDateTime, STATUS_TRANSITIONS } from '#lib/admin/format';
	import type { AdminOrder } from '#lib/admin/types';
	import { money, titleCase } from '#lib/format';
	import type { OrderStatus } from '#lib/types';
	import StatusPill from './StatusPill.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';

	let {
		order,
		onclose,
		onupdated
	}: {
		order: AdminOrder;
		onclose: () => void;
		onupdated: (order: AdminOrder) => void;
	} = $props();

	let current = $state(untrack(() => order));
	let nextStatus = $state<OrderStatus | ''>(untrack(() => STATUS_TRANSITIONS[order.status][0] ?? ''));
	let busy = $state(false);
	let error = $state('');
	let success = $state('');
	let dialogEl = $state<HTMLDialogElement | null>(null);

	const allowed = $derived(STATUS_TRANSITIONS[current.status]);

	$effect(() => {
		if (dialogEl && !dialogEl.open) dialogEl.showModal();
	});

	function close() {
		dialogEl?.close();
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!nextStatus || busy || allowed.length === 0) return;

		busy = true;
		error = '';
		success = '';
		try {
			const data = await adminApi.updateOrderStatus(current.id, nextStatus);
			current = data.order;
			success = 'Status saved.';
			nextStatus = STATUS_TRANSITIONS[data.order.status][0] ?? '';
			onupdated(data.order);
		} catch (caught) {
			if (caught instanceof ApiError && caught.fields.status) error = caught.fields.status;
			else error = errorMessage(caught);
		} finally {
			busy = false;
		}
	}
</script>

<dialog
	bind:this={dialogEl}
	class="a-dialog"
	aria-labelledby="order-dialog-title"
	onclose={onclose}
	oncancel={(event) => {
		if (busy) event.preventDefault();
	}}
>
	<form class="a-dialog-form" onsubmit={save}>
		<header class="a-dialog-head">
			<div class="a-dialog-title">
				<p class="a-dialog-kicker">Order detail</p>
				<h2 id="order-dialog-title">{current.number}</h2>
			</div>
			<button
				class="a-dialog-close"
				type="button"
				aria-label="Close order detail"
				onclick={close}
				disabled={busy}
			>
				Close
			</button>
		</header>

		<div class="a-dialog-body">
			<div class="order-status-panel">
				<div class="order-status-top">
					<h3>Status</h3>
					<StatusPill status={current.status} />
				</div>

				{#if allowed.length > 0}
					<div class="order-status-form">
						<div class="a-field">
							<span class="a-label" id="order-next-status-label">Next status</span>
							<Dropdown
								id="order-next-status"
								class="admin-dropdown"
								label="Next status"
								labelId="order-next-status-label"
								value={nextStatus}
								options={allowed.map((status) => ({ value: status, label: titleCase(status) }))}
								onchange={(value) => (nextStatus = value as OrderStatus)}
								disabled={busy}
							/>
						</div>
						<button class="a-btn a-btn-secondary" type="submit" disabled={busy || !nextStatus}>
							{busy ? 'Saving...' : 'Save status'}
						</button>
					</div>
					<p class="a-field-hint">Only the next steps allowed by the order rules are listed.</p>
				{:else}
					<p class="order-status-final">Delivered and cancelled orders are final.</p>
				{/if}

				{#if error}
					<p class="a-form-error" role="alert">{error}</p>
				{/if}
				{#if success}
					<p class="a-form-success" role="status">{success}</p>
				{/if}
			</div>

			<div class="order-detail-grid">
				<div class="detail-block">
					<h3>Customer</h3>
					<div class="detail-list">
						<span>{current.name}</span>
						<span>{current.email}</span>
						<span>{current.phone}</span>
					</div>
				</div>

				<div class="detail-block">
					<h3>Delivery</h3>
					<div class="detail-list">
						<span>{current.address || 'No address.'}</span>
						<span>{current.city}</span>
					</div>
				</div>

				<div class="detail-block">
					<h3>Placed</h3>
					<div class="detail-list">
						<span>{formatDateTime(current.created_at)}</span>
					</div>
				</div>

				<div class="detail-block">
					<h3>Notes</h3>
					<div class="detail-list">
						<span>{current.notes || 'No notes.'}</span>
					</div>
				</div>
			</div>

			<section class="order-section" aria-labelledby="order-items-title">
				<h3 id="order-items-title">Items</h3>
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div class="a-table-scroll" role="region" aria-label="Order items" tabindex="0">
					<table class="a-table a-items-table">
						<thead>
							<tr>
								<th scope="col">Item</th>
								<th scope="col" class="right">Qty</th>
								<th scope="col" class="right">Unit price</th>
								<th scope="col" class="right">Line total</th>
							</tr>
						</thead>
						<tbody>
							{#each current.items as item (item.id)}
								<tr>
									<td>{item.name}</td>
									<td class="right a-num">{item.quantity}</td>
									<td class="right a-num">{money(item.unit_price_cents, current.currency)}</td>
									<td class="right a-num">{money(item.line_total_cents, current.currency)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>

				<div class="order-totals">
					<div class="order-total-row">
						<span>Subtotal</span>
						<span>{money(current.subtotal_cents, current.currency)}</span>
					</div>
					<div class="order-total-row">
						<span>Shipping</span>
						<span>{money(current.shipping_cents, current.currency)}</span>
					</div>
					<div class="order-total-row order-total-final">
						<span>Total</span>
						<span>{money(current.total_cents, current.currency)}</span>
					</div>
				</div>
			</section>
		</div>

		<footer class="a-dialog-foot">
			<button class="a-btn a-btn-secondary" type="button" onclick={close} disabled={busy}>
				Close
			</button>
		</footer>
	</form>
</dialog>
