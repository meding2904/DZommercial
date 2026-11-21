<script lang="ts">
	import { onMount } from 'svelte';
	import { adminApi, errorMessage, isAbortError } from '#lib/admin/api';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import { formatDate } from '#lib/format';
	import { auth } from '#lib/stores/auth.svelte';
	import type { Role, TeamMember } from '#lib/types';

	const roles: { value: Role; label: string }[] = [
		{ value: 'admin', label: 'Administrator' },
		{ value: 'employee', label: 'Employee' },
		{ value: 'customer', label: 'Customer' }
	];

	let users = $state<TeamMember[]>([]);
	let loading = $state(true);
	let error = $state('');
	let notice = $state('');
	let roleError = $state('');
	let saving = $state('');

	let controller: AbortController | null = null;
	let requestId = 0;

	onMount(() => {
		void load();
		return () => controller?.abort();
	});

	async function load(): Promise<void> {
		const id = ++requestId;
		controller?.abort();
		controller = new AbortController();
		loading = true;
		error = '';
		try {
			const data = await adminApi.team(controller.signal);
			if (id !== requestId) return;
			users = data.users;
		} catch (caught) {
			if (id !== requestId || isAbortError(caught)) return;
			error = errorMessage(caught);
		} finally {
			if (id === requestId) loading = false;
		}
	}

	async function changeRole(user: TeamMember, role: Role): Promise<void> {
		if (role === user.role || saving) return;
		roleError = '';
		notice = '';
		saving = user.id;
		try {
			const data = await adminApi.updateTeamRole(user.id, role);
			users = users.map((item) => (item.id === user.id ? data.user : item));
			notice = `${data.user.name} is now ${roles.find((item) => item.value === data.user.role)?.label.toLowerCase() ?? data.user.role}.`;
		} catch (caught) {
			roleError = errorMessage(caught);
		} finally {
			saving = '';
		}
	}

	function roleLabel(role: Role): string {
		return roles.find((item) => item.value === role)?.label ?? role;
	}

	function isSelf(user: TeamMember): boolean {
		return auth.user?.id === user.id;
	}
</script>

<svelte:head>
	<title>Team · Azahar admin</title>
</svelte:head>

<div class="admin-page">
	<header class="a-page-head">
		<div class="a-page-title">
			<h1>Team</h1>
			<p class="a-page-sub">
				<span>Administrators have full control. Employees can read the catalogue and fulfil orders.</span>
			</p>
		</div>
	</header>

	{#if notice}
		<div class="a-notice a-notice-success" role="status">
			<p>{notice}</p>
			<button class="a-btn a-btn-quiet" type="button" onclick={() => (notice = '')}>
				Dismiss
			</button>
		</div>
	{/if}

	{#if roleError}
		<p class="a-form-error" role="alert">{roleError}</p>
	{/if}

	{#if loading}
		<LoadingState label="Reading the team…" />
	{:else if error}
		<ErrorState title="The team could not be read." message={error} onretry={load} />
	{:else}
		<ul class="team-list">
			{#each users as user (user.id)}
				<li class="a-panel team-member">
					<div class="team-identity">
						<strong>{user.name}{#if isSelf(user)} <span class="a-muted">(you)</span>{/if}</strong>
						<span>{user.email}</span>
						<span class="a-muted-sm">Added {formatDate(user.created_at)}</span>
					</div>

					<div class="team-role">
						<span class="a-label" id={`team-role-label-${user.id}`}>Role</span>
						<Dropdown
							id={`team-role-${user.id}`}
							class="admin-dropdown"
							label={`Role for ${user.name}`}
							labelId={`team-role-label-${user.id}`}
							value={user.role}
							options={roles}
							onchange={(value) => changeRole(user, value as Role)}
							disabled={saving !== '' || (isSelf(user) && user.role === 'admin')}
						/>
						{#if isSelf(user) && user.role === 'admin'}
							<p class="a-field-hint">Ask another administrator to change your role.</p>
						{:else}
							<p class="a-field-hint">{roleLabel(user.role)} access.</p>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.team-list {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 420px), 1fr));
		gap: 14px;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.team-member {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(180px, 220px);
		gap: 18px;
		align-items: start;
		padding: 18px 20px;
	}

	.team-identity {
		display: grid;
		gap: 3px;
		min-width: 0;
	}

	.team-identity strong {
		font-size: 15px;
		font-weight: 500;
		overflow-wrap: anywhere;
	}

	.team-identity span {
		color: var(--a-muted);
		font-size: 13px;
		overflow-wrap: anywhere;
	}

	.team-role {
		display: grid;
		gap: 6px;
	}

	@media (max-width: 560px) {
		.team-member {
			grid-template-columns: 1fr;
		}
	}
</style>
