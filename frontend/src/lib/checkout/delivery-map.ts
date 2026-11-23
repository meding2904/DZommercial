// Delivery map: the wilaya, commune and stop desk lists checkout chooses from,
// plus the cascading resets a change of wilaya or delivery type causes.
// Self-contained on purpose: the storefront never imports the admin resource
// module, so this carries its own one-request-at-a-time guard.

import type { DeliveryType } from './order-validation';
import type { Commune, StopDesk, Wilaya } from '../types';

// The transport both checkout modules take: a path, an optional abort signal,
// and the JSON body the API returned.
export type Request = (path: string, signal?: AbortSignal) => Promise<unknown>;

export type MapStatus = 'idle' | 'loading' | 'ready' | 'error';

export type Option = { value: string; label: string };

export type DeliveryMapState = {
	wilayas: Wilaya[];
	communes: Commune[];
	desks: StopDesk[];
	wilayaStatus: MapStatus;
	communeStatus: MapStatus;
	deskStatus: MapStatus;
	wilayaError: string;
	communeError: string;
	deskError: string;
	wilayaCode: string;
	communeName: string;
	deliveryType: DeliveryType;
	stopDeskId: string;
};

export type MapSelection = {
	wilayaCode: string;
	communeName: string;
	deliveryType: DeliveryType;
	stopDeskId: string;
};

export type DeliveryMap = {
	readonly state: DeliveryMapState;
	load(): Promise<void>;
	chooseWilaya(value: string): void;
	chooseCommune(value: string): void;
	chooseDeliveryType(next: DeliveryType): void;
	chooseDesk(value: string): void;
	retryCommunes(): void;
	retryDesks(): void;
	selection(): MapSelection;
	readonly wilayaOptions: Option[];
	readonly communeOptions: Option[];
	readonly deskOptions: Option[];
	readonly selectedDesk: StopDesk | undefined;
	readonly communeLocked: boolean;
	destroy(): void;
};

export function createDeliveryMapState(): DeliveryMapState {
	return {
		wilayas: [],
		communes: [],
		desks: [],
		wilayaStatus: 'loading',
		communeStatus: 'idle',
		deskStatus: 'idle',
		wilayaError: '',
		communeError: '',
		deskError: '',
		wilayaCode: '',
		communeName: '',
		deliveryType: 'home',
		stopDeskId: ''
	};
}

type Guard = {
	start(): { id: number; signal: AbortSignal };
	current(id: number): boolean;
	cancel(): void;
};

// One last-write-wins guard, one per list. A newer request aborts the previous
// one and bumps the attempt; a response that lost the race is dropped even when
// the transport ignores the abort and resolves late.
function createGuard(): Guard {
	let attempt = 0;
	let controller: AbortController | null = null;

	return {
		start() {
			const id = ++attempt;
			controller?.abort();
			const active = new AbortController();
			controller = active;
			return { id, signal: active.signal };
		},
		current(id) {
			return id === attempt;
		},
		cancel() {
			attempt += 1;
			controller?.abort();
			controller = null;
		}
	};
}

