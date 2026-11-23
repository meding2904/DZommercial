<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { ApiError, apiFetch } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import type { AdminPlugin, AdminProduct } from '#lib/types';

	let { plugin }: { plugin: AdminPlugin } = $props();

	type DeviceStatus = 'in_stock' | 'reserved' | 'sold';

	type PhoneDevice = {
		id: string;
		product_id: string;
		product_name: string;
		variant_key: string;
		variant_label: string;
		config: string;
		color: string;
		imei: string;
		status: DeviceStatus;
		order_id: string;
		order_number: string;
		created_at: string;
	};

	type DeviceCounts = {
		in_stock: number;
		reserved: number;
		sold: number;
	};

	type DevicesResponse = {
		devices: PhoneDevice[];
		counts: DeviceCounts;
	};

	type ConfigKey = 'config' | 'color';

	const configKeys: ConfigKey[] = ['config', 'color'];
	const configLabels: Record<ConfigKey, string> = {
		config: 'Memory and storage',
		color: 'Color'
	};
	const configNouns: Record<ConfigKey, string> = {
		config: 'memory and storage',
		color: 'color'
	};
	const statusLabels: Record<string, string> = {
		in_stock: 'In stock',
		reserved: 'Reserved',
		sold: 'Sold'
	};

	let selectedId = $state('');

	const deviceList = createResource(
		async (signal) => {
			const data = await apiFetch<DevicesResponse>(
				`/plugins/phone/admin/devices?product_id=${encodeURIComponent(selectedId)}`,
				{ signal }
			);
			return { devices: data.devices, counts: data.counts };
		},
		{ loading: false }
	);
	const devicesData = $derived(deviceList.data);
	const devices = $derived(devicesData?.devices ?? []);
	const counts = $derived(devicesData?.counts ?? null);
	const devicesLoading = $derived(deviceList.loading);
	const devicesError = $derived(deviceList.error);

	const productList = createResource(
		async (signal) => {
			const data = await adminApi.products(signal);
			return data.products.filter((product) => product.plugin_id === 'phone');
		},
		{
			onLoaded: async (list) => {
				const current = list.find((product) => product.id === selectedId) ?? list[0] ?? null;
				if (!current) {
					selectedId = '';
					deviceList.reset();
					applyProduct(null);
					return;
				}
				selectedId = current.id;
				applyProduct(current);
				await deviceList.load();
			}
		}
	);
	const products = $derived(productList.data ?? []);
	const productsLoading = $derived(productList.loading);
	const productsError = $derived(productList.error);

	const loadProducts = productList.load;
	const loadDevices = deviceList.load;

	let config = $state<Record<ConfigKey, string>>({ config: '', color: '' });
	let imei = $state('');
	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let busy = $state(false);
	let notice = $state('');
	let formEl = $state<HTMLFormElement | null>(null);

	let deleteTarget = $state<PhoneDevice | null>(null);
	let deleteDialog = $state<HTMLDialogElement | null>(null);
	let deleteBusy = $state(false);
	let deleteError = $state('');
	let deleteTrigger: HTMLElement | null = null;

	const selectedProduct = $derived(products.find((product) => product.id === selectedId) ?? null);
	const missingConfigOptions = $derived(
		!!selectedProduct && (selectedProduct.attributes.config ?? []).length === 0
	);

	$effect(() => {
		if (deleteDialog && !deleteDialog.open) deleteDialog.showModal();
	});

	onMount(() => {
		void loadProducts();
	});

	function applyProduct(product: AdminProduct | null): void {
		config = {
			config: product?.attributes.config?.[0] ?? '',
			color: product?.attributes.color?.[0] ?? ''
		};
		imei = '';
		fieldErrors = {};
		formError = '';
		notice = '';
	}

	function selectProduct(id: string): void {
		if (id === selectedId) return;
		const product = products.find((item) => item.id === id) ?? null;
		if (!product) return;
		selectedId = id;
		applyProduct(product);
		void loadDevices();
	}

	function setConfig(key: ConfigKey, value: string): void {
		config = { ...config, [key]: value };
		clearFieldError(key);
	}

	function clearFieldError(key: string): void {
		if (!fieldErrors[key]) return;
		const next = { ...fieldErrors };
		delete next[key];
		fieldErrors = next;
	}

	function optionsFor(key: ConfigKey): { value: string; label: string }[] {
		return (selectedProduct?.attributes[key] ?? []).map((value) => ({ value, label: value }));
	}

	function optionsHint(key: ConfigKey): boolean {
		return (selectedProduct?.attributes[key]?.length ?? 0) === 0;
	}

	function configErrorId(key: ConfigKey): string {
		return `phone-${key}-error`;
	}

	function configHintId(key: ConfigKey): string {
		return `phone-${key}-hint`;
	}

	async function focusFirstError(): Promise<void> {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"], [data-invalid="true"]')?.focus();
	}

	async function addDevice(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy) return;
		const product = selectedProduct;
		if (!product) {
			formError = 'Choose a phone product first.';
			return;
		}

		formError = '';
		notice = '';
		const errors: Record<string, string> = {};

		for (const key of configKeys) {
			if (config[key]) continue;
			errors[key] = optionsHint(key)
				? `This product has no ${configNouns[key]} options. Edit the product and add them.`
				: `Choose a ${configNouns[key]} option.`;
		}

		const normalized = imei.trim();
		if (!/^\d{15}$/.test(normalized)) {
			errors.imei = 'Enter a 15-digit IMEI.';
		}

		fieldErrors = errors;
		if (Object.keys(errors).length > 0) {
			formError = 'Check the marked fields.';
			await focusFirstError();
			return;
		}

		busy = true;
		try {
			const data = await apiFetch<{ device: PhoneDevice }>('/plugins/phone/admin/devices', {
				method: 'POST',
				body: {
					product_id: product.id,
					config: config.config,
					color: config.color,
					imei: normalized
				}
			});
			if (deviceList.data) {
				deviceList.update((current) => ({
					devices: [data.device, ...current.devices],
					counts: current.counts
						? { ...current.counts, in_stock: current.counts.in_stock + 1 }
						: current.counts
				}));
			} else {
				void loadDevices();
			}
			imei = '';
			fieldErrors = {};
			notice = 'Handset added.';
		} catch (caught) {
			if (caught instanceof ApiError) {
				formError = caught.fields.product_id ?? caught.message;
				fieldErrors = { ...caught.fields };
				if (caught.status === 409 && !fieldErrors.imei) {
					fieldErrors = { ...fieldErrors, imei: 'That IMEI is already registered.' };
				}
				if (Object.keys(fieldErrors).length > 0) {
					busy = false;
					await focusFirstError();
				}
			} else {
				formError = errorMessage(caught);
			}
		} finally {
			busy = false;
		}
	}

	function openDelete(device: PhoneDevice, event: MouseEvent): void {
		deleteTrigger = event.currentTarget as HTMLElement;
		deleteError = '';
		deleteTarget = device;
	}

	function closeDelete(): void {
		deleteTarget = null;
		deleteError = '';
		const trigger = deleteTrigger;
		requestAnimationFrame(() => {
			if (trigger?.isConnected) trigger.focus();
			else document.getElementById('handset-stock-title')?.focus();
		});
	}

	function dismissDelete(): void {
		if (deleteBusy) return;
		deleteDialog?.close();
		closeDelete();
	}

	async function deleteDevice(): Promise<void> {
		if (!deleteTarget || deleteBusy) return;
		const target = deleteTarget;
		deleteBusy = true;
		deleteError = '';

		try {
			await apiFetch<void>(
				`/plugins/phone/admin/devices/${encodeURIComponent(target.id)}`,
				{ method: 'DELETE' }
			);
			deviceList.update((current) => ({
				devices: current.devices.filter((device) => device.id !== target.id),
				counts:
					current.counts && target.status === 'in_stock'
						? { ...current.counts, in_stock: Math.max(0, current.counts.in_stock - 1) }
						: current.counts
			}));
			deleteDialog?.close();
			closeDelete();
			notice = 'Handset removed.';
		} catch (caught) {
			if (caught instanceof ApiError && caught.status === 409) {
				deleteError =
					'That handset can no longer be deleted. It may be reserved or sold. Refresh the list to see its current state.';
			} else {
				deleteError = errorMessage(caught);
			}
		} finally {
			deleteBusy = false;
		}
	}
