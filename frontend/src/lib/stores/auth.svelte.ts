import { apiFetch, ApiError } from '#lib/api';
import type { User } from '#lib/types';

class Auth {
	user = $state<User | null>(null);
	loaded = $state(false);
	busy = $state(false);
	error = $state('');

	get isAdmin(): boolean {
		return this.user?.role === 'admin';
	}

	async refresh(): Promise<void> {
		try {
			const data = await apiFetch<{ user: User }>('/auth/me');
			this.user = data.user;
		} catch (error) {
			if (!(error instanceof ApiError) || error.status !== 401) {
				this.error = error instanceof Error ? error.message : 'Could not load your account.';
			}
			this.user = null;
		} finally {
			this.loaded = true;
		}
	}

	async login(email: string, password: string): Promise<User> {
		this.busy = true;
		this.error = '';
		try {
			const data = await apiFetch<{ user: User }>('/auth/login', {
				method: 'POST',
				body: { email, password }
			});
			this.user = data.user;
			this.loaded = true;
			return data.user;
		} catch (error) {
			this.error = error instanceof Error ? error.message : 'Could not sign in.';
			throw error;
		} finally {
			this.busy = false;
		}
	}

	async register(name: string, email: string, password: string): Promise<User> {
		this.busy = true;
		this.error = '';
		try {
			const data = await apiFetch<{ user: User }>('/auth/register', {
				method: 'POST',
				body: { name, email, password }
			});
			this.user = data.user;
			this.loaded = true;
			return data.user;
		} catch (error) {
			this.error = error instanceof Error ? error.message : 'Could not create the account.';
			throw error;
		} finally {
			this.busy = false;
		}
	}

	async logout(): Promise<void> {
		this.busy = true;
		try {
			await apiFetch<void>('/auth/logout', { method: 'POST' });
		} finally {
			this.user = null;
			this.busy = false;
		}
	}
}

export const auth = new Auth();
