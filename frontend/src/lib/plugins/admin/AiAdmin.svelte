<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { ApiError, apiFetch } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import { formatDate } from '#lib/format';
	import type { AdminPlugin } from '#lib/types';

	let { plugin }: { plugin: AdminPlugin } = $props();

	type AiRun = {
		id: string;
		kind: 'store' | 'product';
		subject: string;
		model: string;
		analysis: string;
		created_at: string;
	};

	type RunKind = 'store' | 'product';

	let kind = $state<RunKind>('store');
	let days = $state('30');
	let productId = $state('');
	let latestId = $state('');

	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let busy = $state(false);
	let notice = $state('');
	let formEl = $state<HTMLFormElement | null>(null);

	const runsList = createResource(
		async (signal) => {
			const data = await apiFetch<{ runs: AiRun[] }>('/plugins/ai/admin/runs?limit=10', {
				signal
			});
			return data.runs;
		},
		{
			onLoaded: (list) => {
				if (!list.some((run) => run.id === latestId)) latestId = list[0]?.id ?? '';
			}
		}
	);
	const runs = $derived(runsList.data ?? []);
	const runsLoading = $derived(runsList.loading);
	const runsError = $derived(runsList.error);
	const loadRuns = runsList.load;

	const productList = createResource(
		async (signal) => (await adminApi.products(signal)).products,
		{
			onLoaded: (list) => {
				if (!list.some((product) => product.id === productId)) {
					productId = list[0]?.id ?? '';
				}
				if (fieldErrors.product_id) {
					const next = { ...fieldErrors };
					delete next.product_id;
					fieldErrors = next;
				}
			}
		}
	);
	const products = $derived(productList.data ?? []);
	const productsLoading = $derived(productList.loading);
	const productsError = $derived(productList.error);
	const loadProducts = productList.load;

	const dayOptions = [
		{ value: '7', label: 'Last 7 days' },
		{ value: '30', label: 'Last 30 days' },
		{ value: '90', label: 'Last 90 days' }
	];

	const kindOptions = [
		{ value: 'store', label: 'Whole store' },
		{ value: 'product', label: 'One product' }
	];

	const latestRun = $derived(runs.find((run) => run.id === latestId) ?? runs[0] ?? null);
	const submitDisabled = $derived(
		busy ||
			(kind === 'product' && (productsLoading || !!productsError || products.length === 0))
	);

	onMount(() => {
		void loadRuns();
		void loadProducts();
	});

	function setKind(value: string): void {
		kind = value === 'product' ? 'product' : 'store';
		clearFieldError('kind');
	}

	function setProduct(value: string): void {
		productId = value;
		clearFieldError('product_id');
	}

	function clearFieldError(key: string): void {
		if (!fieldErrors[key]) return;
		const next = { ...fieldErrors };
		delete next[key];
		fieldErrors = next;
	}

	function subjectLabel(run: AiRun): string {
		if (run.kind !== 'store') return run.subject || 'Product';
		if (run.subject.startsWith('store:')) {
			const count = run.subject.slice(6).replace(/d$/, '');
			return `Store, last ${count} days`;
		}
		return 'Store';
	}

	function productDescription(): string | undefined {
		if (fieldErrors.product_id) return 'ai-product-error';
		if (productsLoading || productsError || products.length === 0) return 'ai-product-hint';
		return undefined;
	}

	async function focusFirstError(): Promise<void> {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"], [data-invalid="true"]')?.focus();
	}

	function analysisError(caught: unknown): string {
		if (caught instanceof ApiError) {
			if (caught.status === 502 || caught.code === 'ai_upstream_error') {
				return 'The AI provider did not return a usable answer. Try again shortly.';
			}
			switch (caught.code) {
				case 'plugin_not_configured':
					return 'The AI plugin is not configured. Save the API key and model in the settings above, then run the analysis again.';
				case 'plugin_disabled':
					return 'The AI plugin is disabled. Turn on “Enable analysis” in the settings above, then run the analysis again.';
				case 'plugin_settings_unreadable':
					return 'The plugin settings cannot be read with the current key. Enter them again in the settings above.';
				case 'not_found':
					return 'That product no longer exists. Choose another product.';
				default:
					return caught.message;
			}
		}
		return errorMessage(caught);
	}

	async function analyze(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy) return;

		formError = '';
		notice = '';
		const errors: Record<string, string> = {};

		if (kind === 'product') {
			if (productsError) errors.product_id = 'The product list could not be read. Retry it first.';
			else if (!productId) errors.product_id = 'Choose a product.';
		}
		if (!dayOptions.some((option) => option.value === days)) {
			errors.days = 'Choose 7, 30 or 90 days.';
		}

		fieldErrors = errors;
		if (Object.keys(errors).length > 0) {
			formError = 'Check the marked fields.';
			await focusFirstError();
			return;
		}

		busy = true;
		try {
			const data = await apiFetch<{ run: AiRun }>('/plugins/ai/admin/analyze', {
				method: 'POST',
				body: {
					kind,
					product_id: kind === 'product' ? productId : '',
					days: Number(days)
				}
			});
			runsList.update((list) => [data.run, ...list].slice(0, 10));
			latestId = data.run.id;
			notice = 'Analysis ready.';
		} catch (caught) {
			formError = analysisError(caught);
			if (caught instanceof ApiError && Object.keys(caught.fields).length > 0) {
				fieldErrors = { ...caught.fields };
				busy = false;
				await focusFirstError();
			}
		} finally {
			busy = false;
		}
	}
