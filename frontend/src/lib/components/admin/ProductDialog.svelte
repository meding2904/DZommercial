<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { ApiError } from '#lib/api';
	import { adminApi, errorMessage } from '#lib/admin/api';
	import { parseList, SLUG_PATTERN } from '#lib/admin/format';
	import { imageLabel, PRODUCT_IMAGES } from '#lib/admin/images';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import type { ProductPayload } from '#lib/admin/types';
	import type { AdminProduct, Category } from '#lib/types';

	let {
		product = null,
		categories,
		onclose,
		onsaved
	}: {
		product?: AdminProduct | null;
		categories: Category[];
		onclose: () => void;
		onsaved: (product: AdminProduct, mode: 'create' | 'edit') => void;
	} = $props();

	type FormState = {
		name: string;
		slug: string;
		subtitle: string;
		description: string;
		price: string | number;
		size: string;
		categorySlug: string;
		concerns: string;
		ingredients: string;
		howTo: string;
		image: string;
		gallery: string[];
		stock: string | number;
		featured: boolean;
		active: boolean;
	};

	function priceValue(cents: number | undefined): string {
		if (typeof cents !== 'number') return '';
		const dinars = cents / 100;
		return Number.isInteger(dinars) ? String(dinars) : dinars.toFixed(2);
	}

	function initialForm(source: AdminProduct | null, categoryList: Category[]): FormState {
		return {
			name: source?.name ?? '',
			slug: source?.slug ?? '',
			subtitle: source?.subtitle ?? '',
			description: source?.description ?? '',
			price: priceValue(source?.price_cents),
			size: source?.size ?? '',
			categorySlug: source?.category.slug ?? categoryList[0]?.slug ?? '',
			concerns: source?.concerns.join(', ') ?? '',
			ingredients: source?.ingredients.join(', ') ?? '',
			howTo: source?.how_to ?? '',
			image: source?.image ?? '',
			gallery: source ? [...source.gallery] : [],
			stock: source ? String(source.stock) : '0',
			featured: source?.featured ?? false,
			active: source?.active ?? true
		};
	}

	let form = $state<FormState>(
		// The dialog is mounted per product. Later prop changes must not overwrite edits.
		untrack(() => initialForm(product, categories))
	);

	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let busy = $state(false);
	let dialogEl = $state<HTMLDialogElement | null>(null);
	let formEl = $state<HTMLFormElement | null>(null);

	const imageOptions = $derived.by(() => {
		const options = [...PRODUCT_IMAGES];
		const add = (path: string) => {
			if (path && !options.some((option) => option.path === path)) {
				options.push({ path, label: imageLabel(path) });
			}
		};
		add(form.image);
		for (const path of form.gallery) add(path);
		return options;
	});

	$effect(() => {
		if (dialogEl && !dialogEl.open) dialogEl.showModal();
	});

	function toggleGallery(path: string, checked: boolean) {
		form.gallery = checked ? [...form.gallery, path] : form.gallery.filter((item) => item !== path);
	}

	function validate(): Record<string, string> {
		const errors: Record<string, string> = {};
		const name = form.name.trim();
		const slug = form.slug.trim();
		const price = Number(form.price);
		const stock = Number(form.stock);
		const concerns = parseList(form.concerns);
		const ingredients = parseList(form.ingredients);

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

		if (!form.size.trim()) errors.size = 'Enter the size.';
		else if (form.size.trim().length > 60) errors.size = 'Keep the size under 60 characters.';

		if (!form.categorySlug) errors.categorySlug = 'Choose a category.';
		if (!form.image) errors.image = 'Choose an image.';

		if (!Number.isInteger(stock) || stock < 0) errors.stock = 'Enter a stock count of 0 or more.';
		else if (stock > 100000) errors.stock = 'Enter a stock count below 100,001.';

		if (concerns.length > 12) errors.concerns = 'Use at most 12 concerns.';
		else if (concerns.some((item) => item.length > 80))
			errors.concerns = 'Keep each concern under 80 characters.';

		if (ingredients.length > 12) errors.ingredients = 'Use at most 12 ingredients.';
		else if (ingredients.some((item) => item.length > 120))
			errors.ingredients = 'Keep each ingredient under 120 characters.';

		if (form.howTo.trim().length > 1000)
			errors.howTo = 'Keep the instructions under 1000 characters.';

		return errors;
	}

	function buildPayload(): ProductPayload {
		return {
			name: form.name.trim(),
			slug: form.slug.trim(),
			subtitle: form.subtitle.trim(),
			description: form.description.trim(),
			price_cents: Math.round(Number(form.price) * 100),
			size: form.size.trim(),
			category_slug: form.categorySlug,
			concerns: parseList(form.concerns),
			ingredients: parseList(form.ingredients),
			how_to: form.howTo.trim(),
			image: form.image,
			gallery: [...form.gallery],
			stock: Number(form.stock),
			featured: form.featured,
			active: form.active
		};
	}

	async function focusFirstError() {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;

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
		if (!busy) dialogEl?.close();
	}
</script>

