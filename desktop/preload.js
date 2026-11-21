const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('azahar', {
	info: () => ipcRenderer.invoke('app:info'),
	health: () => ipcRenderer.invoke('app:health'),
	request: (method, path, body) =>
		ipcRenderer.invoke('api:request', { method, path, body }),
	image: (path) => ipcRenderer.invoke('api:image', path),
	updateSettings: (payload) => ipcRenderer.invoke('settings:update', payload),
	clearSession: () => ipcRenderer.invoke('session:clear'),
	openExternal: (url) => ipcRenderer.invoke('shell:open', url)
});
