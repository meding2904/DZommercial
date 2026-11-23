import assert from 'node:assert/strict';
import test from 'node:test';
import { createLoader } from '../src/lib/admin/resource.ts';

const isAbort = (error) => error instanceof Error && error.name === 'AbortError';
const describeError = (error) => (error instanceof Error ? error.message : 'Something went wrong.');

function abortError() {
	const error = new Error('The operation was aborted.');
	error.name = 'AbortError';
	return error;
}

// A transport the test drives by hand: every call stays pending until the test
// resolves or rejects it, so responses can arrive out of order.
function transport({ honourAbort = true } = {}) {
	const calls = [];
	function fetch(request, signal) {
		return new Promise((resolve, reject) => {
			calls.push({ request, signal, resolve, reject });
			if (honourAbort) {
				signal.addEventListener('abort', () => reject(abortError()), { once: true });
			}
		});
	}
	return { calls, fetch };
}

function loaderFor(request, wiring, config = {}) {
	return createLoader((signal) => wiring.fetch(request, signal), {
		describeError,
		isAbort,
		...config
	});
}

async function verifyLoader(screen, request) {
	// A response that lost the race is discarded, even when the transport
	// ignores the abort signal and resolves it late.
	{
		const wiring = transport({ honourAbort: false });
		const loader = loaderFor(request, wiring);
		const first = loader.load();
		const second = loader.load();
		const fresh = { request, version: 'fresh' };
		wiring.calls[1].resolve(fresh);
		assert.equal((await second).status, 'ok', `${screen}: the newer request settles`);
		wiring.calls[0].resolve({ request, version: 'stale' });
		assert.equal((await first).status, 'discarded', `${screen}: the stale response is discarded`);
		assert.deepEqual(loader.state.data, fresh, `${screen}: the fresh data survives`);
		assert.equal(loader.state.loading, false, `${screen}: loading settles once`);
		assert.equal(loader.state.error, '', `${screen}: a stale response is not an error`);
		loader.destroy();
	}

	// Aborting the previous request is not a failure.
	{
		const wiring = transport();
		const loader = loaderFor(request, wiring);
		const first = loader.load();
		const second = loader.load();
		assert.equal(wiring.calls[0].signal.aborted, true, `${screen}: the previous request aborts`);
		assert.equal((await first).status, 'discarded', `${screen}: the aborted request is swallowed`);
		assert.equal(loader.state.error, '', `${screen}: an abort does not surface an error`);
		wiring.calls[1].resolve({ request });
		assert.equal((await second).status, 'ok');
		assert.equal(loader.state.error, '');
		loader.destroy();
	}

	// One failure, one message, one error state.
	{
		const wiring = transport();
		let described = 0;
		const loader = createLoader((signal) => wiring.fetch(request, signal), {
			describeError: (error) => {
				described += 1;
				return describeError(error);
			},
			isAbort
		});
		let surfaced = 0;
		loader.subscribe(() => {
			if (loader.state.error) surfaced += 1;
		});
		const pending = loader.load();
		wiring.calls[0].reject(new Error('the shop API is not answering'));
		const outcome = await pending;
		assert.equal(outcome.status, 'error', `${screen}: the failure is reported to the caller`);
		assert.equal(loader.state.error, 'the shop API is not answering');
		assert.equal(loader.state.loading, false, `${screen}: loading stops on failure`);
		assert.equal(described, 1, `${screen}: the error is described once`);
		assert.equal(surfaced, 1, `${screen}: the error is surfaced once`);
		loader.destroy();
	}

	// Retry runs the same load again and clears the error on success.
	{
		const wiring = transport();
		const loader = loaderFor(request, wiring);
		const first = loader.load();
		wiring.calls[0].reject(new Error('try again'));
		await first;
		const retry = loader.load();
		assert.equal(wiring.calls.length, 2, `${screen}: retry reaches the transport again`);
		assert.equal(wiring.calls[1].request, request, `${screen}: retry reuses the same request`);
		wiring.calls[1].resolve({ request, retried: true });
		assert.equal((await retry).status, 'ok');
		assert.deepEqual(loader.state.data, { request, retried: true });
		assert.equal(loader.state.error, '', `${screen}: a successful retry clears the error`);
		loader.destroy();
	}

	// Unmount stops the request and freezes what the page can see.
	{
		const wiring = transport({ honourAbort: false });
		const loader = loaderFor(request, wiring);
		let publications = 0;
		loader.subscribe(() => {
			publications += 1;
		});
		const pending = loader.load();
		const beforeCleanup = publications;
		loader.destroy();
		assert.equal(wiring.calls[0].signal.aborted, true, `${screen}: cleanup aborts the request`);
		wiring.calls[0].resolve({ request, arrived: 'after unmount' });
		assert.equal((await pending).status, 'discarded', `${screen}: a late response is dropped`);
		assert.equal(loader.state.data, null, `${screen}: cleanup leaves no data behind`);
		assert.equal(loader.state.error, '', `${screen}: cleanup surfaces no error`);
		assert.equal(publications, beforeCleanup, `${screen}: a destroyed resource stops publishing`);
	}
}

test('the loader contract holds for a list load and an action load', async () => {
	await verifyLoader('the list', '/admin/orders?status=&q=');
	await verifyLoader('the action', '/plugins/codemode/admin/apply');
});

test('abort settles loading, keeps idle data and discards the in-flight load', async () => {
	const wiring = transport({ honourAbort: false });
	const loader = loaderFor('/admin/metrics', wiring);

	const first = loader.load();
	wiring.calls[0].resolve({ metrics: { revenue_cents: 1 } });
	await first;
	loader.abort();
	assert.equal(loader.state.loading, false, 'abort while idle is a no-op');
	assert.deepEqual(loader.state.data, { metrics: { revenue_cents: 1 } }, 'idle abort keeps the data');

	const second = loader.load();
	assert.equal(loader.state.loading, true);
	loader.abort();
	assert.equal(loader.state.loading, false, 'abort cannot leave a spinner behind');
	assert.equal(loader.state.data, null, 'a new load clears the previous data by design');

	wiring.calls[1].resolve({ metrics: { revenue_cents: 2 } });
	assert.equal((await second).status, 'discarded');
	assert.equal(loader.state.data, null);
	assert.equal(loader.state.error, '');
	loader.destroy();
	loader.destroy();
});

test('several loads on one screen do not abort each other', async () => {
	const wiring = transport();
	const runs = loaderFor('/plugins/ai/admin/runs?limit=10', wiring);
	const products = loaderFor('/admin/products', wiring);
	const runsPending = runs.load();
	const productsPending = products.load();
	assert.equal(wiring.calls[0].signal.aborted, false);
	assert.equal(wiring.calls[1].signal.aborted, false);
	wiring.calls[1].resolve({ products: [] });
	wiring.calls[0].resolve({ runs: [] });
	await Promise.all([runsPending, productsPending]);
	assert.deepEqual(runs.state.data, { runs: [] });
	assert.deepEqual(products.state.data, { products: [] });
	runs.destroy();
	products.destroy();
});
