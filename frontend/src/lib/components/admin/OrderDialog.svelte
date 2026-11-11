<script lang="ts">
	import { untrack } from 'svelte';
	import { ApiError } from '#lib/api';
	import { errorMessage, adminApi } from '#lib/admin/api';
	import { formatDateTime, STATUS_TRANSITIONS } from '#lib/admin/format';
	import type { AdminOrder } from '#lib/admin/types';
	import {
		courierLabel,
		courierStatusLabel,
		deliveryTypeLabel,
		money,
		titleCase
	} from '#lib/format';
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
	let shipBusy = $state(false);
	let shipError = $state('');
	let shipSuccess = $state('');
	let dialogEl = $state<HTMLDialogElement | null>(null);

	const allowed = $derived(STATUS_TRANSITIONS[current.status]);
	const deliveryType = $derived(current.delivery_type ?? 'home');
	const placeLine = $derived(
		[current.commune_name || current.city, current.wilaya_name].filter(Boolean).join(', ')
	);
	const courierClosed = $derived(
		current.status === 'cancelled' || current.status === 'delivered'
	);
	const courierReady = $derived(
		(current.wilaya_code ?? 0) >= 1 &&
			Boolean(current.commune_name) &&
			(deliveryType === 'stopdesk'
				? Boolean(current.stop_desk_id)
				: Boolean(current.address)) &&
			!courierClosed
	);
	const courierHint = $derived(
		courierClosed
			? 'This order is closed.'
			: (current.wilaya_code ?? 0) < 1 || !current.commune_name
				? 'This order was placed before courier delivery details were captured.'
				: deliveryType === 'stopdesk' && !current.stop_desk_id
					? 'This order has no stop desk.'
					: deliveryType !== 'stopdesk' && !current.address
						? 'This order has no deliverable address.'
						: 'Create the parcel when the order is packed.'
	);
	const courierState = $derived(
		current.tracking_number
			? courierStatusLabel(current.courier_status)
			: courierClosed
				? 'Closed'
				: courierReady
					? 'Not sent yet'
					: 'Delivery details incomplete'
	);

	$effect(() => {
		if (dialogEl && !dialogEl.open) dialogEl.showModal();
	});

	function close() {
		dialogEl?.close();
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!nextStatus || busy || shipBusy || allowed.length === 0) return;

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

	async function createShipment(): Promise<void> {
		if (shipBusy || !courierReady || current.tracking_number) return;

		shipBusy = true;
		shipError = '';
		shipSuccess = '';
		try {
			const data = await adminApi.createShipment(current.id);
			current = data.order;
			shipSuccess = 'Shipment created.';
			onupdated(data.order);
		} catch (caught) {
			if (caught instanceof ApiError) {
				shipError =
					caught.fields.delivery ??
					caught.fields.phone ??
					caught.fields.commune_name ??
					caught.fields.stop_desk_id ??
					caught.fields.address ??
					caught.message;
			} else {
				shipError = errorMessage(caught);
			}
		} finally {
			shipBusy = false;
		}
	}

	async function refreshTracking(): Promise<void> {
		if (shipBusy || !current.tracking_number) return;

		shipBusy = true;
		shipError = '';
		shipSuccess = '';
		try {
			const data = await adminApi.refreshTracking(current.id);
			current = data.order;
			shipSuccess = 'Tracking updated.';
			onupdated(data.order);
		} catch (caught) {
			shipError = errorMessage(caught);
		} finally {
			shipBusy = false;
		}
	}
</script>

<dialog
	bind:this={dialogEl}
	class="a-dialog"
	aria-labelledby="order-dialog-title"
	aria-busy={busy || shipBusy ? 'true' : undefined}
	onclose={onclose}
	oncancel={(event) => {
		if (busy || shipBusy) event.preventDefault();
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
				disabled={busy || shipBusy}
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
						<button class="a-btn a-btn-secondary" type="submit" disabled={busy || shipBusy || !nextStatus}>
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
						<span>{deliveryTypeLabel(deliveryType)}</span>
						{#if deliveryType === 'stopdesk' && current.stop_desk_name}
							<span>Stop desk: {current.stop_desk_name}</span>
						{/if}
						{#if current.address}
							<span>{current.address}</span>
						{/if}
						{#if placeLine}
							<span>{placeLine}</span>
						{/if}
						<span>{current.phone}</span>
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

			<section class="order-section courier-section" aria-labelledby="order-courier-title">
				<div class="courier-head">
					<h3 id="order-courier-title">Courier</h3>
					<span class="a-muted-sm">{courierState}</span>
				</div>

				{#if current.tracking_number}
					<div class="courier-facts">
						<span>Courier</span>
						<span>{courierLabel(current.courier_key)}</span>
						<span>Tracking</span>
						<span class="a-num">{current.tracking_number}</span>
						<span>Status</span>
						<span>{courierStatusLabel(current.courier_status)}</span>
						<span>Last checked</span>
						<span>{formatDateTime(current.courier_synced_at)}</span>
					</div>
					<div class="courier-actions">
						<button
							class="a-btn a-btn-secondary"
							type="button"
							onclick={refreshTracking}
							disabled={shipBusy}
						>
							{shipBusy ? 'Refreshing...' : 'Refresh tracking'}
						</button>
					</div>
				{:else}
					<p class="a-muted-sm">{courierHint}</p>
					<div class="courier-actions">
						<button
							class="a-btn a-btn-primary"
							type="button"
							onclick={createShipment}
							disabled={shipBusy || !courierReady}
						>
							{shipBusy ? 'Creating shipment...' : 'Create shipment'}
						</button>
					</div>
				{/if}

				{#if shipError}
					<p class="a-form-error" role="alert">{shipError}</p>
				{/if}
				{#if shipSuccess}
					<p class="a-form-success" role="status">{shipSuccess}</p>
				{/if}

				{#if current.courier_events?.length}
					<ol class="courier-events">
						{#each current.courier_events as event, index (index)}
							<li>
								<span>{event.rawStatus || courierStatusLabel(event.status)}</span>
								<span class="a-muted-sm">{formatDateTime(event.timestamp)}</span>
							</li>
						{/each}
					</ol>
				{/if}
			</section>

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
			<button class="a-btn a-btn-secondary" type="button" onclick={close} disabled={busy || shipBusy}>
				Close
			</button>
		</footer>
	</form>
</dialog>

<style>
	.courier-section {
		display: grid;
		gap: 10px;
	}

	.courier-head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 6px 12px;
	}

	.courier-facts {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		gap: 4px 16px;
		font-size: 14px;
	}

	.courier-facts > span:nth-child(odd) {
		color: var(--a-muted);
	}

	.courier-facts > span:nth-child(even) {
		overflow-wrap: anywhere;
	}

	.courier-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}

	.courier-events {
		display: grid;
		gap: 6px;
		margin: 4px 0 0;
		padding: 10px 0 0;
		border-top: 1px solid var(--a-line);
		list-style: none;
	}

	.courier-events li {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		font-size: 13px;
	}

	@media (max-width: 560px) {
		.courier-facts {
			grid-template-columns: 1fr;
			gap: 2px;
		}

		.courier-facts > span:nth-child(odd) {
			margin-top: 6px;
		}
	}
</style>
