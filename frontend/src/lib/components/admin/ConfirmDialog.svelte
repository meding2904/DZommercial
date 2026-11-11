<script lang="ts">
	let {
		title,
		message,
		confirmLabel,
		cancelLabel = 'Cancel',
		busy = false,
		error = '',
		onconfirm,
		ondismiss
	}: {
		title: string;
		message: string;
		confirmLabel: string;
		cancelLabel?: string;
		busy?: boolean;
		error?: string;
		onconfirm: () => void;
		ondismiss: () => void;
	} = $props();

	let dialogEl = $state<HTMLDialogElement | null>(null);

	$effect(() => {
		if (dialogEl && !dialogEl.open) dialogEl.showModal();
	});

	function dismiss() {
		if (!busy) dialogEl?.close();
	}
</script>

<dialog
	bind:this={dialogEl}
	class="a-dialog a-dialog--confirm"
	aria-labelledby="confirm-dialog-title"
	onclose={ondismiss}
	oncancel={(event) => {
		if (busy) event.preventDefault();
	}}
>
	<div class="a-dialog-form">
		<header class="a-dialog-head">
			<div class="a-dialog-title">
				<h2 id="confirm-dialog-title">{title}</h2>
			</div>
		</header>

		<div class="a-dialog-body">
			<p class="a-state-body">{message}</p>
			{#if error}
				<p class="a-form-error" role="alert">{error}</p>
			{/if}
		</div>

		<footer class="a-dialog-foot">
			<button class="a-btn a-btn-secondary" type="button" onclick={dismiss} disabled={busy}>
				{cancelLabel}
			</button>
			<button class="a-btn a-btn-danger" type="button" onclick={onconfirm} disabled={busy}>
				{busy ? 'Archiving...' : confirmLabel}
			</button>
		</footer>
	</div>
</dialog>
