// Products and stock: the shelf table. Pure: state in, HTML out.

import { escapeHtml, money } from '../format.js';
import { imageTag, pageHead } from './common.js';

/**
 * @param {any[]} products
 * @param {{ query: string, lowOnly: boolean }} filters
 * @returns {any[]}
 */
export function filterProducts(products, { query, lowOnly }) {
	const needle = query.trim().toLowerCase();
	return products.filter((product) => {
		if (lowOnly && Number(product.stock) > 5) return false;
		if (!needle) return true;
		return `${product.name} ${product.slug} ${product.category?.name || ''}`
			.toLowerCase()
			.includes(needle);
	});
}

/**
 * @param {any} state
 * @returns {string}
 */
export function productsView(state) {
	const products = filterProducts(state.products, {
		query: state.productQuery,
		lowOnly: state.productLowOnly
	});
	const low = state.products.filter((product) => Number(product.stock) <= 5).length;
	return `<div>
		${pageHead('Products and stock', `${state.products.length} lines · ${low} at or below five units`, '')}
		<div class="toolbar">
			<div class="field field-grow"><label for="product-search">Search</label><input class="input" id="product-search" type="search" placeholder="Name, slug or category" value="${escapeHtml(state.productQuery)}" /></div>
			<label class="checkbox-row" style="min-height:40px;align-items:center"><input type="checkbox" id="low-only"${state.productLowOnly ? ' checked' : ''} /> Low stock only</label>
		</div>
		${
			products.length
				? `<div class="table-wrap"><table>
						<thead><tr><th>Product</th><th>Category</th><th class="numeric">Price</th><th>Stock</th><th>State</th><th></th></tr></thead>
						<tbody>${products
							.map(
								(product) => (() => {
									const imeiStock = product.plugin_id === 'phone';
									return `<tr data-product-row="${escapeHtml(product.id)}" data-clickable="true">
									<td><div class="product-cell"><span class="product-thumb">${imageTag(product.image, product.name, { width: 72, height: 88 })}</span><span><span class="cell-strong">${escapeHtml(product.name)}</span><span class="cell-sub">${escapeHtml(product.slug)}</span></span></div></td>
									<td>${escapeHtml(product.category?.name || '')}</td>
									<td class="numeric">${money(product.effective_price_cents ?? product.price_cents)}${product.on_sale ? `<span class="cell-sub">was ${money(product.price_cents)}</span>` : ''}</td>
									<td><div class="inline-stock" data-stock-control="${escapeHtml(product.id)}" data-clickable="false"><button type="button" data-stock-down="${escapeHtml(product.id)}" aria-label="Reduce stock for ${escapeHtml(product.name)}"${state.user.role === 'admin' && !imeiStock ? '' : ' disabled'}>−</button><output class="${Number(product.stock) === 0 ? 'stock-out' : Number(product.stock) <= 5 ? 'stock-low' : ''}" title="${imeiStock ? 'Stock follows the registered IMEIs' : ''}">${product.stock}</output><button type="button" data-stock-up="${escapeHtml(product.id)}" aria-label="Add stock for ${escapeHtml(product.name)}"${state.user.role === 'admin' && !imeiStock ? '' : ' disabled'}>+</button></div>${imeiStock ? '<span class="cell-sub">IMEI stock</span>' : ''}</td>
									<td>${product.active ? '<span class="status" data-status="delivered">Active</span>' : '<span class="status" data-status="cancelled">Archived</span>'}</td>
									<td><button class="button button-secondary button-small" type="button" data-edit-product="${escapeHtml(product.id)}">${state.user.role === 'admin' ? 'Edit' : 'View'}</button></td>
								</tr>`;
								})()
							)
							.join('')}</tbody>
					</table></div>`
				: `<div class="empty"><h2>No product in this view.</h2><p>Clear the filter or the search.</p></div>`
		}
	</div>`;
}
