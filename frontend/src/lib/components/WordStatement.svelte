<script lang="ts">
	import { onMount } from 'svelte';

	let {
		text,
		accent = ''
	}: {
		text: string;
		accent?: string;
	} = $props();

	let section: HTMLElement;
	let progress = $state(0);
	let reduce = $state(false);

	const words = $derived(text.split(' '));
	const readCount = $derived(reduce ? words.length : Math.round(progress * words.length));

	function clean(word: string): string {
		return word.replace(/[^a-zA-Z0-9]/g, '').toLowerCase();
	}

	onMount(() => {
		const media = window.matchMedia('(prefers-reduced-motion: reduce)');
		reduce = media.matches;
		if (reduce) {
			progress = 1;
			return;
		}

		let frame = 0;
		const update = () => {
			frame = 0;
			if (!section) return;
			const rect = section.getBoundingClientRect();
			const start = window.innerHeight * 0.82;
			const span = rect.height + window.innerHeight * 0.45;
			progress = Math.min(1, Math.max(0, (start - rect.top) / span));
		};
		const onScroll = () => {
			if (!frame) frame = requestAnimationFrame(update);
		};

		update();
		window.addEventListener('scroll', onScroll, { passive: true });
		window.addEventListener('resize', onScroll, { passive: true });
		return () => {
			window.removeEventListener('scroll', onScroll);
			window.removeEventListener('resize', onScroll);
			if (frame) cancelAnimationFrame(frame);
		};
	});
</script>

<section class="statement" bind:this={section} aria-label="The house line">
	<div class="wrap">
		<p>
			{#each words as word, i}
				<span
					class="word"
					class:is-read={i < readCount}
					class:is-accent={accent !== '' && clean(word) === clean(accent)}
				>{word}</span>{i < words.length - 1 ? ' ' : ''}
			{/each}
		</p>
	</div>
</section>
