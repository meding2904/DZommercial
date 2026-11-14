<script lang="ts">
	import type { PageData } from './$types';

	let { data }: { data: PageData } = $props();

	const policies = [
		{ slug: 'shipping', label: 'Shipping' },
		{ slug: 'returns', label: 'Returns' },
		{ slug: 'privacy', label: 'Privacy' },
		{ slug: 'terms', label: 'Terms of service' }
	];
</script>

<svelte:head>
	<title>{data.policy.title} | Azahar</title>
	<meta name="description" content={data.policy.description} />
	<meta property="og:title" content={`${data.policy.title} | Azahar`} />
	<meta property="og:description" content={data.policy.description} />
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<p class="meta">Policies</p>
	<h1>{data.policy.title}</h1>
	<p class="lead">{data.policy.description}</p>
	{#if data.policy.version}
		<p class="meta">Version {data.policy.version}</p>
	{/if}
</div>

<div class="wrap">
	<div class="prose policy-prose">
		{#each data.policy.sections as section}
			<h2>{section.heading}</h2>
			<p>{section.body}</p>
		{/each}
	</div>

	<nav class="policy-nav" aria-label="Policies">
		{#each policies as policy}
			<a
				href={`/policies/${policy.slug}`}
				aria-current={data.slug === policy.slug ? 'page' : undefined}
			>
				{policy.label}
			</a>
		{/each}
		<a class="policy-contact" href="mailto:hello@azahar.example">Questions: hello@azahar.example</a>
	</nav>
</div>

<style>
	.policy-prose {
		padding-bottom: 56px;
	}

	.policy-prose p:last-of-type {
		margin-bottom: 8px;
	}

	.policy-nav {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 22px;
		padding: 20px 0 72px;
		border-top: 1px solid var(--line);
	}

	.policy-nav a {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		padding-inline: 4px;
		font-size: 14px;
		color: var(--muted);
	}

	.policy-nav a:hover {
		color: var(--ink);
	}

	.policy-nav a[aria-current='page'] {
		color: var(--ink);
		text-decoration: underline;
		text-underline-offset: 4px;
	}

	.policy-contact {
		margin-left: auto;
	}

	@media (max-width: 700px) {
		.policy-contact {
			margin-left: 0;
		}
	}
</style>
