<script lang="ts">
	import { page } from '$app/state';

	const status = $derived(page.status);
	const isMissing = $derived(status === 404);
	const title = $derived(isMissing ? 'This page is not on the shelf.' : 'The house could not open this page.');
</script>

<svelte:head>
	<title>{isMissing ? 'Not found' : 'Error'} | Azahar</title>
	<meta name="robots" content="noindex" />
</svelte:head>

<section class="not-found wrap" aria-labelledby="not-found-title">
	<p class="not-found-code" aria-hidden="true">{status}</p>
	<div class="not-found-copy">
		<p class="meta">{isMissing ? 'Not on the shelf' : 'A quiet error'}</p>
		<h1 id="not-found-title">{title}</h1>
		<p class="lead">
			{isMissing
				? 'The page you asked for has moved or never existed. The shelf is still open.'
				: 'The page did not load. Try again, or go back to the shelf.'}
		</p>
		<div class="not-found-actions">
			<a class="btn btn-primary" href="/shop">Shop the shelf</a>
			<a class="btn btn-secondary" href="/">Go home</a>
		</div>
	</div>
</section>
