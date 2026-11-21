const {
	app,
	BrowserWindow,
	Menu,
	ipcMain,
	protocol,
	safeStorage,
	shell,
	screen
} = require('electron');
const crypto = require('node:crypto');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const path = require('node:path');

const APP_VERSION = app.getVersion();
app.setName('Azahar Admin');
let userAgent = `AzaharDesktop/${APP_VERSION}`;
const DEFAULT_SETTINGS = {
	apiUrl: 'http://127.0.0.1:8080',
	origin: '',
	window: null
};
const IMAGE_CACHE_LIMIT = 40;

let mainWindow = null;
let settings = { ...DEFAULT_SETTINGS };
let resolvedOrigin = '';
let settingsPath = '';
let sessionPath = '';
const cookies = new Map();
const imageCache = new Map();

protocol.registerSchemesAsPrivileged([
	{
		scheme: 'azahar-asset',
		privileges: {
			standard: true,
			secure: true,
			supportFetchAPI: true,
			stream: true
		}
	}
]);

class ApiError extends Error {
	constructor(status, code, message, fields = {}) {
		super(message);
		this.name = 'ApiError';
		this.status = status;
		this.code = code;
		this.fields = fields;
	}
}

function resourcesRoot() {
	return app.isPackaged
		? path.join(process.resourcesPath, 'resources')
		: path.join(__dirname, 'resources');
}

function normalizeApiUrl(value) {
	const raw = String(value ?? '').trim();
	if (!raw) throw new Error('Enter the API address.');
	let url;
	try {
		url = new URL(raw);
	} catch {
		throw new Error('The API address must be an absolute http or https URL.');
	}
	if (url.protocol !== 'http:' && url.protocol !== 'https:') {
		throw new Error('The API address must use http or https.');
	}
	if (url.username || url.password) {
		throw new Error('The API address must not contain credentials.');
	}
	if (url.pathname !== '/' && url.pathname !== '') {
		throw new Error('Use the API root, without a path.');
	}
	url.search = '';
	url.hash = '';
	return url.toString().replace(/\/$/, '');
}

function normalizeOrigin(value) {
	const raw = String(value ?? '').trim();
	if (!raw) return '';
	let url;
	try {
		url = new URL(raw);
	} catch {
		throw new Error('The origin must be an absolute http or https URL.');
	}
	if (url.protocol !== 'http:' && url.protocol !== 'https:') {
		throw new Error('The origin must use http or https.');
	}
	if (url.username || url.password) {
		throw new Error('The origin must not contain credentials.');
	}
	return url.origin;
}

function loadSettings() {
	const profile = app.getPath('userData');
	settingsPath = path.join(profile, 'settings.json');
	const device = crypto.createHash('sha256').update(profile).digest('hex').slice(0, 12);
	userAgent = `AzaharDesktop/${APP_VERSION} (${device})`;
	try {
		const parsed = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
		if (parsed && typeof parsed === 'object') {
			settings = { ...DEFAULT_SETTINGS, ...parsed };
		}
	} catch {
		settings = { ...DEFAULT_SETTINGS };
	}

	if (process.env.AZAHAR_API_URL) {
		try {
			settings.apiUrl = normalizeApiUrl(process.env.AZAHAR_API_URL);
		} catch {
			/* keep the saved address when the override is malformed */
		}
	}
	if (process.env.AZAHAR_ORIGIN !== undefined) {
		try {
			settings.origin = normalizeOrigin(process.env.AZAHAR_ORIGIN);
		} catch {
			/* keep the saved origin when the override is malformed */
		}
	}
	try {
		settings.apiUrl = normalizeApiUrl(settings.apiUrl);
	} catch {
		settings.apiUrl = DEFAULT_SETTINGS.apiUrl;
	}
	try {
		settings.origin = normalizeOrigin(settings.origin);
	} catch {
		settings.origin = '';
	}
}

