<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { ApiError } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import { formatDate } from '#lib/format';
	import type { AdminPlugin } from '#lib/types';

	const pluginList = createResource((signal) => adminApi.adminPlugins(signal));
	const plugins = $derived(pluginList.data?.plugins ?? []);
	const loading = $derived(pluginList.loading);
	const error = $derived(pluginList.error);
	const load = pluginList.load;

	let stateError = $state('');
	let saving = $state('');
	let pendingDisable = $state<{ plugin: AdminPlugin; dependents: AdminPlugin[] } | null>(null);
	let confirmPanel = $state<HTMLElement | null>(null);

	onMount(() => {
		void load();
	});

	function stateLabel(plugin: AdminPlugin): string {
		if (plugin.settings_state === 'unreadable') return 'Settings unreadable';
		return plugin.configured ? 'Configured' : 'Not configured';
	}

	function stateMark(plugin: AdminPlugin): string {
		if (plugin.settings_state === 'unreadable') return '!';
		return plugin.configured ? '✓' : '○';
	}

	function stateTone(plugin: AdminPlugin): string {
		if (plugin.settings_state === 'unreadable') return 'unreadable';
		return plugin.configured ? 'ok' : 'pending';
	}

	function updatedLabel(plugin: AdminPlugin): string {
		if (!plugin.settings?.length) return 'No settings';
		return plugin.updated_at ? formatDate(plugin.updated_at) : 'Not saved yet';
	}

	function pluginStateLabel(plugin: AdminPlugin): string {
		if (!plugin.enabled) return 'Disabled';
		if (!plugin.effective_enabled) return 'Blocked';
		return 'Enabled';
	}

	function pluginStateTone(plugin: AdminPlugin): string {
		if (!plugin.enabled) return 'off';
		if (!plugin.effective_enabled) return 'blocked';
		return 'on';
	}

	function pluginNames(ids: string[] | undefined): string {
		if (!ids?.length) return '';
		return ids
			.map((id) => plugins.find((plugin) => plugin.id === id)?.name ?? id)
			.sort((a, b) => a.localeCompare(b))
			.join(', ');
	}

	function enabledDependents(plugin: AdminPlugin): AdminPlugin[] {
		const ids = new Set(plugin.dependents ?? []);
		return plugins.filter((item) => item.enabled && ids.has(item.id));
	}

	async function requestToggle(plugin: AdminPlugin): Promise<void> {
		stateError = '';
		if (plugin.enabled) {
			const dependents = enabledDependents(plugin);
			if (dependents.length > 0) {
				pendingDisable = { plugin, dependents };
				await tick();
				confirmPanel?.focus();
				return;
			}
			await setPluginState(plugin, false);
			return;
		}
		await setPluginState(plugin, true);
	}

	async function setPluginState(
		plugin: AdminPlugin,
		enabled: boolean,
		confirmDependents = false
	): Promise<void> {
		stateError = '';
		saving = plugin.id;
		try {
			await adminApi.setPluginState(plugin.id, enabled, confirmDependents);
			pendingDisable = null;
			await load();
			await tick();
			focusPluginToggle(plugin.id);
		} catch (caught) {
			if (caught instanceof ApiError) {
				stateError = caught.message;
			} else {
				stateError = errorMessage(caught);
			}
		} finally {
			saving = '';
		}
	}

	function focusPluginToggle(id: string): void {
		requestAnimationFrame(() => {
			document.querySelector<HTMLButtonElement>(`[data-plugin-toggle="${id}"]`)?.focus();
		});
	}

	async function cancelWarning(): Promise<void> {
		if (!pendingDisable) return;
		const id = pendingDisable.plugin.id;
		pendingDisable = null;
		await tick();
		focusPluginToggle(id);
	}

	async function confirmDisable(): Promise<void> {
		if (!pendingDisable) return;
		await setPluginState(pendingDisable.plugin, false, true);
	}
</script>

