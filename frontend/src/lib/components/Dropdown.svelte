<script lang="ts">
	import { tick } from 'svelte';

	type Option = { value: string; label: string };

	let {
		id,
		label,
		labelId,
		value,
		options,
		onchange,
		invalid = false,
		describedBy,
		disabled = false,
		class: className = ''
	}: {
		id: string;
		label: string;
		labelId?: string;
		value: string;
		options: Option[];
		onchange: (value: string) => void;
		invalid?: boolean;
		describedBy?: string;
		disabled?: boolean;
		class?: string;
	} = $props();

	let open = $state(false);
	let active = $state(0);
	let root = $state<HTMLDivElement | null>(null);
	let trigger = $state<HTMLButtonElement | null>(null);
	let menu = $state<HTMLDivElement | null>(null);
	let menuStyle = $state('');

	const selected = $derived(options.find((option) => option.value === value) ?? options[0]);
	const activeId = $derived(`${id}-option-${active}`);

	function positionMenu(): void {
		if (!trigger) return;
		const rect = trigger.getBoundingClientRect();
		const estimated = Math.min(options.length * 44 + 8, 260);
		const spaceBelow = window.innerHeight - rect.bottom;
		const openUp = spaceBelow < estimated + 12 && rect.top > estimated + 12;
		const top = openUp
			? Math.max(8, rect.top - estimated - 6)
			: Math.min(window.innerHeight - estimated - 8, rect.bottom + 6);
		const width = Math.min(Math.max(rect.width, 190), window.innerWidth - 16);
		const left = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8));
		menuStyle = `left: ${left}px; top: ${Math.max(8, top)}px; width: ${width}px;`;
	}

	async function openMenu(): Promise<void> {
		if (open || disabled) return;
		active = Math.max(
			0,
			options.findIndex((option) => option.value === value)
		);
		open = true;
		positionMenu();
		await tick();
		menu?.focus();
		fitMenu();
	}

	function fitMenu(): void {
		if (!menu) return;
		const rect = menu.getBoundingClientRect();
		const overflowBottom = rect.bottom - (window.innerHeight - 8);
		if (overflowBottom > 0) {
			menu.style.top = `${Math.max(8, rect.top - overflowBottom)}px`;
		}
		if (rect.top < 8) {
			menu.style.top = '8px';
		}
	}

	function closeMenu(focusTrigger = false): void {
		open = false;
		if (focusTrigger) trigger?.focus();
	}

	function select(option: Option): void {
		if (option.value !== value) onchange(option.value);
		closeMenu(true);
	}

	function onTriggerKey(event: KeyboardEvent): void {
		if (disabled) return;
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp' || event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			void openMenu();
		}
	}

	function onMenuKey(event: KeyboardEvent): void {
		switch (event.key) {
			case 'ArrowDown':
				event.preventDefault();
				active = Math.min(options.length - 1, active + 1);
				break;
			case 'ArrowUp':
				event.preventDefault();
				active = Math.max(0, active - 1);
				break;
			case 'Home':
				event.preventDefault();
				active = 0;
				break;
			case 'End':
				event.preventDefault();
				active = options.length - 1;
				break;
			case 'Enter':
			case ' ':
				event.preventDefault();
				if (options[active]) select(options[active]);
				break;
			case 'Escape':
				event.preventDefault();
				closeMenu(true);
				break;
			case 'Tab':
				closeMenu(false);
				break;
		}
	}

	function onDocumentPointer(event: PointerEvent): void {
		if (root && !root.contains(event.target as Node)) closeMenu(false);
	}

	function onViewportChange(event: Event): void {
		if (!open) return;
		if (event.type === 'scroll' && event.target instanceof Node && root?.contains(event.target)) return;
		closeMenu(false);
	}

	$effect(() => {
		if (!open) return;
		document.addEventListener('pointerdown', onDocumentPointer, true);
		window.addEventListener('resize', onViewportChange);
		window.addEventListener('scroll', onViewportChange, true);
		return () => {
			document.removeEventListener('pointerdown', onDocumentPointer, true);
			window.removeEventListener('resize', onViewportChange);
			window.removeEventListener('scroll', onViewportChange, true);
		};
	});

	$effect(() => {
		if (!open) return;
		const option = document.getElementById(`${id}-option-${active}`);
		option?.scrollIntoView({ block: 'nearest' });
	});
</script>

<div class="dropdown {className}" bind:this={root}>
	<button
		bind:this={trigger}
		id={id}
		type="button"
		class="dropdown-trigger"
		aria-haspopup="listbox"
		aria-expanded={open}
		aria-controls={`${id}-listbox`}
		aria-label={label}
		aria-labelledby={labelId}
		data-invalid={invalid || undefined}
		aria-describedby={describedBy}
		disabled={disabled}
		onclick={() => (open ? closeMenu(true) : void openMenu())}
		onkeydown={onTriggerKey}
	>
		<span class="dropdown-value">{selected?.label ?? 'Choose'}</span>
		<svg
			class="dropdown-chevron"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			stroke-width="1.6"
			stroke-linecap="round"
			stroke-linejoin="round"
			aria-hidden="true"
		>
			<path d="m6 9 6 6 6-6" />
		</svg>
	</button>

	{#if open}
		<div
			class="dropdown-menu"
			id={`${id}-listbox`}
			role="listbox"
			aria-label={label}
			aria-labelledby={labelId}
			tabindex="-1"
			bind:this={menu}
			style={menuStyle}
			onkeydown={onMenuKey}
			aria-activedescendant={activeId}
		>
			{#each options as option, index (option.value)}
				<div
					id={`${id}-option-${index}`}
					class="dropdown-option"
					class:is-active={index === active}
					role="option"
					tabindex="-1"
					aria-selected={option.value === value}
					onpointerdown={(event) => event.preventDefault()}
					onclick={() => select(option)}
					onkeydown={(event) => {
						if (event.key === 'Enter' || event.key === ' ') {
							event.preventDefault();
							select(option);
						}
					}}
					onpointerenter={() => (active = index)}
				>
					<span>{option.label}</span>
					{#if option.value === value}
						<svg
							class="dropdown-check"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							stroke-width="2"
							stroke-linecap="round"
							stroke-linejoin="round"
							aria-hidden="true"
						>
							<path d="m5 12 4 4L19 6" />
						</svg>
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>
