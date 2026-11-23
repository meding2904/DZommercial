// Shared view fragments. Pure functions from arguments to HTML strings.

import { escapeHtml } from '../format.js';
import { titleCase } from '../domain/labels.js';

/**
 * @param {string} [label]
 * @returns {string}
 */
export function loadingView(label = 'Reading the ledger…') {
	return `<div class="loading"><span class="spinner" aria-hidden="true"></span><span>${escapeHtml(label)}</span></div>`;
}

/**
 * @param {string} title
 * @param {string} message
 * @param {string} [retry]
 * @returns {string}
 */
export function errorView(title, message, retry = '') {
	return `<div class="empty"><p class="meta">Out of reach</p><h2>${escapeHtml(title)}</h2><p>${escapeHtml(message)}</p>${retry}</div>`;
}

/**
 * @param {string} title
 * @param {string} sub
 * @param {string} [actions]
 * @returns {string}
 */
export function pageHead(title, sub, actions = '') {
	return `<header class="page-head"><div><h1>${escapeHtml(title)}</h1><p class="page-sub">${sub}</p></div>${actions}</header>`;
}

/**
 * @param {string} status
 * @returns {string}
 */
export function statusTag(status) {
	return `<span class="status" data-status="${escapeHtml(status)}">${escapeHtml(titleCase(status))}</span>`;
}

/**
 * @param {string} src
 * @param {string} alt
 * @param {{ width?: number, height?: number }} [options]
 * @returns {string}
 */
export function imageTag(src, alt, options = {}) {
	if (!src) return '';
	return `<img data-image="${escapeHtml(src)}" alt="${escapeHtml(alt || '')}" width="${options.width || 400}" height="${options.height || 500}" loading="lazy" />`;
}