<dialog
	bind:this={dialogEl}
	class="a-dialog a-dialog--wide"
	aria-labelledby="product-dialog-title"
	onclose={onclose}
	oncancel={(event) => {
		if (busy) event.preventDefault();
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
						<label for="product-size">Size</label>
						<input
							id="product-size"
							type="text"
							bind:value={form.size}
							required
							aria-invalid={fieldErrors.size ? 'true' : undefined}
							aria-describedby={fieldErrors.size ? 'product-size-error' : undefined}
						/>
						{#if fieldErrors.size}
							<p class="a-field-error" id="product-size-error">{fieldErrors.size}</p>
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
							options={categories.map((category) => ({ value: category.slug, label: category.name }))}
							onchange={(value) => (form.categorySlug = value)}
							invalid={!!fieldErrors.categorySlug}
							describedBy={fieldErrors.categorySlug ? 'product-category-error' : undefined}
						/>
						{#if fieldErrors.categorySlug}
							<p class="a-field-error" id="product-category-error">{fieldErrors.categorySlug}</p>
						{:else if categories.length === 0}
							<p class="a-field-error">No categories are available. Reload the page.</p>
						{/if}
					</div>
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
				<h3>Story</h3>
				<div class="form-grid">
					<div class="a-field span-2">
						<label for="product-concerns">Concerns</label>
						<input
							id="product-concerns"
							type="text"
							bind:value={form.concerns}
							aria-invalid={fieldErrors.concerns ? 'true' : undefined}
							aria-describedby={fieldErrors.concerns ? 'product-concerns-error' : 'product-concerns-hint'}
						/>
						{#if fieldErrors.concerns}
							<p class="a-field-error" id="product-concerns-error">{fieldErrors.concerns}</p>
						{:else}
							<p class="a-field-hint" id="product-concerns-hint">
								Separate with commas. For example, dry, sensitive.
							</p>
						{/if}
					</div>

					<div class="a-field span-2">
						<label for="product-ingredients">Ingredients</label>
						<input
							id="product-ingredients"
							type="text"
							bind:value={form.ingredients}
							aria-invalid={fieldErrors.ingredients ? 'true' : undefined}
							aria-describedby={fieldErrors.ingredients ? 'product-ingredients-error' : 'product-ingredients-hint'}
						/>
						{#if fieldErrors.ingredients}
							<p class="a-field-error" id="product-ingredients-error">{fieldErrors.ingredients}</p>
						{:else}
							<p class="a-field-hint" id="product-ingredients-hint">Separate with commas.</p>
						{/if}
					</div>

					<div class="a-field span-2">
						<label for="product-how-to">How to use</label>
						<textarea
							id="product-how-to"
							bind:value={form.howTo}
							aria-invalid={fieldErrors.howTo ? 'true' : undefined}
							aria-describedby={fieldErrors.howTo ? 'product-how-to-error' : undefined}
						></textarea>
						{#if fieldErrors.howTo}
							<p class="a-field-error" id="product-how-to-error">{fieldErrors.howTo}</p>
						{/if}
					</div>
				</div>
			</section>

			<section class="form-section">
				<h3>Image</h3>
				<div class="form-grid">
					<div class="image-select-row span-2">
						<div class="a-field">
							<span class="a-label" id="product-image-label">Main image</span>
							<Dropdown
								id="product-image"
								class="admin-dropdown"
								label="Main image"
								labelId="product-image-label"
								value={form.image}
								options={imageOptions.map((option) => ({ value: option.path, label: option.label }))}
								onchange={(value) => (form.image = value)}
								invalid={!!fieldErrors.image}
								describedBy={fieldErrors.image ? 'product-image-error' : undefined}
							/>
							{#if fieldErrors.image}
								<p class="a-field-error" id="product-image-error">{fieldErrors.image}</p>
							{/if}
						</div>
						{#if form.image}
							<img
								class="image-preview"
								src={form.image}
								alt={`Preview of ${imageLabel(form.image)}`}
								width="56"
								height="70"
								loading="lazy"
							/>
						{/if}
					</div>

					<fieldset class="a-fieldset span-2" aria-describedby={fieldErrors.gallery ? 'product-gallery-error' : undefined}>
						<legend>Gallery (optional)</legend>
						<p class="a-field-hint">Choose any extra images. The main image can be included.</p>
						<div class="gallery-grid">
							{#each imageOptions as option (option.path)}
								<label class="a-check">
									<input
										type="checkbox"
										checked={form.gallery.includes(option.path)}
										onchange={(event) => toggleGallery(option.path, event.currentTarget.checked)}
									/>
									<span>{option.label}</span>
								</label>
							{/each}
						</div>
						{#if fieldErrors.gallery}
							<p class="a-field-error" id="product-gallery-error">{fieldErrors.gallery}</p>
						{/if}
					</fieldset>
				</div>
			</section>
		</div>

		<footer class="a-dialog-foot">
			<button class="a-btn a-btn-secondary" type="button" onclick={close} disabled={busy}>
				Cancel
			</button>
			<button class="a-btn a-btn-primary" type="submit" disabled={busy}>
				{busy ? 'Saving...' : product ? 'Save product' : 'Create product'}
			</button>
		</footer>
	</form>
</dialog>