function saveSettings() {
	const payload = JSON.stringify(
		{
			apiUrl: settings.apiUrl,
			origin: settings.origin,
			window: settings.window
		},
		null,
		2
	);
	fs.writeFileSync(settingsPath, payload, { mode: 0o600 });
}

function loadSession() {
	sessionPath = path.join(app.getPath('userData'), 'session');
	let text = '';
	try {
		text = fs.readFileSync(sessionPath, 'utf8');
	} catch {
		return;
	}
	try {
		if (text.startsWith('enc:')) {
			if (!safeStorage.isEncryptionAvailable()) return;
			text = safeStorage.decryptString(Buffer.from(text.slice(4), 'base64'));
		}
		const parsed = JSON.parse(text);
		if (parsed && typeof parsed === 'object') {
			for (const [name, value] of Object.entries(parsed)) {
				if (typeof value === 'string' && value) cookies.set(name, value);
			}
		}
	} catch {
		/* a session that cannot be read is a signed-out session */
	}
}

function saveSession() {
	const session = Object.fromEntries(
		[...cookies].filter(([name]) => name === 'azahar_session')
	);
	try {
		const text = JSON.stringify(session);
		if (safeStorage.isEncryptionAvailable()) {
			const encrypted = safeStorage.encryptString(text).toString('base64');
			fs.writeFileSync(sessionPath, `enc:${encrypted}`, { mode: 0o600 });
		} else {
			fs.writeFileSync(sessionPath, text, { mode: 0o600 });
		}
	} catch {
		/* the app keeps working in memory when the profile is read-only */
	}
}

function clearSession() {
	cookies.delete('azahar_session');
	cookies.delete('azahar_csrf');
	try {
		fs.rmSync(sessionPath, { force: true });
	} catch {
		/* nothing to remove */
	}
}

function cookieHeader() {
	if (cookies.size === 0) return '';
	return [...cookies].map(([name, value]) => `${name}=${value}`).join('; ');
}

function absorbCookies(headers) {
	if (!headers || typeof headers.getSetCookie !== 'function') return false;
	let sessionChanged = false;
	for (const raw of headers.getSetCookie()) {
		const pair = raw.split(';', 1)[0];
		const index = pair.indexOf('=');
		if (index < 1) continue;
		const name = pair.slice(0, index).trim();
		const value = pair.slice(index + 1).trim();
		const lower = raw.toLowerCase();
		const expired = lower.includes('max-age=0') || lower.includes('max-age=-1');
		if (!value || expired) {
			cookies.delete(name);
		} else {
			cookies.set(name, value);
		}
		if (name === 'azahar_session') sessionChanged = true;
	}
	if (sessionChanged) saveSession();
	return sessionChanged;
}

async function resolveOrigin(force = false) {
	if (settings.origin) return settings.origin;
	if (resolvedOrigin && !force) return resolvedOrigin;
	try {
		const response = await requestRaw('GET', '/api/health', { includeOrigin: false });
		const body = await readJson(response);
		if (body && typeof body.origin === 'string' && body.origin) {
			resolvedOrigin = normalizeOrigin(body.origin);
			return resolvedOrigin;
		}
	} catch {
		/* the connection screen reports the failure with a better message */
	}
	resolvedOrigin = 'http://localhost:3000';
	return resolvedOrigin;
}

function requestHeaders({ json = false, origin = true, csrf = true } = {}) {
	const headers = {
		Accept: 'application/json',
		'User-Agent': userAgent,
		'X-Requested-With': 'azahar-web'
	};
	const cookie = cookieHeader();
	if (cookie) headers.Cookie = cookie;
	if (json) headers['Content-Type'] = 'application/json';
	if (csrf && cookies.get('azahar_csrf')) {
		headers['X-CSRF-Token'] = cookies.get('azahar_csrf');
	}
	if (origin) {
		const value = settings.origin || resolvedOrigin;
		if (value) headers.Origin = value;
	}
	return headers;
}

