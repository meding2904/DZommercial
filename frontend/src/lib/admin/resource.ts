// One request at a time, last write wins. Every admin load used to hand-roll
// this guard; the rune binding in resource.svelte.ts is a thin layer over it.

export type LoadOutcome<T> =
	| { status: 'ok'; data: T }
	| { status: 'discarded' }
	| { status: 'error'; error: unknown; message: string };

export type ResourceState<T> = {
	data: T | null;
	loading: boolean;
	error: string;
};

export type LoaderConfig<T> = {
	initialLoading?: boolean;
	isAbort: (error: unknown) => boolean;
	describeError: (error: unknown) => string;
	onLoaded?: (data: T) => void | Promise<void>;
};

export type Loader<T> = {
	readonly state: ResourceState<T>;
	load(): Promise<LoadOutcome<T>>;
	update(updater: (current: T) => T): void;
	reset(): void;
	abort(): void;
	destroy(): void;
	subscribe(listener: () => void): () => void;
};

export function createLoader<T>(
	fetcher: (signal: AbortSignal) => Promise<T>,
	config: LoaderConfig<T>
): Loader<T> {
	let state: ResourceState<T> = { data: null, loading: config.initialLoading ?? true, error: '' };
	let controller: AbortController | null = null;
	let attempt = 0;
	let destroyed = false;
	const listeners = new Set<() => void>();

	function publish(next: ResourceState<T>): void {
		state = next;
		for (const listener of [...listeners]) listener();
	}

	async function load(): Promise<LoadOutcome<T>> {
		if (destroyed) return { status: 'discarded' };
		const id = ++attempt;
		controller?.abort();
		const active = new AbortController();
		controller = active;
		publish({ data: null, loading: true, error: '' });

		try {
			const data = await fetcher(active.signal);
			if (id !== attempt || destroyed) return { status: 'discarded' };
			await config.onLoaded?.(data);
			if (id !== attempt || destroyed) return { status: 'discarded' };
			publish({ data, loading: false, error: '' });
			return { status: 'ok', data };
		} catch (error) {
			if (id !== attempt || destroyed) return { status: 'discarded' };
			// A transport that honours the signal rejects on abort; an abort is not
			// a failure, so neither the error state nor the outcome report one.
			if (active.signal.aborted || config.isAbort(error)) return { status: 'discarded' };
			const message = config.describeError(error);
			publish({ data: null, loading: false, error: message });
			return { status: 'error', error, message };
		}
	}

	function update(updater: (current: T) => T): void {
		if (destroyed || state.data === null) return;
		publish({ ...state, data: updater(state.data) });
	}

	function reset(): void {
		attempt += 1;
		controller?.abort();
		controller = null;
		publish({ data: null, loading: false, error: '' });
	}

	// Discards whatever is in flight without touching the data already on
	// screen. Bumping the attempt covers transports that resolve late even
	// after the signal aborts, and the loading state settles so the interface
	// cannot stay in a spinner forever.
	function abort(): void {
		attempt += 1;
		controller?.abort();
		controller = null;
		if (state.loading) publish({ ...state, loading: false });
	}

	function destroy(): void {
		destroyed = true;
		attempt += 1;
		controller?.abort();
		controller = null;
		listeners.clear();
	}

	function subscribe(listener: () => void): () => void {
		listeners.add(listener);
		return () => listeners.delete(listener);
	}

	return {
		get state() {
			return state;
		},
		load,
		update,
		reset,
		abort,
		destroy,
		subscribe
	};
}
