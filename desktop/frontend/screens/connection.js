// Connection screen: renders the form, tests the server and clears the saved
// session. The address input and firewall help are delegated on the stable view
// so a typed address survives every repaint of the page.

import { state } from '../state.js';
import { setView } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { connectionView } from '../views/connection.js';
import { markRoute, registerScreen } from '../router.js';
import {
	applyConnectionValues,
	onConnectionAddressInput,
	saveConnection,
	testConnection,
	toggleFirewallHelp
} from '../connection.js';
import { clearLocalSession } from '../session.js';
import { bootstrap } from '../auth.js';
import { toast } from '../toast.js';

export function renderConnection() {
	markRoute('connection');
	setView(connectionView(state));
	applyConnectionValues('app', state.info);
}

delegate(nodes.view, 'input', '#app-api', () => onConnectionAddressInput('app'));

delegate(nodes.view, 'click', '#app-firewall', (event, target) => {
	const guide = document.getElementById('app-firewall-guide');
	if (guide) void toggleFirewallHelp(target, guide);
});

delegate(nodes.view, 'submit', '#app-connection-form', async (event) => {
	event.preventDefault();
	const errorBox = document.getElementById('app-connection-error');
	if (!(await saveConnection('app', errorBox))) return;
	toast('Server address saved.');
	await bootstrap();
	location.hash = '#/connection';
});

delegate(nodes.view, 'click', '#app-test', () => void testConnection());
delegate(nodes.view, 'click', '#app-clear-session', () => void clearLocalSession());

registerScreen('connection', { render: renderConnection });
