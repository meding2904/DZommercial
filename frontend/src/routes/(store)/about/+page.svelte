<script lang="ts">
	import ProductImage from '#lib/components/ProductImage.svelte';

	const materials = [
		['Olive squalane', 'Pressed, light, close to skin'],
		['Orange blossom', 'The city in spring'],
		['Rose clay', 'Soft pink, drawn from the earth'],
		['Prickly pear', 'Seed oil from the dry south'],
		['Fig leaf', 'Green, milky, warm'],
		['Cedar and myrrh', 'Smoke without sweetness']
	];
</script>

<svelte:head>
	<title>The house | Azahar</title>
	<meta
		name="description"
		content="The story of Azahar, a small skincare house in Algiers working with orange blossom, olive, fig and clay."
	/>
	<meta property="og:title" content="The house | Azahar" />
	<meta
		property="og:description"
		content="A small skincare house in Algiers, working with the orange blossom, olive and fig of the Mediterranean edge."
	/>
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<h1>A small shelf in Algiers.</h1>
	<p class="lead">
		Azahar works with the orange blossom, olive and fig of the Mediterranean edge. The shelf
		is short on purpose.
	</p>
</div>

<div class="wrap editorial about-editorial">
	<div class="editorial-media">
		<ProductImage
			src="/images/atelier-1200.webp"
			alt="Amber bottles, ceramic jars, linen and dried botanicals on a work table"
			width={1200}
			height={1500}
			sizes="(max-width: 900px) 100vw, 50vw"
		/>
	</div>

	<div class="editorial-copy">
		<p class="meta">The house</p>
		<h2>Blossom, oil and clay.</h2>
		<p>
			Azahar began with the orange blossom that scents Algiers in spring. We press olives and
			figs, mix clays with rose, and pour everything into amber glass. The work is done in
			small batches.
		</p>
		<p>
			The shelf covers the face, the body and the bath. Nothing else. Each formula has to
			earn its place before it is poured.
		</p>
	</div>
</div>

<section class="section section-rule about-materials" aria-labelledby="materials-title">
	<div class="wrap prose">
		<h2 id="materials-title">The materials</h2>
		<p>
			The work starts with what grows nearby and what the press gives us. These are the
			materials that carry the shelf.
		</p>

		<div class="ingredient-list">
			{#each materials as [name, note]}
				<div>
					<span>{name}</span>
					<span>{note}</span>
				</div>
			{/each}
		</div>

		<h2>How the shelf is kept short</h2>
		<p>
			We make fewer things, with more of the plant in them. No filler, and no fragrance to
			hide the materials. If a formula already does its job, we do not make a second version
			of it.
		</p>
		<p>
			That is the whole method: press, blend, pour, and stop when the shelf is complete.
		</p>

		<a class="btn btn-primary about-link" href="/shop">Shop the shelf</a>
	</div>
</section>

<style>
	.about-editorial {
		padding-bottom: 24px;
	}

	.about-materials {
		padding-top: 72px;
	}

	.about-link {
		justify-self: start;
		margin-top: 12px;
	}
</style>
