<script lang="ts">
	import type { StorefrontTheme } from '#lib/theme/storefront';
	import { DEFAULT_THEME } from '#lib/theme/storefront';

	let { theme = DEFAULT_THEME }: { theme?: StorefrontTheme } = $props();
</script>

<footer class="closing">
	<div class="wrap">
		<p class="closing-word" aria-hidden="true">{theme.brand_name}</p>

		<div class="closing-grid">
			<div>
				<p class="closing-note">{theme.footer_note}</p>
				<p class="meta" style="margin-top: 18px;">
					<a href={`mailto:${theme.contact_email}`}>{theme.contact_email}</a>
				</p>
			</div>

			<div class="footer-group">
				<h3>Shop</h3>
				<a href="/shop">{theme.shop_title}</a>
				<a href="/about">{theme.house_label}</a>
				<a href="/contact">Contact</a>
			</div>

			<div class="footer-group">
				<h3>Practical</h3>
				<a href="/policies/shipping">Shipping</a>
				<a href="/policies/returns">Returns</a>
				<a href="/policies/privacy">Privacy</a>
				<a href="/policies/terms">Terms of service</a>
				<a href="/credits">Photography credits</a>
			</div>
		</div>

		<div class="footer-base">
			<span>© 2026 {theme.brand_name}.</span>
			<span>{theme.footer_credit}</span>
		</div>
	</div>
</footer>