async function requestRaw(method, apiPath, options = {}) {
	if (
		typeof apiPath !== 'string' ||
		!apiPath.startsWith('/') ||
		apiPath.startsWith('//') ||
		apiPath.includes('\\')
	) {
		throw new ApiError(0, 'invalid_path', 'The desktop client only calls the API namespace.');
	}
	const base = new URL(settings.apiUrl);
	const url = new URL(apiPath, base);
	if (url.origin !== base.origin || !url.pathname.startsWith('/api/')) {
		throw new ApiError(0, 'invalid_path', 'The desktop client only calls the API namespace.');
	}
	const init = {
		method,
		headers: requestHeaders(options),
		redirect: 'manual',
		signal: AbortSignal.timeout(options.timeout ?? 20000)
	};
	if (options.body !== undefined) {
		const payload = JSON.stringify(options.body);
		if (payload.length > 2 * 1024 * 1024) {
			throw new ApiError(0, 'payload_too_large', 'The request body is too large.');
		}
		init.body = payload;
		init.headers['Content-Type'] = 'application/json';
	} else if (method !== 'GET' && method !== 'HEAD') {
		init.body = '{}';
		init.headers['Content-Type'] = 'application/json';
	}

	let response;
	try {
		response = await fetch(url, init);
	} catch (error) {
		const message =
			error && error.name === 'TimeoutError'
				? 'The API did not answer in time. Check the service and try again.'
				: 'The API did not answer. Check the address and that the service is running.';
		throw new ApiError(0, 'network_error', message);
	}
	absorbCookies(response.headers);
	return response;
}

async function readJson(response) {
	try {
		return await response.json();
	} catch {
		return null;
	}
}

async function ensureCsrf() {
	if (cookies.get('azahar_csrf')) return;
	const response = await requestRaw('GET', '/api/csrf', { csrf: false });
	const body = await readJson(response);
	if (!response.ok || !body || typeof body.csrf_token !== 'string') {
		throw new ApiError(
			response.status || 0,
			'csrf_unavailable',
			'The API did not issue a security token. Try again.'
		);
	}
	cookies.set('azahar_csrf', body.csrf_token);
}

async function apiRequest(method, apiPath, body, retried = false) {
	const safe = method === 'GET' || method === 'HEAD';
	if (!safe) await ensureCsrf();

	const response = await requestRaw(method, apiPath, { body });
	const parsed = await readJson(response);

	if (!response.ok) {
		const problem = parsed && parsed.error ? parsed.error : {};
		if (
			!retried &&
			response.status === 403 &&
			problem.code === 'csrf_failed' &&
			!safe
		) {
			cookies.delete('azahar_csrf');
			await ensureCsrf();
			return apiRequest(method, apiPath, body, true);
		}
		throw new ApiError(
			response.status,
			problem.code || 'request_failed',
			problem.message || 'The API returned an error. Try again.',
			problem.fields || {}
		);
	}
	return parsed;
}

async function apiImage(src) {
	if (!src.startsWith('/api/images/')) {
		throw new ApiError(0, 'invalid_image', 'That image is not served by the API.');
	}
	if (imageCache.has(src)) return imageCache.get(src);
	const response = await requestRaw('GET', src, { timeout: 15000 });
	if (!response.ok) {
		throw new ApiError(response.status, 'image_failed', 'The image could not be loaded.');
	}
	const type = response.headers.get('content-type') || 'application/octet-stream';
	if (!/^image\/(png|jpeg|webp|gif|avif)$/.test(type)) {
		throw new ApiError(0, 'image_failed', 'The API returned an unsupported image.');
	}
	const bytes = Buffer.from(await response.arrayBuffer());
	const value = `data:${type};base64,${bytes.toString('base64')}`;
	if (imageCache.size >= IMAGE_CACHE_LIMIT) {
		const oldest = imageCache.keys().next().value;
		imageCache.delete(oldest);
	}
	imageCache.set(src, value);
	return value;
}

