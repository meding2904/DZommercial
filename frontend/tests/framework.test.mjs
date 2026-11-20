import assert from 'node:assert/strict';
import test from 'node:test';
import {
	DEFAULT_THEME,
	clampNumber,
	contrastRatio,
	fontStack,
	fontsFor,
	themeCss
} from '../src/lib/theme/storefront.ts';

test('the shipped theme passes the documented contrast floor', () => {
	assert.ok(contrastRatio(DEFAULT_THEME.light.ink, DEFAULT_THEME.light.ground) >= 7);
	assert.ok(contrastRatio(DEFAULT_THEME.light.muted, DEFAULT_THEME.light.ground) >= 4.5);
	assert.ok(contrastRatio(DEFAULT_THEME.light.ink, DEFAULT_THEME.light.surface) >= 4.5);
	assert.ok(contrastRatio(DEFAULT_THEME.light.accent, DEFAULT_THEME.light.ground) >= 4.5);
	assert.ok(contrastRatio(DEFAULT_THEME.light.accent_ink, DEFAULT_THEME.light.accent) >= 4.5);
	assert.ok(contrastRatio(DEFAULT_THEME.dark.ink, DEFAULT_THEME.dark.ground) >= 7);
});

test('theme tokens are written for both schemes', () => {
	const css = themeCss(DEFAULT_THEME);
	assert.match(css, /--p-bg: #f6f0eb/);
	assert.match(css, /--p-bg: #171311/);
	assert.match(css, /--font-display:/);
	assert.match(css, /--text-base: 16\.00px/);
	assert.match(css, /:root\[data-store-theme='dark'\]/);
});

test('unknown colours produce zero contrast, never a crash', () => {
	assert.equal(contrastRatio('red', '#ffffff'), 0);
	assert.equal(contrastRatio('#000000', 'not-a-colour'), 0);
});

test('font stacks only come from the allowlist', () => {
	const ids = new Set();
	for (const kind of ['display', 'body']) {
		for (const choice of fontsFor(kind)) {
			ids.add(choice.id);
			assert.ok(fontStack(choice.id, kind).length > 0);
		}
	}
	assert.deepEqual([...ids].sort(), ['fraunces', 'inter', 'schibsted', 'source_serif', 'system']);
	assert.ok(fontsFor('display').every((choice) => choice.id !== 'inter'));
	assert.ok(fontsFor('body').every((choice) => choice.id !== 'fraunces'));
});

test('numeric inputs are clamped, including NaN', () => {
	assert.equal(clampNumber(Number.NaN, 0, 12), 0);
	assert.equal(clampNumber(99, 0, 12), 12);
	assert.equal(clampNumber(-1, 0, 12), 0);
});

test('a non-default theme does not leak the default palette into light tokens', () => {
	const theme = structuredClone(DEFAULT_THEME);
	theme.light.ground = '#101010';
	theme.light.ink = '#ffffff';
	const css = themeCss(theme);
	assert.match(css, /--p-bg: #101010/);
	assert.match(css, /--p-ink: #ffffff/);
});

test('a colour that is not hex falls back instead of reaching the style element', () => {
	const theme = structuredClone(DEFAULT_THEME);
	theme.light.accent = 'red;}body{display:none}';
	const css = themeCss(theme);
	assert.match(css, /--p-accent: #9a3c4c/);
	assert.doesNotMatch(css, /display:none/);
});