export function createDeliveryMap(
	state: DeliveryMapState,
	{ request }: { request: Request }
): DeliveryMap {
	const wilayas = createGuard();
	const communes = createGuard();
	const desks = createGuard();

	function selectedDesk(): StopDesk | undefined {
		return state.desks.find((desk) => desk.id === state.stopDeskId);
	}

	async function loadWilayas(): Promise<void> {
		state.wilayaStatus = 'loading';
		state.wilayaError = '';
		const { id, signal } = wilayas.start();
		try {
			const data = (await request('/shipping/wilayas', signal)) as { wilayas: Wilaya[] };
			if (signal.aborted || !wilayas.current(id)) return;
			state.wilayas = data.wilayas;
			state.wilayaStatus = 'ready';
			if (state.wilayaCode) void loadCommunes(Number(state.wilayaCode));
		} catch {
			if (signal.aborted || !wilayas.current(id)) return;
			state.wilayaStatus = 'error';
			state.wilayaError =
				'The delivery map could not be read. Check your connection and try again.';
		}
	}

	async function loadCommunes(wilayaCode: number): Promise<void> {
		const { id, signal } = communes.start();
		state.communeStatus = 'loading';
		state.communeError = '';
		state.communes = [];
		try {
			const data = (await request(
				`/shipping/communes?wilaya=${wilayaCode}`,
				signal
			)) as { communes: Commune[] };
			if (signal.aborted || !communes.current(id)) return;
			state.communes = data.communes;
			state.communeStatus = 'ready';
		} catch {
			if (signal.aborted || !communes.current(id)) return;
			state.communeStatus = 'error';
			state.communeError = 'The communes could not be read. Try again.';
		}
	}

	async function loadDesks(wilayaCode: number): Promise<void> {
		const { id, signal } = desks.start();
		state.deskStatus = 'loading';
		state.deskError = '';
		state.desks = [];
		try {
			const data = (await request(`/shipping/desks?wilaya=${wilayaCode}`, signal)) as {
				desks: StopDesk[];
			};
			if (signal.aborted || !desks.current(id)) return;
			state.desks = data.desks;
			state.deskStatus = 'ready';
		} catch {
			if (signal.aborted || !desks.current(id)) return;
			state.deskStatus = 'error';
			state.deskError = 'The stop desks could not be read. Try again, or choose home delivery.';
		}
	}

	function chooseWilaya(value: string): void {
		state.wilayaCode = value;
		state.communeName = '';
		state.stopDeskId = '';
		state.communes = [];
		state.desks = [];
		state.communeStatus = 'idle';
		state.deskStatus = 'idle';
		// A commune or desk from the previous wilaya must not land here.
		communes.cancel();
		desks.cancel();

		const code = Number(value);
		if (!code) return;
		void loadCommunes(code);
		if (state.deliveryType === 'stopdesk') void loadDesks(code);
	}

	function chooseCommune(value: string): void {
		state.communeName = value;
	}

	function chooseDeliveryType(next: DeliveryType): void {
		state.deliveryType = next;
		state.stopDeskId = '';
		state.desks = [];
		state.deskStatus = 'idle';
		desks.cancel();

		if (next === 'stopdesk' && state.wilayaCode) {
			void loadDesks(Number(state.wilayaCode));
		}
	}

	// The desk carries its commune, so choosing one chooses both.
	function chooseDesk(value: string): void {
		state.stopDeskId = value;
		const desk = selectedDesk();
		if (desk?.communeName) state.communeName = desk.communeName;
	}

	function retryCommunes(): void {
		if (!state.wilayaCode) return;
		void loadCommunes(Number(state.wilayaCode));
	}

	function retryDesks(): void {
		if (!state.wilayaCode) return;
		void loadDesks(Number(state.wilayaCode));
	}

	return {
		get state() {
			return state;
		},
		load: loadWilayas,
		chooseWilaya,
		chooseCommune,
		chooseDeliveryType,
		chooseDesk,
		retryCommunes,
		retryDesks,
		selection() {
			return {
				wilayaCode: state.wilayaCode,
				communeName: state.communeName,
				deliveryType: state.deliveryType,
				stopDeskId: state.stopDeskId
			};
		},
		get wilayaOptions() {
			if (state.wilayaStatus === 'loading') return [{ value: '', label: 'Reading wilayas…' }];
			if (state.wilayaStatus === 'error') return [{ value: '', label: 'Wilayas unavailable' }];
			return [
				{ value: '', label: 'Choose a wilaya' },
				...state.wilayas.map((wilaya) => ({ value: String(wilaya.code), label: wilaya.nameFr }))
			];
		},
		get communeOptions() {
			if (!state.wilayaCode) return [{ value: '', label: 'Choose a wilaya first' }];
			if (state.communeStatus === 'loading') return [{ value: '', label: 'Reading communes…' }];
			if (state.communeStatus === 'error') return [{ value: '', label: 'Communes unavailable' }];
			return [
				{ value: '', label: 'Choose a commune' },
				...state.communes.map((commune) => ({ value: commune.nameFr, label: commune.nameFr }))
			];
		},
		get deskOptions() {
			if (!state.wilayaCode) return [{ value: '', label: 'Choose a wilaya first' }];
			if (state.deskStatus === 'loading') return [{ value: '', label: 'Reading stop desks…' }];
			if (state.deskStatus === 'error') return [{ value: '', label: 'Stop desks unavailable' }];
			if (state.desks.length === 0) {
				return [{ value: '', label: 'No stop desks in this wilaya' }];
			}
			return [
				{ value: '', label: 'Choose a stop desk' },
				...state.desks.map((desk) => ({
					value: desk.id,
					label: `${desk.name}${desk.communeName ? `, ${desk.communeName}` : ''}`
				}))
			];
		},
		get selectedDesk() {
			return selectedDesk();
		},
		get communeLocked() {
			return state.deliveryType === 'stopdesk' && Boolean(selectedDesk()?.communeName);
		},
		destroy() {
			wilayas.cancel();
			communes.cancel();
			desks.cancel();
		}
	};
}
