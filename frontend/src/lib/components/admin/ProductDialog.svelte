<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { ApiError } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import { parseList, SLUG_PATTERN } from '#lib/admin/format';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import type { ProductPayload } from '#lib/admin/types';
	import type { AdminProduct, Category, OptionField, PluginManifest } from '#lib/types';

	let {
		product = null,
		categories,
		plugins = [],
		onclose,
		onsaved
	}: {
		product?: AdminProduct | null;
		categories: Category[];
		plugins?: PluginManifest[];
		onclose: () => void;
		onsaved: (product: AdminProduct, mode: 'create' | 'edit') => void;
	} = $props();

	type FormState = {
		name: string;
		slug: string;
		subtitle: string;
		description: string;
		price: string | number;
		salePrice: string | number;
		saleStartsAt: string;
		saleEndsAt: string;
		categorySlug: string;
		image: string;
		gallery: string[];
		stock: string | number;
		featured: boolean;
		active: boolean;
		pluginId: string;
		attributes: Record<string, string>;
	};

	function priceValue(cents: number | undefined): string {
		if (typeof cents !== 'number') return '';
		const dinars = cents / 100;
		return Number.isInteger(dinars) ? String(dinars) : dinars.toFixed(2);
	}

	function localDateTime(value: string | undefined): string {
		if (!value) return '';
		const date = new Date(value);
		if (Number.isNaN(date.getTime())) return '';
		const pad = (part: number) => String(part).padStart(2, '0');
		return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
	}

	function initialForm(source: AdminProduct | null, categoryList: Category[]): FormState {
		const attributes: Record<string, string> = {};
		for (const [key, values] of Object.entries(source?.attributes ?? {})) {
			attributes[key] = values.join(', ');
		}
		const pluginId =
			source?.plugin_id ??
			plugins.find((plugin) => plugin.product_type)?.id ??
			'core';
		const pluginCategories = categoryList.filter((category) => category.plugin_id === pluginId);
		const categorySlug =
			source?.category?.slug &&
			pluginCategories.some((category) => category.slug === source.category.slug)
				? source.category.slug
				: (pluginCategories[0]?.slug ?? '');
		return {
			name: source?.name ?? '',
			slug: source?.slug ?? '',
			subtitle: source?.subtitle ?? '',
			description: source?.description ?? '',
			price: priceValue(source?.price_cents),
			salePrice: priceValue(source?.sale_price_cents ?? undefined),
			saleStartsAt: localDateTime(source?.sale_starts_at),
			saleEndsAt: localDateTime(source?.sale_ends_at),
			categorySlug,
			image: source?.image ?? '',
			gallery: source ? [...source.gallery] : [],
			stock: source ? String(source.stock) : '0',
			featured: source?.featured ?? false,
			active: source?.active ?? true,
			pluginId,
			attributes
		};
	}

	let form = $state<FormState>(
		// The dialog is mounted per product. Later prop changes must not overwrite edits.
		untrack(() => initialForm(product, categories))
	);
	const initialFormSnapshot = untrack(() => JSON.stringify(initialForm(product, categories)));
	const dirty = $derived(JSON.stringify(form) !== initialFormSnapshot);

	const selectedPlugin = $derived(plugins.find((plugin) => plugin.id === form.pluginId) ?? null);
	const pluginCategories = $derived(
		categories.filter((category) => category.plugin_id === form.pluginId)
	);
	const attributeFields = $derived(
		selectedPlugin?.product_type?.fields ??
			Object.keys(form.attributes).map((key) => ({
				key,
				label: key,
				type: 'select' as const,
				required: false,
				suggestions: [] as string[]
			}))
	);
	const typeOptions = $derived.by(() => {
		const options = plugins
			.filter((plugin) => plugin.product_type)
			.map((plugin) => ({ value: plugin.id, label: plugin.product_type?.label ?? plugin.name }));
		if (form.pluginId && !options.some((option) => option.value === form.pluginId)) {
			options.push({ value: form.pluginId, label: product?.plugin_name ?? form.pluginId });
		}
		return options;
	});

	function changePlugin(id: string): void {
		form.pluginId = id;
		const plugin = plugins.find((item) => item.id === id);
		const next: Record<string, string> = {};
		for (const field of plugin?.product_type?.fields ?? []) {
			next[field.key] = form.attributes[field.key] ?? '';
		}
		form.attributes = next;
		const choices = categories.filter((category) => category.plugin_id === id);
		if (!choices.some((category) => category.slug === form.categorySlug)) {
			form.categorySlug = choices[0]?.slug ?? '';
		}
	}

	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let uploadError = $state('');
	let uploadingMain = $state(false);
	let uploadingGallery = $state(false);
	let busy = $state(false);
	let dialogEl = $state<HTMLDialogElement | null>(null);
	let formEl = $state<HTMLFormElement | null>(null);

	$effect(() => {
		if (dialogEl && !dialogEl.open) dialogEl.showModal();
	});

	function validateImageFile(file: File): string {
		if (!['image/jpeg', 'image/png', 'image/webp'].includes(file.type)) {
			return 'Use a JPEG, PNG or WebP image.';
		}
		if (file.size > 5 << 20) return 'Use an image of 5 MB or less.';
		return '';
	}

	async function uploadFile(file: File): Promise<string> {
		const problem = validateImageFile(file);
		if (problem) throw new Error(problem);
		const data = await adminApi.uploadProductImage(file);
		return data.image.url;
	}

	async function uploadMain(event: Event): Promise<void> {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		input.value = '';
		if (!file) return;
		uploadError = '';
		uploadingMain = true;
		try {
			form.image = await uploadFile(file);
			clearFieldError('image');
		} catch (caught) {
			uploadError = errorMessage(caught);
		} finally {
			uploadingMain = false;
		}
	}

	async function uploadGallery(event: Event): Promise<void> {
		const input = event.currentTarget as HTMLInputElement;
		const files = [...(input.files ?? [])];
		input.value = '';
		if (files.length === 0) return;
		uploadError = '';
		uploadingGallery = true;
		try {
			for (const file of files) {
				if (form.gallery.length >= 12) {
					throw new Error('Use at most 12 gallery images.');
				}
				const url = await uploadFile(file);
				if (!form.gallery.includes(url)) form.gallery = [...form.gallery, url];
			}
		} catch (caught) {
			uploadError = errorMessage(caught);
		} finally {
			uploadingGallery = false;
		}
	}

	function removeGallery(path: string): void {
		form.gallery = form.gallery.filter((item) => item !== path);
	}

	function clearFieldError(key: string): void {
		if (!fieldErrors[key]) return;
		const next = { ...fieldErrors };
		delete next[key];
		fieldErrors = next;
	}

	function validate(): Record<string, string> {
		const errors: Record<string, string> = {};
		const name = form.name.trim();
		const slug = form.slug.trim();
		const price = Number(form.price);
		const stock = Number(form.stock);

		if (name.length < 2) errors.name = 'Enter at least 2 characters.';
		else if (name.length > 120) errors.name = 'Keep the name under 120 characters.';

		if (!slug) errors.slug = 'Enter a slug.';
		else if (slug.length > 120) errors.slug = 'Keep the slug under 120 characters.';
		else if (!SLUG_PATTERN.test(slug)) errors.slug = 'Use lowercase letters, numbers and hyphens.';

		if (form.subtitle.trim().length > 160) errors.subtitle = 'Keep the subtitle under 160 characters.';
		if (form.description.trim().length > 4000)
			errors.description = 'Keep the description under 4000 characters.';

		if (!Number.isFinite(price) || price <= 0) errors.price = 'Enter a price above 0.';
		else if (Math.round(price * 100) >= 100000000) errors.price = 'Enter a price below 1,000,000 DZD.';

		const saleValue = form.salePrice == null ? '' : String(form.salePrice).trim();
		const salePrice = Number(form.salePrice);
		const saleCents = Math.round(salePrice * 100);
		if (saleValue !== '') {
			if (!Number.isFinite(salePrice) || salePrice <= 0) {
				errors.salePrice = 'Enter a sale price above 0.';
			} else if (Number.isFinite(price) && saleCents >= Math.round(price * 100)) {
				errors.salePrice = 'Enter a sale price below the regular price.';
			}
			if (!form.saleStartsAt) errors.saleStartsAt = 'Choose when the sale starts.';
			if (!form.saleEndsAt) errors.saleEndsAt = 'Choose when the sale ends.';
			if (
				form.saleStartsAt &&
				form.saleEndsAt &&
				new Date(form.saleEndsAt).getTime() <= new Date(form.saleStartsAt).getTime()
			) {
				errors.saleEndsAt = 'The sale must end after it starts.';
			}
		} else if (form.saleStartsAt || form.saleEndsAt) {
			errors.salePrice = 'Add a sale price or clear the sale window.';
		}

		if (!form.categorySlug) errors.categorySlug = 'Choose a category.';
		else if (!pluginCategories.some((category) => category.slug === form.categorySlug)) {
			errors.categorySlug = 'Choose a category that belongs to this product type.';
		}
		if (!form.image) errors.image = 'Upload a main image.';

		if (!Number.isInteger(stock) || stock < 0) errors.stock = 'Enter a stock count of 0 or more.';
		else if (stock > 100000) errors.stock = 'Enter a stock count below 100,001.';

		for (const field of attributeFields) {
			const key = `attributes.${field.key}`;
			const raw = (form.attributes[field.key] ?? '').trim();
			if (field.type === 'text' || field.type === 'textarea') {
				const max = field.type === 'textarea' ? 2000 : 120;
				if (field.required && !raw) {
					errors[key] = `Enter ${field.label.toLowerCase()}.`;
				} else if (raw.length > max) {
					errors[key] = `Keep this under ${max + 1} characters.`;
				}
				continue;
			}
			const values = parseList(raw);
			if (field.required && values.length === 0) {
				errors[key] = `Add at least one ${field.label.toLowerCase()}.`;
			} else if (values.length > 12) {
				errors[key] = 'Use at most 12 values.';
			} else if (values.some((value) => value.length > 80)) {
				errors[key] = 'Keep each value under 80 characters.';
			}
		}

		return errors;
	}

	function buildPayload(): ProductPayload {
		const attributes: Record<string, string[]> = {};
		for (const field of attributeFields) {
			const raw = (form.attributes[field.key] ?? '').trim();
			attributes[field.key] =
				field.type === 'text' || field.type === 'textarea'
					? raw
						? [raw]
						: []
					: parseList(raw);
		}
		const saleValue = form.salePrice == null ? '' : String(form.salePrice).trim();
		const hasSale = saleValue !== '' && Number(form.salePrice) > 0;
		return {
			name: form.name.trim(),
			slug: form.slug.trim(),
			subtitle: form.subtitle.trim(),
			description: form.description.trim(),
			price_cents: Math.round(Number(form.price) * 100),
			sale_price_cents: hasSale ? Math.round(Number(form.salePrice) * 100) : 0,
			sale_starts_at: hasSale && form.saleStartsAt ? new Date(form.saleStartsAt).toISOString() : '',
			sale_ends_at: hasSale && form.saleEndsAt ? new Date(form.saleEndsAt).toISOString() : '',
			size: '',
			category_slug: form.categorySlug,
			concerns: [],
			ingredients: [],
			how_to: '',
			image: form.image,
			gallery: [...form.gallery],
			stock: Number(form.stock),
			featured: form.featured,
			active: form.active,
			plugin_id: form.pluginId,
			attributes
		};
	}

	async function focusFirstError() {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"], [data-invalid="true"]')?.focus();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || uploadingMain || uploadingGallery) return;

		formError = '';
		const errors = validate();
		fieldErrors = errors;

		if (Object.keys(errors).length > 0) {
			formError = 'Check the marked fields.';
			await focusFirstError();
			return;
		}

		busy = true;
		try {
			const payload = buildPayload();
			const data = product
				? await adminApi.updateProduct(product.id, payload)
				: await adminApi.createProduct(payload);
			onsaved(data.product, product ? 'edit' : 'create');
		} catch (caught) {
			if (caught instanceof ApiError) {
				formError = caught.message;
				const mapped: Record<string, string> = {};
				for (const [key, value] of Object.entries(caught.fields)) {
					const field =
						key === 'price_cents'
							? 'price'
							: key === 'sale_price_cents'
								? 'salePrice'
								: key === 'sale_starts_at'
									? 'saleStartsAt'
									: key === 'sale_ends_at'
										? 'saleEndsAt'
										: key === 'category_slug'
											? 'categorySlug'
											: key === 'how_to'
												? 'howTo'
												: key;
					mapped[field] = value;
				}
				fieldErrors = { ...fieldErrors, ...mapped };
				await focusFirstError();
			} else {
				formError = errorMessage(caught);
			}
		} finally {
			busy = false;
		}
	}

	function close() {
		if (busy || uploadingMain || uploadingGallery) return;
		if (dirty && !window.confirm('Discard the unsaved product changes?')) return;
		dialogEl?.close();
	}
