<script lang="ts">
	import '../app.css';
	import '@fontsource-variable/schibsted-grotesk';
	import '@fontsource-variable/jetbrains-mono';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

{@render children()}
