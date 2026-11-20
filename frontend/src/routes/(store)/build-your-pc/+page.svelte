<script lang="ts">
	import { money } from '#lib/format';
	import { cart } from '#lib/stores/cart.svelte';
	import { toast } from '#lib/stores/toast.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import type { Product } from '#lib/types';
	import type { PageData } from './$types';

	type PartProduct = {
		id: string;
		slug: string;
		name: string;
		subtitle: string;
		description: string;
		price_cents: number;
		sale_price_cents: number | null;
		sale_starts_at: string;
		sale_ends_at: string;
		effective_price_cents: number;
		on_sale: boolean;
		currency: string;
		image: string;
		stock: number;
		plugin_id: string;
		attributes: Record<string, string[]>;
		category: { slug: string; name: string; plugin_id: string };
	};

	type PartCategory = {
		slug: string;
		name: string;
		products: PartProduct[];
	};

	type Slot = {
		slug: string;
		label: string;
		required: boolean;
		empty: string;
	};

	let { data }: { data: PageData } = $props();

	const slots: Slot[] = [
		{ slug: 'cpus', label: 'CPU', required: true, empty: 'No CPUs listed yet.' },
		{
			slug: 'motherboards',
			label: 'Motherboard',
			required: true,
			empty: 'No motherboards listed yet.'
		},
		{ slug: 'memory', label: 'Memory', required: true, empty: 'No memory listed yet.' },
		{ slug: 'storage', label: 'Storage', required: true, empty: 'No storage listed yet.' },
		{
			slug: 'psus',
			label: 'Power supply',
			required: true,
			empty: 'No power supplies listed yet.'
		},
		{ slug: 'cases', label: 'Case', required: true, empty: 'No cases listed yet.' },
		{
			slug: 'gpus',
			label: 'Graphics card',
			required: false,
			empty: 'No graphics cards listed yet.'
		},
		{ slug: 'cooling', label: 'Cooling', required: false, empty: 'No cooling listed yet.' }
	];

	const slotGroups = [
		{
			id: 'required-parts',
			title: 'Required parts',
			slots: slots.filter((slot) => slot.required)
		},
		{
			id: 'optional-parts',
			title: 'Optional parts',
			slots: slots.filter((slot) => !slot.required)
		}
	];

	const specUnits: Record<string, string> = {
		tdp_watts: 'W',
		wattage_watts: 'W',
		length_mm: 'mm',
		max_gpu_length_mm: 'mm'
	};

	const specFields: { key: string; label: string }[] = [
		{ key: 'socket', label: 'Socket' },
		{ key: 'memory_type', label: 'Memory' },
		{ key: 'form_factor', label: 'Form factor' },
		{ key: 'interface', label: 'Interface' },
		{ key: 'tdp_watts', label: 'TDP' },
		{ key: 'wattage_watts', label: 'Wattage' },
		{ key: 'length_mm', label: 'Length' },
		{ key: 'max_gpu_length_mm', label: 'Max GPU' },
		{ key: 'supported_form_factor', label: 'Board sizes' }
	];

	const optionFields: Record<string, string[]> = {
		cpus: ['socket', 'tdp_watts'],
		gpus: ['length_mm', 'tdp_watts'],
		motherboards: ['socket', 'memory_type', 'form_factor'],
		memory: ['memory_type'],
		storage: ['interface'],
		psus: ['wattage_watts'],
		cases: ['supported_form_factor', 'max_gpu_length_mm'],
		cooling: ['socket']
	};

	let selections = $state<Record<string, string>>({});

	const categories = $derived(
		new Map<string, PartCategory>(
			data.categories.map((category) => [category.slug, category] as const)
		)
	);

	const chosen = $derived.by(() => {
		const picked: Record<string, PartProduct | undefined> = {};
		for (const slot of slots) {
			const products = categories.get(slot.slug)?.products ?? [];
			picked[slot.slug] = products.find((product) => product.id === selections[slot.slug]);
		}
		return picked;
	});

	const selectedProducts = $derived(
		slots
			.map((slot) => chosen[slot.slug])
			.filter((product): product is PartProduct => product !== undefined)
	);

	const missingRequired = $derived(
		slots.filter((slot) => slot.required && !chosen[slot.slug]).map((slot) => slot.label)
	);

	const total = $derived(
		selectedProducts.reduce(
			(sum, product) => sum + (product.effective_price_cents ?? product.price_cents),
			0
		)
	);
	const currency = $derived(selectedProducts[0]?.currency ?? 'DZD');

	const blockers = $derived.by(() => {
		const reasons: string[] = [];
		if (missingRequired.length > 0) {
			reasons.push(`Still needed: ${missingRequired.join(', ')}.`);
		}

		const cpu = chosen['cpus'];
		const board = chosen['motherboards'];
		const memory = chosen['memory'];
		const psu = chosen['psus'];
		const pcCase = chosen['cases'];
		const gpu = chosen['gpus'];
		const cooling = chosen['cooling'];

		const cpuSocket = firstAttribute(cpu, 'socket');
		const boardSocket = firstAttribute(board, 'socket');
		if (cpu && !cpuSocket) {
			reasons.push(`The ${cpu.name} does not list a CPU socket, so the fit cannot be checked.`);
		}
		if (board && !boardSocket) {
			reasons.push(`The ${board.name} does not list a CPU socket, so the fit cannot be checked.`);
		}
		if (
			cpu &&
			board &&
			cpuSocket &&
			boardSocket &&
			normalize(cpuSocket) !== normalize(boardSocket)
		) {
			reasons.push(
				`The ${cpu.name} uses the ${cpuSocket} socket, but the ${board.name} uses ${boardSocket}.`
			);
		}

		const memoryType = firstAttribute(memory, 'memory_type');
		const boardMemory = firstAttribute(board, 'memory_type');
		if (memory && !memoryType) {
			reasons.push(`The ${memory.name} does not list a memory type, so the fit cannot be checked.`);
		}
		if (board && !boardMemory) {
			reasons.push(`The ${board.name} does not list a memory type, so the fit cannot be checked.`);
		}
		if (
			memory &&
			board &&
			memoryType &&
			boardMemory &&
			normalize(memoryType) !== normalize(boardMemory)
		) {
			reasons.push(
				`The ${memory.name} is ${memoryType} memory, but the ${board.name} takes ${boardMemory}.`
			);
		}

		if (psu && cpu) {
			const wattage = numberAttribute(psu, 'wattage_watts');
			const cpuTdp = numberAttribute(cpu, 'tdp_watts');
			const gpuTdp = numberAttribute(gpu, 'tdp_watts');
			if (cpuTdp === null) {
				reasons.push(`The ${cpu.name} does not list a TDP, so the power budget cannot be checked.`);
			}
			if (gpu && gpuTdp === null) {
				reasons.push(`The ${gpu.name} does not list a TDP, so the power budget cannot be checked.`);
			}
			const needed = (cpuTdp ?? 0) + (gpuTdp ?? 0) + 150;
			if (wattage === null) {
				reasons.push(
					`The ${psu.name} does not list a wattage, so the power budget cannot be checked.`
				);
			} else if (wattage < needed) {
				reasons.push(
					`The ${psu.name} supplies ${wattage} W, but this build needs at least ${needed} W including 150 W of headroom.`
				);
			}
		}

		const boardForm = firstAttribute(board, 'form_factor');
		const caseForms = listAttribute(pcCase, 'supported_form_factor');
		if (board && !boardForm) {
			reasons.push(`The ${board.name} does not list a form factor, so the case fit cannot be checked.`);
		}
		if (pcCase && caseForms.length === 0) {
			reasons.push(`The ${pcCase.name} does not list supported board sizes, so the fit cannot be checked.`);
		}
		if (
			board &&
			pcCase &&
			boardForm &&
			caseForms.length > 0 &&
			!caseForms.some((form) => normalize(form) === normalize(boardForm))
		) {
			reasons.push(
				`The ${board.name} is ${boardForm}, but the ${pcCase.name} accepts ${displayAttribute(pcCase, 'supported_form_factor')}.`
			);
		}

		const gpuLength = numberAttribute(gpu, 'length_mm');
		const maxGpuLength = numberAttribute(pcCase, 'max_gpu_length_mm');
		if (gpu && gpuLength === null) {
			reasons.push(`The ${gpu.name} does not list its length, so the case fit cannot be checked.`);
		}
		if (pcCase && maxGpuLength === null) {
			reasons.push(`The ${pcCase.name} does not list maximum graphics card length.`);
		}
		if (
			gpu &&
			pcCase &&
			gpuLength !== null &&
			maxGpuLength !== null &&
			gpuLength > maxGpuLength
		) {
			reasons.push(
				`The ${gpu.name} is ${gpuLength} mm long, but the ${pcCase.name} fits cards up to ${maxGpuLength} mm.`
			);
		}

		const coolingSockets = listAttribute(cooling, 'socket');
		if (cooling && coolingSockets.length === 0) {
			reasons.push(`The ${cooling.name} does not list supported sockets, so the fit cannot be checked.`);
		}
		if (
			cooling &&
			cpu &&
			cpuSocket &&
			coolingSockets.length > 0 &&
			!coolingSockets.some((socket) => normalize(socket) === normalize(cpuSocket))
		) {
			reasons.push(
				`The ${cooling.name} fits ${coolingSockets.join(', ')}, not the ${cpuSocket} socket of the ${cpu.name}.`
			);
		}

		return reasons;
	});

	const canAdd = $derived(blockers.length === 0);

	function normalize(value: string): string {
		return value.trim().toLowerCase();
	}

	function firstAttribute(product: PartProduct | undefined, key: string): string {
		const values = product?.attributes?.[key] ?? [];
		for (const value of values) {
			const trimmed = value.trim();
			if (trimmed) return trimmed;
		}
		return '';
	}

	function displayAttribute(product: PartProduct | undefined, key: string): string {
		return (product?.attributes?.[key] ?? [])
			.map((value) => value.trim())
			.filter(Boolean)
			.join(', ');
	}

	function listAttribute(product: PartProduct | undefined, key: string): string[] {
		return (product?.attributes?.[key] ?? [])
			.flatMap((value) => value.split(','))
			.map((value) => value.trim())
			.filter(Boolean);
	}

	function numberAttribute(product: PartProduct | undefined, key: string): number | null {
		const raw = firstAttribute(product, key);
		if (!raw) return null;
		const parsed = Number.parseFloat(raw);
		return Number.isFinite(parsed) ? parsed : null;
	}

	function specValue(key: string, value: string): string {
		const unit = specUnits[key];
		if (!unit) return value;
		return value.toLowerCase().endsWith(unit.toLowerCase()) ? value : `${value} ${unit}`;
	}

	function specRows(product: PartProduct): { label: string; value: string }[] {
		const rows: { label: string; value: string }[] = [];
		for (const field of specFields) {
			const value = displayAttribute(product, field.key);
			if (!value) continue;
			rows.push({ label: field.label, value: specValue(field.key, value) });
		}
		return rows;
	}

	function optionLabel(product: PartProduct, slotSlug: string): string {
		const details: string[] = [];
		for (const key of optionFields[slotSlug] ?? []) {
			const value = displayAttribute(product, key);
			if (value) details.push(specValue(key, value));
		}
		const price = money(product.effective_price_cents ?? product.price_cents, product.currency);
		return details.length > 0
			? `${product.name} · ${price} · ${details.join(' · ')}`
			: `${product.name} · ${price}`;
	}

	function productsFor(slot: Slot): PartProduct[] {
		return categories.get(slot.slug)?.products ?? [];
	}

	function toCartProduct(part: PartProduct): Product {
		return {
			...part,
			option_schema: [],
			sale_price_cents: part.sale_price_cents,
			sale_starts_at: part.sale_starts_at,
			sale_ends_at: part.sale_ends_at,
			effective_price_cents: part.effective_price_cents,
			on_sale: part.on_sale,
			compare_at_cents: null,
			size: '',
			concerns: [],
			ingredients: [],
			how_to: '',
			gallery: [],
			featured: false,
			active: true,
			plugin_name: 'PC parts',
			plugin_summary: 'PC part'
		};
	}

	function addBuild(): void {
		if (!canAdd) return;
		let added = 0;
		for (const part of selectedProducts) {
			if (cart.add(toCartProduct(part), 1)) added += 1;
		}
		toast.show(`${added} parts added to the bag.`, '/cart', 'View bag');
	}
