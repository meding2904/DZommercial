<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { ApiError, apiFetch } from '#lib/api';
	import { errorMessage } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import { formatDate, money } from '#lib/format';
	import type { AdminPlugin } from '#lib/types';

	let { plugin }: { plugin: AdminPlugin } = $props();

	type PlanStatus = 'preview' | 'applied' | 'failed';

	type ProductDraft = {
		name?: string;
		slug?: string;
		price_cents?: number;
		category_slug?: string;
		plugin_id?: string;
		stock?: number;
		image?: string;
		description?: string;
		attributes?: Record<string, string[]>;
	};

	type PlanAction = {
		type: string;
		patch?: Record<string, unknown>;
		product_id?: string;
		product?: ProductDraft;
		order_id?: string;
		status?: string;
	};

	type Plan = {
		id: string;
		summary: string;
		actions: PlanAction[];
		status: PlanStatus;
		created_at: string;
	};

	type ActionResult = {
		type: string;
		ok: boolean;
		message: string;
	};

	type HistoryPlan = {
		id: string;
		instruction: string;
		summary: string;
		status: PlanStatus;
		created_at: string;
	};

	const examples = [
		'Change the hero heading to “A shorter shelf, chosen slowly.”',
		'Raise the price of the face oil by 10 percent.'
	];

	let instruction = $state('');
	let instructionError = $state('');
	let planError = $state('');
	let applyError = $state('');

	let plan = $state<Plan | null>(null);
	let results = $state<ActionResult[]>([]);

	let formEl = $state<HTMLFormElement | null>(null);
	let instructionEl = $state<HTMLTextAreaElement | null>(null);
	let planPanel = $state<HTMLElement | null>(null);

	const historyList = createResource(async (signal) => {
		const data = await apiFetch<{ plans: HistoryPlan[] }>(
			'/plugins/codemode/admin/plans?limit=10',
			{ signal }
		);
		return data.plans;
	});
	const history = $derived(historyList.data ?? []);
	const historyLoading = $derived(historyList.loading);
	const historyError = $derived(historyList.error);
	const loadHistory = historyList.load;

	const preview = createResource(
		async (signal) => {
			const data = await apiFetch<{ plan: Plan }>('/plugins/codemode/admin/preview', {
				method: 'POST',
				body: { instruction: instruction.trim() },
				signal
			});
			return data.plan;
		},
		{ loading: false }
	);
	const planning = $derived(preview.loading);

	const applyRun = createResource(
		async (signal) => {
			const data = await apiFetch<{ plan: Plan; results: ActionResult[] }>(
				'/plugins/codemode/admin/apply',
				{
					method: 'POST',
					body: { plan_id: plan?.id ?? '' },
					signal
				}
			);
			return data;
		},
		{ loading: false }
	);
	const applying = $derived(applyRun.loading);

	const busy = $derived(planning || applying);
	const canApply = $derived(plan !== null && plan.status === 'preview' && !busy);
	const instructionDescribedBy = $derived(
		[instructionError ? 'cm-instruction-error' : '', 'cm-instruction-hint']
			.filter(Boolean)
			.join(' ')
	);

	onMount(() => {
		void loadHistory();
	});

	function instructionChanged(): void {
		if (instructionError) instructionError = '';
		if (plan) {
			plan = null;
			results = [];
			applyError = '';
		}
	}

	function useExample(text: string): void {
		instruction = text;
		instructionChanged();
		instructionEl?.focus();
	}

	async function focusFirstError(): Promise<void> {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
	}

	function requestError(caught: unknown): string {
		if (caught instanceof ApiError) {
			switch (caught.code) {
				case 'plugin_not_configured':
					return 'Code mode is not configured. Add the API key and model in the settings above, then plan again.';
				case 'plugin_disabled':
					return 'Code mode is disabled. Turn on “Enable planning” in the settings above, then plan again.';
				case 'plugin_settings_unreadable':
					return 'The plugin settings cannot be read with the current key. Enter them again in the settings above.';
				case 'plan_already_applied':
					return 'This plan was already applied. Plan the change again to get a fresh preview.';
				case 'plan_stale':
					return 'The store changed after this preview. Plan the change again to get a fresh preview.';
				case 'ai_upstream_error':
					return 'The AI provider did not return a usable plan. Try again shortly.';
				case 'invalid_plan':
					return caught.message || 'The provider returned a plan this server cannot apply. Try rephrasing the instruction.';
				default:
					return caught.message;
			}
		}
		return errorMessage(caught);
	}

	async function planChanges(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy) return;

		planError = '';
		applyError = '';
		if (!instruction.trim()) {
			instructionError = 'Describe the change you want to make.';
			await focusFirstError();
			return;
		}

		const outcome = await preview.load();
		if (outcome.status === 'discarded') return;
		if (outcome.status === 'error') {
			if (outcome.error instanceof ApiError && outcome.error.fields.instruction) {
				instructionError = outcome.error.fields.instruction;
				planError = 'Check the marked field.';
				await focusFirstError();
				return;
			}
			planError = requestError(outcome.error);
			return;
		}
		plan = outcome.data;
		results = [];
		void loadHistory();
		await tick();
		planPanel?.focus();
	}

	async function applyPlan(): Promise<void> {
		if (!plan || !canApply) return;

		applyError = '';

		const outcome = await applyRun.load();
		if (outcome.status === 'discarded') return;
		if (outcome.status === 'error') {
			if (outcome.error instanceof ApiError && outcome.error.code === 'invalid_plan') {
				plan = { ...plan, status: 'failed' };
				results = [];
				applyError = requestError(outcome.error);
				void loadHistory();
				await tick();
				planPanel?.focus();
				return;
			}
			if (outcome.error instanceof ApiError && outcome.error.code === 'plan_already_applied') {
				plan = null;
				results = [];
				planError = requestError(outcome.error);
				void loadHistory();
				return;
			}
			applyError = requestError(outcome.error);
			return;
		}
		plan = outcome.data.plan;
		results = outcome.data.results;
		void loadHistory();
		await tick();
		planPanel?.focus();
	}

	function statusLabel(status: PlanStatus): string {
		switch (status) {
			case 'applied':
				return 'Applied';
			case 'failed':
				return 'Failed';
			default:
				return 'Preview';
		}
	}

	function planHeading(status: PlanStatus): string {
		switch (status) {
			case 'applied':
				return 'Plan applied';
			case 'failed':
				return 'Plan failed';
			default:
				return 'Review the plan';
		}
	}

	function humanField(field: string): string {
		return field.replaceAll('_', ' ');
	}

	function themeFields(patch: Record<string, unknown> | undefined): string[] {
		if (!patch) return [];
		const fields: string[] = [];
		for (const [key, value] of Object.entries(patch)) {
			if ((key === 'light' || key === 'dark') && value && typeof value === 'object') {
				for (const colour of Object.keys(value as Record<string, unknown>)) {
					fields.push(`${key} ${humanField(colour)}`);
				}
				continue;
			}
			fields.push(humanField(key));
		}
		return fields;
	}

	function actionTitle(action: PlanAction): string {
		switch (action.type) {
			case 'theme.update':
				return 'Update the storefront theme';
			case 'product.update':
				return 'Update a product';
			case 'product.archive':
				return 'Archive a product';
			case 'product.create':
				return 'Create a product';
			case 'order.status':
				return 'Move an order';
			default:
				return humanField(action.type);
		}
	}

	function actionDetail(action: PlanAction): string {
		if (action.type === 'theme.update') {
			const fields = themeFields(action.patch);
			return fields.length > 0 ? `Changes ${fields.join(', ')}.` : 'Changes the storefront theme.';
		}
		if (action.type === 'product.update') {
			const parts: string[] = [];
			const patch = action.patch ?? {};
			if (typeof patch.name === 'string') parts.push(`name “${patch.name}”`);
			if (typeof patch.price_cents === 'number') parts.push(`price ${money(patch.price_cents)}`);
			if (typeof patch.stock === 'number') parts.push(`stock ${patch.stock}`);
			if (typeof patch.active === 'boolean') parts.push(patch.active ? 'active' : 'inactive');
			if (parts.length === 0) parts.push('product details');
			return `${parts.join(', ')}. Product ${action.product_id ?? ''}.`;
		}
		if (action.type === 'product.archive') {
			return `Product ${action.product_id ?? ''} is hidden from the shop but kept in the catalogue.`;
		}
		if (action.type === 'product.create') {
			const product = action.product;
			if (!product) return 'Creates a product.';
			const parts = [
				product.name ? `“${product.name}”` : 'a new product',
				typeof product.price_cents === 'number' ? money(product.price_cents) : '',
				product.category_slug ?? '',
				typeof product.stock === 'number' ? `${product.stock} in stock` : ''
			].filter(Boolean);
			return `${parts.join(', ')}.`;
		}
		if (action.type === 'order.status') {
			return `Order ${action.order_id ?? ''} moves to ${action.status ?? 'a new status'}.`;
		}
		return 'A change this console does not recognise.';
	}
