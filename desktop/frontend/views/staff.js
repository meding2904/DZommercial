// Staff: who can open the ledger, and their role. Pure: state in, HTML out.

import { escapeHtml, formatDate } from '../format.js';
import { titleCase } from '../domain/labels.js';
import { pageHead } from './common.js';

/**
 * @param {{ user: any, team: any[] }} state
 * @returns {string}
 */
export function staffView(state) {
	const admin = state.user.role === 'admin';
	return `<div>
		${pageHead('Staff', 'Administrators run the shop. Employees sell and fulfil orders.')}
		<div class="table-wrap"><table><thead><tr><th>Name</th><th>Email</th><th>Role</th><th>Joined</th></tr></thead><tbody>${state.team
			.map(
				(member) => `<tr><td class="cell-strong">${escapeHtml(member.name)}${member.id === state.user.id ? '<span class="cell-sub">You</span>' : ''}</td><td>${escapeHtml(member.email)}</td><td>${
					admin && member.id !== state.user.id
						? `<select class="select" data-role-for="${escapeHtml(member.id)}" aria-label="Role for ${escapeHtml(member.name)}">${['admin', 'employee', 'customer']
								.map(
									(role) =>
										`<option value="${role}"${member.role === role ? ' selected' : ''}>${titleCase(role)}</option>`
								)
								.join('')}</select>`
						: `<span class="status" data-status="${member.role === 'admin' ? 'confirmed' : member.role === 'employee' ? 'packed' : 'cancelled'}">${titleCase(member.role)}</span>`
				}</td><td class="meta">${formatDate(member.created_at)}</td></tr>`
			)
			.join('')}</tbody></table></div>
	</div>`;
}
