import { errorMessage, isAbortError } from '#lib/admin/api';
import { createLoader, type LoadOutcome, type ResourceState } from './resource';

export type ResourceOptions<T> = {
	// Screens that do not read on mount (an action, a sub-list) start settled.
	loading?: boolean;
	// Runs only for the request that wins: stale and aborted responses never see it.
	onLoaded?: (data: T) => void | Promise<void>;
};

export type Resource<T> = {
	readonly data: T | null;
	readonly loading: boolean;
	readonly error: string;
	load(): Promise<LoadOutcome<T>>;
	update(updater: (current: T) => T): void;
	reset(): void;
	abort(): void;
};

export function createResource<T>(
	fetcher: (signal: AbortSignal) => Promise<T>,
	options: ResourceOptions<T> = {}
): Resource<T> {
	const loader = createLoader(fetcher, {
		initialLoading: options.loading,
		isAbort: isAbortError,
		describeError: errorMessage,
		onLoaded: options.onLoaded
	});

	let snapshot = $state<ResourceState<T>>(loader.state);
	const stop = loader.subscribe(() => {
		snapshot = loader.state;
	});

	$effect(() => () => {
		stop();
		loader.destroy();
	});

	return {
		get data() {
			return snapshot.data;
		},
		get loading() {
			return snapshot.loading;
		},
		get error() {
			return snapshot.error;
		},
		load: loader.load,
		update: loader.update,
		reset: loader.reset,
		abort: loader.abort
	};
}