function serializeError(error) {
	if (error instanceof ApiError) {
		return {
			status: error.status,
			code: error.code,
			message: error.message,
			fields: error.fields
		};
	}
	return {
		status: 0,
		code: 'unexpected_error',
		message: 'The desktop client hit an unexpected problem. Try again.',
		fields: {}
	};
}

function windowBounds() {
	if (!settings.window) return { width: 1380, height: 900 };
	const area = screen.getPrimaryDisplay().workArea;
	const width = Math.min(Math.max(Number(settings.window.width) || 1380, 1080), area.width);
	const height = Math.min(Math.max(Number(settings.window.height) || 900, 720), area.height);
	const x = Number.isFinite(settings.window.x) ? settings.window.x : undefined;
	const y = Number.isFinite(settings.window.y) ? settings.window.y : undefined;
	return { width, height, x, y };
}

function createWindow() {
	const bounds = windowBounds();
	mainWindow = new BrowserWindow({
		...bounds,
		minWidth: 1080,
		minHeight: 720,
		backgroundColor: '#f6f0eb',
		title: 'Azahar admin',
		show: false,
		autoHideMenuBar: true,
		webPreferences: {
			preload: path.join(__dirname, 'preload.js'),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: true,
			webSecurity: true,
			spellcheck: false
		}
	});

	mainWindow.loadFile(path.join(__dirname, 'renderer', 'index.html'));
	mainWindow.once('ready-to-show', () => mainWindow.show());

	mainWindow.webContents.setWindowOpenHandler(({ url }) => {
		if (/^https?:\/\//i.test(url)) shell.openExternal(url);
		return { action: 'deny' };
	});
	mainWindow.webContents.on('will-navigate', (event, url) => {
		if (!url.startsWith('file://')) event.preventDefault();
	});
	mainWindow.webContents.on('before-input-event', (event, input) => {
		if (!app.isPackaged && input.type === 'keyDown' && input.key === 'F12') {
			mainWindow.webContents.toggleDevTools();
			event.preventDefault();
		}
	});

	const saveBounds = () => {
		if (!mainWindow || mainWindow.isDestroyed() || mainWindow.isMinimized()) return;
		settings.window = mainWindow.getBounds();
		try {
			saveSettings();
		} catch {
			/* a read-only profile must not break the window */
		}
	};
	mainWindow.on('close', saveBounds);
	mainWindow.on('closed', () => {
		mainWindow = null;
	});
}

function installMenu() {
	if (process.platform !== 'darwin') {
		Menu.setApplicationMenu(null);
		return;
	}
	Menu.setApplicationMenu(
		Menu.buildFromTemplate([
			{
				label: 'Azahar',
				submenu: [
					{ role: 'about' },
					{ type: 'separator' },
					{ role: 'hide' },
					{ role: 'hideOthers' },
					{ role: 'unhide' },
					{ type: 'separator' },
					{ role: 'quit' }
				]
			},
			{
				label: 'Edit',
				submenu: [
					{ role: 'undo' },
					{ role: 'redo' },
					{ type: 'separator' },
					{ role: 'cut' },
					{ role: 'copy' },
					{ role: 'paste' },
					{ role: 'selectAll' }
				]
			},
			{ label: 'Window', submenu: [{ role: 'minimize' }, { role: 'zoom' }] }
		])
	);
}

function registerAssetProtocol() {
	protocol.handle('azahar-asset', async (request) => {
		const url = new URL(request.url);
		let relative;
		try {
			relative = decodeURIComponent(url.pathname).replace(/^[/\\]+/, '');
		} catch {
			return new Response('Not found', { status: 404 });
		}
		const root = resourcesRoot();
		const filePath = path.resolve(root, relative);
		if (filePath !== root && !filePath.startsWith(`${root}${path.sep}`)) {
			return new Response('Not found', { status: 404 });
		}
		const type = {
			'.webp': 'image/webp',
			'.png': 'image/png',
			'.jpg': 'image/jpeg',
			'.jpeg': 'image/jpeg',
			'.svg': 'image/svg+xml',
			'.woff2': 'font/woff2'
		}[path.extname(filePath).toLowerCase()];
		if (!type) return new Response('Not found', { status: 404 });
		try {
			const bytes = await fsp.readFile(filePath);
			return new Response(bytes, {
				headers: {
					'content-type': type,
					'cache-control': 'public, max-age=604800'
				}
			});
		} catch {
			return new Response('Not found', { status: 404 });
		}
	});
}

function registerIpc() {
	ipcMain.handle('app:info', async () => ({
		version: APP_VERSION,
		platform: process.platform,
		apiUrl: settings.apiUrl,
		origin: settings.origin,
		resolvedOrigin: settings.origin || resolvedOrigin || '',
		packaged: app.isPackaged
	}));

	ipcMain.handle('app:health', async () => {
		try {
			const response = await requestRaw('GET', '/api/health', {
				includeOrigin: false
			});
			const body = await readJson(response);
			if (!response.ok) {
				return {
					ok: false,
					status: response.status,
					message: 'The API answered, but not with a healthy status.'
				};
			}
			if (!settings.origin && body && typeof body.origin === 'string') {
				try {
					resolvedOrigin = normalizeOrigin(body.origin);
				} catch {
					/* keep the fallback */
				}
			}
			return {
				ok: true,
				status: response.status,
				env: body && body.env,
				origin: settings.origin || resolvedOrigin,
				serverTime: body && body.server_time
			};
		} catch (error) {
			return { ok: false, status: 0, message: serializeError(error).message };
		}
	});

	ipcMain.handle('settings:update', async (_event, payload) => {
		const apiUrl = normalizeApiUrl(payload && payload.apiUrl);
		const origin = normalizeOrigin(payload && payload.origin);
		const changedApi = apiUrl !== settings.apiUrl;
		const changedOrigin = origin !== settings.origin;
		if (changedApi || changedOrigin) {
			settings.apiUrl = apiUrl;
			settings.origin = origin;
			resolvedOrigin = '';
			imageCache.clear();
			clearSession();
			saveSettings();
			await resolveOrigin(true);
		}
		return {
			apiUrl: settings.apiUrl,
			origin: settings.origin,
			resolvedOrigin: settings.origin || resolvedOrigin
		};
	});

	ipcMain.handle('api:request', async (_event, payload) => {
		try {
			const method = String(payload && payload.method ? payload.method : 'GET').toUpperCase();
			const apiPath = String(payload && payload.path ? payload.path : '');
			const data = await apiRequest(method, apiPath, payload ? payload.body : undefined);
			return { ok: true, data };
		} catch (error) {
			return { ok: false, error: serializeError(error) };
		}
	});

	ipcMain.handle('api:image', async (_event, src) => {
		try {
			const data = await apiImage(String(src || ''));
			return { ok: true, data };
		} catch (error) {
			return { ok: false, error: serializeError(error) };
		}
	});

	ipcMain.handle('session:clear', async () => {
		clearSession();
		imageCache.clear();
		return { ok: true };
	});

	ipcMain.handle('shell:open', async (_event, url) => {
		if (typeof url !== 'string' || !/^https?:\/\//i.test(url)) {
			return { ok: false };
		}
		await shell.openExternal(url);
		return { ok: true };
	});
}

const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
	app.quit();
} else {
	app.on('second-instance', () => {
		if (mainWindow) {
			if (mainWindow.isMinimized()) mainWindow.restore();
			mainWindow.focus();
		}
	});

	app.whenReady().then(() => {
		loadSettings();
		loadSession();
		registerAssetProtocol();
		registerIpc();
		installMenu();
		createWindow();
		void resolveOrigin();

		app.on('activate', () => {
			if (BrowserWindow.getAllWindows().length === 0) createWindow();
		});
	});

	app.on('window-all-closed', () => {
		if (process.platform !== 'darwin') app.quit();
	});
}