</script>

<svelte:head>
	<title>Build your PC | {data.theme.brand_name}</title>
	<meta
		name="description"
		content="Choose a CPU, motherboard, memory, storage, power supply and case, check the fit, and add the build to your bag."
	/>
	<meta property="og:title" content={`Build your PC | ${data.theme.brand_name}`} />
	<meta
		property="og:description"
		content="Choose compatible PC parts and add the whole build to your bag."
	/>
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<p class="meta">The workbench</p>
	<h1>Build your PC</h1>
	<p class="lead">
		Choose one part for each slot. The checks below compare the listed specs before anything goes
		in the bag.
	</p>
</div>

{#if data.apiDown}
	<div class="wrap">
		<div class="empty-state build-state">
			<p class="meta">Parts shelf offline</p>
			<h2>The builder could not load.</h2>
			<p class="muted">The parts API is not answering. Nothing in your bag was lost.</p>
			<button class="btn btn-secondary" type="button" onclick={() => location.reload()}>
				Reload the builder
			</button>
		</div>
	</div>
{:else}
	<div class="wrap builder-layout">
		<div class="parts">
			{#each slotGroups as group (group.id)}
				<section class="part-group" aria-labelledby={group.id}>
					<div class="group-head">
						<h2 id={group.id}>{group.title}</h2>
						<span class="meta">{group.slots.length} slots</span>
					</div>

					<div class="slot-grid">
						{#each group.slots as slot (slot.slug)}
							{@const products = productsFor(slot)}
							{@const available = products.filter((product) => product.stock > 0)}
							{@const picked = chosen[slot.slug]}
							<fieldset class="slot">
								<legend>
									{slot.label}
									<span class="meta">{slot.required ? 'Required' : 'Optional'}</span>
								</legend>

								{#if products.length === 0}
									<p class="slot-empty muted">{slot.empty}</p>
								{:else if available.length === 0}
									<p class="slot-empty muted">Every part in this slot is sold out.</p>
								{:else}
									<Dropdown
										id={`slot-${slot.slug}`}
										class="builder-dropdown"
										label={`Choose a ${slot.label.toLowerCase()}`}
										value={selections[slot.slug] ?? ''}
										options={[
											{ value: '', label: `Choose a ${slot.label.toLowerCase()}` },
											...available.map((product) => ({
												value: product.id,
												label: optionLabel(product, slot.slug)
											}))
										]}
										onchange={(value) => {
											selections = { ...selections, [slot.slug]: value };
										}}
									/>

									{#if picked}
										{@const specs = specRows(picked)}
										<div class="chosen">
											<div class="chosen-head">
												<span class="chosen-name">{picked.name}</span>
												<span class="chosen-price">
													{money(
														picked.effective_price_cents ?? picked.price_cents,
														picked.currency
													)}
												</span>
											</div>
											{#if specs.length > 0}
												<ul class="specs">
													{#each specs as spec (spec.label)}
														<li>
															<span class="spec-label">{spec.label}</span>
															<span>{spec.value}</span>
														</li>
													{/each}
												</ul>
											{/if}
										</div>
									{/if}
								{/if}
							</fieldset>
						{/each}
					</div>
				</section>
			{/each}
		</div>

		<aside class="summary" aria-labelledby="summary-title">
			<h2 id="summary-title">Your build</h2>

			<dl class="build-lines">
				{#each slots as slot (slot.slug)}
					{@const picked = chosen[slot.slug]}
					<div class="build-line">
						<dt>{slot.label}</dt>
						<dd>
							{#if picked}
								<span class="build-name">{picked.name}</span>
								<span class="build-price">
									{money(
										picked.effective_price_cents ?? picked.price_cents,
										picked.currency
									)}
								</span>
							{:else}
								<span class="build-missing">
									{slot.required ? 'Not chosen, required' : 'Not chosen'}
								</span>
							{/if}
						</dd>
					</div>
				{/each}
			</dl>

			<div class="summary-total">
				<span>Total</span>
				<span>{money(total, currency)}</span>
			</div>

			<p class="meta">One of each chosen part goes in the bag.</p>

			{#if blockers.length > 0}
				<div class="blockers" role="status">
					<h3>Before this build can go in the bag</h3>
					<ul>
						{#each blockers as reason}
							<li>{reason}</li>
						{/each}
					</ul>
				</div>
				<button class="btn btn-primary btn-block" type="button" disabled>
					Add this build to the bag
				</button>
			{:else}
				<p class="ready">
					<span class="meta">Ready.</span>
					All required parts are chosen and the listed specs fit together.
				</p>
				<button class="btn btn-primary btn-block" type="button" onclick={addBuild}>
					Add this build to the bag
				</button>
			{/if}

			<a class="text-link summary-link" href="/cart">View the bag</a>
		</aside>
	</div>
{/if}

<style>
	.build-state {
		margin-top: 0;
		border-top: 0;
	}

	.builder-layout {
		display: grid;
		grid-template-columns: minmax(0, 1.65fr) minmax(300px, 0.85fr);
		gap: clamp(32px, 5vw, 72px);
		align-items: start;
		padding-bottom: 100px;
	}

	.parts {
		display: grid;
		gap: 42px;
	}

	.group-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 16px;
		padding-bottom: 14px;
		border-bottom: 1px solid var(--line);
	}

	.group-head h2 {
		font-size: clamp(24px, 2.8vw, 34px);
	}

	.slot-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 14px;
		padding-top: 14px;
	}

	.slot {
		display: grid;
		gap: 12px;
		align-content: start;
		min-width: 0;
		margin: 0;
		padding: 16px;
		border: 1px solid var(--line);
		border-radius: var(--radius);
	}

	.slot legend {
		padding: 0 6px;
		font-family: var(--font-mono);
		font-size: 12px;
		line-height: 1.4;
		color: var(--ink);
	}

	.slot legend .meta {
		margin-left: 6px;
	}

	.slot :global(.builder-dropdown) {
		width: 100%;
	}

	.slot :global(.builder-dropdown .dropdown-trigger) {
		width: 100%;
		min-height: 44px;
		font-size: 13px;
	}

	.slot-empty {
		font-size: 13px;
	}

	.chosen {
		display: grid;
		gap: 8px;
		padding-top: 12px;
		border-top: 1px solid var(--line);
	}

	.chosen-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 12px;
	}

	.chosen-name {
		font-size: 14px;
		font-weight: 500;
		overflow-wrap: anywhere;
	}

	.chosen-price {
		font-family: var(--font-mono);
		font-size: 12px;
		white-space: nowrap;
	}

	.specs {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 14px;
		margin: 0;
		padding: 0;
		list-style: none;
		font-size: 11px;
		line-height: 1.4;
		color: var(--muted);
	}

	.specs li {
		display: flex;
		gap: 5px;
	}

	.spec-label {
		font-family: var(--font-mono);
	}

	.build-lines {
		display: grid;
		gap: 8px;
		margin: 0;
	}

	.build-line {
		display: grid;
		grid-template-columns: 82px minmax(0, 1fr);
		gap: 2px 10px;
		padding-bottom: 8px;
		border-bottom: 1px solid var(--line);
	}

	.build-line dt {
		font-family: var(--font-mono);
		font-size: 11px;
		line-height: 1.4;
		color: var(--muted);
	}

	.build-line dd {
		display: flex;
		justify-content: space-between;
		gap: 10px;
		margin: 0;
		font-size: 13px;
		line-height: 1.4;
	}

	.build-name {
		overflow-wrap: anywhere;
	}

	.build-price {
		font-family: var(--font-mono);
		font-size: 12px;
		white-space: nowrap;
	}

	.build-missing {
		color: var(--muted);
	}

	.blockers {
		display: grid;
		gap: 8px;
		padding: 12px 14px;
		border: 1px solid var(--line-strong);
		border-radius: var(--radius);
	}

	.blockers h3 {
		font-size: 14px;
		letter-spacing: -0.01em;
	}

	.blockers ul {
		display: grid;
		gap: 6px;
		margin: 0;
		padding-left: 18px;
		font-size: 13px;
		line-height: 1.45;
	}

	.ready {
		font-size: 13px;
		line-height: 1.45;
		color: var(--muted);
	}

	.summary-link {
		justify-content: flex-start;
	}

	@media (max-width: 900px) {
		.builder-layout {
			grid-template-columns: 1fr;
			gap: 40px;
			padding-bottom: 72px;
		}

		.summary {
			position: static;
		}
	}

	@media (max-width: 560px) {
		.slot-grid {
			grid-template-columns: 1fr;
			gap: 12px;
		}

		.slot {
			padding: 14px;
		}

		.build-line {
			grid-template-columns: 72px minmax(0, 1fr);
		}
	}
</style>
