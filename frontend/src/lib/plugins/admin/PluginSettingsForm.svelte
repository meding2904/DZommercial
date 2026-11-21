<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { beforeNavigate } from '$app/navigation';
	import { ApiError } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import type { AdminPlugin, PluginSettingField, PluginSettings } from '#lib/types';

	let {
		plugin,
		initial,
		onsaved
	}: {
		plugin: AdminPlugin;
		initial: PluginSettings;
		onsaved?: (settings: PluginSettings) => void;
	} = $props();

	const fields = $derived(plugin.settings ?? []);

	function normalize(
		source: Record<string, string>,
		list: PluginSettingField[]
	): Record<string, string> {
		const next: Record<string, string> = {};
		for (const field of list) {
			const value = source[field.key] ?? '';
			if (field.type === 'secret') {
				next[field.key] = '';
			} else if (field.type === 'boolean') {
				next[field.key] = value === 'true' ? 'true' : 'false';
			} else if (field.type === 'select' && !value && field.options?.length) {
				next[field.key] = field.options[0].value;
			} else {
				next[field.key] = value;
			}
		}
		return next;
	}

	function snapshot(values: Record<string, string>, list: PluginSettingField[]): string {
		return JSON.stringify(list.map((field) => [field.key, values[field.key] ?? '']));
	}

	const start = untrack(() => {
		const list = plugin.settings ?? [];
		const normalized = normalize(initial.settings, list);
		return {
			values: normalized,
			snapshot: snapshot(normalized, list),
			secrets: [...initial.secrets_set]
		};
	});

	let values = $state<Record<string, string>>({ ...start.values });
	let secretsSet = $state<string[]>([...start.secrets]);
	let savedSnapshot = $state(start.snapshot);
	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let busy = $state(false);
	let notice = $state('');
	let formEl = $state<HTMLFormElement | null>(null);

	const dirty = $derived(snapshot(values, fields) !== savedSnapshot);

	beforeNavigate(({ cancel }) => {
		if (!dirty) return;
		if (!window.confirm('Leave these plugin settings? Unsaved changes will be lost.')) {
			cancel();
		}
	});

	function fieldDomId(key: string): string {
		return `setting-${key.replace(/[^a-zA-Z0-9_-]/g, '-')}`;
	}

	function errorId(key: string): string {
		return `${fieldDomId(key)}-error`;
	}

	function hintId(key: string): string {
		return `${fieldDomId(key)}-hint`;
	}

	function hasSavedSecret(key: string): boolean {
		return secretsSet.includes(key);
	}

	function hintFor(field: PluginSettingField): string {
		const parts: string[] = [];
		if (field.type === 'secret' && hasSavedSecret(field.key)) {
			parts.push('A value is saved. Leave this blank to keep it.');
		}
		if (field.help) parts.push(field.help);
		return parts.join(' ');
	}

	function describedBy(field: PluginSettingField): string | undefined {
		if (fieldErrors[field.key]) return errorId(field.key);
		return hintFor(field) ? hintId(field.key) : undefined;
	}

	function setValue(key: string, value: string): void {
		values = { ...values, [key]: value };
		if (fieldErrors[key]) {
			const next = { ...fieldErrors };
			delete next[key];
			fieldErrors = next;
		}
		notice = '';
	}

	function validate(): Record<string, string> {
		const errors: Record<string, string> = {};
		for (const field of fields) {
			if (field.type === 'boolean') continue;
			const value = (values[field.key] ?? '').trim();
			const keepsSecret = field.type === 'secret' && hasSavedSecret(field.key);

			if (!value) {
				if (field.required && !keepsSecret) errors[field.key] = 'Enter this setting.';
				continue;
			}

			if (field.type === 'number' && !Number.isFinite(Number(value))) {
				errors[field.key] = 'Enter a number.';
				continue;
			}

			if (field.type === 'select') {
				const valid = (field.options ?? []).some((option) => option.value === value);
				if (!valid) {
					errors[field.key] = 'Choose one of the listed options.';
					continue;
				}
			}

			if (field.max_length && value.length > field.max_length) {
				errors[field.key] = `Keep this under ${field.max_length + 1} characters.`;
			}
		}
		return errors;
	}

	function payload(): Record<string, string> {
		const next: Record<string, string> = {};
		for (const field of fields) {
			const value = values[field.key] ?? '';
			if (
				value === '' &&
				!field.required &&
				(field.type === 'number' || field.type === 'select')
			) {
				continue;
			}
			next[field.key] = field.type === 'boolean' ? (value === 'true' ? 'true' : 'false') : value;
		}
		return next;
	}

	async function focusFirstError(): Promise<void> {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"], [data-invalid="true"]')?.focus();
	}

	async function submit(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy) return;

		formError = '';
		notice = '';
		const errors = validate();
		fieldErrors = errors;

		if (Object.keys(errors).length > 0) {
			formError = 'Check the marked fields.';
			await focusFirstError();
			return;
		}

		busy = true;
		try {
			const next = await adminApi.savePluginSettings(plugin.id, payload());
			const nextValues = normalize(next.settings, fields);
			values = { ...nextValues };
			secretsSet = [...next.secrets_set];
			savedSnapshot = snapshot(nextValues, fields);
			fieldErrors = {};
			notice = 'Settings saved.';
			onsaved?.(next);
		} catch (caught) {
			if (caught instanceof ApiError) {
				formError = caught.fields.settings ?? caught.message;
				fieldErrors = { ...caught.fields };
				if (Object.keys(caught.fields).length > 0) {
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
</script>

<section class="a-panel" id="plugin-settings" aria-labelledby="plugin-settings-title">
	<header class="a-panel-head">
		<h2 id="plugin-settings-title">Settings</h2>
		<p>Saved on the server. Secret values are never sent back to this page.</p>
	</header>

	<form class="a-panel-body settings-form" bind:this={formEl} onsubmit={submit} novalidate>
		{#if formError}
			<p class="a-form-error" role="alert">{formError}</p>
		{/if}
		{#if notice}
			<div class="a-form-success" role="status">
				<p>{notice}</p>
			</div>
		{/if}

		<div class="form-grid">
			{#each fields as field (field.key)}
				{#if field.type === 'boolean'}
					<div class="a-field span-2">
						<label class="a-check" for={fieldDomId(field.key)}>
							<input
								id={fieldDomId(field.key)}
								type="checkbox"
								checked={values[field.key] === 'true'}
								disabled={busy}
								aria-invalid={fieldErrors[field.key] ? 'true' : undefined}
								aria-describedby={fieldErrors[field.key]
									? errorId(field.key)
									: hintFor(field)
										? hintId(field.key)
										: undefined}
								onchange={(event) =>
									setValue(field.key, event.currentTarget.checked ? 'true' : 'false')}
							/>
							<span>{field.label}</span>
						</label>
						{#if fieldErrors[field.key]}
							<p class="a-field-error" id={errorId(field.key)}>{fieldErrors[field.key]}</p>
						{:else if hintFor(field)}
							<p class="a-field-hint" id={hintId(field.key)}>{hintFor(field)}</p>
						{/if}
					</div>
				{:else if field.type === 'select'}
					<div class="a-field">
						<span class="a-label" id={`${fieldDomId(field.key)}-label`}>{field.label}</span>
						<Dropdown
							id={fieldDomId(field.key)}
							class="admin-dropdown"
							label={field.label}
							labelId={`${fieldDomId(field.key)}-label`}
							value={values[field.key] ?? ''}
							options={field.options ?? []}
							onchange={(value) => setValue(field.key, value)}
							invalid={!!fieldErrors[field.key]}
							describedBy={describedBy(field)}
							disabled={busy}
						/>
						{#if fieldErrors[field.key]}
							<p class="a-field-error" id={errorId(field.key)}>{fieldErrors[field.key]}</p>
						{:else if hintFor(field)}
							<p class="a-field-hint" id={hintId(field.key)}>{hintFor(field)}</p>
						{/if}
					</div>
				{:else}
					<div class="a-field span-2">
						<label for={fieldDomId(field.key)}>{field.label}</label>
						<input
							id={fieldDomId(field.key)}
							type={field.type === 'secret' ? 'password' : field.type === 'number' ? 'number' : 'text'}
							step={field.type === 'number' ? 'any' : undefined}
							inputmode={field.type === 'number' ? 'decimal' : undefined}
							autocomplete={field.type === 'secret' ? 'new-password' : 'off'}
							value={values[field.key] ?? ''}
							placeholder={field.type === 'secret' && hasSavedSecret(field.key)
								? 'Replace saved value'
								: (field.placeholder ?? '')}
							maxlength={field.max_length}
							required={field.required && !(field.type === 'secret' && hasSavedSecret(field.key))}
							disabled={busy}
							aria-invalid={fieldErrors[field.key] ? 'true' : undefined}
							aria-describedby={describedBy(field)}
							oninput={(event) => setValue(field.key, event.currentTarget.value)}
						/>
						{#if fieldErrors[field.key]}
							<p class="a-field-error" id={errorId(field.key)}>{fieldErrors[field.key]}</p>
						{:else if hintFor(field)}
							<p class="a-field-hint" id={hintId(field.key)}>{hintFor(field)}</p>
						{/if}
					</div>
				{/if}
			{/each}
		</div>

		<footer class="settings-foot">
			<p class="settings-note">
				{#if notice}
					Settings saved.
				{:else if dirty}
					Unsaved changes.
				{:else}
					Settings match the saved values.
				{/if}
			</p>
			<button class="a-btn a-btn-primary" type="submit" disabled={busy}>
				{busy ? 'Saving…' : 'Save settings'}
			</button>
		</footer>
	</form>
</section>

<style>
	.settings-form {
		display: grid;
	}

	.settings-foot {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-top: 20px;
		padding-top: 16px;
		border-top: 1px solid var(--a-line);
	}

	.settings-note {
		color: var(--a-muted);
		font-size: 12px;
	}

	.a-form-error {
		margin-bottom: 16px;
	}

	.a-form-success {
		margin-bottom: 16px;
	}

	@media (max-width: 560px) {
		.settings-foot .a-btn {
			width: 100%;
		}
	}
</style>
