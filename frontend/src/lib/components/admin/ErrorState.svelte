<script lang="ts">
	let {
		title = 'Could not load this view.',
		message,
		onretry
	}: {
		title?: string;
		message: string;
		onretry: () => void;
	} = $props();
</script>

<div class="a-state a-state-error" role="alert">
	<div class="a-state-copy">
		<p class="a-state-title">{title}</p>
		<p class="a-state-body">{message}</p>
	</div>
	<button class="a-btn a-btn-secondary" type="button" onclick={onretry}>Retry</button>
</div>
