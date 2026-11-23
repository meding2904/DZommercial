<script lang="ts">
	import { onDestroy, onMount, tick } from 'svelte';
	import { ApiError, apiFetch } from '#lib/api';
	import { errorMessage } from '#lib/admin/api';
	import { createResource } from '#lib/admin/resource.svelte';
	import Dropdown from '#lib/components/Dropdown.svelte';
	import ErrorState from '#lib/components/admin/ErrorState.svelte';
	import LoadingState from '#lib/components/admin/LoadingState.svelte';
	import type { AdminPlugin } from '#lib/types';

	let { plugin }: { plugin: AdminPlugin } = $props();

	type WhatsAppStatus = {
		enabled: boolean;
		configured: boolean;
		settings_error?: string;
		webhook_url: string;
		phone_number_id: string;
		graph_version: string;
		order_template: string;
		order_template_language: string;
		status_template: string;
		status_template_language: string;
		counts: Record<string, number>;
		contacts: number;
		opted_out: number;
		last_webhook_at: string;
		last_failure: string;
		last_failure_at: string;
		reply_window_hours: number;
	};

	type WhatsAppMessage = {
		id: string;
		direction: 'in' | 'out';
		kind: string;
		phone: string;
		contact_name: string;
		body: string;
		template_name: string;
		template_language: string;
		template_params: string[];
		status: string;
		error: string;
		meta_message_id: string;
		order_id: string;
		order_number: string;
		contact_opted_in: boolean;
		attempts: number;
		created_at: string;
		updated_at: string;
	};

	type WhatsAppContact = {
		phone: string;
		name: string;
		opted_in: boolean;
		last_inbound_at: string;
		last_message_at: string;
		reply_open: boolean;
		created_at: string;
		updated_at: string;
	};

	const status = createResource(
		(signal) => apiFetch<WhatsAppStatus>('/plugins/whatsapp/admin/status', { signal }),
		{ loading: false }
	);
	const messageList = createResource(
		(signal) =>
			apiFetch<{ messages: WhatsAppMessage[] }>(
				`/plugins/whatsapp/admin/messages?limit=40&status=${encodeURIComponent(statusFilter)}`,
				{ signal }
			),
		{ loading: false }
	);
	const contactList = createResource(
		(signal) =>
			apiFetch<{ contacts: WhatsAppContact[] }>('/plugins/whatsapp/admin/contacts?limit=20', {
				signal
			}),
		{ loading: false }
	);

	const connection = $derived(status.data);
	const messages = $derived(messageList.data?.messages ?? []);
	const contacts = $derived(contactList.data?.contacts ?? []);
	const messagesLoading = $derived(messageList.loading);
	const messagesError = $derived(messageList.error);
	const contactsLoading = $derived(contactList.loading);
	const contactsError = $derived(contactList.error);

	let kind = $state<'text' | 'template'>('text');
	let phone = $state('');
	let text = $state('');
	let templateName = $state('');
	let templateLanguage = $state('en');
	let parameters = $state('');
	let replyTo = $state('');
	let fieldErrors = $state<Record<string, string>>({});
	let formError = $state('');
	let notice = $state('');
	let copyNotice = $state('');
	let busy = $state(false);
	let retrying = $state('');
	let contactBusy = $state('');
	let contactError = $state('');
	let statusFilter = $state('');
	let formEl = $state<HTMLFormElement | null>(null);

	const kindOptions = [
		{ value: 'text', label: 'Text reply' },
		{ value: 'template', label: 'Approved template' }
	];
	const statusOptions = [
		{ value: '', label: 'All states' },
		{ value: 'queued', label: 'Queued' },
		{ value: 'sent', label: 'Sent' },
		{ value: 'delivered', label: 'Delivered' },
		{ value: 'read', label: 'Read' },
		{ value: 'failed', label: 'Failed' },
		{ value: 'cancelled', label: 'Cancelled' },
		{ value: 'received', label: 'Received' }
	];

	const connectionLabel = $derived.by(() => {
		if (!connection) return 'Reading';
		if (connection.settings_error) return 'Settings unreadable';
		if (!connection.configured) return 'Not configured';
		if (!connection.enabled) return 'Paused';
		return 'Ready to send';
	});
	const connectionTone = $derived(
		connection?.configured && connection.enabled && !connection.settings_error ? 'ok' : 'pending'
	);
	const counts = $derived(connection?.counts ?? {});
	const failedCount = $derived(counts.failed ?? 0);

	onMount(() => {
		void status.load();
		void messageList.load();
		void contactList.load();
	});

	onDestroy(() => {
		status.abort();
		messageList.abort();
		contactList.abort();
	});

	function when(value: string): string {
		if (!value) return 'Not yet';
		const date = new Date(value);
		if (Number.isNaN(date.getTime())) return value;
		return new Intl.DateTimeFormat('en-GB', {
			day: '2-digit',
			month: 'short',
			hour: '2-digit',
			minute: '2-digit'
		}).format(date);
	}

	function stateLabel(value: string): string {
		switch (value) {
			case 'queued':
				return 'Queued';
			case 'sending':
				return 'Sending';
			case 'sent':
				return 'Sent';
			case 'delivered':
				return 'Delivered';
			case 'read':
				return 'Read';
			case 'failed':
				return 'Failed';
			case 'cancelled':
				return 'Cancelled';
			case 'received':
				return 'Received';
			default:
				return value || 'Unknown';
		}
	}

	function stateTone(value: string): string {
		if (value === 'failed') return 'failed';
		if (value === 'cancelled') return 'cancelled';
		if (value === 'received') return 'received';
		if (value === 'delivered' || value === 'read') return 'ok';
		return 'pending';
	}

	function contactLabel(message: WhatsAppMessage): string {
		if (message.contact_name) return message.contact_name;
		return message.phone;
	}

	function messagePreview(message: WhatsAppMessage): string {
		if (message.kind === 'template' && message.template_name) {
			return message.body || `Template ${message.template_name}`;
		}
		return message.body || `[${message.kind}]`;
	}

	function selectKind(value: string): void {
		kind = value === 'template' ? 'template' : 'text';
		clearFieldError('text');
		clearFieldError('template_name');
	}

	function clearFieldError(key: string): void {
		if (!fieldErrors[key]) return;
		const next = { ...fieldErrors };
		delete next[key];
		fieldErrors = next;
	}

	async function focusFirstError(): Promise<void> {
		await tick();
		formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
	}

	function templateParameters(): string[] {
		return parameters
			.split('\n')
			.map((value) => value.trim())
			.filter((value) => value.length > 0);
	}

	async function sendMessage(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (busy) return;
		const errors: Record<string, string> = {};
		const trimmedPhone = phone.trim();
		if (!trimmedPhone) errors.phone = 'Enter the customer phone number.';
		if (kind === 'text' && !text.trim()) errors.text = 'Write the reply.';
		if (kind === 'template') {
			if (!templateName.trim()) errors.template_name = 'Enter the approved template name.';
			if (!templateLanguage.trim()) errors.template_language = 'Enter the template language.';
		}
		fieldErrors = errors;
		formError = '';
		notice = '';
		if (Object.keys(errors).length > 0) {
			formError = 'Check the marked fields.';
			await focusFirstError();
			return;
		}

		busy = true;
		try {
			await apiFetch('/plugins/whatsapp/admin/messages', {
				method: 'POST',
				body: {
					phone: trimmedPhone,
					kind,
					text: kind === 'text' ? text.trim() : '',
					template_name: kind === 'template' ? templateName.trim() : '',
					template_language: kind === 'template' ? templateLanguage.trim() : '',
					template_params: kind === 'template' ? templateParameters() : [],
					order_id: replyTo.trim()
				}
			});
			text = '';
			parameters = '';
			replyTo = '';
			fieldErrors = {};
			notice =
				kind === 'template'
					? 'Template queued. Delivery updates arrive from Meta.'
					: 'Reply queued. Delivery updates arrive from Meta.';
			await Promise.all([messageList.load(), status.load()]);
		} catch (caught) {
			if (caught instanceof ApiError) {
				formError = caught.message;
				fieldErrors = { ...caught.fields };
				if (Object.keys(fieldErrors).length > 0) {
					await focusFirstError();
				}
			} else {
				formError = errorMessage(caught);
			}
		} finally {
			busy = false;
		}
	}

	async function retryMessage(message: WhatsAppMessage): Promise<void> {
		if (retrying) return;
		retrying = message.id;
		contactError = '';
		try {
			await apiFetch(`/plugins/whatsapp/admin/messages/${encodeURIComponent(message.id)}/retry`, {
				method: 'POST'
			});
			notice = 'Message queued again.';
			await Promise.all([messageList.load(), status.load()]);
		} catch (caught) {
			contactError = errorMessage(caught);
		} finally {
			retrying = '';
		}
	}

	async function setConsent(contact: WhatsAppContact): Promise<void> {
		if (contactBusy) return;
		contactBusy = contact.phone;
		contactError = '';
		try {
			await apiFetch(`/plugins/whatsapp/admin/contacts/${encodeURIComponent(contact.phone)}`, {
				method: 'PATCH',
				body: { opted_in: !contact.opted_in }
			});
			await Promise.all([contactList.load(), status.load()]);
		} catch (caught) {
			contactError = errorMessage(caught);
		} finally {
			contactBusy = '';
		}
	}

	function filterMessages(value: string): void {
		statusFilter = value;
		void messageList.load();
	}

	async function copyWebhook(): Promise<void> {
		copyNotice = '';
		if (!connection?.webhook_url) return;
		try {
			await navigator.clipboard.writeText(connection.webhook_url);
			copyNotice = 'Webhook URL copied.';
		} catch {
			copyNotice = 'Select the URL and copy it manually.';
		}
	}

	function reloadMessages(): void {
		void messageList.load();
		void status.load();
	}
