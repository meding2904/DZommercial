<script lang="ts">
	import { PRESETS } from '#lib/admin/presets';
	import { theme } from '#lib/admin/theme.svelte';
	import type { ThemePreset } from '#lib/types';

	let buttons: Array<HTMLButtonElement | undefined> = [];

	function collect(node: HTMLButtonElement, index: number) {
		buttons[index] = node;
		return {
			destroy() {
				if (buttons[index] === node) buttons[index] = undefined;
			}
		};
	}

	function choose(preset: ThemePreset) {
		void theme.choose(preset);
	}

	function onKeydown(event: KeyboardEvent, index: number) {
		let next = index;
		if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (index + 1) % PRESETS.length;
		else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp')
			next = (index - 1 + PRESETS.length) % PRESETS.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = PRESETS.length - 1;
		else return;

		event.preventDefault();
		choose(PRESETS[next].id);
		buttons[next]?.focus();
	}
</script>

<div class="preset">
	<p class="preset-label" id="preset-label">Colour preset</p>
	<div class="preset-group" role="radiogroup" aria-labelledby="preset-label">
		{#each PRESETS as preset, index (preset.id)}
			<button
				use:collect={index}
				type="button"
				role="radio"
				class="preset-option"
				aria-checked={theme.preset === preset.id}
				tabindex={theme.preset === preset.id ? 0 : -1}
				onclick={() => choose(preset.id)}
				onkeydown={(event) => onKeydown(event, index)}
			>
				<span class="preset-swatch" data-preset={preset.id} aria-hidden="true"></span>
				<span class="preset-name">{preset.name}</span>
				{#if theme.preset === preset.id}
					<span class="preset-current">Current</span>
				{/if}
			</button>
		{/each}
	</div>
	{#if theme.saveError}
		<p class="preset-note" role="status">{theme.saveError}</p>
	{/if}
</div>
