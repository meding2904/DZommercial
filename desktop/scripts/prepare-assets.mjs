import { cp, mkdir, copyFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const desktop = path.resolve(here, '..');
const frontend = path.resolve(desktop, '..', 'frontend');
const resources = path.join(desktop, 'resources');

const fonts = [
	['schibsted-grotesk', 'schibsted-grotesk-latin-wght-normal.woff2'],
	['schibsted-grotesk', 'schibsted-grotesk-latin-ext-wght-normal.woff2'],
	['source-serif-4', 'source-serif-4-latin-wght-normal.woff2'],
	['source-serif-4', 'source-serif-4-latin-ext-wght-normal.woff2'],
	['fraunces', 'fraunces-latin-wght-normal.woff2'],
	['fraunces', 'fraunces-latin-ext-wght-normal.woff2'],
	['inter', 'inter-latin-wght-normal.woff2'],
	['inter', 'inter-latin-ext-wght-normal.woff2'],
	['jetbrains-mono', 'jetbrains-mono-latin-wght-normal.woff2'],
	['jetbrains-mono', 'jetbrains-mono-latin-ext-wght-normal.woff2']
];

const images = path.join(frontend, 'static', 'images');
if (!existsSync(images)) {
	console.error('frontend/static/images is missing. Run pnpm install in frontend, then retry.');
	process.exit(1);
}

await mkdir(path.join(resources, 'fonts'), { recursive: true });
await cp(images, path.join(resources, 'images'), { recursive: true });

for (const [packageName, file] of fonts) {
	const source = path.join(
		frontend,
		'node_modules',
		'@fontsource-variable',
		packageName,
		'files',
		file
	);
	if (!existsSync(source)) {
		console.error(`Missing font file ${source}. Run pnpm install in frontend, then retry.`);
		process.exit(1);
	}
	await copyFile(source, path.join(resources, 'fonts', file));
}

console.log(`Copied images and ${fonts.length} font files into desktop/resources.`);
