<script lang="ts">
	import { FREE_SHIPPING_OVER_CENTS, SHIPPING_CENTS } from '#lib/components/store/shipping';
	import { money } from '#lib/format';
	import type { StorefrontTheme } from '#lib/theme/storefront';
	import type { PageData } from './$types';

	let { data }: { data: PageData & { theme: StorefrontTheme } } = $props();
	const theme = $derived(data.theme);
</script>

<svelte:head>
	<title>Contact | {theme.brand_name}</title>
	<meta
		name="description"
		content={`Write to ${theme.brand_name} about an order, a product or a delivery.`}
	/>
	<meta property="og:title" content={`Contact | ${theme.brand_name}`} />
	<meta property="og:description" content={`Write to ${theme.brand_name} about an order.`} />
	<meta property="og:type" content="website" />
</svelte:head>

<div class="wrap page-head">
	<h1>Talk to the house</h1>
	<p class="lead">Write to us about an order, a product or a delivery.</p>
</div>

<div class="wrap contact-layout">
	<div class="contact-main">
		<a class="contact-email" href={`mailto:${theme.contact_email}`}>{theme.contact_email}</a>
		<p class="muted">
			For an order, include the order number from your confirmation. It is the quickest way
			for us to find the parcel.
		</p>
		<p class="muted">The inbox is the way in.</p>
	</div>

	<div class="prose contact-notes">
		<h2>Shipping</h2>
		<p>
			Orders travel by courier across Algeria. Shipping is {money(SHIPPING_CENTS)}, and free
			on orders over {money(FREE_SHIPPING_OVER_CENTS)}. Read the
			<a href="/policies/shipping">shipping policy</a> for dispatch and delivery.
		</p>

		<h2>Returns</h2>
		<p>
			Unopened products can come back within 14 days of delivery. Read the
			<a href="/policies/returns">returns policy</a> before you send anything back.
		</p>

		<h2>Payment</h2>
		<p>Cash on delivery. Pay the courier when the order arrives.</p>
	</div>
</div>

<style>
	.contact-layout {
		display: grid;
		grid-template-columns: 7fr 5fr;
		gap: clamp(36px, 6vw, 88px);
		align-items: start;
		padding-bottom: 96px;
	}

	.contact-main {
		display: grid;
		gap: 20px;
		align-content: start;
	}

	.contact-email {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		width: fit-content;
		max-width: 100%;
		padding-bottom: 8px;
		border-bottom: 1px solid var(--line-strong);
		font-family: var(--font-display);
		font-size: clamp(28px, 4.4vw, 56px);
		line-height: 1.08;
		letter-spacing: -0.04em;
		overflow-wrap: anywhere;
	}

	.contact-notes {
		padding-bottom: 0;
		gap: 18px;
	}

	.contact-notes h2 {
		margin-top: 10px;
	}

	@media (max-width: 900px) {
		.contact-layout {
			grid-template-columns: 1fr;
		}
	}
</style>
