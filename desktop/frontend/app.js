// Entry point loaded by index.html. It wires the modules that make up the
// renderer and starts the session; the screens register themselves through
// router.js when this module's imports evaluate.

import { api } from './bridge.js';
import { toast } from './toast.js';
import { applyTheme, initThemeControls } from './theme.js';
import { bootstrap, initAuth } from './auth.js';
import { route } from './router.js';
import { initDialog } from './shell.js';
import { initSignOut } from './session.js';
import { initLoginConnection } from './connection.js';
import { installNavigationGuards } from './navigation.js';
import './screens/ledger.js';
import './screens/counter.js';
import './screens/orders.js';
import './screens/products.js';
import './screens/customers.js';
import './screens/staff.js';
import './screens/connection.js';

initAuth();
initSignOut();
initThemeControls();
initDialog();
initLoginConnection({ onSaved: bootstrap });
installNavigationGuards({
	window,
	document,
	openExternal: (href) => api.openExternal(href),
	warn: (message) => toast(message, 'error')
});

window.addEventListener('hashchange', () => void route());

applyTheme();
void bootstrap();
