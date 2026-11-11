<script lang="ts">
	const photoIds = [
		'7795755',
		'6621461',
		'8101534',
		'13779108',
		'4735904',
		'35899861',
		'8490122',
		'40557468',
		'31342202',
		'35141870',
		'25745506',
		'8709569',
		'8101673'
	];
</script>

<svelte:head>
	<title>Photography credits | Azahar</title>
	<meta
		name="description"
		content="The Pexels photo IDs used on this site, with a note that the photographs are licensed stand-ins and the bottles are mockups."
	/>
	<meta property="og:title" content="Photography credits | Azahar" />
	<meta
		property="og:description"
		content="The Pexels photographs used on this demo storefront, by photo ID."
	/>
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<h1>Photography credits</h1>
	<p class="lead">
		The photographs are licensed stand-ins from Pexels. The bottles in them are blank
		mockups, not Azahar's own product shots.
	</p>
</div>

<div class="wrap credits-layout">
	<section class="credits-panel" aria-labelledby="photos-title">
		<h2 id="photos-title">Pexels photographs</h2>
		<p class="muted credits-intro">
			Each photograph is listed by its Pexels photo ID. The links open the original on
			Pexels. The set is used under the
			<a class="inline-link" href="https://www.pexels.com/license/">Pexels license</a>.
		</p>

		<ul class="credit-list">
			{#each photoIds as id (id)}
				<li>
					<a
						href={`https://www.pexels.com/photo/${id}/`}
						aria-label={`Pexels photo ${id}`}
					>
						<span class="mono">{id}</span>
						<span class="credit-source">Pexels</span>
					</a>
				</li>
			{/each}
		</ul>
	</section>

	<section class="credits-note prose" aria-labelledby="demo-title">
		<h2 id="demo-title">Demo note</h2>
		<p>
			Azahar is a demonstration storefront. The products, prices, story and policies are
			sample data, and no real payment is taken. The bottles in the photographs are blank
			product mockups, not Azahar's own packaging.
		</p>
	</section>
</div>

<style>
	.credits-layout {
		display: grid;
		grid-template-columns: 7fr 5fr;
		gap: clamp(36px, 6vw, 88px);
		align-items: start;
		padding-bottom: 96px;
	}

	.credits-panel h2 {
		font-size: clamp(28px, 3vw, 38px);
	}

	.credits-intro {
		margin-top: 12px;
		max-width: 52ch;
		font-size: 14px;
	}

	.inline-link {
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.credit-list {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 0 32px;
		margin: 28px 0 0;
		padding: 0;
		list-style: none;
		border-top: 1px solid var(--line);
	}

	.credit-list a {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		min-height: 44px;
		padding: 10px 0;
		border-bottom: 1px solid var(--line);
		font-size: 14px;
	}

	.credit-list a:hover {
		color: var(--accent);
	}

	.credit-source {
		font-size: 12px;
		color: var(--muted);
	}

	.credits-note {
		padding-bottom: 0;
	}

	@media (max-width: 900px) {
		.credits-layout {
			grid-template-columns: 1fr;
		}
	}

	@media (max-width: 560px) {
		.credit-list {
			grid-template-columns: 1fr;
		}
	}
</style>
