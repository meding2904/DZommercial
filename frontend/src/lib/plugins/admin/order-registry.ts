import type { Component } from 'svelte';
import type { AdminOrder } from '#lib/admin/types';
import PhoneOrderPanel from './PhoneOrderPanel.svelte';

export const orderPanels: Record<string, Component<{ order: AdminOrder }>> = {
	phone: PhoneOrderPanel
};

export function orderPanelFor(pluginId: string): Component<{ order: AdminOrder }> | null {
	return orderPanels[pluginId] ?? null;
}