</script>

<section class="a-panel" aria-labelledby="cm-instruction-title">
	<header class="a-panel-head">
		<h2 id="cm-instruction-title">Plan a change</h2>
		<p>{plugin.name} turns one instruction into a reviewable plan.</p>
	</header>
	<div class="a-panel-body">
		<p class="cm-privacy">
			The provider sees the current theme, the catalogue and recent order numbers and statuses.
			It never sees customer names, emails, addresses, payments, secrets or plugin settings.
			Planning does not change the store.
		</p>

		{#if plugin.settings_state === 'unreadable'}
			<p class="a-form-error cm-config-note" role="status">
				The plugin settings cannot be read with the current key. Enter them again above before
				planning.
			</p>
		{:else if !plugin.configured}
			<p class="a-field-hint cm-config-note">
				Add the API key and model in the settings above. The first plan needs both.
			</p>
		{/if}

		<form bind:this={formEl} onsubmit={planChanges} novalidate aria-busy={planning}>
			{#if planError}
				<p class="a-form-error" role="alert">{planError}</p>
			{/if}

			<div class="a-field">
				<label class="a-label" for="cm-instruction">Instruction</label>
				<textarea
					bind:this={instructionEl}
					id="cm-instruction"
					bind:value={instruction}
					oninput={instructionChanged}
					rows="5"
					maxlength="2000"
					placeholder="Change the hero heading to “A shorter shelf, chosen slowly.”"
					aria-invalid={instructionError ? 'true' : undefined}
					aria-describedby={instructionDescribedBy}
					disabled={busy}
				></textarea>
				{#if instructionError}
					<p class="a-field-error" id="cm-instruction-error">{instructionError}</p>
				{/if}
				<p class="a-field-hint" id="cm-instruction-hint">
					Describe one change in plain language. Up to 2000 characters.
				</p>
			</div>

			<div class="cm-examples">
				<span class="cm-examples-label">Try an example</span>
				{#each examples as example}
					<button
						class="a-btn a-btn-quiet cm-example"
						type="button"
						onclick={() => useExample(example)}
						disabled={busy}
					>
						{example}
					</button>
				{/each}
			</div>

			<footer class="cm-submit">
				<p class="a-field-hint">A plan can take a few seconds while the provider answers.</p>
				<button class="a-btn a-btn-primary" type="submit" disabled={busy}>
					{planning ? 'Planning…' : 'Plan changes'}
				</button>
			</footer>
		</form>
	</div>
</section>

{#if plan}
	<section
		class="a-panel cm-plan"
		bind:this={planPanel}
		tabindex="-1"
		aria-labelledby="cm-plan-title"
		aria-busy={applying}
	>
		<header class="a-panel-head">
			<div class="cm-plan-heading">
				<h2 id="cm-plan-title">{planHeading(plan.status)}</h2>
				<span class="cm-status" data-status={plan.status}>{statusLabel(plan.status)}</span>
			</div>
			<p>{formatDate(plan.created_at)}</p>
		</header>
		<div class="a-panel-body">
			<p class="cm-summary">{plan.summary}</p>

			{#if applyError}
				<p class="a-form-error" role="alert">{applyError}</p>
			{/if}

			<ol class="cm-actions" aria-label="Planned actions">
				{#each plan.actions as action, index}
					<li class="cm-action">
						<span class="cm-action-index" aria-hidden="true">{index + 1}</span>
						<div class="cm-action-copy">
							<div class="cm-action-head">
								<span class="cm-action-title">{actionTitle(action)}</span>
								<span class="cm-action-type">{action.type}</span>
							</div>
							<p class="cm-action-detail">{actionDetail(action)}</p>
						</div>
					</li>
				{/each}
			</ol>

			{#if plan.status === 'preview'}
				<footer class="cm-submit">
					<p class="a-field-hint">Nothing has changed yet. Review the list, then apply the plan.</p>
					<button class="a-btn a-btn-primary" type="button" onclick={applyPlan} disabled={!canApply}>
						{applying ? 'Applying…' : 'Apply plan'}
					</button>
				</footer>
			{:else if plan.status === 'applied'}
				<p class="cm-outcome cm-outcome-success" role="status">
					The plan was applied. Every action below was written to the store and the audit log.
				</p>
			{:else}
				<p class="cm-outcome cm-outcome-failed" role="alert">
					The plan failed and no changes were kept. Plan the change again after fixing the cause.
				</p>
			{/if}

			{#if results.length > 0}
				<h3 class="cm-results-title">Action results</h3>
				<ol class="cm-results" aria-label="Action results">
					{#each results as result, index}
						<li class="cm-result" data-ok={result.ok}>
							<span class="cm-result-mark" aria-hidden="true">{result.ok ? '✓' : '×'}</span>
							<div class="cm-result-copy">
								<p class="cm-result-head">
									<span>{index + 1}. {actionTitle({ type: result.type })}</span>
									<span class="cm-result-state">{result.ok ? 'Applied' : 'Failed'}</span>
								</p>
								<p class="cm-result-message">{result.message}</p>
							</div>
						</li>
					{/each}
				</ol>
			{/if}
		</div>
	</section>
{/if}

{#if historyLoading}
	<LoadingState label="Reading previous plans…" />
{:else if historyError}
	<ErrorState
		title="Plan history could not be read."
		message={historyError}
		onretry={loadHistory}
	/>
{:else}
	<section class="a-panel" aria-labelledby="cm-history-title">
		<header class="a-panel-head">
			<h2 id="cm-history-title">History</h2>
			<p>Latest 10 plans, previews included.</p>
		</header>
		<div class="a-panel-body cm-history-body">
			{#if history.length === 0}
				<div class="a-empty">
					<h3>No plans yet.</h3>
					<p>Plan a change above. Each preview is kept here with its status and instruction.</p>
				</div>
			{:else}
				<ol class="cm-history">
					{#each history as item (item.id)}
						<li class="cm-history-row">
							<div class="cm-history-head">
								<span class="cm-status" data-status={item.status}>{statusLabel(item.status)}</span>
								<time class="cm-history-date" datetime={item.created_at}>
									{formatDate(item.created_at)}
								</time>
							</div>
							<p class="cm-history-instruction">{item.instruction}</p>
							<p class="cm-history-summary">{item.summary || 'No summary was saved.'}</p>
						</li>
					{/each}
				</ol>
			{/if}
		</div>
	</section>
{/if}

<style>
	.cm-privacy {
		max-width: 70ch;
		margin-bottom: 18px;
		padding-bottom: 14px;
		border-bottom: 1px solid var(--a-line);
		color: var(--a-muted);
		font-size: 13px;
		line-height: 1.55;
	}

	.cm-config-note {
		margin-bottom: 16px;
	}

	.cm-examples {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px 16px;
		margin-top: 14px;
	}

	.cm-examples-label {
		color: var(--a-muted);
		font-size: 12px;
	}

	.cm-example {
		justify-content: flex-start;
		max-width: 44ch;
		padding-inline: 0;
		color: var(--a-muted);
		font-size: 13px;
		text-align: left;
		white-space: normal;
	}

	.cm-submit {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-top: 18px;
		padding-top: 14px;
		border-top: 1px solid var(--a-line);
	}

	.cm-plan:focus-visible {
		outline: 2px solid var(--a-accent);
		outline-offset: 3px;
	}

	.cm-plan-heading {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
	}

	.cm-status {
		display: inline-flex;
		align-items: center;
		min-height: 24px;
		padding: 1px 8px;
		border: 1px solid var(--a-line-strong);
		border-radius: var(--a-radius-xs);
		color: var(--a-muted);
		font-size: 11px;
		line-height: 1.4;
		white-space: nowrap;
	}

	.cm-status[data-status='applied'] {
		border-color: color-mix(in oklab, var(--a-status-confirmed) 50%, var(--a-line));
		color: var(--a-status-confirmed);
	}

	.cm-status[data-status='failed'] {
		border-color: color-mix(in oklab, var(--a-danger-text) 55%, var(--a-line));
		color: var(--a-danger-text);
	}

	.cm-summary {
		max-width: 70ch;
		margin-bottom: 16px;
		font-size: 16px;
		line-height: 1.5;
	}

	.cm-actions {
		display: grid;
		margin: 0;
		padding: 0;
		border-top: 1px solid var(--a-line);
		list-style: none;
	}

	.cm-action {
		display: grid;
		grid-template-columns: 26px minmax(0, 1fr);
		gap: 10px;
		padding: 13px 0;
		border-bottom: 1px solid var(--a-line);
	}

	.cm-action-index {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 12px;
		line-height: 1.6;
	}

	.cm-action-copy {
		min-width: 0;
	}

	.cm-action-head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 2px 12px;
	}

	.cm-action-title {
		font-size: 14px;
		font-weight: 500;
	}

	.cm-action-type {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 10px;
		overflow-wrap: anywhere;
	}

	.cm-action-detail {
		max-width: 70ch;
		margin-top: 3px;
		color: var(--a-muted);
		font-size: 13px;
		line-height: 1.5;
		overflow-wrap: anywhere;
	}

	.cm-outcome {
		margin-top: 16px;
		padding: 11px 13px;
		border: 1px solid var(--a-line-strong);
		border-radius: var(--a-radius-xs);
		font-size: 13px;
		line-height: 1.5;
	}

	.cm-outcome-success {
		border-color: color-mix(in oklab, var(--a-status-confirmed) 50%, var(--a-line));
		background: color-mix(in oklab, var(--a-status-confirmed) 8%, var(--a-surface));
	}

	.cm-outcome-failed {
		border-color: color-mix(in oklab, var(--a-danger-text) 55%, var(--a-line));
		background: color-mix(in oklab, var(--a-danger-text) 6%, var(--a-surface));
		color: var(--a-danger-text);
	}

	.cm-results-title {
		margin: 20px 0 8px;
		font-size: 15px;
		font-weight: 500;
	}

	.cm-results {
		display: grid;
		margin: 0;
		padding: 0;
		border-top: 1px solid var(--a-line);
		list-style: none;
	}

	.cm-result {
		display: grid;
		grid-template-columns: 20px minmax(0, 1fr);
		gap: 8px;
		padding: 11px 0;
		border-bottom: 1px solid var(--a-line);
	}

	.cm-result-mark {
		color: var(--a-status-confirmed);
		font-family: var(--a-font-mono);
		font-size: 14px;
		line-height: 1.5;
	}

	.cm-result[data-ok='false'] .cm-result-mark {
		color: var(--a-danger-text);
	}

	.cm-result-copy {
		min-width: 0;
	}

	.cm-result-head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 2px 12px;
		font-size: 13px;
		font-weight: 500;
	}

	.cm-result-state {
		color: var(--a-muted);
		font-size: 11px;
		font-weight: 400;
	}

	.cm-result[data-ok='false'] .cm-result-state {
		color: var(--a-danger-text);
	}

	.cm-result-message {
		margin-top: 3px;
		color: var(--a-muted);
		font-size: 13px;
		line-height: 1.5;
		overflow-wrap: anywhere;
	}

	.cm-history-body {
		padding-top: 4px;
	}

	.cm-history {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.cm-history-row {
		padding: 14px 0;
		border-bottom: 1px solid var(--a-line);
	}

	.cm-history-row:last-child {
		border-bottom: 0;
	}

	.cm-history-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 6px 12px;
	}

	.cm-history-date {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
	}

	.cm-history-instruction {
		max-width: 70ch;
		margin-top: 7px;
		font-size: 14px;
		line-height: 1.5;
		overflow-wrap: anywhere;
	}

	.cm-history-summary {
		max-width: 70ch;
		margin-top: 2px;
		color: var(--a-muted);
		font-size: 13px;
		line-height: 1.5;
		overflow-wrap: anywhere;
	}

	@media (max-width: 620px) {
		.cm-examples {
			align-items: stretch;
			flex-direction: column;
			gap: 4px;
		}

		.cm-example {
			max-width: none;
			min-height: 44px;
			white-space: normal;
		}

		.cm-submit {
			align-items: stretch;
			flex-direction: column;
		}

		.cm-submit .a-btn {
			width: 100%;
		}

		.cm-action {
			grid-template-columns: 20px minmax(0, 1fr);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.cm-plan,
		.cm-example {
			transition: none;
		}
	}
</style>
