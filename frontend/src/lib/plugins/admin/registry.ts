import type { Component } from 'svelte';
import type { AdminPlugin } from '#lib/types';
import AiAdmin from './AiAdmin.svelte';
import CodeModeAdmin from './CodeModeAdmin.svelte';
import PhoneAdmin from './PhoneAdmin.svelte';

export const pluginAdmin: Record<string, Component<{ plugin: AdminPlugin }>> = {
	phone: PhoneAdmin,
	ai: AiAdmin,
	codemode: CodeModeAdmin
};

export function adminComponentFor(id: string): Component<{ plugin: AdminPlugin }> | null {
	return pluginAdmin[id] ?? null;
}