</script>

<div class="whatsapp-admin">
	<section class="a-panel" aria-labelledby="wa-connection-title">
		<header class="a-panel-head">
			<h2 id="wa-connection-title">Connection</h2>
			<p>{plugin.name} sends through the Cloud API and receives replies on the signed webhook.</p>
		</header>
		<div class="a-panel-body">
			{#if !connection}
				<LoadingState label="Reading the connection…" />
			{:else}
				{#if connection.settings_error}
					<div class="a-notice key-warning" role="alert">
						<p>
							<strong>Settings cannot be read.</strong> The server encryption key changed. Enter every
							setting again and save to replace the old values.
						</p>
					</div>
				{:else if !connection.configured}
					<div class="a-notice key-warning">
						<p>
							<strong>The line is not configured.</strong> Add the phone number ID, access token, app
							secret and webhook verify token in the settings above.
						</p>
					</div>
				{/if}

				<div class="wa-state">
					<div>
						<p class="a-label">State</p>
						<p class="wa-state-value" data-tone={connectionTone}>
							<span aria-hidden="true">{connectionTone === 'ok' ? '✓' : '○'}</span>
							{connectionLabel}
						</p>
					</div>
					<div>
						<p class="a-label">Phone number ID</p>
						<p class="wa-mono">{connection.phone_number_id || 'Not set'}</p>
					</div>
					<div>
						<p class="a-label">Graph version</p>
						<p class="wa-mono">{connection.graph_version}</p>
					</div>
					<div>
						<p class="a-label">Last webhook</p>
						<p>{when(connection.last_webhook_at)}</p>
					</div>
				</div>

				<div class="a-field wa-webhook">
					<label for="wa-webhook-url">Webhook URL</label>
					<div class="wa-webhook-row">
						<input id="wa-webhook-url" type="text" readonly value={connection.webhook_url} />
						<button class="a-btn a-btn-secondary" type="button" onclick={copyWebhook}>
							Copy URL
						</button>
					</div>
					<p class="a-field-hint">
						Paste this in Meta's WhatsApp webhook setup, verify it with the saved token, then
						subscribe to the messages field. Meta must reach it over HTTPS in production.
					</p>
					{#if copyNotice}
						<p class="a-field-hint" role="status">{copyNotice}</p>
					{/if}
				</div>

				<dl class="wa-counts" aria-label="Message counts">
					<div>
						<dt>Queued</dt>
						<dd class="a-num">{counts.queued ?? 0}</dd>
					</div>
					<div>
						<dt>Sent</dt>
						<dd class="a-num">{counts.sent ?? 0}</dd>
					</div>
					<div>
						<dt>Delivered</dt>
						<dd class="a-num">{(counts.delivered ?? 0) + (counts.read ?? 0)}</dd>
					</div>
					<div>
						<dt>Failed</dt>
						<dd class="a-num" data-danger={failedCount > 0 ? 'true' : undefined}>{failedCount}</dd>
					</div>
					<div>
						<dt>Replies</dt>
						<dd class="a-num">{counts.received ?? 0}</dd>
					</div>
					<div>
						<dt>Contacts</dt>
						<dd class="a-num">{connection.contacts}</dd>
					</div>
				</dl>

				{#if connection.last_failure}
					<p class="wa-failure">
						Last failure {when(connection.last_failure_at)}: {connection.last_failure}
					</p>
				{/if}
			{/if}
		</div>
	</section>

	<section class="a-panel" aria-labelledby="wa-send-title">
		<header class="a-panel-head">
			<h2 id="wa-send-title">Send a message</h2>
			<p>Free-form replies work for {connection?.reply_window_hours ?? 24} hours after the customer's last message. Use an approved template outside that window.</p>
		</header>
		<div class="a-panel-body">
			<form bind:this={formEl} onsubmit={sendMessage} novalidate>
				{#if formError}
					<p class="a-form-error" role="alert">{formError}</p>
				{/if}
				{#if notice}
					<div class="a-notice a-notice-success" role="status">
						<p>{notice}</p>
						<button class="a-btn a-btn-quiet" type="button" onclick={() => (notice = '')}>
							Dismiss
						</button>
					</div>
				{/if}

				<div class="wa-form-grid">
					<div class="a-field">
						<span class="a-label" id="wa-kind-label">Message type</span>
						<Dropdown
							id="wa-kind"
							class="admin-dropdown"
							label="Message type"
							labelId="wa-kind-label"
							value={kind}
							options={kindOptions}
							onchange={selectKind}
							disabled={busy}
						/>
					</div>
					<div class="a-field">
						<label for="wa-phone">Customer phone</label>
						<input
							id="wa-phone"
							type="tel"
							autocomplete="tel"
							placeholder="+213 550 77 88 99"
							value={phone}
							disabled={busy}
							aria-invalid={fieldErrors.phone ? 'true' : undefined}
							aria-describedby={fieldErrors.phone ? 'wa-phone-error' : 'wa-phone-hint'}
							oninput={(event) => {
								phone = event.currentTarget.value;
								clearFieldError('phone');
							}}
						/>
						{#if fieldErrors.phone}
							<p class="a-field-error" id="wa-phone-error">{fieldErrors.phone}</p>
						{:else}
							<p class="a-field-hint" id="wa-phone-hint">
								A local number is completed with the default country code from settings.
							</p>
						{/if}
					</div>

					{#if kind === 'text'}
						<div class="a-field wa-wide">
							<label for="wa-text">Reply</label>
							<textarea
								id="wa-text"
								rows="3"
								maxlength="4096"
								placeholder="Your order is packed and leaves today."
								value={text}
								disabled={busy}
								aria-invalid={fieldErrors.text ? 'true' : undefined}
								aria-describedby={fieldErrors.text ? 'wa-text-error' : 'wa-text-hint'}
								oninput={(event) => {
									text = event.currentTarget.value;
									clearFieldError('text');
								}}
							></textarea>
							{#if fieldErrors.text}
								<p class="a-field-error" id="wa-text-error">{fieldErrors.text}</p>
							{:else}
								<p class="a-field-hint" id="wa-text-hint">
									Only inside the reply window. Outside it the server refuses the reply.
								</p>
							{/if}
						</div>
					{:else}
						<div class="a-field">
							<label for="wa-template">Template name</label>
							<input
								id="wa-template"
								type="text"
								placeholder={connection?.order_template || 'order_update'}
								value={templateName}
								disabled={busy}
								aria-invalid={fieldErrors.template_name ? 'true' : undefined}
								aria-describedby={fieldErrors.template_name ? 'wa-template-error' : 'wa-template-hint'}
								oninput={(event) => {
									templateName = event.currentTarget.value;
									clearFieldError('template_name');
								}}
							/>
							{#if fieldErrors.template_name}
								<p class="a-field-error" id="wa-template-error">{fieldErrors.template_name}</p>
							{:else}
								<p class="a-field-hint" id="wa-template-hint">
									The approved template name from Meta.
								</p>
							{/if}
						</div>
						<div class="a-field">
							<label for="wa-language">Language</label>
							<input
								id="wa-language"
								type="text"
								placeholder="en"
								value={templateLanguage}
								disabled={busy}
								aria-invalid={fieldErrors.template_language ? 'true' : undefined}
								aria-describedby={fieldErrors.template_language
									? 'wa-language-error'
									: 'wa-language-hint'}
								oninput={(event) => {
									templateLanguage = event.currentTarget.value;
									clearFieldError('template_language');
								}}
							/>
							{#if fieldErrors.template_language}
								<p class="a-field-error" id="wa-language-error">{fieldErrors.template_language}</p>
							{:else}
								<p class="a-field-hint" id="wa-language-hint">For example en or fr.</p>
							{/if}
						</div>
						<div class="a-field wa-wide">
							<label for="wa-parameters">Body parameters</label>
							<textarea
								id="wa-parameters"
								rows="3"
								placeholder={'Amina\nAZ-2026-0001\n5,400 DZD'}
								value={parameters}
								disabled={busy}
								aria-invalid={fieldErrors.template_params ? 'true' : undefined}
								aria-describedby={fieldErrors.template_params
									? 'wa-parameters-error'
									: 'wa-parameters-hint'}
								oninput={(event) => {
									parameters = event.currentTarget.value;
									clearFieldError('template_params');
								}}
							></textarea>
							{#if fieldErrors.template_params}
								<p class="a-field-error" id="wa-parameters-error">{fieldErrors.template_params}</p>
							{:else}
								<p class="a-field-hint" id="wa-parameters-hint">
									One value per line, in the order the template expects {'{{1}}, {{2}}, {{3}}'}.
								</p>
							{/if}
						</div>
					{/if}

					<div class="a-field wa-wide">
						<label for="wa-order">Link an order (optional)</label>
						<input
							id="wa-order"
							type="text"
							placeholder="Order id"
							value={replyTo}
							disabled={busy}
							aria-invalid={fieldErrors.order_id ? 'true' : undefined}
							aria-describedby={fieldErrors.order_id ? 'wa-order-error' : 'wa-order-hint'}
							oninput={(event) => {
								replyTo = event.currentTarget.value;
								clearFieldError('order_id');
							}}
						/>
						{#if fieldErrors.order_id}
							<p class="a-field-error" id="wa-order-error">{fieldErrors.order_id}</p>
						{:else}
							<p class="a-field-hint" id="wa-order-hint">
								Links the log row to an order. Leave blank for a general reply.
							</p>
						{/if}
					</div>
				</div>

				<footer class="wa-submit">
					<p class="a-field-hint">The dispatcher sends queued messages in the background and records Meta's delivery states.</p>
					<button class="a-btn a-btn-primary" type="submit" disabled={busy}>
						{busy ? 'Queueing…' : 'Queue message'}
					</button>
				</footer>
			</form>
		</div>
	</section>

	<section class="a-panel" aria-labelledby="wa-log-title">
		<header class="a-panel-head">
			<div class="wa-head-row">
				<div>
					<h2 id="wa-log-title">Message log</h2>
					<p>Outbound confirmations and customer replies, newest first.</p>
				</div>
				<div class="wa-log-tools">
					<Dropdown
						id="wa-status-filter"
						class="admin-dropdown"
						label="Filter by state"
						value={statusFilter}
						options={statusOptions}
						onchange={filterMessages}
					/>
					<button class="a-btn a-btn-secondary" type="button" onclick={reloadMessages}>
						Refresh
					</button>
				</div>
			</div>
		</header>
		<div class="a-panel-body">
			{#if contactError}
				<p class="a-form-error" role="alert">{contactError}</p>
			{/if}
			{#if messagesLoading && messages.length === 0}
				<LoadingState label="Reading the message log…" />
			{:else if messagesError}
				<ErrorState title="Messages could not be read." message={messagesError} onretry={reloadMessages} />
			{:else if messages.length === 0}
				<div class="a-empty">
					<h3>No messages {statusFilter ? 'in this state' : 'yet'}.</h3>
					<p>
						The first order confirmation appears here after checkout. Customer replies arrive through
						the webhook.
					</p>
				</div>
			{:else}
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div class="a-table-scroll" role="region" aria-label="Messages" tabindex="0">
					<table class="a-table a-table--cards wa-table">
						<thead>
							<tr>
								<th scope="col">When</th>
								<th scope="col">Direction</th>
								<th scope="col">Contact</th>
								<th scope="col">Message</th>
								<th scope="col">State</th>
								<th scope="col" class="right">Action</th>
							</tr>
						</thead>
						<tbody>
							{#each messages as message (message.id)}
								<tr>
									<td>
										<span class="a-cell-label">When</span>
										{when(message.created_at)}
									</td>
									<td>
										<span class="a-cell-label">Direction</span>
										{message.direction === 'out' ? 'Out' : 'In'}
									</td>
									<td>
										<span class="a-cell-label">Contact</span>
										<span class="wa-contact">
											{contactLabel(message)}
											<span class="wa-mono wa-phone">{message.phone}</span>
										</span>
									</td>
									<td>
										<span class="a-cell-label">Message</span>
										<span class="wa-preview">{messagePreview(message)}</span>
										{#if message.order_number}
											<span class="wa-order">Order {message.order_number}</span>
										{/if}
										{#if message.error}
											<span class="wa-error">{message.error}</span>
										{/if}
									</td>
									<td>
										<span class="a-cell-label">State</span>
										<span class="wa-state-cell" data-tone={stateTone(message.status)}>
											{stateLabel(message.status)}
											{#if message.attempts > 0 && message.status !== 'sent'}
												<span class="wa-attempts">{message.attempts} {message.attempts === 1 ? 'try' : 'tries'}</span>
											{/if}
										</span>
									</td>
									<td>
										<span class="a-cell-label">Action</span>
										<div class="cell-actions">
											{#if message.direction === 'out' && message.status === 'failed'}
												<button
													class="a-btn a-btn-quiet a-btn-table"
													type="button"
													onclick={() => retryMessage(message)}
													disabled={retrying === message.id}
												>
													{retrying === message.id ? 'Queueing…' : 'Retry'}
												</button>
											{:else}
												<span class="wa-muted">None</span>
											{/if}
										</div>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	</section>

	<section class="a-panel" aria-labelledby="wa-contacts-title">
		<header class="a-panel-head">
			<h2 id="wa-contacts-title">Contacts</h2>
			<p>Consent is recorded from customer messages. A pause stops every send to that number.</p>
		</header>
		<div class="a-panel-body">
			{#if contactsLoading && contacts.length === 0}
				<LoadingState label="Reading contacts…" />
			{:else if contactsError}
				<ErrorState
					title="Contacts could not be read."
					message={contactsError}
					onretry={() => contactList.load()}
				/>
			{:else if contacts.length === 0}
				<div class="a-empty">
					<h3>No contacts yet.</h3>
					<p>Contacts appear when a customer writes on WhatsApp or an admin sends a first message.</p>
				</div>
			{:else}
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div class="a-table-scroll" role="region" aria-label="Contacts" tabindex="0">
					<table class="a-table a-table--cards wa-contact-table">
						<thead>
							<tr>
								<th scope="col">Contact</th>
								<th scope="col">Consent</th>
								<th scope="col">Last reply</th>
								<th scope="col">Reply window</th>
								<th scope="col" class="right">Action</th>
							</tr>
						</thead>
						<tbody>
							{#each contacts as contact (contact.phone)}
								<tr>
									<td>
										<span class="a-cell-label">Contact</span>
										<span class="wa-contact">
											{contact.name || 'Unnamed contact'}
											<span class="wa-mono wa-phone">{contact.phone}</span>
										</span>
									</td>
									<td>
										<span class="a-cell-label">Consent</span>
										<span class="wa-consent" data-consent={contact.opted_in ? 'on' : 'off'}>
											{contact.opted_in ? 'Receiving' : 'Paused'}
										</span>
									</td>
									<td>
										<span class="a-cell-label">Last reply</span>
										{when(contact.last_inbound_at)}
									</td>
									<td>
										<span class="a-cell-label">Reply window</span>
										{contact.reply_open ? 'Open' : 'Closed'}
									</td>
									<td>
										<span class="a-cell-label">Action</span>
										<div class="cell-actions">
											<button
												class="a-btn a-btn-quiet a-btn-table"
												type="button"
												onclick={() => setConsent(contact)}
												disabled={contactBusy === contact.phone}
											>
												{contact.opted_in ? 'Pause messages' : 'Resume messages'}
											</button>
										</div>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	</section>

	<section class="a-panel" aria-labelledby="wa-templates-title">
		<header class="a-panel-head">
			<h2 id="wa-templates-title">Order messages</h2>
			<p>The two templates the store sends on its own. Change them in the settings above.</p>
		</header>
		<div class="a-panel-body">
			<dl class="wa-templates">
				<div>
					<dt>Order confirmation</dt>
					<dd>
						<span class="wa-mono">{connection?.order_template || 'order_confirmation'}</span>
						<span class="wa-muted">{connection?.order_template_language || 'en'}</span>
						<p>Parameters: customer name, order number, total.</p>
					</dd>
				</div>
				<div>
					<dt>Status update</dt>
					<dd>
						<span class="wa-mono">{connection?.status_template || 'order_update'}</span>
						<span class="wa-muted">{connection?.status_template_language || 'en'}</span>
						<p>Parameters: customer name, order number, status, total.</p>
					</dd>
				</div>
			</dl>
		</div>
	</section>
</div>

<style>
	.whatsapp-admin {
		display: grid;
		gap: 20px;
	}

	.wa-state {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 0;
		border-top: 1px solid var(--a-line);
		border-bottom: 1px solid var(--a-line);
	}

	.wa-state > div {
		padding: 12px 16px;
		border-right: 1px solid var(--a-line);
	}

	.wa-state > div:first-child {
		padding-left: 0;
	}

	.wa-state > div:last-child {
		border-right: 0;
	}

	.wa-state-value {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-top: 2px;
		font-size: 14px;
	}

	.wa-state-value[data-tone='ok'] {
		color: var(--a-status-delivered);
	}

	.wa-state-value[data-tone='pending'] {
		color: var(--a-muted);
	}

	.wa-mono {
		font-family: var(--a-font-mono);
		font-size: 12px;
		font-variant-numeric: tabular-nums;
		overflow-wrap: anywhere;
	}

	.wa-webhook {
		margin-top: 18px;
	}

	.wa-webhook-row {
		display: flex;
		gap: 10px;
		align-items: center;
	}

	.wa-webhook-row input {
		flex: 1 1 auto;
		min-width: 0;
	}

	.wa-webhook-row .a-btn {
		flex: 0 0 auto;
		white-space: nowrap;
	}

	.wa-counts {
		display: flex;
		flex-wrap: wrap;
		margin-top: 18px;
		border-top: 1px solid var(--a-line);
		border-bottom: 1px solid var(--a-line);
	}

	.wa-counts div {
		display: flex;
		align-items: baseline;
		gap: 8px;
		min-width: 120px;
		padding: 10px 18px;
		border-right: 1px solid var(--a-line);
	}

	.wa-counts div:first-child {
		padding-left: 0;
	}

	.wa-counts div:last-child {
		border-right: 0;
	}

	.wa-counts dt {
		color: var(--a-muted);
		font-size: 12px;
	}

	.wa-counts dd {
		font-size: 16px;
	}

	.wa-counts dd[data-danger='true'] {
		color: var(--a-danger-text);
	}

	.wa-failure {
		margin-top: 12px;
		color: var(--a-danger-text);
		font-size: 12px;
		overflow-wrap: anywhere;
	}

	.wa-form-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 14px;
	}

	.wa-wide {
		grid-column: 1 / -1;
	}

	.wa-submit {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-top: 18px;
		padding-top: 14px;
		border-top: 1px solid var(--a-line);
	}

	.wa-head-row {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-start;
		justify-content: space-between;
		gap: 14px;
	}

	.wa-log-tools {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
	}

	.wa-table {
		min-width: 940px;
	}

	.wa-contact-table {
		min-width: 780px;
	}

	.wa-contact {
		display: grid;
		gap: 1px;
	}

	.wa-phone {
		color: var(--a-muted);
	}

	.wa-preview {
		display: block;
		max-width: 44ch;
		overflow-wrap: anywhere;
	}

	.wa-order {
		display: block;
		margin-top: 2px;
		color: var(--a-muted);
		font-family: var(--a-font-mono);
		font-size: 11px;
	}

	.wa-error {
		display: block;
		margin-top: 3px;
		color: var(--a-danger-text);
		font-size: 11px;
		overflow-wrap: anywhere;
	}

	.wa-state-cell {
		display: block;
	}

	.wa-state-cell[data-tone='failed'] {
		color: var(--a-danger-text);
	}

	.wa-state-cell[data-tone='cancelled'] {
		color: var(--a-muted);
	}

	.wa-state-cell[data-tone='ok'] {
		color: var(--a-status-delivered);
	}

	.wa-state-cell[data-tone='received'] {
		color: var(--a-status-confirmed);
	}

	.wa-attempts {
		display: block;
		color: var(--a-muted);
		font-size: 11px;
	}

	.wa-consent[data-consent='off'] {
		color: var(--a-danger-text);
	}

	.wa-consent[data-consent='on'] {
		color: var(--a-status-delivered);
	}

	.wa-muted {
		color: var(--a-muted);
	}

	.wa-templates {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 0;
		border-top: 1px solid var(--a-line);
		border-bottom: 1px solid var(--a-line);
	}

	.wa-templates > div {
		padding: 12px 16px;
		border-right: 1px solid var(--a-line);
	}

	.wa-templates > div:first-child {
		padding-left: 0;
	}

	.wa-templates > div:last-child {
		border-right: 0;
	}

	.wa-templates dt {
		color: var(--a-muted);
		font-size: 12px;
	}

	.wa-templates dd {
		display: grid;
		gap: 3px;
		margin-top: 4px;
		font-size: 13px;
	}

	.wa-templates p {
		color: var(--a-muted);
		font-size: 12px;
	}

	@media (max-width: 900px) {
		.wa-state {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}

		.wa-state > div:nth-child(2) {
			border-right: 0;
		}

		.wa-state > div:nth-child(-n + 2) {
			border-bottom: 1px solid var(--a-line);
		}
	}

	@media (max-width: 720px) {
		.wa-form-grid,
		.wa-templates {
			grid-template-columns: minmax(0, 1fr);
		}

		.wa-webhook-row {
			flex-wrap: wrap;
		}

		.wa-webhook-row input {
			flex-basis: 100%;
		}

		.wa-table,
		.wa-contact-table {
			min-width: 0;
		}

		.wa-templates > div {
			border-right: 0;
			border-bottom: 1px solid var(--a-line);
		}

		.wa-templates > div:last-child {
			border-bottom: 0;
		}
	}

	@media (max-width: 560px) {
		.wa-state {
			grid-template-columns: minmax(0, 1fr);
		}

		.wa-state > div {
			padding-left: 0;
			border-right: 0;
			border-bottom: 1px solid var(--a-line);
		}

		.wa-state > div:last-child {
			border-bottom: 0;
		}

		.wa-counts div {
			flex: 1 1 100%;
			padding-left: 0;
			border-right: 0;
			border-bottom: 1px solid var(--a-line);
		}

		.wa-counts div:last-child {
			border-bottom: 0;
		}

		.wa-submit .a-btn {
			width: 100%;
		}
	}
</style>