<svelte:head>
	<title>Plugins · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div class="a-page-title">
			<h1>Plugins</h1>
			<p class="a-page-sub">
				<span>Installed modules extend the catalog, checkout and admin without changing the core.</span>
				{#if !loading && !error}
					<span>{plugins.length} {plugins.length === 1 ? 'plugin' : 'plugins'}</span>
				{/if}
			</p>
		</div>
	</header>

	{#if loading}
		<LoadingState label="Reading installed plugins…" />
	{:else if error}
		<ErrorState title="Plugins could not be read." message={error} onretry={load} />
	{:else if plugins.length === 0}
		<div class="a-panel">
			<div class="a-empty">
				<h2>No plugins are installed.</h2>
				<p>The core catalog, checkout and admin still work without any plugin.</p>
			</div>
		</div>
	{:else}
		{#if stateError}
			<p class="a-form-error" role="alert">{stateError}</p>
		{/if}

		{#if pendingDisable}
			<section
				bind:this={confirmPanel}
				class="a-panel plugin-confirm"
				role="alert"
				aria-labelledby="disable-title"
				aria-describedby="disable-body"
				tabindex="-1"
			>
				<h2 id="disable-title">Disable {pendingDisable.plugin.name}?</h2>
				<p id="disable-body">
					These enabled plugins depend on it: {pendingDisable.dependents
						.map((plugin) => plugin.name)
						.join(', ')}. They will stop working until {pendingDisable.plugin.name} is enabled
					again.
				</p>
				<div class="plugin-confirm-actions">
					<button
						class="a-btn a-btn-secondary"
						type="button"
						onclick={cancelWarning}
						disabled={saving !== ''}
					>
						Keep enabled
					</button>
					<button
						class="a-btn a-btn-danger"
						type="button"
						onclick={confirmDisable}
						disabled={saving !== ''}
					>
						{saving === pendingDisable.plugin.id ? 'Disabling…' : 'Disable anyway'}
					</button>
				</div>
			</section>
		{/if}

		<ul class="plugin-grid">
			{#each plugins as plugin, index (plugin.id)}
				<li>
					<article class="a-panel plugin-card">
						<div class="plugin-stamp">
							<span class="plugin-index" aria-hidden="true">{String(index + 1).padStart(2, '0')}</span>
							<div class="plugin-badges">
								<span class="state-mark" data-state={pluginStateTone(plugin)}>
									{pluginStateLabel(plugin)}
								</span>
								<span class="state-mark" data-state={stateTone(plugin)}>
									<span aria-hidden="true">{stateMark(plugin)}</span>
									{stateLabel(plugin)}
								</span>
							</div>
						</div>

						<h2>
							<span>{plugin.name}</span>
							<span class="plugin-version">v{plugin.version}</span>
						</h2>

						<p class="plugin-desc">{plugin.description}</p>

						{#if plugin.product_type?.label}
							<p class="plugin-type">Product type: {plugin.product_type.label}</p>
						{/if}

						{#if plugin.enabled && !plugin.effective_enabled}
							<p class="plugin-warning">
								<strong>Blocked by {pluginNames(plugin.blocked_by)}.</strong>
								Enable the dependency to use this plugin.
							</p>
						{/if}

						{#if plugin.settings_state === 'unreadable'}
							<p class="plugin-warning">
								<strong>The encryption key changed.</strong>
								Saved settings cannot be read. Open the plugin and enter them again.
							</p>
						{/if}

						<footer class="plugin-foot">
							<p class="plugin-updated">
								<span>Last updated</span>
								{updatedLabel(plugin)}
							</p>
							<div class="plugin-actions">
								<button
									class="a-btn a-btn-secondary a-btn-table"
									type="button"
									data-plugin-toggle={plugin.id}
									onclick={() => requestToggle(plugin)}
									disabled={saving !== ''}
								>
									{saving === plugin.id
										? 'Saving…'
										: plugin.enabled
											? 'Disable'
											: 'Enable'}
								</button>
								<a
									class="a-btn a-btn-secondary a-btn-table"
									href={`/admin/plugins/${plugin.id}`}
								>
									Open plugin
								</a>
							</div>
						</footer>
					</article>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.plugin-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 340px), 1fr));
		gap: 16px;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.plugin-card {
		display: flex;
		flex-direction: column;
		height: 100%;
		padding: 22px;
	}

	.plugin-stamp {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-bottom: 14px;
	}

	.plugin-badges {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: flex-end;
		gap: 10px;
	}

	.plugin-index {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
		font-variant-numeric: tabular-nums;
	}

	.state-mark {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--a-muted);
		font-size: 12px;
		white-space: nowrap;
	}

	.state-mark span[aria-hidden='true'] {
		font-family: var(--a-font-mono);
		font-size: 12px;
	}

	.state-mark[data-state='ok'] {
		color: var(--a-ink);
	}

	.state-mark[data-state='on'] {
		color: var(--a-ink);
	}

	.state-mark[data-state='off'] {
		color: var(--a-muted);
	}

	.state-mark[data-state='blocked'] {
		color: var(--a-danger-text);
	}

	.state-mark[data-state='unreadable'] {
		color: var(--a-danger-text);
	}

	.plugin-card h2 {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 6px 10px;
		font-size: 22px;
		font-weight: 500;
		letter-spacing: -0.02em;
	}

	.plugin-version {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
		font-weight: 400;
		letter-spacing: 0;
	}

	.plugin-desc {
		max-width: 58ch;
		margin-top: 9px;
		color: var(--a-muted);
		font-size: 14px;
		line-height: 1.55;
	}

	.plugin-type {
		margin-top: 11px;
		font-size: 13px;
	}

	.plugin-warning {
		max-width: 58ch;
		margin-top: 14px;
		padding: 10px 12px;
		border: 1px solid color-mix(in oklab, var(--a-danger-text) 50%, var(--a-line));
		border-radius: var(--a-radius-xs);
		color: var(--a-danger-text);
		font-size: 12px;
		line-height: 1.5;
	}

	.plugin-warning strong {
		font-weight: 600;
	}

	.plugin-foot {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-top: auto;
		padding-top: 16px;
		border-top: 1px solid var(--a-line);
	}

	.plugin-updated {
		display: grid;
		gap: 2px;
		color: var(--a-ink);
		font-size: 13px;
	}

	.plugin-updated span {
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
	}

	.plugin-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}

	.plugin-confirm {
		padding: 20px 22px;
		border-color: color-mix(in oklab, var(--a-danger-text) 40%, var(--a-line));
		background: color-mix(in oklab, var(--a-danger-text) 5%, var(--a-surface));
	}

	.plugin-confirm:focus-visible {
		outline: 2px solid var(--a-accent);
		outline-offset: 2px;
	}

	.plugin-confirm h2 {
		font-size: 18px;
		font-weight: 500;
		letter-spacing: -0.01em;
	}

	.plugin-confirm p {
		max-width: 70ch;
		margin-top: 6px;
		color: var(--a-muted);
		font-size: 13px;
		line-height: 1.55;
	}

	.plugin-confirm-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 10px;
		margin-top: 16px;
	}

	@media (max-width: 560px) {
		.plugin-card {
			padding: 18px;
		}

		.plugin-actions {
			width: 100%;
		}

		.plugin-actions .a-btn {
			flex: 1 1 100%;
		}
	}
</style>
