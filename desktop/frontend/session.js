// Session chrome: the login screen, the signed-in shell, and the two ways out
// (sign out and the connection page's clear session). Both exits drop the
// transient state so a previous account cannot repaint the view.

import { api } from './bridge.js';
import { call } from './api.js';
import { state, resetTransientState } from './state.js';
import { nodes } from './dom.js';
import { toast } from './toast.js';

/**
 * @param {string} [message]
 */
export function showLogin(message = '') {
	nodes.boot.hidden = true;
	nodes.adminShell.hidden = true;
	nodes.loginScreen.hidden = false;
	// A successful sign-in disables the button; signing out must re-enable it,
	// or the screen has no way back in without a restart.
	const submit = document.getElementById('login-submit');
	if (submit) submit.disabled = false;
	const error = document.getElementById('login-error');
	if (message) {
		error.textContent = message;
		error.hidden = false;
	} else {
		error.hidden = true;
	}
	document.getElementById('login-email').focus();
}

export function showApp() {
	nodes.boot.hidden = true;
	nodes.loginScreen.hidden = true;
	nodes.adminShell.hidden = false;
	document.getElementById('rail-user-name').textContent = state.user.name;
	document.getElementById('rail-user-role').textContent =
		state.user.role === 'admin' ? 'Administrator' : 'Employee';
}

// Clears the saved session and returns to the login screen.
export async function clearLocalSession() {
	try {
		await api.clearSession();
	} catch (error) {
		toast(error.message || 'The local session could not be cleared.', 'error');
		return false;
	}
	state.user = null;
	resetTransientState();
	showLogin('The local session is cleared.');
	return true;
}

export async function signOut() {
	let serverConfirmed = true;
	try {
		await call('POST', '/api/auth/logout');
	} catch {
		serverConfirmed = false;
	}
	let localCleared = true;
	try {
		await api.clearSession();
	} catch (error) {
		localCleared = false;
		toast(error.message || 'The local session could not be cleared.', 'error');
	}
	state.user = null;
	resetTransientState();
	if (!localCleared) {
		showLogin(
			'Signed out here, but the saved session could not be removed. Sign in again and use Clear session.'
		);
		return;
	}
	if (!serverConfirmed) {
		showLogin('Signed out on this device. The server did not confirm the logout.');
		return;
	}
	showLogin();
}

export function initSignOut() {
	document.getElementById('sign-out').addEventListener('click', () => void signOut());
}
