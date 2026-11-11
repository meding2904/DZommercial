<script lang="ts">
	let {
		src,
		alt,
		width,
		height,
		sizes = '(max-width: 560px) 100vw, (max-width: 1080px) 50vw, 25vw',
		priority = false,
		class: className = ''
	}: {
		src: string;
		alt: string;
		width: number;
		height: number;
		sizes?: string;
		priority?: boolean;
		class?: string;
	} = $props();

	function srcset(path: string): string {
		const match = path.match(/-(1600|1400|1200)\.webp$/);
		if (!match) return path;
		const base = path.slice(0, -match[0].length);
		const large = match[1];
		return `${base}-${large}.webp ${large}w, ${base}-960.webp 960w, ${base}-480.webp 480w`;
	}
</script>

<img
	{src}
	srcset={srcset(src)}
	{sizes}
	{alt}
	{width}
	{height}
	class={className}
	loading={priority ? 'eager' : 'lazy'}
	fetchpriority={priority ? 'high' : 'auto'}
	decoding="async"
/>
