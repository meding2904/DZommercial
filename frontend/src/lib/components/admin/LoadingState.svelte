<script lang="ts">
	let { label }: { label: string } = $props();
</script>

<div class="a-state" role="status">
	<span class="a-spinner" aria-hidden="true"></span>
	<p>{label}</p>
</div>
