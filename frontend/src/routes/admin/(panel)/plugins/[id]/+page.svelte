<script lang="ts">
	import { page } from '$app/state';
	import { ApiError } from '#lib/api';
	import { adminApi } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import { adminComponentFor } from '#lib/plugins/admin/registry';
	import PluginSettingsForm from '#lib/plugins/admin/PluginSettingsForm.svelte';
	import { formatDate } from '#lib/format';
	import type { AdminPlugin, PluginSettings } from '#lib/types';

	type PluginDetail = {
		plugins: AdminPlugin[];
		plugin: AdminPlugin | null;
		settings: PluginSettings | null;
		settingsNote: string;
		settingsUnreadable: boolean;
	};

	const detail = createResource(async (signal): Promise<PluginDetail> => {
		const id = page.params.id;
		const data = await adminApi.adminPlugins(signal);
		const found = id ? (data.plugins.find((item) => item.id === id) ?? null) : null;
		const empty: PluginDetail = {
			plugins: data.plugins,
			plugin: found,
			settings: null,
			settingsNote: '',
			settingsUnreadable: false
		};
		if (!found) return { ...empty, plugin: null };
		if (!found.settings?.length) return empty;
		if (found.settings_state === 'unreadable') {
			return { ...empty, settings: blankSettings(found), settingsUnreadable: true };
		}

		try {
			return { ...empty, settings: await adminApi.pluginSettings(found.id) };
		} catch (caught) {
			if (caught instanceof ApiError && caught.status === 404) {
				return {
					...empty,
					settingsNote:
						'This plugin reports settings, but they could not be loaded. The rest of the plugin still works.'
				};
			}
			if (caught instanceof ApiError && caught.code === 'plugin_settings_unreadable') {
				return { ...empty, settings: blankSettings(found), settingsUnreadable: true };
			}
			throw caught;
		}
	});

	const plugin = $derived(detail.data?.plugin ?? null);
	const allPlugins = $derived(detail.data?.plugins ?? []);
	const settings = $derived(detail.data?.settings ?? null);
	const settingsNote = $derived(detail.data?.settingsNote ?? '');
	const settingsUnreadable = $derived(detail.data?.settingsUnreadable ?? false);
	const loading = $derived(detail.loading);
	const error = $derived(detail.error);
	let missing = $state(false);
	const notFound = $derived(missing || (detail.data !== null && detail.data.plugin === null));

	const AdminBody = $derived(plugin ? adminComponentFor(plugin.id) : null);
	const showAdminPath = $derived(
		plugin
			? (plugin.admin_path ?? '') !== '' &&
				plugin.admin_path !== page.url.pathname &&
				plugin.effective_enabled
			: false
	);

	$effect(() => {
		const id = page.params.id;
		missing = !id;
		if (id) void detail.load();
		else detail.reset();
		return () => detail.abort();
	});

	function blankSettings(item: AdminPlugin): PluginSettings {
		const values: Record<string, string> = {};
		for (const field of item.settings ?? []) {
			if (field.type !== 'secret' && field.default !== undefined) {
				values[field.key] = field.default;
			}
		}
		return {
			plugin_id: item.id,
			settings: values,
			secrets_set: [],
			configured: false
		};
	}

	function reload(): void {
		if (page.params.id) void detail.load();
	}

	function stateLabel(item: AdminPlugin): string {
		if (item.settings_state === 'unreadable') return 'Settings unreadable';
		return item.configured ? 'Configured' : 'Not configured';
	}

	function dependencyNames(ids: string[] | undefined): string {
		if (!ids?.length) return 'a dependency';
		return ids
			.map((id) => allPlugins.find((item) => item.id === id)?.name ?? id)
			.sort((a, b) => a.localeCompare(b))
			.join(', ');
	}

	function stateMark(item: AdminPlugin): string {
		if (item.settings_state === 'unreadable') return '!';
		return item.configured ? '✓' : '○';
	}

	function stateTone(item: AdminPlugin): string {
		if (item.settings_state === 'unreadable') return 'unreadable';
		return item.configured ? 'ok' : 'pending';
	}

	function settingsSaved(next: PluginSettings): void {
		detail.update((current) => {
			if (!current.plugin || current.plugin.id !== next.plugin_id) return current;
			return {
				...current,
				settings: next,
				settingsUnreadable: false,
				plugin: {
					...current.plugin,
					configured: next.configured,
					settings_state: 'ready',
					updated_at: next.updated_at
				}
			};
		});
	}
</script>

<svelte:head>
	<title>{plugin ? `${plugin.name} · Azahar admin` : 'Plugin · Azahar admin'}</title>