</script>

{#if productsLoading}
	<LoadingState label="Reading phone products…" />
{:else if productsError}
	<ErrorState
		title="Phone products could not be read."
		message={productsError}
		onretry={loadProducts}
	/>
{:else if products.length === 0}
	<div class="a-panel">
		<div class="a-empty">
			<h2>No phone products yet.</h2>
			<p>Create a product, set its type to Phone, then add IMEIs here.</p>
			<a class="a-btn a-btn-secondary" href="/admin/products">Open products</a>
		</div>
	</div>
{:else}
	<div class="phone-admin">
		<section class="a-panel">
			<div class="a-panel-body phone-picker">
				<div class="a-field phone-product">
					<span class="a-label" id="phone-product-label">Phone product</span>
					<Dropdown
						id="phone-product"
						class="admin-dropdown"
						label="Phone product"
						labelId="phone-product-label"
						value={selectedId}
						options={products.map((product) => ({ value: product.id, label: product.name }))}
						onchange={selectProduct}
						disabled={busy}
					/>
				</div>

				{#if selectedProduct}
					<div class="variant-block">
						<p class="variant-title">Options sold for {selectedProduct.name}</p>
						<dl class="variant-list">
							{#each configKeys as key (key)}
								<div class="variant-row">
									<dt>{configLabels[key]}</dt>
									<dd>{(selectedProduct.attributes[key] ?? []).join(', ') || 'None set'}</dd>
								</div>
							{/each}
						</dl>
					</div>
				{/if}
			</div>
		</section>

		{#if missingConfigOptions}
			<section class="a-panel">
				<div class="a-empty">
					<h2>Update this product's options first.</h2>
					<p>
						{selectedProduct?.name ?? 'This phone'} has no Memory and storage configuration. Edit the
						product and add one configuration per pairing sold, such as 12 GB RAM + 256 GB, before
						adding IMEIs.
					</p>
					<a class="a-btn a-btn-secondary" href="/admin/products">Edit products</a>
				</div>
			</section>
		{:else}
			<section class="a-panel" aria-labelledby="add-handset-title">
				<header class="a-panel-head">
					<h2 id="add-handset-title">Add a handset</h2>
					<p>{plugin.name} tracks one unit per IMEI.</p>
				</header>
				<div class="a-panel-body">
					<form bind:this={formEl} onsubmit={addDevice} novalidate>
						{#if formError}
							<p class="a-form-error" role="alert">{formError}</p>
						{/if}

						<div class="phone-form-grid">
							{#each configKeys as key (key)}
								<div class="a-field">
									<span class="a-label" id={`phone-${key}-label`}>{configLabels[key]}</span>
									<Dropdown
										id={`phone-${key}`}
										class="admin-dropdown"
										label={configLabels[key]}
										labelId={`phone-${key}-label`}
										value={config[key]}
										options={optionsFor(key)}
										onchange={(value) => setConfig(key, value)}
										invalid={!!fieldErrors[key]}
										describedBy={fieldErrors[key]
											? configErrorId(key)
											: optionsHint(key)
												? configHintId(key)
												: undefined}
										disabled={busy || optionsHint(key)}
									/>
									{#if fieldErrors[key]}
										<p class="a-field-error" id={configErrorId(key)}>{fieldErrors[key]}</p>
									{:else if optionsHint(key)}
										<p class="a-field-hint" id={configHintId(key)}>
											No {configNouns[key]} options on this product.
										</p>
									{/if}
								</div>
							{/each}

							<div class="a-field phone-imei">
								<label for="phone-imei">IMEI</label>
								<input
									id="phone-imei"
									type="text"
									inputmode="numeric"
									autocomplete="off"
									spellcheck="false"
									maxlength="15"
									placeholder="15 digits"
									value={imei}
									disabled={busy}
									aria-invalid={fieldErrors.imei ? 'true' : undefined}
									aria-describedby={fieldErrors.imei ? 'phone-imei-error' : 'phone-imei-hint'}
									oninput={(event) => {
										imei = event.currentTarget.value;
										clearFieldError('imei');
									}}
								/>
								{#if fieldErrors.imei}
									<p class="a-field-error" id="phone-imei-error">{fieldErrors.imei}</p>
								{:else}
									<p class="a-field-hint" id="phone-imei-hint">
										Printed on the handset or its box.
									</p>
								{/if}
							</div>
						</div>

						<footer class="phone-submit">
							<p class="a-field-hint">Stock counts update when an order reserves or sells a unit.</p>
							<button class="a-btn a-btn-primary" type="submit" disabled={busy}>
								{busy ? 'Adding…' : 'Add handset'}
							</button>
						</footer>
					</form>
				</div>
			</section>

			<section class="a-panel" aria-labelledby="handset-stock-title">
				<header class="a-panel-head">
					<h2 id="handset-stock-title" tabindex="-1">Stock</h2>
					{#if selectedProduct}
						<p>{selectedProduct.name}</p>
					{/if}
				</header>
				<div class="a-panel-body">
					{#if notice}
						<div class="a-notice a-notice-success" role="status">
							<p>{notice}</p>
							<button class="a-btn a-btn-quiet" type="button" onclick={() => (notice = '')}>
								Dismiss
							</button>
						</div>
					{/if}

					{#if devicesLoading}
						<LoadingState label="Reading handsets…" />
					{:else if devicesError}
						<ErrorState
							title="Handsets could not be read."
							message={devicesError}
							onretry={loadDevices}
						/>
					{:else if counts}
						<dl class="phone-counts" aria-label="Stock counts">
							<div>
								<dt>In stock</dt>
								<dd class="a-num">{counts.in_stock}</dd>
							</div>
							<div>
								<dt>Reserved</dt>
								<dd class="a-num">{counts.reserved}</dd>
							</div>
							<div>
								<dt>Sold</dt>
								<dd class="a-num">{counts.sold}</dd>
							</div>
						</dl>

						{#if devices.length === 0}
							<div class="a-empty">
								<h3>No handsets for this product yet.</h3>
								<p>Add the first IMEI above. The stock counts fill in as units arrive and sell.</p>
							</div>
						{:else}
							<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
							<div class="a-table-scroll" role="region" aria-label="Handsets" tabindex="0">
								<table class="a-table a-table--cards phone-table">
									<thead>
										<tr>
											<th scope="col">Configuration</th>
											<th scope="col">IMEI</th>
											<th scope="col">State</th>
											<th scope="col">Order</th>
											<th scope="col" class="right">Actions</th>
										</tr>
									</thead>
									<tbody>
										{#each devices as device (device.id)}
											<tr>
												<td>
													<span class="a-cell-label">Configuration</span>
													{device.variant_label}
												</td>
												<td>
													<span class="a-cell-label">IMEI</span>
													<span class="phone-imei-value">{device.imei}</span>
												</td>
												<td>
													<span class="a-cell-label">State</span>
													<span class="phone-status" data-status={device.status}>
														{statusLabels[device.status] ?? device.status}
													</span>
												</td>
												<td>
													<span class="a-cell-label">Order</span>
													{device.order_number || 'Not linked'}
												</td>
												<td>
													{#if device.status === 'in_stock'}
														<span class="a-cell-label">Actions</span>
														<div class="cell-actions">
															<button
																class="a-btn a-btn-quiet a-btn-table"
																type="button"
																aria-label={`Delete IMEI ${device.imei}`}
																onclick={(event) => openDelete(device, event)}
															>
																Delete
															</button>
														</div>
													{/if}
												</td>
											</tr>
										{/each}
									</tbody>
								</table>
							</div>
						{/if}
					{/if}
				</div>
			</section>
		{/if}
	</div>
{/if}

{#if deleteTarget}
	<dialog
		bind:this={deleteDialog}
		class="a-dialog a-dialog--confirm"
		aria-labelledby="delete-handset-title"
		oncancel={(event) => {
			event.preventDefault();
			dismissDelete();
		}}
	>
		<div class="a-dialog-form">
			<header class="a-dialog-head">
				<div class="a-dialog-title">
					<h2 id="delete-handset-title">Delete this handset?</h2>
				</div>
			</header>
			<div class="a-dialog-body">
				<p class="a-state-body">
					Delete IMEI {deleteTarget.imei}? This removes one unit of stock and cannot be undone.
				</p>
				{#if deleteError}
					<p class="a-form-error" role="alert">{deleteError}</p>
				{/if}
			</div>
			<footer class="a-dialog-foot">
				<button
					class="a-btn a-btn-secondary"
					type="button"
					onclick={dismissDelete}
					disabled={deleteBusy}
				>
					Keep handset
				</button>
				<button
					class="a-btn a-btn-danger"
					type="button"
					onclick={deleteDevice}
					disabled={deleteBusy}
				>
					{deleteBusy ? 'Deleting…' : 'Delete handset'}
				</button>
			</footer>
		</div>
	</dialog>
{/if}

<style>
	.phone-admin {
		display: grid;
		gap: 20px;
	}

	.phone-picker {
		display: grid;
		gap: 18px;
	}

	.phone-product {
		max-width: 420px;
	}

	.variant-block {
		display: grid;
		gap: 6px;
		padding-top: 16px;
		border-top: 1px solid var(--a-line);
	}

	.variant-title {
		color: var(--a-muted);
		font-size: 12px;
	}

	.variant-list {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 0;
		border-top: 1px solid var(--a-line);
		border-bottom: 1px solid var(--a-line);
	}

	.variant-row {
		padding: 10px 12px;
		border-right: 1px solid var(--a-line);
	}

	.variant-row:last-child {
		border-right: 0;
	}

	.variant-row dt {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
	}

	.variant-row dd {
		margin-top: 2px;
		font-size: 13px;
	}

	.phone-form-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 14px;
	}

	.phone-imei {
		grid-column: 1 / -1;
		max-width: 420px;
	}

	.phone-submit {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-top: 18px;
		padding-top: 14px;
		border-top: 1px solid var(--a-line);
	}

	.phone-counts {
		display: flex;
		flex-wrap: wrap;
		margin-bottom: 16px;
		border-top: 1px solid var(--a-line);
		border-bottom: 1px solid var(--a-line);
	}

	.phone-counts div {
		display: flex;
		align-items: baseline;
		gap: 8px;
		min-width: 120px;
		padding: 10px 18px;
		border-right: 1px solid var(--a-line);
	}

	.phone-counts div:first-child {
		padding-left: 0;
	}

	.phone-counts div:last-child {
		border-right: 0;
	}

	.phone-counts dt {
		color: var(--a-muted);
		font-size: 12px;
	}

	.phone-counts dd {
		font-size: 16px;
	}

	.phone-table {
		min-width: 760px;
	}

	.phone-imei-value {
		font-family: var(--a-font-mono);
		font-size: 13px;
		font-variant-numeric: tabular-nums;
	}

	.phone-status[data-status='in_stock'] {
		color: var(--a-muted);
	}

	.phone-status[data-status='reserved'] {
		font-weight: 500;
	}

	.a-form-error {
		margin-bottom: 16px;
	}

	@media (max-width: 820px) {
		.phone-table {
			min-width: 0;
		}
	}

	@media (max-width: 720px) {
		.variant-list {
			grid-template-columns: minmax(0, 1fr);
		}

		.variant-row {
			border-right: 0;
			border-bottom: 1px solid var(--a-line);
		}

		.variant-row:last-child {
			border-bottom: 0;
		}

		.phone-form-grid {
			grid-template-columns: minmax(0, 1fr);
		}
	}

	@media (max-width: 560px) {
		.phone-counts div {
			flex: 1 1 100%;
			padding-left: 0;
			border-right: 0;
			border-bottom: 1px solid var(--a-line);
		}

		.phone-counts div:last-child {
			border-bottom: 0;
		}

		.phone-submit .a-btn {
			width: 100%;
		}
	}
</style>
