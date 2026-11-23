// The one toast. Screens and the shell ask for a message and a tone; the node
// itself lives in the static page.

import { nodes } from './dom.js';
import { state } from './state.js';

/**
 * @param {string} message
 * @param {'info' | 'error'} [tone]
 */
export function toast(message, tone = 'info') {
	const node = nodes.toast;
	clearTimeout(state.toastTimer);
	node.textContent = message;
	node.dataset.tone = tone;
	node.hidden = false;
	node.style.animation = 'none';
	void node.offsetWidth;
	node.style.animation = '';
	state.toastTimer = setTimeout(() => {
		node.hidden = true;
	}, 3800);
}