</svelte:head>

<div class="admin-page plugin-detail">
	<header class="a-page-head">
		<div class="a-page-title">
			<a class="back-link" href="/admin/plugins">Back to plugins</a>
			<h1>{plugin?.name ?? 'Plugin'}</h1>
			{#if plugin}
				<p class="a-page-sub">
					<span class="plugin-version">Version {plugin.version}</span>
					<span class="state-mark" data-state={stateTone(plugin)}>
						<span aria-hidden="true">{stateMark(plugin)}</span>
						{stateLabel(plugin)}
					</span>
					{#if plugin.product_type?.label}
						<span>Product type: {plugin.product_type.label}</span>
					{/if}
					{#if plugin.updated_at}
						<span>Updated {formatDate(plugin.updated_at)}</span>
					{/if}
				</p>
				<p class="plugin-lede">{plugin.description}</p>
			{/if}
		</div>
	</header>

	{#if loading}
		<LoadingState label="Reading the plugin…" />
	{:else if error}
		<ErrorState title="This plugin could not be read." message={error} onretry={reload} />
	{:else if notFound || !plugin}
		<div class="a-panel">
			<div class="a-empty">
				<h2>Plugin not found.</h2>
				<p>It is not installed, or its identifier changed. Check the plugin list and open it from there.</p>
				<a class="a-btn a-btn-secondary" href="/admin/plugins">Back to plugins</a>
			</div>
		</div>
	{:else}
		{#if !plugin.effective_enabled}
			<div class="a-notice key-warning">
				<p>
					<strong>{plugin.enabled ? `Blocked by ${dependencyNames(plugin.blocked_by)}.` : 'This plugin is disabled.'}</strong>
					{plugin.enabled
						? 'Enable the dependency on the Plugins page to use it.'
						: 'Enable it on the Plugins page to use its storefront and admin features.'}
				</p>
			</div>
		{/if}

		{#if settingsUnreadable}
			<div class="a-notice key-warning">
				<p>
					<strong>Settings cannot be read.</strong> The server encryption key changed. Enter
					every setting again and save to replace the old values.
				</p>
			</div>
		{/if}

		{#if plugin.settings?.length && settings}
			{#key plugin.id}
				<PluginSettingsForm {plugin} initial={settings} onsaved={settingsSaved} />
			{/key}
		{:else if settingsNote}
			<div class="a-state">
				<div class="a-state-copy">
					<p class="a-state-title">Settings are not available.</p>
					<p class="a-state-body">{settingsNote}</p>
				</div>
				<button class="a-btn a-btn-secondary" type="button" onclick={reload}>Retry</button>
			</div>
		{/if}

		{#if AdminBody}
			{#if plugin.effective_enabled}
				<AdminBody {plugin} />
			{:else}
				<div class="a-panel">
					<div class="a-empty">
						<h2>This plugin is not running.</h2>
						<p>
							Enable it on the Plugins page before using its admin screen.
						</p>
					</div>
				</div>
			{/if}
		{:else}
			<div class="a-panel">
				<div class="a-empty">
					<h2>No admin screen for this plugin.</h2>
					<p>
						{plugin.name} is installed. It changes the storefront or checkout and has no separate
						admin controls.
					</p>
					{#if showAdminPath}
						<a class="a-btn a-btn-secondary" href={plugin.admin_path}>Open plugin page</a>
					{/if}
				</div>
			</div>
		{/if}
	{/if}
</div>

<style>
	.plugin-detail {
		display: grid;
		gap: 20px;
		align-content: start;
	}

	.plugin-detail .a-page-head {
		margin-bottom: 0;
	}

	.back-link {
		display: inline-flex;
		align-items: center;
		justify-self: start;
		min-height: 44px;
		color: var(--a-muted);
		font-size: 13px;
		text-underline-offset: 3px;
	}

	.back-link:hover {
		color: var(--a-ink);
		text-decoration: underline;
	}

	.plugin-version {
		font-family: var(--a-font-mono);
		font-size: 11px;
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

	.state-mark[data-state='unreadable'] {
		color: var(--a-danger-text);
	}

	.plugin-lede {
		max-width: 70ch;
		color: var(--a-muted);
		font-size: 14px;
		line-height: 1.6;
	}

	.key-warning {
		border-color: color-mix(in oklab, var(--a-danger-text) 50%, var(--a-line));
	}

	.key-warning p {
		max-width: 70ch;
		line-height: 1.55;
	}

	@media (max-width: 560px) {
		.plugin-detail {
			gap: 16px;
		}
	}
</style>
