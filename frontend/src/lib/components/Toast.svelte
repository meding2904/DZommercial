<script lang="ts">
	import { toast } from '#lib/stores/toast.svelte';
</script>

{#if toast.message}
	<div class="toast" role="status" aria-live="polite">
		<span>{toast.message}</span>
		{#if toast.href}
			<a href={toast.href} onclick={() => toast.hide()}>{toast.hrefLabel}</a>
		{/if}
		<button class="link-button" type="button" onclick={() => toast.hide()} aria-label="Dismiss notification">
			Close
		</button>
	</div>
{/if}
