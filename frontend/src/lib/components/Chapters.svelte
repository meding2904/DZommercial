<script lang="ts">
	import { onMount } from 'svelte';
	import ProductImage from './ProductImage.svelte';

	type Chapter = {
		title: string;
		body: string;
		image: string;
		alt: string;
	};

	let { chapters }: { chapters: Chapter[] } = $props();

	let active = $state(0);
	let reduce = $state(false);
	let root: HTMLElement;

	onMount(() => {
		if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
			reduce = true;
			return;
		}

		const nodes = Array.from(root.querySelectorAll<HTMLElement>('[data-chapter]'));
		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (!entry.isIntersecting) continue;
					const index = Number((entry.target as HTMLElement).dataset.index ?? 0);
					if (!Number.isNaN(index)) active = index;
				}
			},
			{ rootMargin: '-42% 0px -48% 0px', threshold: 0 }
		);

		nodes.forEach((node) => observer.observe(node));
		return () => observer.disconnect();
	});
</script>

<div class="chapters" data-reduced={reduce} bind:this={root}>
	<div class="chapters-copy">
		{#each chapters as chapter, i}
			<article
				class="chapter"
				class:is-active={active === i}
				data-index={i}
				data-chapter
			>
				<div class="chapter-mobile-media">
					<ProductImage
						src={chapter.image}
						alt={chapter.alt}
						width={1200}
						height={1500}
						sizes="100vw"
					/>
				</div>
				<p class="meta">0{i + 1}</p>
				<h3>{chapter.title}</h3>
				<p>{chapter.body}</p>
			</article>
		{/each}
	</div>

	<div class="chapters-media" aria-hidden="true">
		{#each chapters as chapter, i}
			<ProductImage
				src={chapter.image}
				alt=""
				width={1200}
				height={1500}
				sizes="(max-width: 900px) 0px, 50vw"
				class={active === i ? 'is-active' : ''}
			/>
		{/each}
	</div>
</div>

<style>
	.chapters[data-reduced='true'] .chapter {
		opacity: 1;
	}
</style>