</script>

<dialog
	bind:this={dialogEl}
	class="a-dialog a-dialog--wide"
	aria-labelledby="product-dialog-title"
	onclose={onclose}
	oncancel={(event) => {
		if (
			busy ||
			uploadingMain ||
			uploadingGallery ||
			(dirty && !window.confirm('Discard the unsaved product changes?'))
		) {
			event.preventDefault();
		}
	}}
>
	<form bind:this={formEl} class="a-dialog-form" onsubmit={submit} novalidate>
		<header class="a-dialog-head">
			<div class="a-dialog-title">
				<p class="a-dialog-kicker">Product</p>
				<h2 id="product-dialog-title">{product ? 'Edit product' : 'New product'}</h2>
			</div>
			<button
				class="a-dialog-close"
				type="button"
				aria-label="Close product form"
				onclick={close}
				disabled={busy}
			>
				Close
			</button>
		</header>

		<div class="a-dialog-body">
			{#if formError}
				<p class="a-form-error" role="alert">{formError}</p>
			{/if}

			<section class="form-section">
				<h3>Details</h3>
				<div class="form-grid">
					<div class="a-field">
						<label for="product-name">Name</label>
						<input
							id="product-name"
							type="text"
							bind:value={form.name}
							required
							aria-invalid={fieldErrors.name ? 'true' : undefined}
							aria-describedby={fieldErrors.name ? 'product-name-error' : undefined}
						/>
						{#if fieldErrors.name}
							<p class="a-field-error" id="product-name-error">{fieldErrors.name}</p>
						{/if}
					</div>

					<div class="a-field">
						<label for="product-slug">Slug</label>
						<input
							id="product-slug"
							type="text"
							bind:value={form.slug}
							required
							autocapitalize="none"
							spellcheck="false"
							aria-invalid={fieldErrors.slug ? 'true' : undefined}
							aria-describedby={fieldErrors.slug ? 'product-slug-error' : 'product-slug-hint'}
						/>
						{#if fieldErrors.slug}
							<p class="a-field-error" id="product-slug-error">{fieldErrors.slug}</p>
						{:else}
							<p class="a-field-hint" id="product-slug-hint">
								Lowercase letters, numbers and hyphens.
							</p>
						{/if}
					</div>

					<div class="a-field span-2">
						<label for="product-subtitle">Subtitle</label>
						<input
							id="product-subtitle"
							type="text"
							bind:value={form.subtitle}
							aria-invalid={fieldErrors.subtitle ? 'true' : undefined}
							aria-describedby={fieldErrors.subtitle ? 'product-subtitle-error' : undefined}
						/>
						{#if fieldErrors.subtitle}
							<p class="a-field-error" id="product-subtitle-error">{fieldErrors.subtitle}</p>
						{/if}
					</div>

					<div class="a-field span-2">
						<label for="product-description">Description</label>
						<textarea
							id="product-description"
							bind:value={form.description}
							aria-invalid={fieldErrors.description ? 'true' : undefined}
							aria-describedby={fieldErrors.description ? 'product-description-error' : undefined}
						></textarea>
						{#if fieldErrors.description}
							<p class="a-field-error" id="product-description-error">{fieldErrors.description}</p>
						{/if}
					</div>

					<div class="a-field">
						<span class="a-label" id="product-category-label">Category</span>
						<Dropdown
							id="product-category"
							class="admin-dropdown"
							label="Category"
							labelId="product-category-label"
							value={form.categorySlug}
							options={pluginCategories.map((category) => ({
								value: category.slug,
								label: category.name
							}))}
							onchange={(value) => (form.categorySlug = value)}
							invalid={!!fieldErrors.categorySlug}
							describedBy={fieldErrors.categorySlug ? 'product-category-error' : undefined}
						/>
						{#if fieldErrors.categorySlug}
							<p class="a-field-error" id="product-category-error">{fieldErrors.categorySlug}</p>
						{:else if pluginCategories.length === 0}
							<p class="a-field-error">This plugin has no categories. Add one in the plugin manifest.</p>
						{/if}
					</div>
				</div>
			</section>

			<section class="form-section">
				<h3>Type and options</h3>
				<div class="form-grid">
					<div class="a-field">
						<span class="a-label" id="product-type-label">Product type</span>
						<Dropdown
							id="product-type"
							class="admin-dropdown"
							label="Product type"
							labelId="product-type-label"
							value={form.pluginId}
							options={typeOptions}
							onchange={changePlugin}
							invalid={!!fieldErrors.plugin_id}
							describedBy={fieldErrors.plugin_id ? 'product-type-error' : 'product-type-hint'}
						/>
						{#if fieldErrors.plugin_id}
							<p class="a-field-error" id="product-type-error">{fieldErrors.plugin_id}</p>
						{:else}
							<p class="a-field-hint" id="product-type-hint">
								The plugin decides which fields and customer options this product uses.
							</p>
						{/if}
					</div>

					{#if selectedPlugin?.product_type?.summary}
						<p class="a-field-hint type-summary">{selectedPlugin.product_type.summary}</p>
					{/if}

					{#each attributeFields as field (field.key)}
						<div class="a-field span-2">
							<label for={`attr-${field.key}`}>{field.label}</label>
							{#if field.type === 'textarea'}
								<textarea
									id={`attr-${field.key}`}
									rows="4"
									value={form.attributes[field.key] ?? ''}
									oninput={(event) =>
										(form.attributes[field.key] = event.currentTarget.value)}
									aria-invalid={fieldErrors[`attributes.${field.key}`] ? 'true' : undefined}
									aria-describedby={fieldErrors[`attributes.${field.key}`]
										? `attr-${field.key}-error`
										: `attr-${field.key}-hint`}
								></textarea>
							{:else}
								<input
									id={`attr-${field.key}`}
									type="text"
									value={form.attributes[field.key] ?? ''}
									oninput={(event) =>
										(form.attributes[field.key] = event.currentTarget.value)}
									aria-invalid={fieldErrors[`attributes.${field.key}`] ? 'true' : undefined}
									aria-describedby={fieldErrors[`attributes.${field.key}`]
										? `attr-${field.key}-error`
										: `attr-${field.key}-hint`}
								/>
							{/if}
							{#if fieldErrors[`attributes.${field.key}`]}
								<p class="a-field-error" id={`attr-${field.key}-error`}>
									{fieldErrors[`attributes.${field.key}`]}
								</p>
							{:else}
								<p class="a-field-hint" id={`attr-${field.key}-hint`}>
									{#if field.type === 'text' || field.type === 'textarea'}
										{field.help ?? ''}
									{:else}
										Separate with commas.
										{#if field.suggestions?.length}
											Common values: {field.suggestions.join(', ')}.
										{/if}
									{/if}
								</p>
							{/if}
						</div>
					{/each}
				</div>
			</section>

			<section class="form-section">
				<h3>Price and stock</h3>
				<div class="form-grid">
					<div class="a-field">
						<label for="product-price">Price</label>
						<div class="a-input-suffix">
							<input
								id="product-price"
								type="number"
								min="0.01"
								step="0.01"
								inputmode="decimal"
								bind:value={form.price}
								required
								aria-invalid={fieldErrors.price ? 'true' : undefined}
								aria-describedby={fieldErrors.price ? 'product-price-error' : undefined}
							/>
							<span aria-hidden="true">DZD</span>
						</div>
						{#if fieldErrors.price}
							<p class="a-field-error" id="product-price-error">{fieldErrors.price}</p>
						{/if}
					</div>

					<div class="a-field">
						<label for="product-stock">Stock</label>
						<input
							id="product-stock"
							type="number"
							min="0"
							step="1"
							inputmode="numeric"
							bind:value={form.stock}
							required
							aria-invalid={fieldErrors.stock ? 'true' : undefined}
							aria-describedby={fieldErrors.stock ? 'product-stock-error' : undefined}
						/>
						{#if fieldErrors.stock}
							<p class="a-field-error" id="product-stock-error">{fieldErrors.stock}</p>
						{/if}
					</div>

					<div class="a-check-row span-2">
						<label class="a-check">
							<input type="checkbox" bind:checked={form.featured} />
							<span>Featured on the storefront</span>
						</label>
						<label class="a-check">
							<input type="checkbox" bind:checked={form.active} />
							<span>Active in the storefront</span>
						</label>
					</div>
				</div>
			</section>

			<section class="form-section">
				<h3>Limited-time sale</h3>
				<div class="form-grid">
					<div class="a-field">
						<label for="product-sale-price">Sale price</label>
						<div class="a-input-suffix">
							<input
								id="product-sale-price"
								type="number"
								min="0.01"
								step="0.01"
								inputmode="decimal"
								bind:value={form.salePrice}
								aria-invalid={fieldErrors.salePrice ? 'true' : undefined}
								aria-describedby={fieldErrors.salePrice
									? 'product-sale-price-error'
									: 'product-sale-price-hint'}
							/>
							<span aria-hidden="true">DZD</span>
						</div>
						{#if fieldErrors.salePrice}
							<p class="a-field-error" id="product-sale-price-error">
								{fieldErrors.salePrice}
							</p>
						{:else}
							<p class="a-field-hint" id="product-sale-price-hint">
								Leave it empty for no sale. It must be below the regular price.
							</p>
						{/if}
					</div>

					<div class="a-field">
						<label for="product-sale-starts">Sale starts</label>
						<input
							id="product-sale-starts"
							type="datetime-local"
							bind:value={form.saleStartsAt}
							aria-invalid={fieldErrors.saleStartsAt ? 'true' : undefined}
							aria-describedby={fieldErrors.saleStartsAt
								? 'product-sale-starts-error'
								: undefined}
						/>
						{#if fieldErrors.saleStartsAt}
							<p class="a-field-error" id="product-sale-starts-error">
								{fieldErrors.saleStartsAt}
							</p>
						{/if}
					</div>

					<div class="a-field">
						<label for="product-sale-ends">Sale ends</label>
						<input
							id="product-sale-ends"
							type="datetime-local"
							bind:value={form.saleEndsAt}
							aria-invalid={fieldErrors.saleEndsAt ? 'true' : undefined}
							aria-describedby={fieldErrors.saleEndsAt
								? 'product-sale-ends-error'
								: undefined}
						/>
						{#if fieldErrors.saleEndsAt}
							<p class="a-field-error" id="product-sale-ends-error">
								{fieldErrors.saleEndsAt}
							</p>
						{/if}
					</div>
				</div>
			</section>

			<section class="form-section">
				<h3>Images</h3>
				<div class="form-grid">
					<div class="a-field span-2">
						<span class="a-label">Main image</span>
						<div class="upload-row">
							{#if form.image}
								<img
									class="image-preview"
									src={form.image}
									alt="Main product preview"
									width="56"
									height="70"
									loading="lazy"
								/>
							{:else}
								<div class="image-placeholder" aria-hidden="true">No image</div>
							{/if}
							<div class="upload-copy">
								<label class="a-btn a-btn-secondary upload-button">
									{uploadingMain ? 'Uploading…' : 'Upload main image'}
									<input
										id="product-image-input"
										type="file"
										accept="image/jpeg,image/png,image/webp"
										onchange={uploadMain}
										disabled={uploadingMain || busy}
										data-invalid={fieldErrors.image ? 'true' : undefined}
										aria-invalid={fieldErrors.image ? 'true' : undefined}
										aria-describedby={fieldErrors.image
											? 'product-image-error'
											: 'product-image-hint'}
									/>
								</label>
								<p class="a-field-hint" id="product-image-hint">
									JPEG, PNG or WebP, 5 MB or less.
								</p>
							</div>
						</div>
						{#if fieldErrors.image}
							<p class="a-field-error" id="product-image-error" role="alert">
								{fieldErrors.image}
							</p>
						{/if}
					</div>

					<div class="a-field span-2">
						<span class="a-label">Gallery (optional)</span>
						<div class="upload-copy">
							<label class="a-btn a-btn-secondary upload-button">
								{uploadingGallery ? 'Uploading…' : 'Upload gallery images'}
								<input
									id="product-gallery-input"
									type="file"
									accept="image/jpeg,image/png,image/webp"
									multiple
									onchange={uploadGallery}
									disabled={uploadingGallery || busy}
									aria-describedby={
										fieldErrors.gallery ? 'product-gallery-error' : 'product-gallery-hint'
									}
									aria-invalid={fieldErrors.gallery ? 'true' : undefined}
									data-invalid={fieldErrors.gallery ? 'true' : undefined}
								/>
							</label>
							<p class="a-field-hint" id="product-gallery-hint">
								Add any extra views. The main image can be included.
							</p>
						</div>
						{#if fieldErrors.gallery}
							<p class="a-field-error" id="product-gallery-error" role="alert">
								{fieldErrors.gallery}
							</p>
						{/if}
						{#if form.gallery.length > 0}
							<ul class="gallery-grid uploaded-gallery">
								{#each form.gallery as path (path)}
									<li>
										<img
											class="image-preview"
											src={path}
											alt="Gallery preview"
											width="56"
											height="70"
											loading="lazy"
										/>
										<button
											class="a-btn a-btn-quiet a-btn-table"
											type="button"
											onclick={() => removeGallery(path)}
											aria-label="Remove gallery image"
										>
											Remove
										</button>
									</li>
								{/each}
							</ul>
						{/if}
					</div>

					{#if uploadError}
						<p class="a-form-error span-2" role="alert">{uploadError}</p>
					{/if}
				</div>
			</section>
		</div>

		<footer class="a-dialog-foot">
			<button
				class="a-btn a-btn-secondary"
				type="button"
				onclick={close}
				disabled={busy || uploadingMain || uploadingGallery}
			>
				Cancel
			</button>
			<button
				class="a-btn a-btn-primary"
				type="submit"
				disabled={busy || uploadingMain || uploadingGallery}
			>
				{busy ? 'Saving…' : product ? 'Save product' : 'Create product'}
			</button>
		</footer>
	</form>
</dialog>

<style>
	.upload-row {
		display: flex;
		align-items: center;
		gap: 14px;
	}

	.upload-copy {
		display: grid;
		gap: 6px;
	}

	.upload-button {
		position: relative;
		overflow: hidden;
		min-height: 44px;
	}

	.upload-button input {
		position: absolute;
		inset: 0;
		opacity: 0;
		cursor: pointer;
	}

	.upload-button:focus-within {
		outline: 2px solid var(--a-accent);
		outline-offset: 2px;
	}

	.image-placeholder {
		display: grid;
		place-items: center;
		width: 56px;
		height: 70px;
		border: 1px dashed var(--a-line-strong);
		border-radius: var(--a-radius);
		color: var(--a-muted);
		font-size: 10px;
	}

	.uploaded-gallery {
		margin: 10px 0 0;
		padding: 0;
		list-style: none;
	}

	.uploaded-gallery li {
		display: grid;
		justify-items: start;
		gap: 4px;
	}
</style>
