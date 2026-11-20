<script lang="ts">
	import '../app.css';
	import '@fontsource-variable/schibsted-grotesk';
	import '@fontsource-variable/jetbrains-mono';
	import '@fontsource-variable/source-serif-4';
	import '@fontsource-variable/fraunces';
	import '@fontsource-variable/inter';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

{@render children()}
