import { ADMIN_ACCESS_TOKEN, APP_ENV } from '$app/env/private';
import { gateConfigured, gateMatches } from './gate-policy';

export const GATE_COOKIE = 'azahar_ops';
export const GATE_PATH = `/ops-${ADMIN_ACCESS_TOKEN}`;
export const GATE_CONFIGURED = gateConfigured(APP_ENV, ADMIN_ACCESS_TOKEN);

type CookieReader = {
	get(name: string): string | undefined;
};

export function hasGate(cookies: CookieReader): boolean {
	return gateMatches(cookies.get(GATE_COOKIE), ADMIN_ACCESS_TOKEN, GATE_CONFIGURED);
}