</script>

<section class="a-panel" aria-labelledby="ai-analyze-title">
	<header class="a-panel-head">
		<h2 id="ai-analyze-title">New analysis</h2>
		<p>{plugin.name} reads recorded sales facts for the period you choose.</p>
	</header>
	<div class="a-panel-body">
		<p class="ai-privacy">
			Analysis uses recorded sales facts only. Customer names, emails, phones and addresses are
			never sent.
		</p>

		<form bind:this={formEl} onsubmit={analyze} novalidate>
			{#if formError}
				<p class="a-form-error" role="alert">{formError}</p>
			{/if}
			{#if notice}
				<div class="a-notice a-notice-success ai-notice" role="status">
					<p>{notice}</p>
					<button class="a-btn a-btn-quiet" type="button" onclick={() => (notice = '')}>
						Dismiss
					</button>
				</div>
			{/if}

			<div class="form-grid">
				<div class="a-field">
					<span class="a-label" id="ai-kind-label">What to analyse</span>
					<Dropdown
						id="ai-kind"
						class="admin-dropdown"
						label="What to analyse"
						labelId="ai-kind-label"
						value={kind}
						options={kindOptions}
						onchange={setKind}
						invalid={!!fieldErrors.kind}
						describedBy={fieldErrors.kind ? 'ai-kind-error' : undefined}
						disabled={busy}
					/>
					{#if fieldErrors.kind}
						<p class="a-field-error" id="ai-kind-error">{fieldErrors.kind}</p>
					{/if}
				</div>

				{#if kind === 'store'}
					<div class="a-field">
						<span class="a-label" id="ai-days-label">Period</span>
						<Dropdown
							id="ai-days"
							class="admin-dropdown"
							label="Period"
							labelId="ai-days-label"
							value={days}
							options={dayOptions}
							onchange={(value) => (days = value)}
							invalid={!!fieldErrors.days}
							describedBy={fieldErrors.days ? 'ai-days-error' : undefined}
							disabled={busy}
						/>
						{#if fieldErrors.days}
							<p class="a-field-error" id="ai-days-error">{fieldErrors.days}</p>
						{/if}
					</div>
				{:else}
					<div class="a-field">
						<span class="a-label" id="ai-product-label">Product</span>
						<Dropdown
							id="ai-product"
							class="admin-dropdown"
							label="Product"
							labelId="ai-product-label"
							value={productId}
							options={products.map((product) => ({ value: product.id, label: product.name }))}
							onchange={setProduct}
							invalid={!!fieldErrors.product_id}
							describedBy={productDescription()}
							disabled={busy || productsLoading || !!productsError}
						/>
						{#if fieldErrors.product_id}
							<p class="a-field-error" id="ai-product-error">{fieldErrors.product_id}</p>
						{:else if productsLoading}
							<p class="a-field-hint" id="ai-product-hint">Reading products…</p>
						{:else if productsError}
							<p class="a-field-hint" id="ai-product-hint">
								The product list could not be read.
							</p>
							<button
								class="a-btn a-btn-secondary a-btn-table"
								type="button"
								onclick={loadProducts}
							>
								Retry product list
							</button>
						{:else if products.length === 0}
							<p class="a-field-hint" id="ai-product-hint">No products are available.</p>
						{/if}
					</div>
				{/if}
			</div>

			<footer class="ai-submit">
				<p class="a-field-hint">A run can take a few seconds while the provider answers.</p>
				<button class="a-btn a-btn-primary" type="submit" disabled={submitDisabled}>
					{busy ? 'Running…' : 'Run analysis'}
				</button>
			</footer>
		</form>
	</div>
</section>

{#if latestRun}
	<section class="a-panel" aria-labelledby="ai-latest-title">
		<header class="a-panel-head">
			<h2 id="ai-latest-title">Latest analysis</h2>
			<p>
				{subjectLabel(latestRun)} · {latestRun.model} · {formatDate(latestRun.created_at)}
			</p>
		</header>
		<div class="a-panel-body">
			<p class="ai-analysis">{latestRun.analysis}</p>
		</div>
	</section>
{/if}

{#if runsLoading}
	<LoadingState label="Reading previous analyses…" />
{:else if runsError}
	<ErrorState
		title="Analysis history could not be read."
		message={runsError}
		onretry={loadRuns}
	/>
{:else}
	<section class="a-panel" aria-labelledby="ai-history-title">
		<header class="a-panel-head">
			<h2 id="ai-history-title">History</h2>
			<p>Latest 10 analyses, kept on this store.</p>
		</header>
		<div class="a-panel-body ai-history">
			{#if runs.length === 0}
				<div class="a-empty">
					<h3>No analyses yet.</h3>
					<p>Run the first analysis above. It will appear here with its model and date.</p>
				</div>
			{:else}
				{#each runs as run (run.id)}
					<details class="ai-run" open={run.id === latestId}>
						<summary>
							<span class="ai-run-subject">{subjectLabel(run)}</span>
							<span class="ai-run-meta">{run.model} · {formatDate(run.created_at)}</span>
						</summary>
						<p class="ai-analysis">{run.analysis}</p>
					</details>
				{/each}
			{/if}
		</div>
	</section>
{/if}

<style>
	.ai-privacy {
		max-width: 70ch;
		margin-bottom: 18px;
		padding-bottom: 14px;
		border-bottom: 1px solid var(--a-line);
		color: var(--a-muted);
		font-size: 13px;
		line-height: 1.55;
	}

	.ai-notice {
		margin-bottom: 16px;
	}

	.a-form-error {
		margin-bottom: 16px;
	}

	.ai-submit {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-top: 18px;
		padding-top: 14px;
		border-top: 1px solid var(--a-line);
	}

	.ai-analysis {
		max-width: 70ch;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		font-size: 14px;
		line-height: 1.6;
	}

	.ai-history {
		display: grid;
		gap: 0;
	}

	.ai-run {
		border-bottom: 1px solid var(--a-line);
	}

	.ai-run:first-child {
		border-top: 1px solid var(--a-line);
	}

	.ai-run summary {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 4px 16px;
		min-height: 44px;
		padding: 12px 0;
		cursor: pointer;
	}

	.ai-run summary:hover {
		color: var(--a-accent);
	}

	.ai-run-subject {
		font-size: 14px;
		font-weight: 500;
	}

	.ai-run-meta {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
	}

	.ai-run .ai-analysis {
		padding: 0 0 18px;
	}

	.ai-run:not([open]) .ai-analysis {
		display: none;
	}

	@media (max-width: 560px) {
		.ai-submit .a-btn {
			width: 100%;
		}
	}
</style>
