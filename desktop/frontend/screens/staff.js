// Staff screen: the team list and the role select an administrator can change.

import { state } from '../state.js';
import { call } from '../api.js';
import { captureRender } from '../stale.js';
import { setView } from '../shell.js';
import { nodes, delegate } from '../dom.js';
import { errorView, loadingView, pageHead } from '../views/common.js';
import { staffView } from '../views/staff.js';
import { markRoute, registerScreen } from '../router.js';
import { toast } from '../toast.js';

export async function renderStaff() {
	markRoute('staff');
	setView(loadingView('Reading the team…'));
	const fresh = captureRender(state);
	try {
		const data = await call('GET', '/api/admin/team');
		if (!fresh()) return;
		state.team = data.users || [];
		setView(staffView(state));
	} catch (error) {
		if (!fresh()) return;
		setView(
			pageHead('Staff', 'Who can open this ledger.') +
				errorView('The team could not be read.', error.message)
		);
	}
}

delegate(nodes.view, 'change', '[data-role-for]', async (event, target) => {
	const id = target.dataset.roleFor;
	const previous = state.team.find((member) => member.id === id)?.role;
	target.disabled = true;
	try {
		const data = await call('PATCH', `/api/admin/team/${encodeURIComponent(id)}`, {
			role: target.value
		});
		state.team = state.team.map((member) => (member.id === id ? data.user : member));
		toast(`${data.user.name} is now ${data.user.role}.`);
		setView(staffView(state));
	} catch (error) {
		if (previous) target.value = previous;
		target.disabled = false;
		toast(error.message || 'The role could not be changed.', 'error');
	}
});

registerScreen('staff', { render: renderStaff });
