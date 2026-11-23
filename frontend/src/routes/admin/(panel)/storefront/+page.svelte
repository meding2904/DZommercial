<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { beforeNavigate } from '$app/navigation';
	import { ApiError } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import {
		DEFAULT_THEME,
		STOREFRONT_STYLES,
		clampNumber,
		contrastRatio,
		fontStack,
		fontsFor
	} from '#lib/theme/storefront';
	import type { FontId, Palette, StorefrontTheme } from '#lib/theme/storefront';

	type PaletteKey = keyof Palette;
	type ThemeScalarKey = Exclude<keyof StorefrontTheme, 'light' | 'dark' | 'updated_at'>;
	type ThemeScalars = Pick<StorefrontTheme, ThemeScalarKey>;

	type ContrastCheck = {
		key: string;
		label: string;
		min: number;
		ratio: number;
		valid: boolean;
		pass: boolean;
	};

	const PALETTE_FIELDS: { key: PaletteKey; label: string }[] = [
		{ key: 'ground', label: 'Ground' },
		{ key: 'surface', label: 'Surface' },
		{ key: 'ink', label: 'Ink' },
		{ key: 'muted', label: 'Muted' },
		{ key: 'accent', label: 'Accent' },
		{ key: 'accent_ink', label: 'Accent ink' }
	];

	const BUNDLED_IMAGES = [
		{ value: '/images/hero-1600.webp', label: 'Hero, 1600 wide' },
		{ value: '/images/atelier-1200.webp', label: 'Atelier, 1200 wide' },
		{ value: '/images/hero-960.webp', label: 'Hero, 960 wide' },
		{ value: '/images/atelier-960.webp', label: 'Atelier, 960 wide' }
	];

	const DISPLAY_FONTS = fontsFor('display');
	const BODY_FONTS = fontsFor('body');
	const HEX = /^#[0-9a-f]{6}$/i;
	const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/;
	const EMAIL = /^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$/;

	function cloneTheme(source: StorefrontTheme): StorefrontTheme {
		return { ...source, light: { ...source.light }, dark: { ...source.dark } };
	}

	function paletteSnapshot(colors: Palette) {
		return {
			ground: colors.ground,
			surface: colors.surface,
			ink: colors.ink,
			muted: colors.muted,
			accent: colors.accent,
			accent_ink: colors.accent_ink
		};
	}

	function themeScalars(source: StorefrontTheme, normalize: boolean): ThemeScalars {
		const text = (value: string): string => (normalize ? value.trim() : value);
		return {
			brand_name: text(source.brand_name),
			tagline: text(source.tagline),
			description: text(source.description),
			hero_heading: text(source.hero_heading),
			hero_body: text(source.hero_body),
			hero_alt: text(source.hero_alt),
			hero_cta: text(source.hero_cta),
			hero_image: text(source.hero_image),
			statement: text(source.statement),
			shelf_heading: text(source.shelf_heading),
			shop_title: text(source.shop_title),
			house_label: text(source.house_label),
			about_heading: text(source.about_heading),
			about_body: text(source.about_body),
			about_image: text(source.about_image),
			about_alt: text(source.about_alt),
			closing_heading: text(source.closing_heading),
			footer_note: text(source.footer_note),
			footer_credit: text(source.footer_credit),
			contact_email: normalize
				? source.contact_email.trim().toLowerCase()
				: source.contact_email,
			announcement: text(source.announcement),
			concerns_enabled: normalize ? Boolean(source.concerns_enabled) : source.concerns_enabled,
			font_display: source.font_display,
			font_body: source.font_body,
			font_scale: normalize ? Number(source.font_scale) : source.font_scale,
			radius: normalize ? Math.round(Number(source.radius)) : source.radius
		};
	}

	function snapshot(source: StorefrontTheme): string {
		return JSON.stringify({
			...themeScalars(source, false),
			light: paletteSnapshot(source.light),
			dark: paletteSnapshot(source.dark)
		});
	}

	function normalizePalette(colors: Palette): Palette {
		return {
			ground: colors.ground.trim().toLowerCase(),
			surface: colors.surface.trim().toLowerCase(),
			ink: colors.ink.trim().toLowerCase(),
			muted: colors.muted.trim().toLowerCase(),
			accent: colors.accent.trim().toLowerCase(),
			accent_ink: colors.accent_ink.trim().toLowerCase()
		};
	}

	function payloadFrom(source: StorefrontTheme): StorefrontTheme {
		return {
			...themeScalars(source, true),
			light: normalizePalette(source.light),
			dark: normalizePalette(source.dark)
		};
	}

	function validImagePath(value: string): boolean {
		return value.startsWith('/images/') && !value.includes('..') && value.length <= 300;
	}

	function runeLength(value: string): number {
		return [...value].length;
	}

	function validEmail(value: string): boolean {
		return value.length <= 254 && EMAIL.test(value);
	}

	function fieldId(key: string): string {
		return `sf-${key.replace(/\./g, '-').replace(/_/g, '-')}`;
	}

	function errorId(key: string): string {
		return `${fieldId(key)}-error`;
	}

	function hintId(key: string): string {
		return `${fieldId(key)}-hint`;
	}

	function safeHex(value: string, fallback: string): string {
		const trimmed = value.trim();
		return HEX.test(trimmed) ? trimmed : fallback;
	}

	function thresholdLabel(minimum: number): string {
		return `${Number.isInteger(minimum) ? minimum : minimum.toFixed(1)}:1`;
	}

	function formatScale(value: number): string {
		return `${Number.isFinite(value) ? value.toFixed(2) : '1.00'}×`;
	}

	function formatRadius(value: number): string {
		return `${Number.isFinite(value) ? Math.round(value) : 0}px`;
	}

	function makeCheck(
		mode: 'light' | 'dark',
		key: string,
		label: string,
		foreground: string,
		background: string,
		minimum: number
	): ContrastCheck {
		const fg = foreground.trim();
		const bg = background.trim();
		const valid = HEX.test(fg) && HEX.test(bg);
		const ratio = valid ? contrastRatio(fg, bg) : 0;
		return {
			key: `${mode}.${key}`,
			label,
			min: minimum,
			ratio,
			valid,
			pass: valid && ratio >= minimum
		};
	}

	function checksFor(mode: 'light' | 'dark', colors: Palette): ContrastCheck[] {
		return [
			makeCheck(mode, 'ink', 'Ink on ground', colors.ink, colors.ground, 7),
			makeCheck(mode, 'muted', 'Muted on ground', colors.muted, colors.ground, 4.5),
			makeCheck(mode, 'surface_ink', 'Ink on surface', colors.ink, colors.surface, 4.5),
			makeCheck(mode, 'accent', 'Accent on ground', colors.accent, colors.ground, 4.5),
			makeCheck(mode, 'accent_ink', 'Accent ink on accent', colors.accent_ink, colors.accent, 4.5)
		];
	}

	function imageOptions(current: string): { value: string; label: string }[] {
		const options = [...BUNDLED_IMAGES];
		const trimmed = current.trim();
		if (!options.some((option) => option.value === trimmed)) {
			options.unshift({
				value: trimmed,
				label: trimmed ? 'Current path' : 'Choose a bundled image'
			});
		}
		return options;
	}

	function validateTheme(source: StorefrontTheme): Record<string, string> {
		const errors: Record<string, string> = {};

		const optional = (key: string, value: string, max: number) => {
			const clean = value.trim();
			if (runeLength(clean) > max) {
				errors[key] = `Use ${max} characters or fewer.`;
				return;
			}
			if (CONTROL.test(clean)) errors[key] = 'Use plain text without control characters.';
		};

		const text = (key: string, value: string, min: number, max: number) => {
			const clean = value.trim();
			const count = runeLength(clean);
			if (count < min) {
				errors[key] = min === 1 ? 'Enter a value.' : `Use at least ${min} characters.`;
				return;
			}
			if (count > max) {
				errors[key] = `Use ${max} characters or fewer.`;
				return;
			}
			if (CONTROL.test(clean)) errors[key] = 'Use plain text without control characters.';
		};

		text('brand_name', source.brand_name, 1, 40);
		text('tagline', source.tagline, 1, 120);
		optional('description', source.description, 400);
		text('hero_heading', source.hero_heading, 1, 120);
		optional('hero_body', source.hero_body, 500);
		optional('hero_alt', source.hero_alt, 160);
		text('hero_cta', source.hero_cta, 1, 60);
		text('statement', source.statement, 1, 220);
		text('shelf_heading', source.shelf_heading, 1, 120);
		text('shop_title', source.shop_title, 1, 80);
		text('house_label', source.house_label, 1, 40);
		optional('about_heading', source.about_heading, 120);
		optional('about_body', source.about_body, 2000);
		optional('about_alt', source.about_alt, 160);
		text('closing_heading', source.closing_heading, 1, 120);
		optional('footer_note', source.footer_note, 300);
		optional('footer_credit', source.footer_credit, 300);
		optional('announcement', source.announcement, 140);

		if (!validImagePath(source.hero_image.trim())) {
			errors.hero_image = 'Use a bundled /images/ path.';
		}
		if (!validImagePath(source.about_image.trim())) {
			errors.about_image = 'Use a bundled /images/ path.';
		}
		if (!validEmail(source.contact_email.trim())) {
			errors.contact_email = 'Enter a valid email address.';
		}

		if (!DISPLAY_FONTS.some((font) => font.id === source.font_display)) {
			errors.font_display = 'Choose one of the available display fonts.';
		}
		if (!BODY_FONTS.some((font) => font.id === source.font_body)) {
			errors.font_body = 'Choose one of the available text fonts.';
		}
		if (
			!Number.isFinite(source.font_scale) ||
			source.font_scale < 0.85 ||
			source.font_scale > 1.15
		) {
			errors.font_scale = 'Use a body text scale between 0.85 and 1.15.';
		}
		if (!Number.isInteger(source.radius) || source.radius < 0 || source.radius > 12) {
			errors.radius = 'Use a corner radius between 0 and 12.';
		}

		const palette = (mode: 'light' | 'dark') => {
			const colors = source[mode];
			let valid = true;
			for (const field of PALETTE_FIELDS) {
				if (!HEX.test(colors[field.key].trim())) {
					errors[`${mode}.${field.key}`] =
						'Use a six-digit hex colour, for example #2c2325.';
					valid = false;
				}
			}
			if (!valid) return;

			const check = (key: string, foreground: string, background: string, minimum: number) => {
				const ratio = contrastRatio(foreground.trim(), background.trim());
				if (ratio < minimum) {
					errors[`${mode}.${key}`] =
						`This pair measures ${ratio.toFixed(2)}:1. Use at least ${minimum.toFixed(1)}:1.`;
				}
			};

			check('ink', colors.ink, colors.ground, 7);
			check('muted', colors.muted, colors.ground, 4.5);
			check('surface_ink', colors.ink, colors.surface, 4.5);
			check('accent', colors.accent, colors.ground, 4.5);
			check('accent_ink', colors.accent_ink, colors.accent, 4.5);
		};

		palette('light');
		palette('dark');
		return errors;
	}

	const initialTheme = cloneTheme(DEFAULT_THEME);

	let theme = $state<StorefrontTheme>(initialTheme);
	let baseline = $state(snapshot(initialTheme));
	let defaults = $state<StorefrontTheme | null>(null);
	let busy = $state(false);
	let fieldErrors = $state<Record<string, string>>({});
	let styleChoice = $state('');
	let formError = $state('');
	let notice = $state('');
	let resetNotice = $state('');
	let formEl = $state<HTMLFormElement | null>(null);
	let previewMode = $state<'light' | 'dark'>('light');

	const storefront = createResource(() => adminApi.storefrontTheme(), {
		onLoaded: (data) => {
			theme = cloneTheme(data.theme);
			defaults = cloneTheme(data.defaults);
			baseline = snapshot(theme);
			styleChoice = detectStyle(theme);
		}
	});
	const loading = $derived(storefront.loading);
	const error = $derived(storefront.error);

	const brand = $derived(theme.brand_name.trim() || 'Azahar');
	const previewBrand = $derived(theme.brand_name.trim() || 'Brand name');
	const previewHeading = $derived(theme.hero_heading.trim() || 'The hero heading.');
	const previewBody = $derived(theme.tagline.trim() || 'The tagline line.');
	const previewCta = $derived(theme.hero_cta.trim() || 'Shop the shelf');
	const dirty = $derived(snapshot(theme) !== baseline);
	const lightChecks = $derived(checksFor('light', theme.light));
	const darkChecks = $derived(checksFor('dark', theme.dark));

	beforeNavigate(({ cancel }) => {
		if (!dirty) return;
		if (!window.confirm('Leave the storefront editor? Unsaved changes will be lost.')) {
			cancel();
		}
	});
	const heroImageOptions = $derived(imageOptions(theme.hero_image));
	const aboutImageOptions = $derived(imageOptions(theme.about_image));

	const previewStyle = $derived.by(() => {
		const colors = theme[previewMode];
		const radius = Math.round(clampNumber(theme.radius, 0, 12));
		const scale = clampNumber(theme.font_scale, 0.85, 1.15);
		return [
			`--pv-bg: ${safeHex(colors.ground, DEFAULT_THEME[previewMode].ground)}`,
			`--pv-surface: ${safeHex(colors.surface, DEFAULT_THEME[previewMode].surface)}`,
			`--pv-ink: ${safeHex(colors.ink, DEFAULT_THEME[previewMode].ink)}`,
			`--pv-muted: ${safeHex(colors.muted, DEFAULT_THEME[previewMode].muted)}`,
			`--pv-accent: ${safeHex(colors.accent, DEFAULT_THEME[previewMode].accent)}`,
			`--pv-accent-ink: ${safeHex(colors.accent_ink, DEFAULT_THEME[previewMode].accent_ink)}`,
			`--pv-radius: ${radius}px`,
			`--pv-scale: ${scale}`,
			`--pv-font-display: ${fontStack(theme.font_display, 'display')}`,
			`--pv-font-body: ${fontStack(theme.font_body, 'body')}`
		].join('; ');
	});

	function invalid(key: string): 'true' | undefined {
		return fieldErrors[key] ? 'true' : undefined;
	}

	function describedBy(key: string, hint?: string): string | undefined {
		return fieldErrors[key] ? errorId(key) : hint;
	}

	// The server reports the ink-on-surface check as surface_ink, which has no input of its own.
	function paletteDescription(mode: 'light' | 'dark', key: PaletteKey): string | undefined {
		const ids: string[] = [];
		const own = `${mode}.${key}`;
		if (fieldErrors[own]) ids.push(errorId(own));
		if ((key === 'ink' || key === 'surface') && fieldErrors[`${mode}.surface_ink`]) {
			ids.push(errorId(`${mode}.surface_ink`));
		}
		return ids.length ? ids.join(' ') : undefined;
	}

	function paletteInvalid(mode: 'light' | 'dark', key: PaletteKey): boolean {
		if (fieldErrors[`${mode}.${key}`]) return true;
		return key === 'ink' && Boolean(fieldErrors[`${mode}.surface_ink`]);
	}

	function clearFieldError(key: string) {
		if (!fieldErrors[key]) return;
		const next = { ...fieldErrors };
		delete next[key];
		fieldErrors = next;
		if (formError && Object.keys(next).length === 0) formError = '';
	}

	function applyStyle(id: string): void {
		styleChoice = id;
		const style = STOREFRONT_STYLES.find((item) => item.id === id);
		if (!style) return;
		theme.light = { ...style.light };
		theme.dark = { ...style.dark };
		theme.font_display = style.font_display;
		theme.font_body = style.font_body;
		theme.radius = style.radius;
		fieldErrors = {};
		formError = '';
	}

	function paletteMatches(left: Palette, right: Palette): boolean {
		return (Object.keys(left) as (keyof Palette)[]).every(
			(key) => left[key].toLowerCase() === right[key].toLowerCase()
		);
	}

	function detectStyle(source: StorefrontTheme): string {
		return (
			STOREFRONT_STYLES.find(
				(style) =>
					style.font_display === source.font_display &&
					style.font_body === source.font_body &&
					style.radius === Math.round(source.radius) &&
					paletteMatches(style.light, source.light) &&
					paletteMatches(style.dark, source.dark)
			)?.id ?? ''
		);
	}

	function handleInput(event: Event) {
		const target = event.target;
		if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) return;
		const key = target.dataset.field;
		if (!key) return;

		if (key === 'radius' || key.startsWith('light.') || key.startsWith('dark.')) {
			styleChoice = '';
		}
		if (key.includes('.')) {
			const mode = key.split('.')[0];
			const next: Record<string, string> = {};
			for (const [name, message] of Object.entries(fieldErrors)) {
				if (!name.startsWith(`${mode}.`)) next[name] = message;
			}
			fieldErrors = next;
		} else if (fieldErrors[key]) {
			const next = { ...fieldErrors };
			delete next[key];
			fieldErrors = next;
		}

		if (formError && Object.keys(fieldErrors).length === 0) formError = '';
	}

	function setFont(kind: 'display' | 'body', id: FontId) {
		styleChoice = '';
		if (kind === 'display') theme.font_display = id;
		else theme.font_body = id;
		clearFieldError(kind === 'display' ? 'font_display' : 'font_body');
	}

	async function focusFirstError() {
		await tick();
		formEl
			?.querySelector<HTMLElement>('[aria-invalid="true"], .font-card input[data-invalid="true"]')
			?.focus();
	}

	onMount(() => {
		void load();
	});

	$effect(() => {
		if (!dirty) return;
		const warn = (event: BeforeUnloadEvent) => {
			event.preventDefault();
			event.returnValue = '';
		};
		window.addEventListener('beforeunload', warn);
		return () => window.removeEventListener('beforeunload', warn);
	});

	async function load(): Promise<void> {
		notice = '';
		resetNotice = '';
		formError = '';
		fieldErrors = {};
		await storefront.load();
	}

	function resetToDefaults() {
		if (!defaults || busy) return;
		const changed = snapshot(defaults) !== baseline;
		theme = cloneTheme(defaults);
		styleChoice = detectStyle(theme);
		fieldErrors = {};
		formError = '';
		notice = '';
		resetNotice = changed
			? 'Shipped defaults loaded. Save to apply them.'
			: 'The form already matches the shipped defaults.';
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (busy || loading) return;

		notice = '';
		resetNotice = '';
		formError = '';
		const errors = validateTheme(theme);
		fieldErrors = errors;

		if (Object.keys(errors).length > 0) {
			formError = 'Check the marked fields.';
			await focusFirstError();
			return;
		}

		busy = true;
		try {
			const data = await adminApi.saveStorefrontTheme(payloadFrom(theme));
			theme = cloneTheme(data.theme);
			baseline = snapshot(theme);
			fieldErrors = {};
			formError = '';
			notice = 'Storefront saved.';
		} catch (caught) {
			if (caught instanceof ApiError) {
				formError = caught.message;
				fieldErrors = { ...caught.fields };
				if (Object.keys(fieldErrors).length > 0) {
					// The panel is inert while busy, so release it before moving focus.
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

<svelte:head>
	<title>Storefront · {brand} admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div class="a-page-title">
			<h1 id="storefront-title" tabindex="-1">Storefront</h1>
			<p class="a-page-sub">
				<span>Brand, copy, type and palette for the shop. Only admins can change this.</span>
				{#if dirty}
					<span class="dirty-chip">Unsaved changes</span>
				{/if}
			</p>
		</div>

		<div class="a-page-actions">
			<button
				class="a-btn a-btn-secondary"
				type="button"
				onclick={resetToDefaults}
				disabled={loading || busy || !defaults || error !== ''}
			>
				Reset to the shipped defaults
			</button>
			<button
				class="a-btn a-btn-primary"
				type="submit"
				form="storefront-form"
				disabled={loading || busy || error !== ''}
			>
				{busy ? 'Saving…' : 'Save storefront'}
			</button>
		</div>
	</header>

	{#if notice && !dirty}
		<div class="a-notice a-notice-success" role="status">
			<p>{notice}</p>
			<button class="a-btn a-btn-quiet" type="button" onclick={() => (notice = '')}>
				Dismiss
			</button>
		</div>
	{/if}

	{#if resetNotice}
		<div class="a-notice" role="status">
			<p>{resetNotice}</p>
			<button class="a-btn a-btn-quiet" type="button" onclick={() => (resetNotice = '')}>
				Dismiss
			</button>
		</div>
	{/if}

	{#if loading}
		<LoadingState label="Reading the storefront theme…" />
	{:else if error}
		<ErrorState
			title="The storefront theme could not be read."
			message={error}
			onretry={load}
		/>
	{:else}
		<div class="storefront-layout">
			<aside class="preview-col" aria-labelledby="storefront-preview-title">
				<div class="preview-panel">
					<div class="preview-head">
						<h2 id="storefront-preview-title">Preview</h2>
						<div class="preview-modes" role="group" aria-label="Preview palette">
							<button
								type="button"
								class="preview-mode"
								aria-pressed={previewMode === 'light'}
								onclick={() => (previewMode = 'light')}
							>
								Light
							</button>
							<button
								type="button"
								class="preview-mode"
								aria-pressed={previewMode === 'dark'}
								onclick={() => (previewMode = 'dark')}
							>
								Dark
							</button>
						</div>
					</div>

					<div class="storefront-preview" style={previewStyle} aria-hidden="true">
						<p class="pv-wordmark">{previewBrand}</p>
						<h3 class="pv-heading">{previewHeading}</h3>
						<p class="pv-body">{previewBody}</p>
						<span class="pv-cta">{previewCta}</span>
						<div class="pv-input">
							<span>Email address</span>
						</div>
					</div>

					<p class="preview-note">
						Sample type, colour and radius from the {previewMode} palette. This card is not
						the live shop.
					</p>
				</div>
			</aside>

			<form
				id="storefront-form"
				bind:this={formEl}
				class="storefront-form"
				aria-labelledby="storefront-title"
				aria-busy={busy}
				onsubmit={save}
				oninput={handleInput}
				novalidate
			>
				{#if formError}
					<div class="a-form-error form-alert" role="alert">{formError}</div>
				{/if}

				<div class="a-panel" inert={busy}>
					<div class="a-panel-body">
						<section class="form-section" aria-labelledby="sf-style-title">
							<h2 id="sf-style-title">Store style</h2>
							<div class="form-grid">
								<div class="a-field span-2">
									<span class="a-label" id="sf-style-label">Start from a style</span>
									<Dropdown
										id="sf-style"
										class="admin-dropdown"
										label="Start from a style"
										labelId="sf-style-label"
										value={styleChoice}
										options={[
											{ value: '', label: 'Choose a style' },
											...STOREFRONT_STYLES.map((style) => ({
												value: style.id,
												label: style.name
											}))
										]}
										onchange={applyStyle}
									/>
									<p class="a-field-hint">
										Applies a palette, font pairing and corner radius. Your copy and images stay
										exactly as they are.
									</p>
								</div>
							</div>
						</section>

						<section class="form-section" aria-labelledby="sf-identity-title">
							<h2 id="sf-identity-title">Identity</h2>
							<div class="form-grid">
								<div class="a-field">
									<label for={fieldId('brand_name')}>Brand name</label>
									<input
										id={fieldId('brand_name')}
										type="text"
										bind:value={theme.brand_name}
										required
										aria-invalid={invalid('brand_name')}
										aria-describedby={describedBy(
											'brand_name',
											hintId('brand_name')
										)}
										data-field="brand_name"
									/>
									{#if fieldErrors.brand_name}
										<p class="a-field-error" id={errorId('brand_name')}>
											{fieldErrors.brand_name}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('brand_name')}>
											The wordmark on the shop and in this page title.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('tagline')}>Tagline</label>
									<input
										id={fieldId('tagline')}
										type="text"
										bind:value={theme.tagline}
										required
										aria-invalid={invalid('tagline')}
										aria-describedby={describedBy('tagline', hintId('tagline'))}
										data-field="tagline"
									/>
									{#if fieldErrors.tagline}
										<p class="a-field-error" id={errorId('tagline')}>
											{fieldErrors.tagline}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('tagline')}>
											One line under the wordmark.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('description')}>Description</label>
									<textarea
										id={fieldId('description')}
										rows="3"
										bind:value={theme.description}
										aria-invalid={invalid('description')}
										aria-describedby={describedBy(
											'description',
											hintId('description')
										)}
										data-field="description"
									></textarea>
									{#if fieldErrors.description}
										<p class="a-field-error" id={errorId('description')}>
											{fieldErrors.description}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('description')}>
											Used for search results and the share card.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('contact_email')}>Contact email</label>
									<input
										id={fieldId('contact_email')}
										type="email"
										autocomplete="email"
										bind:value={theme.contact_email}
										required
										aria-invalid={invalid('contact_email')}
										aria-describedby={describedBy(
											'contact_email',
											hintId('contact_email')
										)}
										data-field="contact_email"
									/>
									{#if fieldErrors.contact_email}
										<p class="a-field-error" id={errorId('contact_email')}>
											{fieldErrors.contact_email}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('contact_email')}>
											The address customers can write to.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('announcement')}>Announcement</label>
									<input
										id={fieldId('announcement')}
										type="text"
										bind:value={theme.announcement}
										aria-invalid={invalid('announcement')}
										aria-describedby={describedBy(
											'announcement',
											hintId('announcement')
										)}
										data-field="announcement"
									/>
									{#if fieldErrors.announcement}
										<p class="a-field-error" id={errorId('announcement')}>
											{fieldErrors.announcement}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('announcement')}>
											Shown in the hero footnote and on the about page. Leave empty to hide it.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label class="a-check" for={fieldId('concerns_enabled')}>
										<input
											id={fieldId('concerns_enabled')}
											type="checkbox"
											bind:checked={theme.concerns_enabled}
											aria-describedby={hintId('concerns_enabled')}
											data-field="concerns_enabled"
										/>
										<span>Show the concern filter on the shop</span>
									</label>
									<p class="a-field-hint" id={hintId('concerns_enabled')}>
										Turn this off for a shop whose products have no skin concerns.
									</p>
								</div>
							</div>
						</section>

						<section class="form-section" aria-labelledby="sf-home-title">
							<h2 id="sf-home-title">Home</h2>
							<div class="form-grid">
								<div class="a-field span-2">
									<label for={fieldId('hero_heading')}>Hero heading</label>
									<input
										id={fieldId('hero_heading')}
										type="text"
										bind:value={theme.hero_heading}
										required
										aria-invalid={invalid('hero_heading')}
										aria-describedby={describedBy(
											'hero_heading',
											hintId('hero_heading')
										)}
										data-field="hero_heading"
									/>
									{#if fieldErrors.hero_heading}
										<p class="a-field-error" id={errorId('hero_heading')}>
											{fieldErrors.hero_heading}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('hero_heading')}>
											The first line on the home page.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('hero_body')}>Hero body</label>
									<textarea
										id={fieldId('hero_body')}
										rows="3"
										bind:value={theme.hero_body}
										aria-invalid={invalid('hero_body')}
										aria-describedby={describedBy(
											'hero_body',
											hintId('hero_body')
										)}
										data-field="hero_body"
									></textarea>
									{#if fieldErrors.hero_body}
										<p class="a-field-error" id={errorId('hero_body')}>
											{fieldErrors.hero_body}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('hero_body')}>
											The paragraph under the hero heading.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('hero_cta')}>Hero button</label>
									<input
										id={fieldId('hero_cta')}
										type="text"
										bind:value={theme.hero_cta}
										required
										aria-invalid={invalid('hero_cta')}
										aria-describedby={describedBy('hero_cta', hintId('hero_cta'))}
										data-field="hero_cta"
									/>
									{#if fieldErrors.hero_cta}
										<p class="a-field-error" id={errorId('hero_cta')}>
											{fieldErrors.hero_cta}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('hero_cta')}>
											The button label in the hero.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('hero_alt')}>Hero image description</label>
									<input
										id={fieldId('hero_alt')}
										type="text"
										bind:value={theme.hero_alt}
										aria-invalid={invalid('hero_alt')}
										aria-describedby={describedBy('hero_alt', hintId('hero_alt'))}
										data-field="hero_alt"
									/>
									{#if fieldErrors.hero_alt}
										<p class="a-field-error" id={errorId('hero_alt')}>
											{fieldErrors.hero_alt}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('hero_alt')}>
											Describe the photograph for screen readers.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('statement')}>Statement</label>
									<textarea
										id={fieldId('statement')}
										rows="3"
										bind:value={theme.statement}
										required
										aria-invalid={invalid('statement')}
										aria-describedby={describedBy('statement', hintId('statement'))}
										data-field="statement"
									></textarea>
									{#if fieldErrors.statement}
										<p class="a-field-error" id={errorId('statement')}>
											{fieldErrors.statement}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('statement')}>
											A short line about how the house works.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('shelf_heading')}>Shelf heading</label>
									<input
										id={fieldId('shelf_heading')}
										type="text"
										bind:value={theme.shelf_heading}
										required
										aria-invalid={invalid('shelf_heading')}
										aria-describedby={describedBy(
											'shelf_heading',
											hintId('shelf_heading')
										)}
										data-field="shelf_heading"
									/>
									{#if fieldErrors.shelf_heading}
										<p class="a-field-error" id={errorId('shelf_heading')}>
											{fieldErrors.shelf_heading}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('shelf_heading')}>
											The heading above the product shelf.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('shop_title')}>Shop page title</label>
									<input
										id={fieldId('shop_title')}
										type="text"
										bind:value={theme.shop_title}
										required
										aria-invalid={invalid('shop_title')}
										aria-describedby={describedBy('shop_title', hintId('shop_title'))}
										data-field="shop_title"
									/>
									{#if fieldErrors.shop_title}
										<p class="a-field-error" id={errorId('shop_title')}>
											{fieldErrors.shop_title}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('shop_title')}>
											The heading on the shop page and the label for its footer link.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('house_label')}>House nav label</label>
									<input
										id={fieldId('house_label')}
										type="text"
										bind:value={theme.house_label}
										required
										aria-invalid={invalid('house_label')}
										aria-describedby={describedBy('house_label', hintId('house_label'))}
										data-field="house_label"
									/>
									{#if fieldErrors.house_label}
										<p class="a-field-error" id={errorId('house_label')}>
											{fieldErrors.house_label}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('house_label')}>
											The short label for the about link and page title.
										</p>
									{/if}
								</div>
							</div>
						</section>

						<section class="form-section" aria-labelledby="sf-story-title">
							<h2 id="sf-story-title">Story</h2>
							<div class="form-grid">
								<div class="a-field">
									<label for={fieldId('about_heading')}>About heading</label>
									<input
										id={fieldId('about_heading')}
										type="text"
										bind:value={theme.about_heading}
										aria-invalid={invalid('about_heading')}
										aria-describedby={describedBy(
											'about_heading',
											hintId('about_heading')
										)}
										data-field="about_heading"
									/>
									{#if fieldErrors.about_heading}
										<p class="a-field-error" id={errorId('about_heading')}>
											{fieldErrors.about_heading}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('about_heading')}>
											The heading beside the story.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('closing_heading')}>Closing heading</label>
									<input
										id={fieldId('closing_heading')}
										type="text"
										bind:value={theme.closing_heading}
										required
										aria-invalid={invalid('closing_heading')}
										aria-describedby={describedBy(
											'closing_heading',
											hintId('closing_heading')
										)}
										data-field="closing_heading"
									/>
									{#if fieldErrors.closing_heading}
										<p class="a-field-error" id={errorId('closing_heading')}>
											{fieldErrors.closing_heading}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('closing_heading')}>
											The last line before the footer.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('about_body')}>About body</label>
									<textarea
										id={fieldId('about_body')}
										rows="5"
										bind:value={theme.about_body}
										aria-invalid={invalid('about_body')}
										aria-describedby={describedBy(
											'about_body',
											hintId('about_body')
										)}
										data-field="about_body"
									></textarea>
									{#if fieldErrors.about_body}
										<p class="a-field-error" id={errorId('about_body')}>
											{fieldErrors.about_body}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('about_body')}>
											The story of the house.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('about_alt')}>About image description</label>
									<input
										id={fieldId('about_alt')}
										type="text"
										bind:value={theme.about_alt}
										aria-invalid={invalid('about_alt')}
										aria-describedby={describedBy(
											'about_alt',
											hintId('about_alt')
										)}
										data-field="about_alt"
									/>
									{#if fieldErrors.about_alt}
										<p class="a-field-error" id={errorId('about_alt')}>
											{fieldErrors.about_alt}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('about_alt')}>
											Describe the photograph for screen readers.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('footer_note')}>Footer note</label>
									<textarea
										id={fieldId('footer_note')}
										rows="3"
										bind:value={theme.footer_note}
										aria-invalid={invalid('footer_note')}
										aria-describedby={describedBy(
											'footer_note',
											hintId('footer_note')
										)}
										data-field="footer_note"
									></textarea>
									{#if fieldErrors.footer_note}
										<p class="a-field-error" id={errorId('footer_note')}>
											{fieldErrors.footer_note}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('footer_note')}>
											The small print at the foot of the shop.
										</p>
									{/if}
								</div>

								<div class="a-field span-2">
									<label for={fieldId('footer_credit')}>Footer credit</label>
									<textarea
										id={fieldId('footer_credit')}
										rows="2"
										bind:value={theme.footer_credit}
										aria-invalid={invalid('footer_credit')}
										aria-describedby={describedBy('footer_credit', hintId('footer_credit'))}
										data-field="footer_credit"
									></textarea>
									{#if fieldErrors.footer_credit}
										<p class="a-field-error" id={errorId('footer_credit')}>
											{fieldErrors.footer_credit}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('footer_credit')}>
											Keep the photography credit if you use the bundled Pexels photographs.
										</p>
									{/if}
								</div>
							</div>
						</section>

						<section class="form-section" aria-labelledby="sf-type-title">
							<h2 id="sf-type-title">Type</h2>

							{#snippet fontPicker(
								kind: 'display' | 'body',
								legend: string,
								hint: string
							)}
								{@const errorKey = kind === 'display' ? 'font_display' : 'font_body'}
								{@const choices = fontsFor(kind)}
								<fieldset
									class="a-fieldset font-fieldset"
									aria-describedby={fieldErrors[errorKey]
										? errorId(errorKey)
										: hintId(errorKey)}
								>
									<legend>{legend}</legend>
									<p class="a-field-hint" id={hintId(errorKey)}>{hint}</p>
									<div class="font-grid">
										{#each choices as font (font.id)}
											<label
												class="font-card"
												data-selected={kind === 'display'
													? theme.font_display === font.id
													: theme.font_body === font.id}
											>
												<input
													type="radio"
													name={`sf-font-${kind}`}
													value={font.id}
													checked={kind === 'display'
														? theme.font_display === font.id
														: theme.font_body === font.id}
													onchange={() => setFont(kind, font.id)}
													data-invalid={fieldErrors[errorKey] ? 'true' : undefined}
													aria-describedby={fieldErrors[errorKey]
														? errorId(errorKey)
														: hintId(errorKey)}
													data-field={errorKey}
												/>
												<span class="font-copy" style={`font-family: ${font.stack}`}>
													<span class="font-sample" aria-hidden="true">
														{kind === 'display'
															? 'The small shelf.'
															: 'Oils and clays.'}
													</span>
													<span class="font-name">{font.label}</span>
												</span>
											</label>
										{/each}
									</div>
									{#if fieldErrors[errorKey]}
										<p class="a-field-error" id={errorId(errorKey)}>
											{fieldErrors[errorKey]}
										</p>
									{/if}
								</fieldset>
							{/snippet}

							{@render fontPicker(
								'display',
								'Display font',
								'Used for the wordmark, headings and buttons.'
							)}
							{@render fontPicker(
								'body',
								'Body font',
								'Used for body copy, labels and form fields.'
							)}

							<div class="form-grid type-measures">
								<div class="a-field">
									<label for={fieldId('font_scale')}>Body text scale</label>
									<div class="range-row">
										<input
											id={fieldId('font_scale')}
											type="range"
											min="0.85"
											max="1.15"
											step="0.01"
											bind:value={theme.font_scale}
											aria-invalid={invalid('font_scale')}
											aria-describedby={describedBy(
												'font_scale',
												hintId('font_scale')
											)}
											data-field="font_scale"
										/>
										<span class="range-value" aria-hidden="true">
											{formatScale(theme.font_scale)}
										</span>
									</div>
									{#if fieldErrors.font_scale}
										<p class="a-field-error" id={errorId('font_scale')}>
											{fieldErrors.font_scale}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('font_scale')}>
											0.85 to 1.15. The shop's base size follows this scale.
										</p>
									{/if}
								</div>

								<div class="a-field">
									<label for={fieldId('radius')}>Corner radius</label>
									<div class="range-row">
										<input
											id={fieldId('radius')}
											type="range"
											min="0"
											max="12"
											step="1"
											bind:value={theme.radius}
											aria-invalid={invalid('radius')}
											aria-describedby={describedBy('radius', hintId('radius'))}
											data-field="radius"
										/>
										<span class="range-value" aria-hidden="true">
											{formatRadius(theme.radius)}
										</span>
									</div>
									{#if fieldErrors.radius}
										<p class="a-field-error" id={errorId('radius')}>
											{fieldErrors.radius}
										</p>
									{:else}
										<p class="a-field-hint" id={hintId('radius')}>
											0 to 12 pixels. Cards, buttons and fields use this corner radius.
										</p>
									{/if}
								</div>
							</div>
						</section>

						<section class="form-section" aria-labelledby="sf-palette-title">
							<h2 id="sf-palette-title">Palette</h2>
							<p class="a-field-hint palette-hint">
								Six-digit hex, for example #2c2325. Five pairs are measured. Ink on ground
								needs 7:1. The other pairs need 4.5:1.
							</p>

							{#snippet palettePanel(mode: 'light' | 'dark', title: string)}
								<fieldset class="palette-block">
									<legend>{title}</legend>
									<div class="palette-grid">
										{#each PALETTE_FIELDS as field (field.key)}
											<div class="palette-field">
												<label for={fieldId(`${mode}.${field.key}`)}>
													{field.label}
												</label>
												<div class="palette-controls">
													<input
														type="color"
														value={safeHex(
															theme[mode][field.key],
															DEFAULT_THEME[mode][field.key]
														)}
														aria-label={`${title} ${field.label} colour picker`}
														oninput={(event) => {
															theme[mode][field.key] =
																event.currentTarget.value;
														}}
														data-field={`${mode}.${field.key}`}
													/>
													<input
														id={fieldId(`${mode}.${field.key}`)}
														class="palette-hex"
														type="text"
														maxlength="7"
														spellcheck="false"
														autocapitalize="none"
														value={theme[mode][field.key]}
														oninput={(event) => {
															theme[mode][field.key] =
																event.currentTarget.value;
														}}
														aria-invalid={paletteInvalid(
															mode,
															field.key
														)
															? 'true'
															: undefined}
														aria-describedby={paletteDescription(
															mode,
															field.key
														)}
														data-field={`${mode}.${field.key}`}
													/>
												</div>
												{#if fieldErrors[`${mode}.${field.key}`]}
													<p
														class="a-field-error"
														id={errorId(`${mode}.${field.key}`)}
													>
														{fieldErrors[`${mode}.${field.key}`]}
													</p>
												{/if}
											</div>
										{/each}
									</div>

									<ul
										class="contrast-list"
										aria-label={`${title} contrast checks`}
									>
										{#each (mode === 'light' ? lightChecks : darkChecks) as check (check.key)}
											<li
												class="contrast-item"
												data-pass={check.pass}
												data-valid={check.valid}
											>
												<span class="contrast-name">{check.label}</span>
												<span class="contrast-ratio">
													{check.valid
														? `${check.ratio.toFixed(2)}:1`
														: 'No ratio'}
												</span>
												<span
													class="contrast-verdict"
													aria-live="polite"
													data-state={!check.valid
														? 'invalid'
														: check.pass
															? 'pass'
															: 'fail'}
												>
													{#if !check.valid}
														Check hex
													{:else if check.pass}
														Pass, min {thresholdLabel(check.min)}
													{:else}
														Fail, needs {thresholdLabel(check.min)}
													{/if}
												</span>
												{#if check.key.endsWith('surface_ink') && fieldErrors[check.key]}
													<p
														class="a-field-error contrast-error"
														id={errorId(check.key)}
													>
														{fieldErrors[check.key]}
													</p>
												{/if}
											</li>
										{/each}
									</ul>
								</fieldset>
							{/snippet}

							{@render palettePanel('light', 'Light palette')}
							{@render palettePanel('dark', 'Dark palette')}
						</section>

						<section class="form-section" aria-labelledby="sf-images-title">
							<h2 id="sf-images-title">Images</h2>
							<div class="form-grid">
								<div class="image-row span-2">
									<div class="a-field">
										<span class="a-label" id="sf-hero-image-select-label">
											Hero image, bundled
										</span>
										<Dropdown
											id="sf-hero-image-select"
											class="admin-dropdown"
											label="Bundled hero image"
											labelId="sf-hero-image-select-label"
											value={theme.hero_image}
											options={heroImageOptions}
											onchange={(value) => {
												theme.hero_image = value;
												clearFieldError('hero_image');
											}}
											invalid={Boolean(fieldErrors.hero_image)}
											describedBy={describedBy(
												'hero_image',
												hintId('hero_image')
											)}
										/>
									</div>

									<div class="a-field">
										<label for={fieldId('hero_image')}>
											Hero image path
										</label>
										<input
											id={fieldId('hero_image')}
											type="text"
											bind:value={theme.hero_image}
											autocapitalize="none"
											spellcheck="false"
											aria-invalid={invalid('hero_image')}
											aria-describedby={describedBy(
												'hero_image',
												hintId('hero_image')
											)}
											data-field="hero_image"
										/>
										{#if fieldErrors.hero_image}
											<p class="a-field-error" id={errorId('hero_image')}>
												{fieldErrors.hero_image}
											</p>
										{:else}
											<p class="a-field-hint" id={hintId('hero_image')}>
												Choose a bundled photograph or enter an /images/ path.
											</p>
										{/if}
									</div>
								</div>

								<div class="image-row span-2">
									<div class="a-field">
										<span class="a-label" id="sf-about-image-select-label">
											About image, bundled
										</span>
										<Dropdown
											id="sf-about-image-select"
											class="admin-dropdown"
											label="Bundled about image"
											labelId="sf-about-image-select-label"
											value={theme.about_image}
											options={aboutImageOptions}
											onchange={(value) => {
												theme.about_image = value;
												clearFieldError('about_image');
											}}
											invalid={Boolean(fieldErrors.about_image)}
											describedBy={describedBy(
												'about_image',
												hintId('about_image')
											)}
										/>
									</div>

									<div class="a-field">
										<label for={fieldId('about_image')}>
											About image path
										</label>
										<input
											id={fieldId('about_image')}
											type="text"
											bind:value={theme.about_image}
											autocapitalize="none"
											spellcheck="false"
											aria-invalid={invalid('about_image')}
											aria-describedby={describedBy(
												'about_image',
												hintId('about_image')
											)}
											data-field="about_image"
										/>
										{#if fieldErrors.about_image}
											<p class="a-field-error" id={errorId('about_image')}>
												{fieldErrors.about_image}
											</p>
										{:else}
											<p class="a-field-hint" id={hintId('about_image')}>
												Choose a bundled photograph or enter an /images/ path.
											</p>
										{/if}
									</div>
								</div>
							</div>
						</section>

						<div class="form-foot">
							<p class="a-field-hint">
								{#if dirty}
									Unsaved changes. Save to publish them.
								{:else}
									All changes are saved.
								{/if}
							</p>
							<button class="a-btn a-btn-primary" type="submit" disabled={busy}>
								{busy ? 'Saving…' : 'Save storefront'}
							</button>
						</div>
					</div>
				</div>
			</form>
		</div>
	{/if}
</div>

<style>
	.dirty-chip {
		display: inline-flex;
		align-items: center;
		min-height: 24px;
		padding: 2px 7px;
		border: 1px solid var(--a-line-strong);
		border-radius: var(--a-radius-xs);
		color: var(--a-ink);
		font-family: var(--a-font-mono);
		font-size: 10px;
		line-height: 1.5;
	}

	.storefront-layout {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 340px;
		grid-template-areas: 'form preview';
		gap: 24px;
		align-items: start;
	}

	.storefront-form {
		grid-area: form;
		min-width: 0;
	}

	.preview-col {
		grid-area: preview;
		position: sticky;
		top: 24px;
		min-width: 0;
	}

	.preview-panel {
		overflow: clip;
		border: 1px solid var(--a-line);
		border-radius: var(--a-radius);
		background: var(--a-surface);
	}

	.preview-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		padding: 12px 16px;
		border-bottom: 1px solid var(--a-line);
	}

	.preview-head h2 {
		font-size: 14px;
		font-weight: 500;
		letter-spacing: 0;
	}

	.preview-modes {
		display: flex;
		gap: 2px;
		padding: 2px;
		border: 1px solid var(--a-line);
		border-radius: var(--a-radius);
		background: var(--a-field-bg);
	}

	.preview-mode {
		min-height: 44px;
		padding: 0 10px;
		border: 0;
		border-radius: var(--a-radius-xs);
		background: transparent;
		color: var(--a-muted);
		font-size: 12px;
		line-height: 1;
		cursor: pointer;
	}

	.preview-mode:hover {
		background: var(--a-hover);
		color: var(--a-ink);
	}

	.preview-mode[aria-pressed='true'] {
		background: var(--a-ink);
		color: var(--a-bg);
		font-weight: 500;
	}

	.storefront-preview {
		margin: 16px;
		padding: 20px;
		border: 1px solid color-mix(in srgb, var(--pv-ink) 18%, transparent);
		border-radius: var(--pv-radius);
		background: var(--pv-bg);
		color: var(--pv-ink);
		font-family: var(--pv-font-body);
		font-size: calc(15px * var(--pv-scale));
	}

	.pv-wordmark {
		font-family: var(--pv-font-display);
		font-size: calc(20px * var(--pv-scale));
		font-weight: 600;
		line-height: 1.1;
		letter-spacing: 0.01em;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.pv-heading {
		display: -webkit-box;
		margin-top: 26px;
		overflow: hidden;
		font-family: var(--pv-font-display);
		font-size: calc(30px * var(--pv-scale));
		font-weight: 400;
		line-height: 1.05;
		letter-spacing: -0.035em;
		text-wrap: balance;
		overflow-wrap: anywhere;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 3;
		line-clamp: 3;
	}

	.pv-body {
		display: -webkit-box;
		max-width: 32ch;
		margin-top: 12px;
		overflow: hidden;
		color: var(--pv-muted);
		font-size: calc(14px * var(--pv-scale));
		line-height: 1.5;
		overflow-wrap: anywhere;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
	}

	.pv-cta {
		display: flex;
		align-items: center;
		justify-content: center;
		width: fit-content;
		max-width: 100%;
		min-height: 44px;
		margin-top: 18px;
		padding: 10px 16px;
		overflow: hidden;
		border-radius: var(--pv-radius);
		background: var(--pv-accent);
		color: var(--pv-accent-ink);
		font-size: calc(13px * var(--pv-scale));
		font-weight: 500;
		line-height: 1.2;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.pv-input {
		display: flex;
		align-items: center;
		min-height: 44px;
		margin-top: 12px;
		padding: 8px 11px;
		border: 1px solid color-mix(in srgb, var(--pv-ink) 28%, transparent);
		border-radius: var(--pv-radius);
		background: var(--pv-surface);
		color: var(--pv-ink);
		font-size: calc(13px * var(--pv-scale));
	}

	.preview-note {
		padding: 0 16px 16px;
		color: var(--a-muted);
		font-size: 12px;
		line-height: 1.45;
	}

	.form-alert {
		margin-bottom: 16px;
	}

	.form-section h2 {
		margin-bottom: 12px;
		font-size: 14px;
		font-weight: 500;
		letter-spacing: 0;
	}

	.font-fieldset + .font-fieldset {
		margin-top: 16px;
	}

	.font-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 8px;
		margin-top: 5px;
	}

	.font-card {
		display: grid;
		grid-template-columns: 18px minmax(0, 1fr);
		align-items: center;
		gap: 10px;
		min-height: 72px;
		padding: 10px 12px;
		border: 1px solid var(--a-line-strong);
		border-radius: var(--a-radius-xs);
		background: var(--a-field-bg);
		cursor: pointer;
		transition:
			background-color 180ms var(--a-ease),
			border-color 180ms var(--a-ease);
	}

	.font-card:hover {
		background: var(--a-hover);
	}

	.font-card[data-selected='true'] {
		border-color: var(--a-accent);
		background: var(--a-selected);
	}

	.font-card:has(input:focus-visible) {
		outline: 2px solid var(--a-accent);
		outline-offset: 3px;
	}

	.font-card input {
		width: 18px;
		height: 18px;
		margin: 0;
		accent-color: var(--a-accent);
		cursor: pointer;
	}

	.font-copy {
		display: grid;
		gap: 3px;
		min-width: 0;
	}

	.font-sample {
		display: block;
		overflow: hidden;
		font-size: 18px;
		line-height: 1.2;
		letter-spacing: -0.02em;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.font-name {
		color: var(--a-muted);
		font-size: 12px;
		line-height: 1.3;
	}

	.font-card[data-selected='true'] .font-name {
		color: var(--a-ink);
	}

	.type-measures {
		margin-top: 18px;
	}

	.range-row {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 64px;
		gap: 12px;
		align-items: center;
		min-height: 44px;
	}

	.range-row input[type='range'] {
		width: 100%;
		min-height: 44px;
		accent-color: var(--a-accent);
		cursor: pointer;
	}

	.range-row input[type='range']:focus-visible {
		outline: 2px solid var(--a-accent);
		outline-offset: 3px;
	}

	.range-row input[type='range'][aria-invalid='true'] {
		outline: 2px solid var(--a-danger-text);
		outline-offset: 3px;
	}

	.range-value {
		color: var(--a-ink);
		font-family: var(--a-font-mono);
		font-size: 13px;
		font-variant-numeric: tabular-nums;
		text-align: right;
		white-space: nowrap;
	}

	.palette-hint {
		max-width: 72ch;
		margin-top: -4px;
		margin-bottom: 4px;
	}

	.palette-block {
		display: grid;
		gap: 10px;
		min-width: 0;
		margin: 16px 0 0;
		padding: 16px 0 0;
		border: 0;
		border-top: 1px solid var(--a-line);
	}

	.palette-block legend {
		padding: 0;
		color: var(--a-ink);
		font-size: 13px;
		font-weight: 500;
	}

	.palette-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 12px;
	}

	.palette-field {
		display: grid;
		gap: 5px;
		min-width: 0;
	}

	.palette-field > label {
		color: var(--a-muted);
		font-size: 12px;
		line-height: 1.3;
	}

	.palette-controls {
		display: grid;
		grid-template-columns: 44px minmax(0, 1fr);
		gap: 6px;
	}

	.palette-controls input[type='color'] {
		width: 44px;
		height: 44px;
		min-height: 44px;
		padding: 4px;
		border: 1px solid var(--a-line-strong);
		border-radius: var(--a-radius-xs);
		background: var(--a-field-bg);
		cursor: pointer;
	}

	.palette-controls .palette-hex {
		width: 100%;
		min-height: 44px;
		padding: 9px 11px;
		border: 1px solid var(--a-line-strong);
		border-radius: var(--a-radius-xs);
		background: var(--a-field-bg);
		color: var(--a-ink);
		font-family: var(--a-font-mono);
		font-size: 13px;
	}

	.palette-controls .palette-hex:focus-visible {
		border-color: var(--a-accent);
		box-shadow: 0 0 0 3px color-mix(in oklab, var(--a-accent) 22%, transparent);
		outline: 2px solid var(--a-accent);
		outline-offset: 1px;
	}

	.palette-controls .palette-hex[aria-invalid='true'] {
		border-color: var(--a-danger-text);
	}

	.contrast-list {
		display: grid;
		gap: 0;
		margin: 4px 0 0;
		padding: 0;
		border-top: 1px solid var(--a-line);
		list-style: none;
	}

	.contrast-item {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 4px 10px;
		padding: 8px 0;
		border-bottom: 1px solid var(--a-line);
		font-size: 12px;
		line-height: 1.4;
	}

	.contrast-item:last-child {
		border-bottom: 0;
		padding-bottom: 0;
	}

	.contrast-name {
		flex: 1 1 130px;
		min-width: 0;
		color: var(--a-muted);
		overflow-wrap: anywhere;
	}

	.contrast-ratio {
		color: var(--a-ink);
		font-family: var(--a-font-mono);
		font-size: 12px;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}

	.contrast-verdict {
		padding: 1px 5px;
		border: 1px solid currentColor;
		border-radius: var(--a-radius-xs);
		color: var(--a-ink);
		font-size: 11px;
		line-height: 1.4;
		white-space: nowrap;
	}

	.contrast-verdict[data-state='fail'] {
		color: var(--a-danger-text);
	}

	.contrast-verdict[data-state='invalid'] {
		color: var(--a-muted);
	}

	.contrast-error {
		flex: 1 1 100%;
	}

	.image-row {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
		gap: 12px;
		align-items: start;
	}

	.form-foot {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px 20px;
		margin-top: 22px;
		padding-top: 18px;
		border-top: 1px solid var(--a-line-strong);
	}

	.form-foot .a-btn {
		margin-left: auto;
	}

	@media (max-width: 1080px) {
		.storefront-layout {
			grid-template-columns: minmax(0, 1fr);
			grid-template-areas:
				'preview'
				'form';
		}

		.preview-col {
			position: static;
			max-width: 440px;
		}
	}

	@media (max-width: 720px) {
		.font-grid,
		.image-row {
			grid-template-columns: minmax(0, 1fr);
		}

		.form-foot {
			align-items: stretch;
		}

		.form-foot .a-btn {
			width: 100%;
			margin-left: 0;
		}
	}

	@media (max-width: 560px) {
		.preview-col {
			max-width: none;
		}

		.palette-grid {
			grid-template-columns: minmax(0, 1fr);
		}

		.range-row {
			grid-template-columns: minmax(0, 1fr) 58px;
		}

		.a-page-actions {
			flex-direction: column;
			align-items: stretch;
		}

		.contrast-item {
			display: grid;
			grid-template-columns: minmax(0, 1fr) auto;
		}

		.contrast-name {
			grid-column: 1 / -1;
		}

		.contrast-error {
			grid-column: 1 / -1;
		}

		.storefront-preview {
			padding: 16px;
		}
	}
</style>
